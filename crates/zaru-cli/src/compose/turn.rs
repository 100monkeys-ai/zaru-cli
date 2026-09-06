// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! One turn: from a resolved configuration to an exit code.
//!
//! # The order is a dependency order and every step can refuse
//!
//! ```text
//! tier          ADR-0001 D2, resolved once, immutable for the session
//! model         ADR-0012 D4, through ADR-0014's five layers
//! kind          which provider holds a key on this machine -- see `kind_for`
//! key           ADR-0007's sealed store, opened for reading
//! manifest      ADR-0009 D1; declared validators refuse, see below
//! session       ADR-0010 D1's directory, its three files, meta.toml
//! notice        ADR-0011 D2's line, once per session, at bare tier only
//! witness       ADR-0012 clause 3: the model is asked before the loop starts
//! turn          ADR-0008 D1's outer loop, with ADR-0009 D4's branch at None
//! line          ADR-0009 D4's recommendation, at the end of the first turn
//! exit          ADR-0016 D5
//! ```
//!
//! Every refusal before the session is created leaves `~/.zaru/sessions/`
//! untouched, which is [ADR-0010]'s own inode consequence: a turn that never
//! began is not a session.
//!
//! # The three codes this makes observable, and the one that needs a key
//!
//! [ADR-0016] D5 has six and the artefact reached four before this: `0`, `2`,
//! `4`, and `70` — the last from a corrupt transcript under `--resume`, which
//! is reachable on `main` and which two records say is not. This adds:
//!
//! - **`1`**, the work's own failure: a turn the model stopped, or one that
//!   reached [`TOOL_CALL_CEILING`](crate::cli::layers::TOOL_CALL_CEILING) with
//!   the model still asking for tools. D1 row 1 and D5's own "loop exhausted".
//!   **It needs a real model**, so on the artefact it is behind a key.
//! - **`3`**, environmental: a provider that answered `5xx` or a socket that
//!   never opened, read off the typed [`GeminiFailure`] the
//!   [`Classifying`] adapter kept.
//! - **`70` through the loop**, where a port failure whose class
//!   [ADR-0016]'s Update deliberately leaves unmapped is carried as a defect.
//!
//! # What is deliberately not here
//!
//! **No retry.** D4 has environmental failures retry with backoff, and no
//! record states a policy — [ADR-0016]'s clause 5 says the numbers "arrive from
//! the caller and neither has a default", and `providers::gemini` says a client
//! that retried on its own "would be answering that question silently". So a
//! provider failure ends the turn and says so, and clause 5 does not move.
//!
//! # One invocation is one turn; one *session* need not be
//!
//! [`task`] is the out-of-session surface and it is exactly what it was: one
//! process, one turn, one new session. What changed on 2026-09-05 is that the
//! order above is split at the line the session sits on — [`Prepared`] is
//! everything resolved *before* a session can exist, and [`run_one`] is the
//! act — so that the in-session shell can resolve once and run many turns over
//! one session, which is [ADR-0008] D1's "turns are the outer loop's unit".
//!
//! The cut is at that line for a reason of the records' rather than of
//! convenience: [ADR-0012] clause 3 asks the model whether it can call tools
//! **before the loop starts**, once, and a witness re-taken every turn would
//! be a different reading of that clause. Everything a turn may legitimately
//! rebuild — the allowlist, the child environment, the spawner, the fetcher,
//! the executor — stays in [`run_one`] and stays in its existing order, so
//! that every refusal happens exactly where it happened before.
//!
//! [`Owed`] is the other half of the split. [ADR-0011] D2's notice is stated
//! "once at session start" and [ADR-0002] D8's recommendation "fires at most
//! once ever": with one turn per process those were the same as once per call,
//! and in a session that holds a conversation they are not. So the two
//! take-once carriers are the **session's** and are handed to each turn.
//!
//! **And a session outlives the process it was opened in, since 2026-09-05.**
//! `Owed` is still built when a process opens, because a take-once value
//! cannot span one; what spans it is the transcript. Each line's emission
//! below appends a `crate::session::Record::Said`, [ADR-0010] D2's sixth
//! producer, and [`Owed::of`] takes the session's own reading of that as its
//! second argument. The counter is nowhere else: not in `context.json`, not
//! in `meta.toml`, not in a file of its own.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [`GeminiFailure`]: crate::providers::GeminiFailure

use crate::cli::classify::Surface;
use crate::cli::layers;
use crate::compose::{Classifying, ModelSummariser, Records, SessionContext, context, prose};
use crate::config::Resolution;
use crate::credentials::{CredentialStore, HarnessKeys, OsKeyring};
use crate::failure::{Classified, Exit, SessionEvidence};
use crate::providers::gemini::{Endpoint, GeminiClient};
use crate::providers::{ModelAlias, ModelTable, ProviderKind, ResolvedModel};
use crate::redaction::{HeldSecrets, held_secrets_for_redaction};
use crate::runtime::ResolvedTier;
use crate::session::{
    Checkpoint, Meta, MetaFile, MetaStore, SessionId, SessionStore, SystemWallClock, Transcript,
};
use crate::tools::{Executor, Mode, SessionNotice, WorkingDirectory};
use zaru_core::iteration::SystemClock;
use zaru_core::tool_call::{self, Outcome as TurnOutcome, Ports, Start, ToolCalling};

/// What one turn produced, as the binary writes it.
pub struct Ran {
    /// Standard output.
    pub lines: Vec<String>,
    /// [ADR-0016] D5's code.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    pub exit: Exit,
}

impl Ran {
    /// A refusal reached before anything was said.
    fn refused(classified: Classified) -> Self {
        Self {
            lines: Vec::new(),
            exit: Exit::Failed(classified),
        }
    }

    /// A refusal reached after the session had already said something.
    ///
    /// **[ADR-0011] D2's line is owed by a session that started**, whatever
    /// happens next: the sentence is stated "once at session start" and a turn
    /// that then failed at its provider still started. Dropping it would make
    /// the one tier that is not a sandbox say so only when the turn succeeded,
    /// which is exactly backwards.
    ///
    /// [`Outcome`](crate::cli::Outcome)'s own documentation carries the
    /// general form: "a failed run may still have lines -- a partial listing is
    /// worth more than nothing".
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    fn refused_having_said(lines: Vec<String>, classified: Classified) -> Self {
        Self {
            lines,
            exit: Exit::Failed(classified),
        }
    }
}

