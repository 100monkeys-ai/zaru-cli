// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The store itself: what is on disk, what is not, and the modes it carries.
//!
//! # Unix only, and it says so at compile time rather than at run time
//!
//! ADR-0004 D3 is the harness's own precedent for a secret-bearing file: two
//! keys, "both mode `0600`". Modes are a Unix concept and `std::os::unix` is
//! where they live. No record names a platform for the harness, so this
//! module refuses to build off Unix rather than building and quietly
//! enforcing nothing — [Verification lessons] §26 is exactly this shape, a
//! rule that holds by circumstance reading identically to one that holds by
//! construction. The Windows question has a row on the ADR backlog
//! ("Windows support strategy") and belongs there rather than in a `cfg`
//! branch invented here.
//!
//! # There is no secret on disk, and no field one could go in
//!
//! [`Record`] is what is serialised. It has no `secret`. The bearer value
//! goes to [`SecretStore`], which this crate declares and does not implement,
//! so this arc writes no secret anywhere at all. That is the same argument
//! ADR-0014 D4 makes about configuration files, applied one layer down.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

#[cfg(not(unix))]
compile_error!(
    "the credential store enforces ADR-0004 D3's 0600/0700 file modes through std::os::unix, \
     and no record names a non-Unix platform for the harness. Building here without those \
     modes would leave a credential directory readable by every process on the machine while \
     every check still passed. See the ADR backlog row \"Windows support strategy\"."
);

use crate::credentials::alias::Alias;
use crate::credentials::entry::{Entry, Reach, Role, ToolScope};
use crate::credentials::port::{Confirm, SealFailure, SecretStore};
use crate::credentials::secret::Secret;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The directory the harness keeps its own files in.
///
/// Established by ADR-0004 D3 (`~/.zaru/node.key`), ADR-0010 D1
/// (`~/.zaru/sessions/`), ADR-0014 D1 (`~/.zaru/config.toml`) and ADR-0015 D3
/// (`~/.zaru/commands/`). This module adds one file to it.
pub const HOME_DIRECTORY: &str = ".zaru";

/// The store's file, mirroring ADR-093's `~/.aegis/auth.json`.
pub const STORE_FILE: &str = "credentials.json";

/// The mode the store's directory carries. ADR-0004 D3's precedent.
pub const DIRECTORY_MODE: u32 = 0o700;

/// The mode the store's file carries. ADR-0004 D3's precedent.
pub const FILE_MODE: u32 = 0o600;

