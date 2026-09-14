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
//! # There is a secret on disk now, and it is sealed
//!
//! [`Record`] is what is serialised, and since 2026-09-05 it carries exactly
//! one field a bearer value is inside: [`Record::sealed`], AES-256-GCM
//! ciphertext under a key from the OS keyring. Until then it had no such field
//! at all, because sealing was a port with no implementation and a store that
//! wrote a secret in plaintext until the sealing arc arrived would have been
//! the "for now" this harness forbids.
//!
//! What replaced that argument is a narrower one of the same shape. The field's
//! type is [`Sealed`], which **has no constructor that takes a plaintext** —
//! [`Sealed::seal`] needs a key and an alias, and deserialisation validates a
//! version byte and a length before it yields anything. So "the file carries no
//! plaintext secret" is still a property of the type rather than a claim about
//! a code path. It is also not optional: a [`Record`] without a sealed value
//! does not exist, so "every entry's secret is sealed" needs no check either.
//!
//! # The file is replaced atomically, and that became load-bearing here
//!
//! [`CredentialStore::save`] goes through [`crate::atomic::write`]. Until this
//! file carried secrets it truncated and rewrote in place, which was survivable
//! for metadata; it is not survivable for the only copy of every credential the
//! user has.
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
use crate::credentials::entry::{Description, Entry, Held, Reach, Role, ToolScope};
use crate::credentials::family::Family;
use crate::credentials::port::Confirm;
use crate::credentials::sealing::blob::Sealed;
use crate::credentials::sealing::failure::SealingError;
use crate::credentials::sealing::key::KeyStore;
use crate::credentials::secret::Secret;
use crate::providers::ProviderKind;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
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
    /// A provider record names a kind this build does not have.
    UnknownProviderKind {
        /// The kind as the file spells it. **Not a value** — a provider kind
        /// is `gemini` or `anthropic`, never a credential.
        offered: String,
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
    ///
    /// Carries a closed [`SealingError`] rather than an implementation's own
    /// wording, which is what makes [ADR-0016]'s taxonomy able to read it —
    /// see [`crate::credentials::sealing::failure`].
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    Sealing(SealingError),
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
                "the alias \"{alias}\" is already in the store; an alias is a local unique name, \
                 and two tokens answering to one name is a namespace whose \
                 destination cannot be read off the transcript"
            ),
            Self::UnknownAlias { alias } => {
                write!(f, "nothing in the store answers to the alias \"{alias}\"")
            }
            Self::ApexNeedsConfirmation { alias, grants } => write!(
                f,
                "the token \"{alias}\" is apex and no confirmer was supplied, so it was refused \
                 rather than stored silently. An apex credential requires an explicit \
                 confirmation stating what it grants -- never silent, never a default -- and it \
                 grants: \
                 {grants}"
            ),
            Self::ApexDeclined { alias } => write!(
                f,
                "the apex token \"{alias}\" was not confirmed, so it was not stored"
            ),
            Self::SecondComposerRole { existing, offered } => write!(
                f,
                "the alias \"{existing}\" already carries the composer role and \"{offered}\" was \
                 offered it too. Exactly one token is flagged composer and a token cannot be \
                 both, so the store refuses the configuration. Move the role \
                 rather than granting a second"
            ),
            Self::ComposerScopeExceeded { alias, tool } => write!(
                f,
                "the alias \"{alias}\" was offered the composer role and its cached scope carries \
                 {tool:?}, which the composer's credential does not carry. That credential is \
                 scoped to the read_only_memory set plus me.set_current_workspace and nothing \
                 else, so that the composer token cannot write, enforced at all three gates, \
                 regardless of what any code in the harness attempts. This is the local half of \
                 that: the harness refuses to use as the composer a credential whose own scope \
                 says it could do more. What the server actually granted is the server's three \
                 gates to enforce and not the harness's to verify"
            ),
            Self::UnknownProviderKind { offered } => write!(
                f,
                "the credential store holds a provider key under the kind {offered:?}, and \
                 no such kind is named in this build: {}. A credential this harness \
                 cannot classify is one it cannot redact from a prompt, so it is reported here \
                 rather than skipped",
                ProviderKind::ALL
                    .iter()
                    .map(|kind| format!("`{kind}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Self::Sealing(failure) => write!(f, "{failure}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Sealing(failure) => Some(failure),
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

/// The half of a record that belongs to one family and not the other.
///
/// [`Held`]'s on-disk form. Externally tagged
/// rather than internally tagged, so the file reads
/// `"held": {"notes": {…}}` — a shape a person opening
/// `~/.zaru/credentials.json` can classify at a glance, and one
/// `deny_unknown_fields` covers on both the wrapper and each variant.
///
/// **A provider record has no `reach`, no `tools` and no `workspace`, and a
/// Notes record has no provider kind.** That is the whole reason this is an
/// enum; see [`Held`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum StoredHeld {
    /// A Nuclear Notes token.
    Notes {
        /// D2's `kind` for a Notes token: `personal` or `app`. Written down
        /// so a reader of the file can see it, and still derived from the
        /// secret at every use, so the file is a report rather than a source
        /// of truth.
        kind: String,
        /// D8's instance boundary, or its absence.
        reach: StoredReach,
        /// D2's `role`. `None` is D2's "unset". Only a Notes token can carry
        /// one: ADR-0007 D4's composer role is a Nuclear Notes pointer's.
        role: Option<String>,
        /// D6's cached tool names.
        tools: Vec<String>,
        /// D2's `workspace`, "informational only".
        workspace: Option<String>,
    },
    /// A model provider's API key.
    Provider {
        /// The provider kind this key authenticates against, as
        /// [ADR-0012](https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction)
        /// D3 spells it.
        kind: String,
    },
}

/// One credential as the store keeps it.
///
/// Three fields: [ADR-0007] D2's `description`, the family-specific half, and
/// the bearer value, sealed — see the module documentation for why the last is
/// not optional and why its type cannot be built from a plaintext.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// ADR-0007 D2's `description`.
    pub description: String,
    /// Everything that belongs to this credential's family and not the other.
    pub held: StoredHeld,
    /// D2's `secret`, at rest.
    ///
    /// The only field in this crate a bearer value is inside, and the only one
    /// whose type refuses to be built from one.
    pub sealed: Sealed,
}

impl Record {
    /// The kind, whichever family this is.
    ///
    /// One accessor for both, because both are rendered into the same column
    /// of the same two listings.
    #[must_use]
    pub fn kind(&self) -> &str {
        match &self.held {
            StoredHeld::Notes { kind, .. } | StoredHeld::Provider { kind } => kind,
        }
    }

    /// Whether this is a Nuclear Notes token.
    ///
    /// The predicate both listings are filtered by, so ADR-0007 D7's
    /// `notes tokens` lists Notes tokens and nothing else.
    #[must_use]
    pub const fn is_notes(&self) -> bool {
        matches!(self.held, StoredHeld::Notes { .. })
    }

    /// D2's `role`, which only a Notes token can carry.
    #[must_use]
    pub fn role(&self) -> Option<&str> {
        match &self.held {
            StoredHeld::Notes { role, .. } => role.as_deref(),
            StoredHeld::Provider { .. } => None,
        }
    }

    /// D6's cached tool names, which only a Notes token has.
    #[must_use]
    pub fn tools(&self) -> &[String] {
        match &self.held {
            StoredHeld::Notes { tools, .. } => tools,
            StoredHeld::Provider { .. } => &[],
        }
    }

    /// D2's informational workspace pointer, which only a Notes token has.
    #[must_use]
    pub fn workspace(&self) -> Option<&str> {
        match &self.held {
            StoredHeld::Notes { workspace, .. } => workspace.as_deref(),
            StoredHeld::Provider { .. } => None,
        }
    }

    /// D8's reach, which only a Notes token has.
    #[must_use]
    pub const fn reach(&self) -> Option<&StoredReach> {
        match &self.held {
            StoredHeld::Notes { reach, .. } => Some(reach),
            StoredHeld::Provider { .. } => None,
        }
    }

    /// Which family this record declares, before anything is decrypted.
    ///
    /// # Errors
    ///
    /// [`StoreError::UnknownProviderKind`] when a provider record names a
    /// kind [ADR-0012](https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction)
    /// D3 does not. That is a file written by a build that knew a kind this
    /// one does not, or a hand edit, and it is reported rather than silently
    /// skipped: a credential the store cannot classify is one it cannot
    /// redact, and [`crate::redaction::held_secrets_for_redaction`] walks
    /// every record.
    pub fn family(&self) -> Result<Family, StoreError> {
        match &self.held {
            StoredHeld::Notes { .. } => Ok(Family::Notes),
            StoredHeld::Provider { kind } => ProviderKind::parse(kind)
                .map(Family::Provider)
                .ok_or_else(|| StoreError::UnknownProviderKind {
                    offered: kind.clone(),
                }),
        }
    }
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

/// What [`CredentialStore::remove`] took out.
///
/// # One field, and what it is not
///
/// It carries **no secret, no ciphertext and no record** — only whether the
/// credential that is now gone held [ADR-0007] D4's composer role, which is
/// the one consequence of removing it that a person cannot see in the listing
/// afterwards, because the thing that would have shown it is the row that was
/// removed.
///
/// Returning the [`Record`] instead would have been the obvious shape and is
/// the wrong one: a `Record` owns a [`Sealed`], so the caller would be holding
/// a removed credential's ciphertext for no reason any surface has. D3's rule
/// is that a bearer value never leaves the store except through the two named
/// functions that exist for it, and this is not one of them.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Removed {
    /// Whether the credential removed carried the composer role.
    pub held_composer_role: bool,
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
        crate::config::home::default_root().ok_or(StoreError::NoHome)
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

        // One parse, shared with `reading`, so the two openers cannot come to
        // differ about what the file says.
        let entries = Self::entries_at(&root.join(STORE_FILE))?;

        Ok(Self { root, entries })
    }

    /// Read the store under `root` **without creating anything**.
    ///
    /// [`open`] re-asserts `0700` on the directory on every call because it is
    /// about to write; this never writes, so it has nothing to protect and
    /// nothing to fix, and creating a `~/.zaru` in order to find no tokens in
    /// it would be creating state to read state -- ADR-0014's port carries
    /// that argument for the configuration loader and it holds identically
    /// here.
    ///
    /// An absent file is an empty store rather than an error, exactly as it is
    /// for [`open`]: a user who has never added a token has not made a
    /// mistake, and [ADR-0007] D7's listing is what tells them so.
    ///
    /// **A caller that is going to write calls [`open`].** Nothing in this
    /// harness can: `add` needs a [`KeyStore`] and, for an apex token, a
    /// [`Confirm`] — and while a `KeyStore` now has a product implementation,
    /// nothing reaches one, because no command adds a token.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] when the file exists and cannot be read, and
    /// [`StoreError::Malformed`] when it does not parse.
    ///
    /// [`open`]: CredentialStore::open
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    pub fn reading(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        let path = root.join(STORE_FILE);
        let entries = Self::entries_at(&path)?;
        Ok(Self { root, entries })
    }

    /// Read the stored file, or an empty map when it is not there.
    fn entries_at(path: &Path) -> Result<BTreeMap<Alias, Record>, StoreError> {
        match fs::read_to_string(path) {
            Ok(text) => {
                let stored: StoredFile =
                    serde_json::from_str(&text).map_err(|error| StoreError::Malformed {
                        path: path.to_path_buf(),
                        detail: error.to_string(),
                    })?;
                let mut entries = BTreeMap::new();
                for (key, record) in stored.entries {
                    let alias = Alias::new(&key).map_err(|refusal| StoreError::Malformed {
                        path: path.to_path_buf(),
                        detail: refusal.to_string(),
                    })?;
                    entries.insert(alias, record);
                }
                Ok(entries)
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(source) => Err(StoreError::Io {
                action: "read the credential store",
                path: path.to_path_buf(),
                source,
            }),
        }
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
    /// [`StoreError::ApexDeclined`], [`StoreError::Sealing`] and
    /// [`StoreError::Io`].
    pub fn add(
        &mut self,
        entry: Entry,
        keys: &dyn KeyStore,
        confirmer: Option<&dyn Confirm>,
    ) -> Result<(), StoreError> {
        let alias = entry.alias().clone();
        if self.entries.contains_key(&alias) {
            return Err(StoreError::DuplicateAlias { alias });
        }

        // The apex gate is D8's and applies to a Nuclear Notes token alone: a
        // provider key has no instance boundary to cross, which is why
        // `Entry::reach` answers `None` for one rather than answering a
        // default that would read as "instance-locked" and be meaningless.
        if let Some(reach) = entry.reach()
            && reach.is_apex()
        {
            let grants = apex_grants(
                entry
                    .tools()
                    .expect("an entry with a reach is a Notes entry and has a tool scope"),
            );
            match confirmer {
                None => return Err(StoreError::ApexNeedsConfirmation { alias, grants }),
                Some(confirmer) if !confirmer.confirm_apex(&alias, &grants) => {
                    return Err(StoreError::ApexDeclined { alias });
                }
                Some(_) => {}
            }
        }

        // The key is asked for **after** the apex gate, so a token the user
        // declined never causes a key to be minted into their keyring.
        let key = keys.key().map_err(StoreError::Sealing)?;
        let sealed = Sealed::seal(&key, &alias, entry.secret()).map_err(StoreError::Sealing)?;

        let held = match entry.held() {
            Held::Notes {
                reach,
                tools,
                workspace,
            } => StoredHeld::Notes {
                kind: entry.secret().kind().as_str().to_owned(),
                reach: match reach {
                    Reach::InstanceLocked(instance) => {
                        StoredReach::InstanceLocked(instance.as_str().to_owned())
                    }
                    Reach::Apex => StoredReach::Apex,
                },
                role: None,
                tools: tools.names().to_vec(),
                workspace: workspace.clone(),
            },
            Held::Provider { kind } => StoredHeld::Provider {
                kind: kind.as_str().to_owned(),
            },
        };

        let record = Record {
            description: entry.description().as_str().to_owned(),
            held,
            sealed,
        };
        self.entries.insert(alias, record);
        self.save()
    }

    /// [ADR-0007] D7's `describe`: replace one credential's description.
    ///
    /// # It replaces the line rather than editing it
    ///
    /// D2 calls `description` "One line on what this token is for", and D7
    /// calls this surface "set or edit the description". One line has no
    /// interior for an edit to address, so setting it is the whole operation
    /// and there is nothing to append to.
    ///
    /// # The text is a [`Description`], so the refusal cannot be skipped
    ///
    /// It takes the parsed type rather than a `&str`, which means a newline —
    /// or any other control character — is refused by [`Description::new`]
    /// **before this is called**, at the one place that rule lives. A
    /// signature taking `&str` would let a call site store a description this
    /// store would not have accepted from [`Self::add`], and D7's listing
    /// renders both through the same column.
    ///
    /// Until now that refusal had no reachable caller: the only description
    /// this harness composed was `notes_entry`'s machine-made sentence, which
    /// cannot carry a control character. It is the user's to trip now, and it
    /// is classified as the user's.
    ///
    /// # The family is a parameter, on this store's own precedent
    ///
    /// `family` is what the *calling surface* is for, and a record of the
    /// other family answers [`StoreError::UnknownAlias`] — the same reading
    /// [`Self::grant_composer_role`] and [`Self::move_composer_role`] already
    /// take of a provider key, because from the caller's side "there is no
    /// Nuclear Notes token by that name" is exactly what happened. The
    /// accepted Update of 2026-09-05 states the rule it serves: **"D7's
    /// listing does not lie about what it lists."** `zaru notes tokens` is
    /// filtered to Notes tokens, so a `notes tokens` verb that reached past
    /// that filter would act on a credential its own listing cannot show.
    ///
    /// # Errors
    ///
    /// [`StoreError::UnknownAlias`] when nothing answers to `alias` and when
    /// what answers belongs to the other family,
    /// [`StoreError::UnknownProviderKind`] when the stored record names a kind
    /// this build does not have, and [`StoreError::Io`] from the write.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    pub fn describe(
        &mut self,
        alias: &Alias,
        text: &Description,
        family: Family,
    ) -> Result<(), StoreError> {
        // --- every refusal, before anything is written ---
        self.of_family(alias, family)?;

        // --- nothing above this line has written; nothing below it refuses ---
        self.entries
            .get_mut(alias)
            .expect("the record was found above")
            .description = text.as_str().to_owned();
        self.save()
    }

    /// [ADR-0007] D7's `rm`: remove one credential, and its secret with it.
    ///
    /// # The sealed value goes in the same write
    ///
    /// [`Record`] owns its [`Sealed`] and the file is the serialisation of the
    /// map, so dropping the entry drops the ciphertext: there is no second
    /// place a removed credential could persist and no tombstone left behind.
    /// [`Self::save`] writes through [`crate::atomic::write`], so a reader —
    /// or a crash — sees the whole file with the entry or the whole file
    /// without it, never a store in between.
    ///
    /// # It removes the composer's token, deliberately
    ///
    /// D4 flags exactly one token `composer`, and this will remove it if that
    /// is the alias named. **Revoking a credential is the person's to do**,
    /// and a store that refused would leave someone unable to remove a token
    /// they had revoked on the server. What D4 does not say is what happens
    /// when *no* token carries the role, and that is answered — measured, on
    /// every machine that exists — by the amendment of 2026-09-14 on
    /// [the amendments page]: a lone stored token still serves the composer's
    /// reads, several with no role serve nothing.
    ///
    /// So [`Removed`] carries whether the role was held, and the caller says
    /// so in its outcome rather than leaving a person to discover at the next
    /// session that their hint strip went quiet.
    ///
    /// # Errors
    ///
    /// [`StoreError::UnknownAlias`] when nothing answers to `alias` and when
    /// what answers belongs to the other family,
    /// [`StoreError::UnknownProviderKind`] when the stored record names a kind
    /// this build does not have, and [`StoreError::Io`] from the write.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    /// [the amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store-updates
    pub fn remove(&mut self, alias: &Alias, family: Family) -> Result<Removed, StoreError> {
        // --- every refusal, before anything is written ---
        self.of_family(alias, family)?;
        let held_composer_role = self.composer().is_some_and(|(held_by, _)| held_by == alias);

        // --- nothing above this line has written; nothing below it refuses ---
        self.entries
            .remove(alias)
            .expect("the record was found above");
        self.save()?;
        Ok(Removed { held_composer_role })
    }

    /// The record under `alias`, refusing one that is not `family`'s.
    ///
    /// Shared by [`Self::describe`] and [`Self::remove`] so the two cannot
    /// come to disagree about which credentials a surface may reach.
    fn of_family(&self, alias: &Alias, family: Family) -> Result<&Record, StoreError> {
        let record = self
            .entries
            .get(alias)
            .ok_or_else(|| StoreError::UnknownAlias {
                alias: alias.clone(),
            })?;
        if record.family()? != family {
            return Err(StoreError::UnknownAlias {
                alias: alias.clone(),
            });
        }
        Ok(record)
    }

    /// Take a token's bearer value back out, for dispatch and nothing else.
    ///
    /// # Errors
    ///
    /// [`StoreError::UnknownAlias`] and [`StoreError::Sealing`].
    pub fn secret(&self, alias: &Alias, keys: &dyn KeyStore) -> Result<Secret, StoreError> {
        let record = self
            .entries
            .get(alias)
            .ok_or_else(|| StoreError::UnknownAlias {
                alias: alias.clone(),
            })?;
        let family = record.family()?;
        let key = keys.key().map_err(StoreError::Sealing)?;
        record
            .sealed
            .open(&key, alias, family)
            .map_err(StoreError::Sealing)
    }

    /// Give one token ADR-0007 D4's composer role, taking it from no other.
    ///
    /// Refuses rather than moves: D4 says "The store refuses the
    /// configuration", and a grant that silently demoted whichever token held
    /// the role would be exactly the invisible reassignment ADR-0006 D2
    /// exists to prevent. D7's `/notes use <alias>` is the surface that moves
    /// it deliberately, and **that surface is now built** — see
    /// [`CredentialStore::move_composer_role`]. This function is unchanged and
    /// still refuses, which is what clause 6 asserts about a *second grant*.
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

        let scope = ToolScope::new(record.tools().to_vec());
        if let Some(tool) = scope.outside_composer_scope() {
            return Err(StoreError::ComposerScopeExceeded {
                alias: alias.clone(),
                tool: tool.to_owned(),
            });
        }

        // A provider key cannot hold the composer role, and the enum is what
        // says so: `StoredHeld::Provider` has no `role` field to write. The
        // arm returns the same refusal an unknown alias does rather than
        // inventing a variant, because from the caller's side "there is no
        // Notes token by that name" is exactly what happened.
        match &mut self
            .entries
            .get_mut(alias)
            .expect("the record was found above")
            .held
        {
            StoredHeld::Notes { role, .. } => *role = Some(Role::Composer.as_str().to_owned()),
            StoredHeld::Provider { .. } => {
                return Err(StoreError::UnknownAlias {
                    alias: alias.clone(),
                });
            }
        }
        self.save()
    }

    /// [ADR-0007] D7's `use`: move the composer role to `alias`.
    ///
    /// # Why this is a second operation and not a flag on the grant
    ///
    /// [`Self::grant_composer_role`] refuses when another token holds the
    /// role, which is right and is what clause 6 asserts: a **second grant**
    /// is a configuration D4 says the store refuses, and a grant that silently
    /// demoted the incumbent would be the invisible reassignment [ADR-0006] D2
    /// exists to prevent.
    ///
    /// **D7's `use` is not a second grant. It is a move, and the record says
    /// so in as many words** — "move the composer role to another token".
    /// Built on 2026-09-14 under a delegated coordinator ruling, open to
    /// Jeshua's veto, because a `use` that refused whenever any token held the
    /// role could succeed at most **once on a machine, ever**, and the person
    /// who meets that is the person adding their second token.
    ///
    /// So the two operations stay two: one refuses a second holder, the other
    /// replaces the holder deliberately, and each says which it is at the call
    /// site. Nothing about clause 6 changes.
    ///
    /// # The refusal decides before anything moves
    ///
    /// Every reason to refuse — an alias nothing holds, a provider key, a
    /// scope reaching outside [ADR-0006] D4's set — is evaluated **before the
    /// first field is written**, so a refused move leaves the incumbent
    /// holding the role exactly as it found it. A revoke-then-grant that
    /// checked the scope in between would, on a refusal, leave the store with
    /// **no** composer at all: the person would have asked for a change that
    /// was refused and lost the setting they already had.
    ///
    /// It is also one write. `save` is called once, after both fields are
    /// set, so no reader can observe a store with two composers or none.
    ///
    /// # Errors
    ///
    /// [`StoreError::UnknownAlias`] for a name nothing holds and for a
    /// provider key, [`StoreError::ComposerScopeExceeded`] naming the first
    /// tool outside D4's set, and [`StoreError::Io`].
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    pub fn move_composer_role(&mut self, alias: &Alias) -> Result<(), StoreError> {
        // --- every refusal, before anything is written ---
        let record = self
            .entries
            .get(alias)
            .ok_or_else(|| StoreError::UnknownAlias {
                alias: alias.clone(),
            })?;

        // A provider key has no `role` field to write, and from the caller's
        // side "there is no Notes token by that name" is exactly what
        // happened -- the same reading `grant_composer_role` already gives it.
        if !record.is_notes() {
            return Err(StoreError::UnknownAlias {
                alias: alias.clone(),
            });
        }

        let scope = ToolScope::new(record.tools().to_vec());
        if let Some(tool) = scope.outside_composer_scope() {
            return Err(StoreError::ComposerScopeExceeded {
                alias: alias.clone(),
                tool: tool.to_owned(),
            });
        }

        // --- nothing above this line has written; nothing below it refuses ---
        let incumbent = self
            .composer()
            .map(|(held_by, _)| held_by.clone())
            .filter(|held_by| held_by != alias);

        if let Some(held_by) = incumbent
            && let Some(record) = self.entries.get_mut(&held_by)
            && let StoredHeld::Notes { role, .. } = &mut record.held
        {
            *role = None;
        }

        match &mut self
            .entries
            .get_mut(alias)
            .expect("the record was found above")
            .held
        {
            StoredHeld::Notes { role, .. } => *role = Some(Role::Composer.as_str().to_owned()),
            // Unreachable: `is_notes` refused above, before any write. Named
            // rather than left to a wildcard so a third family fails here.
            StoredHeld::Provider { .. } => {
                return Err(StoreError::UnknownAlias {
                    alias: alias.clone(),
                });
            }
        }
        self.save()
    }

    /// The token carrying the composer role, if one does.
    #[must_use]
    pub fn composer(&self) -> Option<(&Alias, &Record)> {
        self.entries
            .iter()
            .find(|(_, record)| record.role() == Some(Role::Composer.as_str()))
    }

    /// Replace a token's cached tool scope and write the store.
    ///
    /// [ADR-0007] D6's cache is [`Record::tools`], and this is the only thing
    /// that moves it after an entry is added. It is `pub(crate)` because the
    /// public door is [`CredentialStore::refresh_tool_scope`](super::notes) —
    /// a scope may only be replaced by a `tools/list`, and a setter anybody
    /// could call would make that a convention rather than a mechanism.
    ///
    /// The write-through is what makes D5 and D6 one read: the projection to
    /// the agent is built from this same field, so a refreshed scope reaches
    /// the agent's namespace without a second call.
    ///
    /// # Errors
    ///
    /// [`StoreError::UnknownAlias`] when nothing answers to `alias`, and
    /// [`StoreError::Io`] when the file cannot be written.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    pub(crate) fn replace_tools(
        &mut self,
        alias: &Alias,
        scope: &ToolScope,
    ) -> Result<(), StoreError> {
        if !self.entries.contains_key(alias) {
            return Err(StoreError::UnknownAlias {
                alias: alias.clone(),
            });
        }
        match &mut self
            .entries
            .get_mut(alias)
            .expect("the alias was found above")
            .held
        {
            StoredHeld::Notes { tools, .. } => *tools = scope.names().to_vec(),
            StoredHeld::Provider { .. } => {
                return Err(StoreError::UnknownAlias {
                    alias: alias.clone(),
                });
            }
        }
        self.save()
    }

    /// Write the store to disk at [`FILE_MODE`], atomically.
    ///
    /// Through [`crate::atomic::write`], so a reader — or a crash — sees the
    /// whole previous file or the whole new one. That mattered less when this
    /// file carried only metadata and matters a great deal now that it carries
    /// every sealed secret the store holds.
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

        crate::atomic::write(&path, text.as_bytes(), FILE_MODE).map_err(|failure| {
            StoreError::Io {
                action: failure.action,
                path: failure.path,
                source: failure.source,
            }
        })?;

        // The mode is re-asserted on the live file as well as set on the
        // temporary, because a file that already existed at a wider mode is a
        // defect whoever created it and a call that noticed and did nothing
        // would be a comment rather than a mechanism. The modes check reads
        // both back off the filesystem rather than from what was asked for.
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