/// The kinds of [ADR-0012] D3's five this build carries a client for.
///
/// # No record maps an alias to a kind, and this does not invent one
///
/// That record's own Update says it in as many words: "**Nothing maps an alias
/// to a provider kind.** D4 resolves an alias to a model identifier and D3
/// names the kinds, and no clause says which kind serves which alias. The code
/// takes the kind as a parameter rather than inventing a key for it."
///
/// A composition has to take it from somewhere, and today the question does
/// not arise: **exactly one kind has a client**, so the kind that serves any
/// resolved model is that one and there is nothing to map. Inventing a
/// configuration key would settle the question ADR-0012 reserves, and guessing
/// from a model identifier's shape would be a rule about somebody else's
/// product names, wrong the first time a provider renames a model.
///
/// **The day a second client lands this array has two entries and the question
/// is real.** It is an array rather than a constant for exactly that reason:
/// the arc that adds the second client cannot do so without meeting this, and
/// what it has to answer is ADR-0012's own gap rather than something invented
/// here. Recorded on that record as a delegated coordinator ruling of
/// 2026-09-05, open to Jeshua's veto.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
pub const KINDS_WITH_A_CLIENT: [ProviderKind; 1] = [ProviderKind::Gemini];

/// Which of D3's five kinds this machine holds a key for.
///
/// A fact rather than a configuration key: the store keys a provider secret by
/// [`ProviderKind::credential_alias`], and `zaru providers keys` already lists
/// exactly this.
#[must_use]
pub fn kinds_held(store: &CredentialStore) -> Vec<ProviderKind> {
    ProviderKind::ALL
        .into_iter()
        .filter(|kind| {
            let alias = kind.credential_alias();
            store.records().any(|(held, _)| held == &alias)
        })
        .collect()
}

/// Everything one session's turns need that is resolved before a session can
/// exist.
///
/// # The cut is where the session is, and that is not an arbitrary line
///
/// The order in this module's documentation runs from the tier to the exit
/// code, and the session sits in the middle of it. Everything above that line
/// is a property of the machine, the project and the configuration — it does
/// not change between two turns of one session, and re-deriving it per turn
/// would re-ask [ADR-0012] clause 3's capability question, which that clause
/// puts **before the loop starts**.
///
/// Everything below the line is rebuilt per turn by [`run_one`], in the order
/// it was already in.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug)]
pub struct Prepared {
    tier: ResolvedTier,
    /// ADR-0009 D2's declared validators in dependency order. Empty where the
    /// project declared none, which is legal and is what `iterating` reads.
    plan: zaru_core::iteration::validator::Plan,
    /// ADR-0009 D4's branch, decided once for the session.
    iterating: bool,
    /// ADR-0001 D3's iteration ceiling for this run.
    ceiling: zaru_core::iteration::Ceiling,
    mode: Mode,
    model: crate::providers::ModelId,
    here: WorkingDirectory,
    manifest: Option<crate::manifest::Manifest>,
    kind: ProviderKind,
    held: HeldSecrets,
    client: GeminiClient,
    witness: ToolCalling,
    store_root: std::path::PathBuf,
}

impl Prepared {
    /// The tier this session resolved. [ADR-0001] D2.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    #[must_use]
    pub const fn tier(&self) -> ResolvedTier {
        self.tier
    }

    /// What [ADR-0012] D4's `default` alias resolved to, once, for the
    /// session.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    #[must_use]
    pub const fn model(&self) -> &crate::providers::ModelId {
        &self.model
    }

    /// [ADR-0011] D3's permission mode, resolved once for the session.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Which of [ADR-0012] D3's kinds is answering.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    #[must_use]
    pub const fn kind(&self) -> ProviderKind {
        self.kind
    }

    /// The one product [`Redactor`](zaru_core::redaction::Redactor), over
    /// every value the credential store holds.
    #[must_use]
    pub const fn redactor(&self) -> &HeldSecrets {
        &self.held
    }

    /// What the last exchange cost, for [ADR-0012] D7's status-line half.
    ///
    /// **The last exchange, and not the turn and not the session.** That is
    /// what `Provider::usage` reports — one slot every response replaces — and
    /// saying so here rather than at the call site is deliberate: this record's
    /// **proposed** Update of 2026-09-05 raises an accumulating total, is
    /// explicitly *not taken*, and is on this record's human-owned list. A
    /// caller that summed here would settle it silently.
    ///
    /// `None` before the first exchange, which is the client's own answer and
    /// not a zero invented on its behalf.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    #[must_use]
    pub fn usage(&self) -> Option<crate::providers::TokenUsage> {
        use crate::providers::Provider as _;
        self.client.usage()
    }

    /// Whether this project declared no manifest, so [ADR-0002] D8's line is
    /// owed.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    #[must_use]
    pub const fn manifest(&self) -> Option<&crate::manifest::Manifest> {
        self.manifest.as_ref()
    }

    /// The provider client this session's turns go through.
    ///
    /// Exposed so a surface with somewhere to paint can hand it a channel for
    /// the answer's text — see
    /// [`GeminiClient::stream_deltas_to`](crate::providers::gemini::GeminiClient::stream_deltas_to).
    /// A surface with no pane never calls it, and the client then builds no
    /// delta at all.
    #[must_use]
    pub const fn client(&self) -> &GeminiClient {
        &self.client
    }

    /// Where [ADR-0010] D1's session directory lives.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn store_root(&self) -> &std::path::Path {
        &self.store_root
    }
}