/// What went wrong, in words that never carry a bearer value.
#[derive(Debug)]
pub enum StoreError {
    /// The home directory could not be resolved.
    NoHome,
    /// A filesystem operation failed.
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// The store's file did not parse, or carried a key nothing reads.
    Malformed {
        /// The file.
        path: PathBuf,
        /// What the parser said. Positional; it quotes no field value.
        detail: String,
    },
    /// An alias is already taken.
    DuplicateAlias {
        /// The alias offered.
        alias: Alias,
    },
    /// There is nothing stored under that alias.
    UnknownAlias {
        /// The alias asked for.
        alias: Alias,
    },
    /// An apex token was offered with no way to ask the user about it.
    ApexNeedsConfirmation {
        /// The alias offered.
        alias: Alias,
        /// What the user would have been told it grants.
        grants: String,
    },
    /// The user was asked about an apex token and said no.
    ApexDeclined {
        /// The alias offered.
        alias: Alias,
    },
    /// A second token was offered the composer role.
    SecondComposerRole {
        /// The alias that already holds it.
        existing: Alias,
        /// The alias that was offered it.
        offered: Alias,
    },
    /// A token was offered the composer role and its scope reaches further
    /// than ADR-0006 D4 allows the composer's credential to reach.
    ComposerScopeExceeded {
        /// The alias that was offered the role.
        alias: Alias,
        /// The first tool in its cached scope that D4 does not name.
        tool: String,
    },
    /// Sealing or unsealing failed.
    Seal(SealFailure),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHome => f.write_str(
                "no home directory could be resolved, so there is nowhere to put ~/.zaru",
            ),
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "could not {action} {}: {source}", path.display()),
            Self::Malformed { path, detail } => write!(
                f,
                "the credential store at {} did not parse: {detail}",
                path.display()
            ),
            Self::DuplicateAlias { alias } => write!(
                f,
                "the alias \"{alias}\" is already in the store; ADR-0007 D2 makes an alias a local \
                 unique name, and two tokens answering to one name is a namespace whose \
                 destination cannot be read off the transcript"
            ),
            Self::UnknownAlias { alias } => {
                write!(f, "nothing in the store answers to the alias \"{alias}\"")
            }
            Self::ApexNeedsConfirmation { alias, grants } => write!(
                f,
                "the token \"{alias}\" is apex and no confirmer was supplied, so it was refused \
                 rather than stored silently. ADR-0007 D8 requires an explicit confirmation \
                 stating what it grants -- \"never silent, never a default\" -- and it grants: \
                 {grants}"
            ),
            Self::ApexDeclined { alias } => write!(
                f,
                "the apex token \"{alias}\" was not confirmed, so it was not stored"
            ),
            Self::SecondComposerRole { existing, offered } => write!(
                f,
                "the alias \"{existing}\" already carries the composer role and \"{offered}\" was \
                 offered it too. ADR-0007 D4: \"Exactly one token is flagged composer\" and \"A \
                 token cannot be both. The store refuses the configuration.\" Move the role \
                 rather than granting a second"
            ),
            Self::ComposerScopeExceeded { alias, tool } => write!(
                f,
                "the alias \"{alias}\" was offered the composer role and its cached scope carries \
                 {tool:?}, which ADR-0006 D4 does not put in the composer's credential. D4 scopes \
                 it to the read_only_memory set plus me.set_current_workspace \"and nothing \
                 else\", so that the composer token \"cannot write, enforced at all three gates, \
                 regardless of what any code in the harness attempts\". This is the local half of \
                 that: the harness refuses to use as the composer a credential whose own scope \
                 says it could do more. What the server actually granted is ADR-0135's three \
                 gates to enforce and not the harness's to verify"
            ),
            Self::Seal(failure) => write!(f, "the secret could not be sealed: {failure}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Seal(failure) => Some(failure),
            _ => None,
        }
    }
}

/// How a [`Reach`] is written down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum StoredReach {
    /// ADR-0007 D8's default.
    InstanceLocked(String),
    /// D8's marked exception.
    Apex,
}

/// One token as the store keeps it. **No secret, and no field for one.**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// ADR-0007 D2's `description`.
    pub description: String,
    /// D2's `kind`, written down so a reader of the file can see it. It is
    /// still derived from the secret at every use, so the file is a report
    /// rather than a source of truth.
    pub kind: String,
    /// D8's instance boundary, or its absence.
    pub reach: StoredReach,
    /// D2's `role`. `None` is D2's "unset".
    pub role: Option<String>,
    /// D6's cached tool names.
    pub tools: Vec<String>,
    /// D2's `workspace`, "informational only".
    pub workspace: Option<String>,
}

/// The file's whole shape.
///
/// `deny_unknown_fields` here and on [`Record`] is deliberate. ADR-0014 D5
/// makes the same argument for configuration: "A typo that silently does
/// nothing is the worst outcome of any config system, because the user sees
/// no change and concludes the setting does not work." A key nothing reads in
/// a credential file is worse still, because the thing that silently does
/// nothing might have been a restriction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredFile {
    entries: BTreeMap<String, Record>,
}

/// The local store of named tokens.
#[derive(Debug)]
pub struct CredentialStore {
    root: PathBuf,
    entries: BTreeMap<Alias, Record>,
}

impl CredentialStore {
    /// Where the store lives when nobody says otherwise.
    ///
    /// # Errors
    ///
    /// [`StoreError::NoHome`] when no home directory can be resolved.
    pub fn default_root() -> Result<PathBuf, StoreError> {
        std::env::home_dir()
            .map(|home| home.join(HOME_DIRECTORY))
            .ok_or(StoreError::NoHome)
    }

