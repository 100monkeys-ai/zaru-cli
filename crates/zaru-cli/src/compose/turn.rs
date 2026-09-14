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
use crate::providers::ProviderClient;
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
/// # The order is landing order, and appending is what keeps it harmless
///
/// **Not [`ProviderKind::ALL`]'s order**, which puts `ollama` before `gemini`.
/// This array is the order clients landed in, and a third one **appends**
/// rather than inserting — which is a decision about who it can affect rather
/// than about tidiness.
///
/// Part 2 of [`crate::providers::select`] takes the first kind whose
/// requirement holds, so the order *is* the tie-break on a machine where two
/// hold. Appending cannot change any machine's present answer: one that
/// resolves `gemini` today resolves `gemini` tomorrow, one that resolves
/// `ollama` still does, and the only machine whose behaviour moves is one that
/// had **no** answer at all — no Gemini key and no `ollama` endpoint — and now
/// has one. Inserting would silently move a working machine to a different
/// provider, which is a change nobody asked for arriving in a release note
/// nobody wrote.
///
/// `the_client_bearing_kinds_are_in_landing_order_so_a_later_one_cannot_displace_an_earlier`
/// asserts it, and its name says why, so an insertion reddens rather than
/// quietly re-tie-breaking.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
pub const KINDS_WITH_A_CLIENT: [ProviderKind; 3] = [
    ProviderKind::Gemini,
    ProviderKind::Ollama,
    ProviderKind::OpenAiCompatible,
];

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
    client: ProviderClient,
    witness: ToolCalling,
    store_root: std::path::PathBuf,
    /// [ADR-0012] D3's window for the kind that answered, in tokens.
    ///
    /// Not `Option`: `prepare` refuses a kind that could not state one, so a
    /// `Prepared` that exists has a window.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    window: u64,
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

    /// [ADR-0013]'s window and pressure threshold, for the kind that
    /// answered.
    ///
    /// **This is the whole of what replaced two constants in
    /// `crate::cli::layers`.** Those were one model's numbers — Google's
    /// 1,048,576 and three quarters of it — applied to every provider,
    /// which was already wrong for two of the three kinds with a client. The
    /// window now comes from [ADR-0012] D3's capability descriptor, per kind
    /// and from that kind's own source, and the threshold is three quarters
    /// of *it*.
    ///
    /// Three quarters is unchanged and is still nobody's published number:
    /// [ADR-0013] D2 crosses "the window pressure threshold" and names none,
    /// and this is the one place the fraction is written.
    ///
    /// # Panics
    ///
    /// Never. `require_context_size` refuses zero's only source — a
    /// descriptor with no window — before a `Prepared` exists, and three
    /// quarters of a non-zero window is neither zero nor above it.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    #[must_use]
    pub fn context_limits(&self) -> zaru_core::context::ContextLimits {
        crate::cli::layers::context_limits(self.window)
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
    pub const fn client(&self) -> &ProviderClient {
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
    // --- Which kind serves this alias -------------------------------------
    //
    // **A proposed reading of 2026-09-14, settled in code nowhere**; the rule
    // and its reasoning are `crate::providers::selection`'s, and the amendment
    // was written on ADR-0012 before this line existed.
    //
    // This block chose the first kind with a client that the store held a key
    // for, and that was a complete answer while every kind with a client
    // needed a key. `ollama` needs none, so credential presence alone can no
    // longer express the question: an explicit `provider.<alias>.kind` decides
    // it, and absent that key each kind is asked for its own requirement.
    let held_kinds = kinds_held(&store);
    let configured_kind = match resolution.get(&crate::providers::kind_key(ModelAlias::Default)) {
        Some(crate::config::Value::Text(named)) => match ProviderKind::parse(named) {
            Some(kind) => Some(kind),
            // A kind nobody can spell is the user's to fix, and naming the
            // five is what makes it fixable.
            None => {
                return Err(Box::new(Ran::refused(Surface::unknown_provider_kind(
                    ModelAlias::Default,
                    named,
                ))));
            }
        },
        _ => None,
    };
    let kind = match crate::providers::select(
        ModelAlias::Default,
        configured_kind,
        &KINDS_WITH_A_CLIENT,
        |kind| held_kinds.contains(&kind),
        // "An endpoint set at any layer other than the built-in default":
        // nothing declares a layer-1 value for this key, so a value resolving
        // at all is a value a user set.
        |kind| {
            matches!(
                resolution.get(&kind.endpoint_key()),
                Some(crate::config::Value::Text(_))
            )
        },
    ) {
        Ok(kind) => kind,
        // The two refusals are different classes and the difference is what
        // the user can do: a machine holding keys only for kinds this build
        // cannot reach has configured something correctly that this build does
        // not carry, and a machine with nothing at all has two routes out.
        Err(refusal) if !held_kinds.is_empty() => {
            drop(refusal);
            return Err(Box::new(Ran::refused(
                surface.no_client_for_the_kinds_held(&model, &held_kinds),
            )));
        }
        // Nothing on this machine says who should answer. The refusal names
        // both routes out -- a stored key, or a configured keyless kind --
        // because since 2026-09-14 there are two.
        Err(refusal) => {
            drop(refusal);
            return Err(Box::new(Ran::refused(Surface::no_key_for(
                &KINDS_WITH_A_CLIENT,
                &model,
            ))));
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

    // --- The key, where this kind needs one, and the redactor always -------
    //
    // **The key is read only for a kind whose requirement is a held key.**
    // Before 2026-09-14 every kind with a client needed one, so this was
    // unconditional; `ollama` is reached with no credential at all, and
    // reading a secret that does not exist would refuse a provider that works.
    let keyring = OsKeyring::for_store(&store_root);
    let keys = HarnessKeys::from_process(&keyring);
    let alias = kind.credential_alias();
    // **Read off `KeyUse` rather than `Requirement` since 2026-09-14.** The two
    // answered one question while every kind either needed a key or took none;
    // `openai-compatible` is selected by an endpoint AND sends a key when one
    // is held, so the requirement can no longer say whether to read the store.
    let secret = match crate::providers::KeyUse::of(kind) {
        crate::providers::KeyUse::Required => match store.secret(&alias, &keys) {
            Ok(secret) => Some(secret),
            Err(failure) => {
                return Err(Box::new(Ran::refused(
                    surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                )));
            }
        },
        // **A key only if this machine has one, and its absence is not a
        // failure.** The store is asked only when `kinds_held` already said the
        // alias is there, so a reader with a local server never meets a
        // credential refusal for a credential they were never asked for.
        crate::providers::KeyUse::Optional if held_kinds.contains(&kind) => {
            match store.secret(&alias, &keys) {
                Ok(secret) => Some(secret),
                Err(failure) => {
                    return Err(Box::new(Ran::refused(
                        surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                    )));
                }
            }
        }
        crate::providers::KeyUse::Optional | crate::providers::KeyUse::Never => None,
    };
    // ADR-0008 clause 6's port, over every value the store holds -- built from
    // the store rather than from the secret above, which is why it is
    // unconditional even for a kind that sends none: the harness's OTHER
    // secrets must still not reach a model, and a local provider is not a
    // reason to relax that.
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
        // Each kind's own default, which D5 leaves to whoever proposes one.
        // A wildcard-free match, so a kind that gains a client without
        // proposing a default fails to compile here rather than silently
        // borrowing another provider's origin.
        _ => match kind {
            ProviderKind::Gemini => Endpoint::default_endpoint(),
            ProviderKind::Ollama => crate::providers::ollama::Endpoint::default_endpoint(),
            // **This kind has a client and deliberately no default**, which is
            // why it is the one arm here that is reachable in a working build.
            // `providers::openai_compatible::endpoint` carries the argument:
            // the kind covers vLLM, LM Studio, llama.cpp, Ollama's own `/v1`
            // and every hosted gateway, whose origins differ with no majority,
            // so a default would be one vendor's port painted on all of them.
            //
            // It is reached only through an explicit `provider.<alias>.kind`,
            // since part 2 of the selection would not have chosen a kind whose
            // endpoint is unset -- so the reader said "use this one" and the
            // one thing left to tell them is where.
            ProviderKind::OpenAiCompatible => {
                return Err(Box::new(Ran::refused(Surface::endpoint_not_set(kind))));
            }
            ProviderKind::Anthropic | ProviderKind::Aegis => {
                return Err(Box::new(Ran::refused(
                    surface.no_client_for_the_kinds_held(&model, &held_kinds),
                )));
            }
        },
    };

    // --- ADR-0012 D3's window: configuration, then this kind's own source --
    //
    // Read exactly the way the endpoint above is read, and for its reason:
    // `provider.<kind>.context_tokens` is this record's key and ADR-0014's
    // Neutral section leaves each record its own. What differs is where an
    // unset key lands. `gemini` and `ollama` have a built-in row in layer 1
    // -- `cli::layers::BuiltIn` -- so the `unwrap_or` below is unreachable
    // while that row exists, and is written rather than `expect`ed because
    // the row and this read live in different modules and a panic here would
    // be this harness reporting its own disagreement as a crash.
    // `openai-compatible` has no row, so `None` reaches its descriptor and
    // `require_context_size` refuses it by name.
    let configured_window = match resolution.get(&kind.context_tokens_key()) {
        Some(crate::config::Value::Integer(tokens)) => u64::try_from(*tokens).ok(),
        _ => None,
    };

    // A wildcard-free match, so a third client cannot be added to
    // `KINDS_WITH_A_CLIENT` without being built here.
    let client = match (kind, secret) {
        (ProviderKind::Gemini, Some(secret)) => {
            match GeminiClient::new(
                endpoint,
                model.clone(),
                alias,
                secret,
                configured_window.unwrap_or(crate::providers::gemini::CONTEXT_WINDOW_TOKENS),
            ) {
                Ok(client) => ProviderClient::Gemini(client),
                Err(failure) => {
                    return Err(Box::new(Ran::refused(Surface::provider(&failure.into()))));
                }
            }
        }
        (ProviderKind::Ollama, None) => {
            match crate::providers::ollama::OllamaClient::new(
                endpoint,
                model.clone(),
                configured_window
                    .unwrap_or(crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS),
            ) {
                Ok(client) => ProviderClient::Ollama(client),
                Err(failure) => {
                    return Err(Box::new(Ran::refused(Surface::provider(&failure.into()))));
                }
            }
        }
        // **The one arm that takes either**, because this kind's key is
        // optional: `Some` for a gateway whose key this machine holds, `None`
        // for a local server that wants none, and the client is the same
        // client. See `providers::selection::KeyUse`.
        (ProviderKind::OpenAiCompatible, secret) => {
            match crate::providers::openai_compatible::OpenAiCompatibleClient::new(
                endpoint,
                model.clone(),
                alias,
                secret,
                configured_window,
            ) {
                Ok(client) => ProviderClient::OpenAiCompatible(client),
                Err(failure) => {
                    return Err(Box::new(Ran::refused(Surface::provider(&failure.into()))));
                }
            }
        }
        // Unreachable while `Requirement::of` and `KINDS_WITH_A_CLIENT` agree:
        // the selection above returns only a kind with a client, and the
        // secret above is `Some` exactly for a kind whose requirement is a
        // held key. Written as a refusal rather than an `expect` because the
        // two facts live in different modules and a panic here would be this
        // harness reporting its own disagreement as a crash.
        _ => {
            return Err(Box::new(Ran::refused(
                surface.no_client_for_the_kinds_held(&model, &held_kinds),
            )));
        }
    };

    // --- ADR-0013's window, refused before anything is built on it --------
    //
    // **Beside the tool-calling witness below and ahead of it**, because a
    // window nobody states is the worse of the two to discover late: a
    // provider that cannot call tools says so, and a provider whose window
    // this harness guessed says nothing at all -- it accepts the request and
    // truncates it, and what the reader sees is a model that forgot something
    // they remember saying. The descriptor is D3's data and this is the one
    // place it is consulted for this concern.
    let window = match crate::providers::Provider::capabilities(&client)
        .require_context_size(ModelAlias::Default, kind)
    {
        Ok(tokens) => tokens,
        Err(refusal) => {
            return Err(Box::new(Ran::refused(refusal.into())));
        }
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
        window,
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
async fn ran(
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

    // --- ADR-0010 D2's seventh producer, the user's half -------------------
    //
    // Written **here**: after the two records above and before anything the
    // loop emits, so that `cat transcript.jsonl` reads in the order the turn
    // happened -- the question, then `turn_started`, then the work. It is also
    // the last moment it can be written at all, because `Executor` below
    // borrows `transcript` for the length of the turn.
    //
    // **Before the loop rather than beside the answer, and that is what makes
    // an interruption legible.** A killed process writes nothing, so a `user`
    // half with no `zaru` half after it *is* the interruption -- the mechanism
    // `Phase::Started` already carries for a tool call, and the only one a
    // reader can be given. A turn that merely *stopped* stays distinguishable,
    // because it has a `turn_ended` record and an interrupted one does not.
    //
    // **Deferring it costs more than the order, which the mutation showed and
    // this comment did not predict.** Moving the write to sit beside the
    // answer was expected to reverse two records; what it actually did was
    // lose the question altogether, because every refusal between here and
    // there returns through `Ran::refused_having_said` and never reaches that
    // point. So a turn that failed -- the turn a person is most likely to
    // read back -- would have recorded neither half. Measured 2026-09-06
    // against a closed provider endpoint.
    //
    // **A resumed turn writes nothing.** `Start::Resumed` carries no task --
    // "the work and the conversation are what the policy restored" -- and
    // minting a user line for it would be the harness putting words in the
    // person's mouth.
    if let Start::Task(task) = start
        && let Err(failure) = transcript.record(&crate::compose::boundary::spoken_by_the_user(
            &prepared.held,
            n,
            task,
        ))
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

    let (mut ran, answer) = match outcome {
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

    // --- ADR-0010 D2's seventh producer, the harness's half ----------------
    //
    // After the turn and before ADR-0009 D4's line, so the recommendation's
    // own `Record::Said` stays last on the file exactly as it was.
    //
    // **A turn that did not answer writes nothing**, and that absence is not a
    // gap: `Event::TurnEnded` already carries how the turn finished, and the
    // pane renders it as `turn 3 stopped without an answer`. Writing an empty
    // `zaru` half would be a claim that the harness said something.
    if let Some(answer) = &answer
        && let Err(failure) = transcript.record(&crate::compose::boundary::spoken_by_zaru(
            &prepared.held,
            n,
            answer,
        ))
    {
        return Ran::refused_having_said(ran.lines, Surface::transcript(&failure, evidence));
    }

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
    workspace: Option<String>,
    here: &std::path::Path,
    limits: zaru_core::context::ContextLimits,
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
        // ADR-0006 D5's pin, resolved by the caller out of ADR-0009 D1's
        // `project.workspace`. The comment here said "ADR-0001 D1 gives
        // `bare` no cortex, and nothing here attaches one" and passed `None`
        // unconditionally, which made ADR-0006 D5's whole first sentence --
        // "`zaru.toml` pins the workspace per project" -- unreachable, and
        // made the composer's fast tier scoped to an empty string on every
        // machine. The tier is still not what decides it: `bare` is about the
        // membrane a tool call runs inside, and a cortex the composer
        // searches is not a tool call.
        workspace,
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
    let context = SessionContext::opened(context::prefix_for(), limits, 0);
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
        crate::manifest::attached_workspace(resolution),
        prepared.here.root(),
        prepared.context_limits(),
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
        //
        // **And it is recorded, since 2026-09-14.** A run whose turn answered
        // and whose checkpoint would not write is a failed run, and ADR-0010
        // D5's "every byte" is as false for it as for a turn refused at its
        // provider -- `run_one`'s wrapper cannot see this one, because the
        // write happens after it has returned. One recording function, two
        // callers; see `record_the_failure`.
        let classified = Surface::checkpoint(&failure, evidence);
        record_the_failure(&session, &classified);
        return Ran::refused_having_said(ran.lines, classified);
    }

    ran
}

/// Run one turn, and record what refused it if it was refused.
///
/// This is the composition's entry point. What it adds to the turn itself —
/// a private `ran`, named in prose rather than linked because rustdoc is
/// right to refuse a public page pointing at something its reader cannot
/// open — is [ADR-0010] D2's **eighth producer**: a turn whose outcome is
/// [`Exit::Failed`] writes `Record::Failure` before returning.
///
/// # One seam, and not fourteen
///
/// The turn carries **fourteen** `Ran::refused` and `Ran::refused_having_said`
/// returns. Recording at each is what the defect row that opened this
/// proposed, and it cannot be held: a fifteenth site added later would record
/// nothing and no check would notice. Wrapping is what makes "every refusal is
/// recorded" a property of the **shape** rather than of somebody having
/// remembered — the same reason `Record` is a closed enum rather than a `kind`
/// string. Twelve of the fourteen are recorded here; the other two are the
/// transcript-open pair below.
///
/// It also fixes the order. The failure is the turn's **last** record, so a
/// person reading with `cat` gets the question, `turn_started`, the work, and
/// then what stopped it.
///
/// # What is deliberately outside it, both halves
///
/// **The two refusals that could not open the transcript.** They are the first
/// thing the turn does, and a transcript that will not open cannot record
/// that it would not open. A producer that pretended otherwise would be the
/// "variant whose condition nothing can satisfy" `session::record`'s own
/// documentation warns against, inverted into a promise that cannot be kept.
/// They stay printed lines and an [ADR-0016] D5 code, with the session
/// evidence they already carry.
///
/// **Every refusal in [`prepare`].** That function resolves everything
/// **before a session exists** — its own documentation states the consequence,
/// "a turn that never began is not a session" — so there is no file to write
/// to, and minting a session in order to record that none was warranted would
/// reverse [ADR-0010] D1's reading. The check named in this module's own
/// out-of-tree file holds that arm, so moving this up reddens.
///
/// # The refusal that follows the turn is recorded too, by the same function
///
/// [ADR-0010] D3's checkpoint is written **after** the turn, by the turn's
/// callers rather than by the turn, so this wrapper cannot see it: [`task`]
/// writes it once this function has returned and, on failure, classifies it
/// and returns. A run whose turn answered and whose checkpoint would not
/// write is a failed run, and D5's "every byte" is as false for it as for a
/// turn refused at its provider — so it is recorded, through
/// `record_the_failure`, which is the one function that builds the variant.
/// One `Record` variant, one construction, two places that decide a run has
/// failed.
///
/// **`terminal::driver::run_a_turn` is a third such place and is deliberately
/// untouched.** It performs the same two acts in the same order, but what it
/// does with a refused checkpoint is paint `format!("{failure}")` into the
/// pane — a `Display` of the failure rather than an [ADR-0016] D1
/// classification — so recording it would mean classifying it first, which
/// changes what a person reads. That is a decision about a user-facing line
/// and is named here rather than taken.
///
/// # A transcript that will not take the record does not replace the reason
///
/// If the write fails, the turn's own classified failure is still what the
/// reader is given. Reporting the bookkeeping failure instead would replace
/// the reason the turn stopped with the reason it could not be written down,
/// which is strictly less useful to the person reading it — [ADR-0016] D6's
/// "partial success is reported as partial", applied to a record rather than
/// to a task.
///
/// # Errors
///
/// None: a refused turn is a [`Ran`] carrying [ADR-0016] D5's code, which is
/// what every caller of this function already handles.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "    it takes exactly what the turn it wraps takes, and a struct here would be     a second name for that list"
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
    let outcome = ran(
        version, report_at, resolution, prepared, session, n, start, confirmer, extra, narrator,
        owed, context,
    )
    .await;

    let Exit::Failed(classified) = &outcome.exit else {
        return outcome;
    };
    record_the_failure(session, classified);
    outcome
}

/// Write [ADR-0010] D2's eighth producer: what refused a turn.
///
/// # One function, two callers, and that is the whole design
///
/// A failed turn reaching no file is the defect this producer exists for, and
/// a turn is failed by two different things: the turn itself, which
/// [`run_one`] wraps, and the **checkpoint write that follows it**, which
/// [`task`] performs after the turn has returned. Both are failures of one
/// run, so both are recorded — and by *one* function rather than by two call
/// sites each constructing the record, because two constructions of one
/// record are two things that can come to disagree the day the record gains a
/// field. That is the same closed-enum discipline `Record` itself carries.
///
/// **What it is not is a second producer.** There is one `Record` variant,
/// one place that builds it, and two places that decide a turn has failed.
/// `adr_0010_d2s_failure_record_is_written_by_one_function_for_both_callers`
/// holds the count, so a third construction of the variant fails it.
///
/// # A transcript that will not take the record does not replace the reason
///
/// If the write fails, the caller's own classified failure is still what the
/// reader is given. Reporting the bookkeeping failure instead would replace
/// the reason the run stopped with the reason it could not be written down,
/// which is strictly less useful to the person reading it — [ADR-0016] D6's
/// "partial success is reported as partial", applied to a record rather than
/// to a task.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub(super) fn record_the_failure(session: &crate::session::Session, classified: &Classified) {
    if let Ok(mut transcript) = Transcript::append_to(session.transcript_path()) {
        let _ = transcript.record(&crate::session::Record::Failure(
            crate::session::FailureLine::of(classified),
        ));
    }
}

/// What the reader is shown, and what the process exits with.
///
/// [ADR-0012] D7's "per session on exit" is the usage line, read off the
/// provider rather than recomputed: the client reports what it was told and
/// nothing here adds a cost, because "nothing publishes any" pricing.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// What an iterating turn says when every declared validator passed.
///
/// Named because two callers compose it: the line the reader is shown, and
/// [`answer_of`] below, which is what [ADR-0010] D2's seventh producer keeps.
/// Two spellings of one sentence are two things that can come to disagree.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
fn satisfied(iterations: u32) -> String {
    format!("the declared validators are satisfied after {iterations} iteration(s)")
}

/// What this turn **answered**, if it answered anything.
///
/// # This is not [`Ran::lines`], and that is the whole point of it
///
/// By the time a turn is recorded, its lines also carry [ADR-0011] D2's
/// not-a-sandbox notice, [ADR-0013] D2's compaction announcements and
/// [ADR-0012] D7's usage line. **Each of those is already on the transcript**
/// — the first two as [`crate::session::Record::Said`] and
/// [`crate::session::Record::Compacted`], written by this same function's
/// caller — so recording the joined lines as the answer would put three
/// producers' words on that file a second time. [ADR-0010] D3 exists to keep
/// one thing from having two stores; this keeps one *sentence* from having
/// two.
///
/// # `None` is a fact, not a gap
///
/// Three of the five outcomes answered nothing: the model stopped, the turn
/// reached its ceiling, or the iteration loop was exhausted. Each already has
/// a carrier a reader sees — `Event::TurnEnded` renders as `turn 3 stopped
/// without an answer` — so writing an empty `zaru` half would be the harness
/// claiming it said something. The absence is what the pair with no closer
/// means, and it is the same absence an interrupted turn leaves.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub(crate) fn answer_of(outcome: &TurnOutcome) -> Option<String> {
    match outcome {
        TurnOutcome::Answered { text, .. } => Some(text.clone()),
        TurnOutcome::Iterated(zaru_core::iteration::Outcome::Succeeded { iterations, .. }) => {
            Some(satisfied(*iterations))
        }
        TurnOutcome::Stopped { .. }
        | TurnOutcome::Exhausted { .. }
        | TurnOutcome::Iterated(zaru_core::iteration::Outcome::Exhausted { .. }) => None,
    }
}

fn rendered(
    provider: &Classifying<'_>,
    outcome: &TurnOutcome,
    lines: &mut Vec<String>,
) -> (Ran, Option<String>) {
    use crate::providers::Provider as _;

    let mut lines = core::mem::take(lines);
    let answer = answer_of(outcome);
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
            lines.push(satisfied(*iterations));
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
    (Ran { lines, exit }, answer)
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