/// The two lines a **session** owes once, whatever its turns or its processes
/// do.
///
/// [ADR-0011] D2's notice is "stated once at session start" and [ADR-0002]
/// D8's event-anchored recommendation "fires at most once ever". Both are
/// take-once values, and holding them here rather than building them inside a
/// turn is what makes the scope the session's rather than the turn's.
///
/// **This value is still built when a process opens, and that is why it is
/// built from the transcript.** A take-once value cannot span a process, so
/// what spans it is [ADR-0010] D2's own record stream: each line's emission
/// appends a `crate::session::Record::Said`, and [`Owed::of`] starts the
/// carrier already spent when the session's transcript holds one. There is no
/// second store and nothing in the checkpoint.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug, Default)]
pub struct Owed {
    notice: Option<SessionNotice>,
    recommendation: Option<crate::manifest::MissingManifest>,
}

impl Owed {
    /// What this session owes, from what it resolved and from what it has
    /// already said.
    ///
    /// # This function decides nothing, and that is the point
    ///
    /// `said` is [`AlreadySaid`](crate::session::AlreadySaid), read off this
    /// session's own transcript by `crate::session::resume`. It is handed to
    /// the two constructors and **not consulted here**: each line's rule lives
    /// beside its carrier, because what "already said" means differs for them
    /// and so does what re-checks the condition each process. Deciding both
    /// here by one test is the shape [ADR-0002]'s Status tracking names as
    /// "two rules in one place", and it would be wrong for whichever line's
    /// own condition changed between two processes.
    ///
    /// A session being minted passes `AlreadySaid::none()`, which is a fact
    /// about a directory that has just been created rather than a default.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    #[must_use]
    pub fn of(prepared: &Prepared, said: &crate::session::AlreadySaid) -> Self {
        Self {
            notice: SessionNotice::for_tier_in_session(
                prepared.tier.tier(),
                prose::NOT_A_SANDBOX,
                said,
            ),
            recommendation: crate::manifest::MissingManifest::for_manifest_in_session(
                prepared.manifest.as_ref(),
                crate::failure::Statement::sanitised(prose::NO_VALIDATORS),
                crate::failure::Statement::sanitised(prose::DECLARE_ONE),
                said,
            ),
        }
    }

    /// Whether either line is still owed. A reader for a check, not a second
    /// copy of the state.
    #[must_use]
    pub fn anything_owed(&self) -> bool {
        self.notice.as_ref().is_some_and(SessionNotice::is_owed)
            || self
                .recommendation
                .as_ref()
                .is_some_and(crate::manifest::MissingManifest::is_owed)
    }
}