    /// Open the store under `root`, creating the directory if it is absent.
    ///
    /// The root is a parameter rather than always `~/.zaru` because the
    /// product needs it to be — ADR-0014's layers can move it — and because
    /// that is what lets a check be an ordinary caller writing to the paths
    /// the product actually writes to, rather than a fake standing in for the
    /// filesystem.
    ///
    /// The directory's mode is set on every open rather than only on
    /// creation. A credential directory that is group- or world-readable is a
    /// defect whoever created it, and an open that noticed and did nothing
    /// would be a comment rather than a mechanism.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when the directory or file cannot be reached, and
    /// [`StoreError::Malformed`] when the file does not parse or carries a
    /// key nothing reads.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        // `~/.zaru/` has exactly one creator and it is not this module -- see
        // `crate::config::home`. Before 2026-09-04 the creation and the mode
        // were written out here and every other module refused to create the
        // directory; the substitution keeps this store's own two sentences and
        // moves the mechanism to the one function four records' files share.
        crate::config::home::ensure(&root).map_err(|failure| {
            let action = if failure.is_creation() {
                "create the credential store directory"
            } else {
                "set 0700 on the credential store directory"
            };
            let (path, source) = failure.into_parts();
            StoreError::Io {
                action,
                path,
                source,
            }
        })?;

        let path = root.join(STORE_FILE);
        let entries = match fs::read_to_string(&path) {
            Ok(text) => {
                let stored: StoredFile =
                    serde_json::from_str(&text).map_err(|error| StoreError::Malformed {
                        path: path.clone(),
                        detail: error.to_string(),
                    })?;
                let mut entries = BTreeMap::new();
                for (key, record) in stored.entries {
                    let alias = Alias::new(&key).map_err(|refusal| StoreError::Malformed {
                        path: path.clone(),
                        detail: refusal.to_string(),
                    })?;
                    entries.insert(alias, record);
                }
                entries
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(source) => {
                return Err(StoreError::Io {
                    action: "read the credential store",
                    path,
                    source,
                });
            }
        };

        Ok(Self { root, entries })
    }