/// Resolve everything a session's turns need, or refuse.
///
/// Every refusal below is the one that was here before the split, in the order
/// it was in, and each leaves `~/.zaru/sessions/` untouched — this module's own
/// inode consequence: a turn that never began is not a session.
///
/// `version` and `report_at` are the binary's own package metadata, for
/// [ADR-0016] D3's report.
///
/// # Errors
///
/// The [`Ran`] the caller should return. Boxed for the reason
/// [`crate::terminal::open::shell_for`]'s error is: a [`Ran`] carries a whole
/// [ADR-0016] D1 classification and `clippy::result_large_err` refuses a
/// `Result` shaped that way.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[allow(
    clippy::too_many_lines,
    reason = "\
    the resolution is one dependency order and splitting it into functions \
    would put the order in the call graph rather than on the page, where the \
    only thing that makes it reviewable is reading it top to bottom"
)]
pub fn prepare(
    version: &str,
    report_at: &str,
    resolution: &Resolution,
) -> Result<Prepared, Box<Ran>> {
    let surface = Surface::new(version, report_at);

    // --- ADR-0001 D2's tier, resolved once ---------------------------------
    let tier = match ResolvedTier::from_configuration(resolution) {
        Ok(tier) => tier,
        Err(refusal) => return Err(Box::new(Ran::refused(surface.tier(&refusal)))),
    };

    // --- ADR-0011 D3's mode, resolved beside the tier and for its reason ---
    //
    // This read `Mode::default()` where the `Executor` is built until
    // 2026-09-05, with a comment saying no key was declared for it and that a
    // user could not change the mode from the terminal. `tools.mode` is that
    // key, and it is read **here** rather than there because a value the user
    // just typed must be answered before the harness complains about
    // something they did not: with the read at the `Executor`, `--mode fast`
    // on a machine with no model configured was silently discarded by the
    // model refusal above it, which is exactly ADR-0014 D5's "a typo that
    // silently does nothing is the worst outcome of any config system".
    //
    // Beside the tier, because they are the same kind of thing: two values
    // this turn resolves out of one configuration before it attempts
    // anything, each refused naming its own key. Unset is `Ask`, which is
    // D3's default rather than this composition's choice.
    //
    // Classified through `From<ModeRefused>` rather than through a `Surface`
    // arm of its own, which is what `ModelTable` above already does. The
    // taxonomy has carried a remedy **per arm** since it landed — the project
    // arm names the file and where the mode does belong, the misspelling arm
    // names the three values D3 defines — and a `Surface` arm here would have
    // replaced both with one sentence about `config explain`, which is a
    // worse answer to "I typed a word that is not a mode" than the record's
    // own vocabulary is.
    let mode = match Mode::from_configuration(resolution) {
        Ok(mode) => mode,
        Err(refusal) => return Err(Box::new(Ran::refused(Classified::from(refusal)))),
    };

    // --- ADR-0012 D4's model, through ADR-0014's five layers ---------------
    let model = match ModelTable::from_configuration(resolution) {
        Err(refusal) => return Err(Box::new(Ran::refused(Classified::from(refusal)))),
        Ok(table) => match table.row(ModelAlias::Default) {
            ResolvedModel::Unresolved => {
                return Err(Box::new(Ran::refused(
                    Surface::no_model_for_the_default_alias(),
                )));
            }
            ResolvedModel::Resolved { model, .. } => model.clone(),
        },
    };

    // --- ADR-0011 D4's boundary, canonicalised once ------------------------
    let here = match WorkingDirectory::of_this_process() {
        Ok(here) => here,
        Err(failure) => return Err(Box::new(Ran::refused(Surface::working_directory(&failure)))),
    };

    // --- ADR-0009 D1's manifest, and D4's branch ---------------------------
    //
    // Read before the session is created, so a project that cannot be served
    // is refused without writing a directory for a turn that never ran.
    let manifest_file =
        crate::manifest::ManifestFile::in_directory(here.clone(), layers::file_ceiling());
    let manifest = match manifest_file.parse() {
        Ok(manifest) => manifest,
        Err(refusal) => return Err(Box::new(Ran::refused(Surface::manifest(&refusal)))),
    };
    // ADR-0009 D2's dependency order, derived once and here, so a project
    // whose `after` list does not resolve is refused before a session
    // directory exists. `Plan::from_declared` refuses a cycle, an unknown
    // prerequisite and a duplicate name, all of which are the project's file
    // being wrong.
    let declared: Vec<_> = manifest
        .as_ref()
        .map(|manifest| manifest.validators().to_vec())
        .unwrap_or_default();
    let iterating = !declared.is_empty();
    let plan = match zaru_core::iteration::validator::Plan::from_declared(declared) {
        Ok(plan) => plan,
        Err(refusal) => return Err(Box::new(Ran::refused(Surface::validator_plan(&refusal)))),
    };

    // --- ADR-0007's store, and which kind this machine can reach -----------
    let store_root = match CredentialStore::default_root() {
        Ok(root) => root,
        Err(failure) => {
            return Err(Box::new(Ran::refused(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            )));
        }
    };
    let store = match CredentialStore::reading(store_root.clone()) {
        Ok(store) => store,
        Err(failure) => {
            return Err(Box::new(Ran::refused(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            )));
        }
    };
    // The one kind with a client, and only if this machine has its key. The
    // two refusals are different classes and the difference is what the user
    // can do: a machine with no key at all has a key to add, and a machine
    // holding keys only for kinds this build cannot reach has configured
    // something correctly that this build does not carry.
    let held_kinds = kinds_held(&store);
    let kind = match KINDS_WITH_A_CLIENT
        .into_iter()
        .find(|kind| held_kinds.contains(kind))
    {
        Some(kind) => kind,
        None if held_kinds.is_empty() => {
            return Err(Box::new(Ran::refused(Surface::no_key_for(
                &KINDS_WITH_A_CLIENT,
                &model,
            ))));
        }
        None => {
            return Err(Box::new(Ran::refused(
                surface.no_client_for_the_kinds_held(&model, &held_kinds),
            )));
        }
    };

    // --- ADR-0001 D3's ceiling, and ADR-0014 D3's key over it --------------
    //
    // Resolved here, before a session directory exists, because a ceiling
    // nobody can use is the project's or the user's configuration being wrong
    // and ADR-0010's own inode consequence is that a turn that never began is
    // not a session.
    let inference = match crate::providers::inference_of(resolution, ModelAlias::Default, kind) {
        Ok(inference) => inference,
        Err(refusal) => return Err(Box::new(Ran::refused(Classified::from(refusal)))),
    };
    let ceiling = match crate::runtime::ceiling_for(
        resolution,
        tier.tier(),
        inference,
        crate::providers::Placement::of(kind),
    ) {
        Ok(ceiling) => ceiling,
        Err(refusal) => {
            return Err(Box::new(Ran::refused(Surface::iteration_ceiling(&refusal))));
        }
    };

    // --- The key, and the redactor over everything the store holds ---------
    let keyring = OsKeyring::for_store(&store_root);
    let keys = HarnessKeys::from_process(&keyring);
    let alias = kind.credential_alias();
    let secret = match store.secret(&alias, &keys) {
        Ok(secret) => secret,
        Err(failure) => {
            return Err(Box::new(Ran::refused(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            )));
        }
    };
    // ADR-0008 clause 6's port, over every value the store holds -- including
    // the one this turn is about to send, which is why it is built from the
    // store rather than from the secret above.
    let held: HeldSecrets = match held_secrets_for_redaction(&store, &keys) {
        Ok(held) => held,
        Err(failure) => {
            return Err(Box::new(Ran::refused(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            )));
        }
    };

    // --- ADR-0012 D5's endpoint: configuration, then this kind's default ---
    let endpoint = match resolution.get(&kind.endpoint_key()) {
        Some(crate::config::Value::Text(configured)) => {
            match crate::providers::ProviderEndpoint::new(configured) {
                Ok(endpoint) => endpoint,
                Err(refusal) => {
                    return Err(Box::new(Ran::refused(Surface::endpoint(kind, &refusal))));
                }
            }
        }
        _ => Endpoint::default_endpoint(),
    };

    let client = match GeminiClient::new(endpoint, model.clone(), alias, secret) {
        Ok(client) => client,
        Err(failure) => return Err(Box::new(Ran::refused(Surface::provider(&failure)))),
    };

    // --- ADR-0012 clause 3: the model is asked before the loop starts ------
    //
    // **Here rather than in `run_one`, and that is the clause's own word.**
    // "The model is asked before the loop starts" -- once, for the session,
    // and not once per turn. The witness is `Copy`, so every turn of the
    // session is handed the same proof rather than re-taking it. It is asked
    // of the client directly because `Classifying` is a per-turn wrapper and
    // the question is not.
    let witness = match ToolCalling::required(&client, model.as_str()) {
        Ok(witness) => witness,
        Err(refusal) => {
            return Err(Box::new(Ran::refused(Surface::model_cannot_call_tools(
                &refusal,
            ))));
        }
    };

    Ok(Prepared {
        tier,
        plan,
        iterating,
        ceiling,
        mode,
        model,
        here,
        manifest,
        kind,
        held,
        client,
        witness,
        store_root,
    })
}
/// Run one turn of a session that already exists.
///
/// # What this half rebuilds, and why it is not in [`Prepared`]
///
/// Everything below is per turn because it *is* per turn: the allowlist is
/// read from configuration at the moment the turn asks, the child environment
/// and the spawner are the boundary this turn's commands start in, the
/// executor holds a `&mut` on the transcript for the length of one turn, and
/// `Classifying` keeps the last provider failure of one turn. The order is the
/// order it was in before the split, so every refusal happens where it
/// happened.
///
/// `n` is the turn's position in the session, which
/// [`zaru_core::tool_call::run`] says is "the caller's,
/// because a session spans many calls to this function and a number invented
/// here would restart at one every turn". [`task`] passes `1`; a shell counts
/// up.
///
/// `start` is what the turn is about, and it is the **caller's** for the same
/// reason `n` is. [`task`] passes [`Start::Task`]; a shell passes that for a
/// line the user typed and [`Start::Resumed`] for the one turn a session
/// resumed over an interrupted transcript owes the model first — [ADR-0010]
/// D4's "the model is told it did not complete". It is a required parameter
/// rather than a defaulted one so that the compiler names every call site that
/// should have been asked which of the two this is (library
/// [Verification lessons] §14).
///
/// `confirmer` is [ADR-0011] D3's `ask`. `None` refuses a call that needed one
/// rather than performing it, which is `Decision::permit`'s own rule.
///
/// `extra` is [ADR-0008] clause 3's second consumer. `run` "constructs each
/// event once and hands the same value to every registered sink in turn", so a
/// slice holding this session's transcript writer and a renderer is **one
/// emission reaching two consumers** — the clause is about the caller, and
/// this is the caller.
///
/// `owed` is the session's, not the turn's. See [`Owed`].
///
/// `context` is [ADR-0013]'s, and it is the **session's** too: a second turn
/// assembles over layer 6, which is what that record's own Status tracking
/// says becomes load-bearing "the day a session holds more than one turn".
/// Nothing here compacts — `Context::compact` takes `&mut self` and the policy
/// holds a shared borrow — so a context that will not fit refuses with D7's
/// own answer.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[must_use]
#[allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "\
    the composition is one dependency order and splitting it into functions \
    would put the order in the call graph rather than on the page, where the \
    only thing that makes it reviewable is reading it top to bottom; every \
    argument is a port or a value some record owns, and bundling them into a \
    struct would be a second name for the same list"
)]
pub async fn run_one(
    version: &str,
    report_at: &str,
    resolution: &Resolution,
    prepared: &Prepared,
    session: &crate::session::Session,
    n: u32,
    start: Start<'_>,
    confirmer: Option<&(dyn crate::tools::Confirm + Sync)>,
    extra: &mut [&mut dyn zaru_core::tool_call::EventSink],
    narrator: Option<&dyn crate::compose::Narrator>,
    owed: &mut Owed,
    context: &mut SessionContext,
) -> Ran {
    let surface = Surface::new(version, report_at);
    let evidence = session.evidence();
    let provider = Classifying::over(&prepared.client);

    let transcript_path = session.transcript_path();
    let mut transcript = match Transcript::append_to(session.transcript_path()) {
        Ok(transcript) => transcript,
        Err(failure) => return Ran::refused(Surface::transcript(&failure, evidence)),
    };
    let mut events = match Records::appending_to(session.transcript_path()) {
        Ok(events) => events,
        Err(failure) => return Ran::refused(Surface::transcript(&failure, evidence)),
    };

    let mut lines = Vec::new();

    // --- ADR-0011 D2's line, once, and only where it is true ---------------
    //
    // The notice is the **session's** now. "Stated once at session start" was
    // the same as once per call while one invocation was one session; it is
    // not once a session holds a conversation.
    //
    // **And a session outlives its process, since 2026-09-05.** The line is
    // pushed to the reader first and recorded second: a transcript that will
    // not take the record must not silence a statement about a missing
    // membrane, and ADR-0010 D2's "a crash loses at most the event in flight"
    // is what bounds the other order's cost. The failure ends the turn with
    // the line already said, exactly as the compaction record below it does.
    if let Some(notice) = owed.notice.as_mut()
        && let Some(sentence) = notice.state_once()
    {
        lines.push(sentence.clone());
        lines.push(String::new());
        if let Err(failure) =
            transcript.record(&crate::session::Record::Said(crate::session::Said {
                line: crate::session::SaidOnce::Notice,
                text: sentence,
            }))
        {
            return Ran::refused_having_said(lines, Surface::transcript(&failure, evidence));
        }
    }

    // --- ADR-0013 D2's turn boundary, before this turn assembles -----------
    //
    // This is the one place a compaction may happen, and it is reached once
    // per turn. On turn 1 layer 6 is empty and `Context::compact` returns
    // through its own threshold check without spending a model call; from
    // turn 2 it is the previous turns that fill it, which is what this
    // record's own Status tracking calls the day the threshold becomes
    // load-bearing.
    // **Awaited rather than blocked on, since 2026-09-05.** `context-summariser`
    // wrote this as `block_on(...)` because `run_one` was synchronous and the
    // only runtime in reach was the one it built for itself. `run_one` is a
    // future now -- the terminal races it against a beat and a keystroke -- so
    // a `block_on` here would be a second runtime started inside the first,
    // which tokio refuses at run time. It reached ADR-0016 D3's boundary as a
    // defect on six checks the moment the two changes met, and the fix is not
    // a nested runtime but no nesting at all: this is one sequence of awaits.
    let summariser = ModelSummariser::over(&provider, &prepared.held);
    let compaction = match context.at_turn_boundary(&summariser, &prepared.held).await {
        Ok(compaction) => compaction,
        Err(failure) => {
            return Ran::refused_having_said(
                lines,
                surface.summarisation(&failure, provider.taken().as_ref(), evidence),
            );
        }
    };
    // D3 and D4: announced once, with what it cost, before the turn they made
    // room for. ADR-0002 D3's interrupt channel -- this reports on a turn the
    // user's own message caused.
    for announced in &compaction.announcements {
        lines.push(format!(
            "{} {}",
            crate::cli::render::ANNOUNCEMENT_MARKER,
            crate::cli::render::announcement(announced)
        ));
        lines.push(String::new());
    }
    // D2: "the raw span stays in the transcript."
    if !compaction.announcements.is_empty()
        && let Err(failure) = transcript.record(&crate::session::Record::Compacted(compaction))
    {
        return Ran::refused_having_said(lines, Surface::transcript(&failure, evidence));
    }

    // --- ADR-0011's acting half, over every port it needs ------------------
    let mut overflow = crate::tools::SessionOverflow::in_session(session.directory());
    let allowlist = match crate::tools::Allowed::from_configuration(resolution) {
        Ok(allowlist) => allowlist,
        Err(refusal) => return Ran::refused_having_said(lines, Surface::allowlist(&refusal)),
    };
    let destructive = crate::tools::Shapes;
    let verdicts = crate::tools::NoMembrane;
    let environment = match crate::process::Environment::inherited_minimum() {
        Ok(environment) => environment,
        Err(refusal) => {
            return Ran::refused_having_said(lines, Surface::child_environment(&refusal));
        }
    };
    let spawn = crate::process::Spawn::new(&prepared.here, environment, layers::process_ceiling());
    let fetch = match crate::web::WebClient::new(layers::fetch_bounds()) {
        Ok(fetch) => fetch,
        Err(refusal) => {
            return Ran::refused_having_said(lines, Surface::web_client(&refusal));
        }
    };

    let executor = Executor {
        working_directory: &prepared.here,
        // ADR-0011 D3's mode, resolved once for the session rather than per
        // turn. `mode-key` put the read where `prepare` now is, and that is
        // where it belongs: the mode is a property of the configuration a
        // session opened under, and a value that could change between two
        // turns of one conversation would make a permission decision depend
        // on when inside the session it was asked.
        mode: prepared.mode,

        allowlist: &allowlist,
        destructive: &destructive,
        confirmer,
        verdicts: &verdicts,
        budget: layers::output_budget(),
        search_ceiling: layers::search_ceiling(),
        overflow: &mut overflow,
        transcript: &mut transcript,
        redactor: &prepared.held,
        subprocess: &spawn,
        fetch: &fetch,
    };

    // --- ADR-0013's context, assembled once inside the turn ----------------
    let clock = SystemClock::started_now();
    let outcome = {
        let policy = context.policy(&prepared.held, prepared.iterating);
        // ADR-0008's execution, decided 2026-09-05: one tool surface, reached
        // by both loops. See `crate::compose::shared` for why it is a lock and
        // why sharing the value rather than building a second one is what
        // makes "a candidate cannot do what a turn cannot" a property.
        let tool_surface = tokio::sync::Mutex::new(executor);
        let mut tools = crate::compose::Shared::over(&tool_surface);

        // --- ADR-0009 D4's inner loop, over the same surface ---------------
        //
        // Built whichever way the branch goes, because `tool_call::run` takes
        // one `I` and two arms with two types would not compile. Only the
        // `Some` arm ever runs it: an empty plan reports nothing, and a loop
        // whose validators report nothing succeeds on its first iteration
        // having checked nothing, which is ADR-0009 D2's silent green.
        let patterns = crate::validators::Patterns::new(layers::pattern_ceiling());
        let schemas = crate::validators::SchemaFiles::new(&prepared.here, layers::file_ceiling());
        let dispatch = zaru_core::iteration::validator::Dispatch::new(
            &prepared.plan,
            &spawn,
            &patterns,
            &schemas,
        );
        let generating = crate::compose::Generating::over(&provider);
        let applying = crate::compose::Applying::through(tools);
        let inner = crate::compose::Inner::over(
            zaru_core::iteration::Ports {
                generator: &generating,
                executor: &applying,
                validators: &dispatch,
                context: &policy,
                clock: &clock,
                redactor: &prepared.held,
            },
            zaru_core::iteration::Limits {
                ceiling: prepared.ceiling,
                // ADR-0008 D4's head-and-tail budget. **ADR-0011 D5's budget
                // and not a second number**, ruled 2026-09-05 under directive
                // 20: one budget bounds captured bytes on their way into a
                // prompt, and a second constant here would be a second answer
                // to one question.
                budget: zaru_core::iteration::TruncationBudget::new(layers::output_budget().get())
                    .expect("ADR-0011 D5's budget is refused at zero by its own constructor"),
            },
            &transcript_path,
            // ADR-0028 D3's subscriber, when there is a terminal to subscribe.
            // `None` is `zaru "<task>"`, which writes the transcript and
            // prints an outcome and has no pane to paint.
            narrator,
        );

        // ADR-0008 clause 3's slice. The transcript writer first, so that the
        // record on disk is written before anything renders it -- a renderer
        // that painted an event the file does not hold would be showing the
        // user something a resume could not reproduce.
        let mut sinks: Vec<&mut dyn zaru_core::tool_call::EventSink> = vec![&mut events];
        for sink in extra.iter_mut() {
            sinks.push(&mut **sink);
        }
        let ran = tool_call::run(
            n,
            start,
            layers::tool_call_ceiling(),
            prepared.witness,
            Ports {
                model: &provider,
                tools: &mut tools,
                context: &policy,
                clock: &clock,
                redactor: &prepared.held,
            },
            // ADR-0009 D4's branch: "A project with no `zaru.toml` runs the
            // tool-call loop only."
            prepared.iterating.then_some(&inner),
            &mut sinks,
        )
        .await;
        (ran, inner.kept())
    };
    let (outcome, kept) = outcome;

    // A transcript that lost an event has not recorded what happened, whatever
    // the loop returned, and ADR-0010 D2 makes this file the replayable record.
    // Both sinks are read, because an iteration's events go through the
    // inner loop's own handle on the same file.
    if let Some(failure) = events.first_failure() {
        return Ran::refused_having_said(lines, Surface::transcript(failure, evidence));
    }
    if let Some(failure) = kept.transcript.as_ref() {
        return Ran::refused_having_said(lines, Surface::transcript(failure, evidence));
    }

    let mut ran = match outcome {
        Ok(outcome) => rendered(&provider, &outcome, &mut lines),
        Err(error) => {
            // The class of an inner-loop failure is the **inner** port's, off
            // the typed error `Inner` kept -- see `crate::compose::iterate`
            // for why it is kept rather than carried through a port that has
            // nowhere to put it.
            let classified = match kept.error.as_ref() {
                Some(inner) => surface.inner_loop(inner, provider.taken().as_ref(), evidence),
                None => surface.turn(&error, provider.taken().as_ref(), evidence),
            };
            return Ran::refused_having_said(lines, classified);
        }
    };

    // --- ADR-0009 D4's line, at the end of the turn that caused it ---------
    //
    // ADR-0002 D8's event-anchored kind, settled under directive 20: appended
    // to the end of the triggering turn rather than stated at session start,
    // because "a manifest is absent before the user does anything" and a line
    // at session start would be a timer wearing a costume by D8's own test.
    // It is the session's take-once value, so a second turn does not repeat it.
    // It is recorded here too, for the same reason and in the same order: the
    // line reaches the reader first, and ADR-0010 D2's sixth producer is what
    // makes "at most once ever" outlive this process. `transcript`'s borrow by
    // the executor ended with the block above, so the handle is this scope's
    // again and the file still has exactly one writer.
    if let Some(recommendation) = owed.recommendation.as_mut()
        && let Some(recommendation) = recommendation.state_once()
    {
        let line = recommendation.to_string();
        ran.lines.push(String::new());
        ran.lines.push(line.clone());
        if let Err(failure) =
            transcript.record(&crate::session::Record::Said(crate::session::Said {
                line: crate::session::SaidOnce::Recommendation,
                text: line,
            }))
        {
            return Ran::refused_having_said(ran.lines, Surface::transcript(&failure, evidence));
        }
    }

    ran
}

/// Run one turn for `task` in a session of its own, and say what the process
/// exits with.
///
/// [ADR-0010] D1's session, minted: the directory, its three files, and the
/// context the first of them holds.
///
/// # One minting, because there are two callers now
///
/// This was inline in [`task`] until 2026-09-06, when a bare `zaru` at a
/// terminal became the second thing that starts a session — see
/// [`crate::terminal::open`]. Two mintings would be two `meta.toml` writers
/// and two first checkpoints, agreeing until a field was added to one.
///
/// **The provider is an `Option`, and that is what lets a terminal open a
/// session it cannot run a turn in.** `task` always has one, because it
/// refuses before it mints; a shell opens whether or not a provider resolved,
/// exactly as `--resume` already does, and a session with none records `None`
/// rather than a kind nobody chose. D1 makes that field optional for this
/// reason.
///
/// The context is built here rather than by the caller because [ADR-0010] D3's
/// checkpoint holds it: `session::checkpoint` writes an opaque value it never
/// interprets, and [`SessionContext`] is the one type that knows what goes in
/// it. On a session that has not had a turn that is an empty layer 6, and it
/// is that shape because the type says so rather than because a literal agrees
/// with the type by hand.
///
/// # Errors
///
/// The classified refusal for a store that cannot be reached, a ULID that
/// cannot be minted, or a `meta.toml` or checkpoint that cannot be written.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
pub fn start(
    root: std::path::PathBuf,
    tier: ResolvedTier,
    provider: Option<ProviderKind>,
    here: &std::path::Path,
    surface: &Surface<'_>,
) -> Result<(crate::session::Session, SessionContext), Box<crate::failure::Classified>> {
    let session_store = SessionStore::open(root).map_err(|failure| surface.session(&failure))?;
    let id = SessionId::mint(&SystemWallClock)
        .map_err(|failure| Box::new(Surface::session_not_started(&failure)))?;
    let session = session_store
        .start(id.clone())
        .map_err(|failure| surface.session(&failure))?;
    let evidence = session.evidence();
    let meta = Meta::new(
        tier,
        // ADR-0001 D1 gives `bare` no cortex, and nothing here attaches one.
        None,
        provider.map(|kind| kind.to_string()),
        // ADR-0010 D4's `--continue` scope, and it is ADR-0011 D4's boundary
        // rather than a second reading of the process: the caller canonicalised
        // it once, and a session that recorded a different answer to one
        // question would be found under one path and written under another.
        here.to_path_buf(),
        id.minted_at(),
    );
    MetaFile::at(session.meta_path())
        .write(&meta)
        .map_err(|failure| Box::new(Surface::meta(&failure, evidence.clone())))?;
    let context = SessionContext::opened(context::prefix_for(), layers::context_limits());
    Checkpoint::at(session.checkpoint_path())
        .write(&context.checkpoint())
        .map_err(|failure| Box::new(Surface::checkpoint(&failure, evidence.clone())))?;
    // **D1's third file, created empty and closed again.**
    //
    // Clause 1 is "a session directory is created with the three files", and
    // until 2026-09-06 that was true only *after* a turn: the transcript was
    // created by the first `Transcript::append_to` the turn made, so a session
    // held two files until something was written to it. That was unobservable
    // while every session ran a turn immediately. A bare `zaru` at a terminal
    // can be opened and left, so it is observable now, and the clause is what
    // decides it rather than the convenience.
    //
    // `append_to` creates or opens; dropping the handle leaves the file at
    // `FILE_MODE` with nothing in it, which is what a session that has said
    // nothing has. The turn's own `append_to` then opens the same file and
    // appends, so nothing else changes.
    drop(
        Transcript::append_to(session.transcript_path())
            .map_err(|failure| Box::new(Surface::transcript(&failure, evidence)))?,
    );
    Ok((session, context))
}