    /// The directory this store lives in.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file this store writes.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.root.join(STORE_FILE)
    }

    /// How many tokens are stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every token, by alias, in alias order.
    pub fn records(&self) -> impl Iterator<Item = (&Alias, &Record)> {
        self.entries.iter()
    }

    /// One token's record.
    #[must_use]
    pub fn record(&self, alias: &Alias) -> Option<&Record> {
        self.entries.get(alias)
    }

    /// Store a token: seal its secret, keep its metadata, write the file.
    ///
    /// `confirmer` answers ADR-0007 D8. It is needed only for an apex token,
    /// and an apex token offered without one is refused rather than stored —
    /// a confirmation nobody can answer is the silent default D8 forbids.
    ///
    /// # Errors
    ///
    /// [`StoreError::DuplicateAlias`], [`StoreError::ApexNeedsConfirmation`],
    /// [`StoreError::ApexDeclined`], [`StoreError::Seal`] and
    /// [`StoreError::Io`].
    pub fn add(
        &mut self,
        entry: Entry,
        sealer: &mut dyn SecretStore,
        confirmer: Option<&dyn Confirm>,
    ) -> Result<(), StoreError> {
        let alias = entry.alias().clone();
        if self.entries.contains_key(&alias) {
            return Err(StoreError::DuplicateAlias { alias });
        }

        if entry.reach().is_apex() {
            let grants = apex_grants(entry.tools());
            match confirmer {
                None => return Err(StoreError::ApexNeedsConfirmation { alias, grants }),
                Some(confirmer) if !confirmer.confirm_apex(&alias, &grants) => {
                    return Err(StoreError::ApexDeclined { alias });
                }
                Some(_) => {}
            }
        }

        sealer
            .seal(&alias, entry.secret())
            .map_err(StoreError::Seal)?;

        let record = Record {
            description: entry.description().as_str().to_owned(),
            kind: entry.secret().kind().as_str().to_owned(),
            reach: match entry.reach() {
                Reach::InstanceLocked(instance) => {
                    StoredReach::InstanceLocked(instance.as_str().to_owned())
                }
                Reach::Apex => StoredReach::Apex,
            },
            role: None,
            tools: entry.tools().names().to_vec(),
            workspace: entry.workspace().map(str::to_owned),
        };
        self.entries.insert(alias, record);
        self.save()
    }

    /// Take a token's bearer value back out, for dispatch and nothing else.
    ///
    /// # Errors
    ///
    /// [`StoreError::UnknownAlias`] and [`StoreError::Seal`].
    pub fn secret(&self, alias: &Alias, sealer: &dyn SecretStore) -> Result<Secret, StoreError> {
        if !self.entries.contains_key(alias) {
            return Err(StoreError::UnknownAlias {
                alias: alias.clone(),
            });
        }
        sealer.unseal(alias).map_err(StoreError::Seal)
    }

    /// Give one token ADR-0007 D4's composer role, taking it from no other.
    ///
    /// Refuses rather than moves: D4 says "The store refuses the
    /// configuration", and a grant that silently demoted whichever token held
    /// the role would be exactly the invisible reassignment ADR-0006 D2
    /// exists to prevent. D7's `/notes use <alias>` is the surface that moves
    /// it deliberately, and that surface is not built.
    ///
    /// # Errors
    ///
    /// [`StoreError::UnknownAlias`], [`StoreError::SecondComposerRole`],
    /// [`StoreError::ComposerScopeExceeded`] and [`StoreError::Io`].
    pub fn grant_composer_role(&mut self, alias: &Alias) -> Result<(), StoreError> {
        let record = self
            .entries
            .get(alias)
            .ok_or_else(|| StoreError::UnknownAlias {
                alias: alias.clone(),
            })?;

        if let Some((existing, _)) = self.composer()
            && existing != alias
        {
            return Err(StoreError::SecondComposerRole {
                existing: existing.clone(),
                offered: alias.clone(),
            });
        }

        let scope = ToolScope::new(record.tools.clone());
        if let Some(tool) = scope.outside_composer_scope() {
            return Err(StoreError::ComposerScopeExceeded {
                alias: alias.clone(),
                tool: tool.to_owned(),
            });
        }

        self.entries
            .get_mut(alias)
            .expect("the record was found above")
            .role = Some(Role::Composer.as_str().to_owned());
        self.save()
    }

    /// The token carrying the composer role, if one does.
    #[must_use]
    pub fn composer(&self) -> Option<(&Alias, &Record)> {
        self.entries
            .iter()
            .find(|(_, record)| record.role.as_deref() == Some(Role::Composer.as_str()))
    }

    /// Write the store to disk at [`FILE_MODE`].
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when the file cannot be written, and
    /// [`StoreError::Malformed`] when the store cannot be rendered — which
    /// cannot happen for the shapes [`Record`] admits and is reported rather
    /// than unwrapped so that a future field cannot make it a panic.
    pub fn save(&self) -> Result<(), StoreError> {
        let path = self.path();
        let stored = StoredFile {
            entries: self
                .entries
                .iter()
                .map(|(alias, record)| (alias.as_str().to_owned(), record.clone()))
                .collect(),
        };
        let text =
            serde_json::to_string_pretty(&stored).map_err(|error| StoreError::Malformed {
                path: path.clone(),
                detail: error.to_string(),
            })?;

        // `mode` applies only when this call creates the file, so the
        // permissions are set again below for a file that already existed
        // with the wrong ones.
        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(FILE_MODE)
            .open(&path)
            .map_err(|source| StoreError::Io {
                action: "open the credential store for writing",
                path: path.clone(),
                source,
            })?;
        drop(file);

        fs::write(&path, text).map_err(|source| StoreError::Io {
            action: "write the credential store",
            path: path.clone(),
            source,
        })?;
        fs::set_permissions(&path, fs::Permissions::from_mode(FILE_MODE)).map_err(|source| {
            StoreError::Io {
                action: "set 0600 on the credential store",
                path,
                source,
            }
        })
    }
}

/// The sentence ADR-0007 D8 requires an apex confirmation to state.
///
/// Composed here and passed to the confirmer rather than composed by the
/// confirmer, so that what the user is told and what the store believes it
/// asked cannot drift apart.
fn apex_grants(tools: &ToolScope) -> String {
    format!(
        "no instance boundary -- this credential matches every instance you can reach through \
         workspace membership, and it grants {} tool(s)",
        tools.count()
    )
}