/// The out-of-session surface, and it is exactly what it was: resolve, open a
/// session, run turn one. `version` and `report_at` are the binary's own
/// package metadata, for [ADR-0016] D3's report.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[must_use]
pub fn task(version: &str, report_at: &str, resolution: &Resolution, task: &str) -> Ran {
    let surface = Surface::new(version, report_at);
    let prepared = match prepare(version, report_at, resolution) {
        Ok(prepared) => prepared,
        Err(refused) => return *refused,
    };
    // A session this call is about to mint has said nothing, and that is a
    // fact about a directory that does not exist yet rather than a default.
    let mut owed = Owed::of(&prepared, &crate::session::AlreadySaid::none());

    // --- ADR-0010 D1's session, and the first `meta.toml` a product writes --
    let (session, mut context) = match start(
        prepared.store_root.clone(),
        prepared.tier,
        Some(prepared.kind),
        prepared.here.root(),
        &surface,
    ) {
        Ok(started) => started,
        Err(refused) => return Ran::refused(*refused),
    };
    let evidence = session.evidence();

    // ADR-0011 D3's prompt over the terminal. `None` when standard input is
    // not one, at which point a call that needed a confirmation is refused
    // rather than performed -- which is `Decision::permit`'s own rule and the
    // reason this is an `Option` rather than a stub that answers yes.
    let confirmer = crate::tools::prompt::Prompt::from_process();

    // ADR-0013 D1's layer 6, read off ADR-0008 clause 3's own emission. This
    // turn had no such sink until 2026-09-05, which is why the checkpoint it
    // left was the empty one written above: a session created here recorded
    // no exchange at all, so resuming it restored nothing however much had
    // been said. See `crate::compose::ToolLines`.
    let mut tools = crate::compose::ToolLines::default();

    let ran = block_on(run_one(
        version,
        report_at,
        resolution,
        &prepared,
        &session,
        1,
        Start::Task(task),
        confirmer
            .as_ref()
            .map(|prompt| prompt as &(dyn crate::tools::Confirm + Sync)),
        &mut [&mut tools],
        // ADR-0028 D3's subscriber, and there is no pane here: `zaru "<task>"`
        // writes the transcript and prints an outcome. The narrative is on
        // disk and `--resume` renders it; nothing paints it as it happens.
        None,
        &mut owed,
        &mut context,
    ));

    // --- ADR-0013 D1's layer 6 and ADR-0010 D3's checkpoint over it --------
    //
    // The same two acts `crate::terminal::driver::run_a_turn` performs, in
    // the same order, through the same two functions. D3's "overwritten each
    // turn" is about every turn, and this is one — a session whose only turn
    // ran here is exactly the session a later `--resume` opens.
    context.record(crate::compose::boundary::exchange_of_turn(
        prepared.redactor(),
        task,
        &tools.taken(),
        &ran.lines.join("\n"),
    ));
    if let Err(failure) = crate::compose::boundary::checkpointed(&context, &session) {
        // The turn happened and `ran` already carries what the user is told,
        // so this is appended rather than replacing it: reporting only the
        // checkpoint failure would discard the answer, which is ADR-0016 D6's
        // "partial success is reported as partial".
        return Ran::refused_having_said(ran.lines, Surface::checkpoint(&failure, evidence));
    }

    ran
}

/// What the reader is shown, and what the process exits with.
///
/// [ADR-0012] D7's "per session on exit" is the usage line, read off the
/// provider rather than recomputed: the client reports what it was told and
/// nothing here adds a cost, because "nothing publishes any" pricing.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
fn rendered(provider: &Classifying<'_>, outcome: &TurnOutcome, lines: &mut Vec<String>) -> Ran {
    use crate::providers::Provider as _;

    let mut lines = core::mem::take(lines);
    let exit = match outcome {
        TurnOutcome::Answered { text, .. } => {
            lines.push(text.clone());
            Exit::Succeeded
        }
        // ADR-0016 D5's `1`: "the work failed". D1 row 1 calls the same row
        // "not an error -- this is the loop working", and both are true: the
        // mechanism operated and the outcome is what a wrapping CI job needs.
        TurnOutcome::Stopped { reason, rounds, .. } => {
            Exit::Failed(Surface::turn_stopped(reason, *rounds))
        }
        TurnOutcome::Exhausted { rounds, calls, .. } => {
            Exit::Failed(Surface::turn_exhausted(*rounds, *calls))
        }
        // ADR-0009 D4's branch was taken: this turn's body was an iteration.
        // ADR-0008 D5 puts exhaustion in a register of its own, so a loop that
        // ran and did not satisfy the validators is `Expected` at ADR-0016
        // D5's `1` and never the error register.
        TurnOutcome::Iterated(zaru_core::iteration::Outcome::Succeeded { iterations, .. }) => {
            lines.push(format!(
                "the declared validators are satisfied after {iterations} iteration(s)"
            ));
            Exit::Succeeded
        }
        TurnOutcome::Iterated(zaru_core::iteration::Outcome::Exhausted {
            iterations,
            reason,
            last_failure,
        }) => Exit::Failed(Surface::loop_exhausted(
            *iterations,
            *reason,
            last_failure.as_deref(),
        )),
    };
    if let Some(usage) = provider.client().usage() {
        lines.push(String::new());
        lines.push(crate::cli::render::usage(&usage));
    }
    Ran { lines, exit }
}

/// A current-thread runtime, built once by whoever is going to poll a turn.
///
/// **One per session rather than one per turn, since 2026-09-05.** This
/// function used to build a runtime, poll one turn and drop it, which a shell
/// holding a conversation did once for every turn the user typed. It also made
/// [`run_one`] impossible to race against anything, because the runtime was
/// *inside* it: a caller that wanted the turn and a terminal at once had no
/// future to select over. `run_one` is `async` now and this is what a caller
/// drives it with -- [`task`] builds one for its single turn, and
/// [`crate::terminal::open`] builds one for the session and reuses it.
///
/// # Errors
///
/// When the runtime cannot be built, which is the reactor failing to register
/// with the operating system.
pub fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

/// Poll a future to completion on a current-thread runtime.
///
/// # A reactor is what `reqwest` needs and what nothing here had
///
/// [ADR-0009]'s Status tracking predicted this: "`zaru-cli`'s product tree
/// carries no async runtime, and taking one is a reactor in the binary, which
/// is a record-level decision rather than an import … it belongs to whichever
/// arc has a reason — a provider client, most likely, since D3's streaming
/// needs one anyway."
///
/// This is that arc and the reason is `reqwest`: its futures are driven by
/// tokio's I/O driver and do nothing at all without one. `tokio` has been a
/// **product** dependency of this crate since the `gemini` client landed, and
/// [ADR-0003] D2's table has named it throughout, so this is a caller arriving
/// rather than a dependency being added — that record's clause 7's own
/// distinction. What changed in the manifest is two features, `net` and
/// `time`, which `reqwest` already enables on the same crate: measured, the
/// lock and the build tree are unchanged.
///
/// **Current-thread rather than multi-thread.** One turn is one sequence of
/// awaits and nothing here is concurrent, so a thread pool would be threads
/// nobody uses; and the transcript's two handles are safe precisely because
/// the turn runs on one thread.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
fn block_on<F: core::future::Future>(future: F) -> F::Output {
    runtime()
        .expect("a current-thread runtime with the io and time drivers")
        .block_on(future)
}
