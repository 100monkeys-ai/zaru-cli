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
//! notice        ADR-0011 D2's line, once, at bare tier only
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
use crate::compose::{Classifying, Records, TurnContext, context, prose};
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
use zaru_core::context::Context;
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

    /// Whether this project declared no manifest, so [ADR-0002] D8's line is
    /// owed.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    #[must_use]
    pub const fn manifest(&self) -> Option<&crate::manifest::Manifest> {
        self.manifest.as_ref()
    }

    /// Where [ADR-0010] D1's session directory lives.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn store_root(&self) -> &std::path::Path {
        &self.store_root
    }
}

/// The two lines a **session** owes once, whatever its turns do.
///
/// [ADR-0011] D2's notice is "stated once at session start" and [ADR-0002]
/// D8's event-anchored recommendation "fires at most once ever". Both are
/// take-once values, and holding them here rather than building them inside a
/// turn is what makes the scope the session's rather than the process's.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug, Default)]
pub struct Owed {
    notice: Option<SessionNotice>,
    recommendation: Option<crate::manifest::MissingManifest>,
}

impl Owed {
    /// What this session owes, from what it resolved.
    #[must_use]
    pub fn of(prepared: &Prepared) -> Self {
        Self {
            notice: SessionNotice::for_tier(prepared.tier.tier(), prose::NOT_A_SANDBOX),
            recommendation: crate::manifest::MissingManifest::for_manifest(
                prepared.manifest.as_ref(),
                crate::failure::Statement::sanitised(prose::NO_VALIDATORS),
                crate::failure::Statement::sanitised(prose::DECLARE_ONE),
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
    let here = match std::env::current_dir()
        .map_err(crate::tools::TreeError::from_current_directory)
        .and_then(WorkingDirectory::at)
    {
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
    if let Some(declared) = manifest.as_ref()
        && !declared.validators().is_empty()
    {
        return Err(Box::new(Ran::refused(Surface::no_inner_loop(
            declared.validators().len(),
        ))));
    }

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
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
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
pub fn run_one(
    version: &str,
    report_at: &str,
    resolution: &Resolution,
    prepared: &Prepared,
    session: &crate::session::Session,
    n: u32,
    task: &str,
    confirmer: Option<&(dyn crate::tools::Confirm + Sync)>,
    extra: &mut [&mut dyn zaru_core::tool_call::EventSink],
    owed: &mut Owed,
    context: &mut Context,
) -> Ran {
    let surface = Surface::new(version, report_at);
    let evidence = session.evidence();
    let provider = Classifying::over(&prepared.client);

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
    if let Some(notice) = owed.notice.as_mut()
        && let Some(sentence) = notice.state_once()
    {
        lines.push(sentence);
        lines.push(String::new());
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

    // --- ADR-0013's context, assembled once at the turn boundary -----------
    let clock = SystemClock::started_now();
    let outcome = {
        let policy = TurnContext::over(context, &prepared.held);
        // ADR-0008 clause 3's slice. The transcript writer first, so that the
        // record on disk is written before anything renders it -- a renderer
        // that painted an event the file does not hold would be showing the
        // user something a resume could not reproduce.
        // ADR-0008's execution, decided 2026-09-05: one tool surface, reached
        // by both loops. See `crate::compose::shared` for why it is a lock and
        // why sharing the value rather than building a second one is what
        // makes "a candidate cannot do what a turn cannot" a property.
        let tool_surface = tokio::sync::Mutex::new(executor);
        let mut tools = crate::compose::Shared::over(&tool_surface);
        let mut sinks: Vec<&mut dyn zaru_core::tool_call::EventSink> = vec![&mut events];
        for sink in extra.iter_mut() {
            sinks.push(&mut **sink);
        }
        block_on(tool_call::run(
            n,
            Start::Task(task),
            layers::tool_call_ceiling(),
            prepared.witness,
            Ports {
                model: &provider,
                tools: &mut tools,
                context: &policy,
                clock: &clock,
                redactor: &prepared.held,
            },
            // ADR-0009 D4's branch. `None` always: see the module
            // documentation and `Surface::no_inner_loop`, which is what a
            // project that declared validators was refused with in `prepare`.
            Option::<&crate::compose::NoInnerLoop>::None,
            &mut sinks,
        ))
    };

    // A transcript that lost an event has not recorded what happened, whatever
    // the loop returned, and ADR-0010 D2 makes this file the replayable record.
    if let Some(failure) = events.first_failure() {
        return Ran::refused_having_said(lines, Surface::transcript(failure, evidence));
    }

    let mut ran = match outcome {
        Ok(outcome) => rendered(&provider, &outcome, &mut lines),
        Err(error) => {
            return Ran::refused_having_said(
                lines,
                surface.turn(&error, provider.taken().as_ref(), evidence),
            );
        }
    };

    // --- ADR-0009 D4's line, at the end of the turn that caused it ---------
    //
    // ADR-0002 D8's event-anchored kind, settled under directive 20: appended
    // to the end of the triggering turn rather than stated at session start,
    // because "a manifest is absent before the user does anything" and a line
    // at session start would be a timer wearing a costume by D8's own test.
    // It is the session's take-once value, so a second turn does not repeat it.
    if let Some(recommendation) = owed.recommendation.as_mut()
        && let Some(recommendation) = recommendation.state_once()
    {
        ran.lines.push(String::new());
        ran.lines.push(recommendation.to_string());
    }

    ran
}

/// Run one turn for `task` in a session of its own, and say what the process
/// exits with.
///
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
    let mut owed = Owed::of(&prepared);

    // --- ADR-0010 D1's session, and the first `meta.toml` a product writes --
    let session_store = match SessionStore::open(prepared.store_root.clone()) {
        Ok(store) => store,
        Err(failure) => return Ran::refused(surface.session(&failure)),
    };
    let id = match SessionId::mint(&SystemWallClock) {
        Ok(id) => id,
        Err(failure) => return Ran::refused(Surface::session_not_started(&failure)),
    };
    let session = match session_store.start(id.clone()) {
        Ok(session) => session,
        Err(failure) => return Ran::refused(surface.session(&failure)),
    };
    let evidence = session.evidence();
    let meta = Meta::new(
        prepared.tier,
        // ADR-0001 D1 gives `bare` no cortex, and nothing here attaches one.
        None,
        Some(prepared.kind.to_string()),
        id.minted_at(),
    );
    if let Err(failure) = MetaFile::at(session.meta_path()).write(&meta) {
        return Ran::refused(Surface::meta(&failure, evidence));
    }
    let checkpoint = Checkpoint::at(session.checkpoint_path());
    if let Err(failure) = checkpoint.write(&serde_json::json!({ "exchanges": [] })) {
        return Ran::refused(Surface::checkpoint(&failure, evidence));
    }

    // ADR-0011 D3's prompt over the terminal. `None` when standard input is
    // not one, at which point a call that needed a confirmation is refused
    // rather than performed -- which is `Decision::permit`'s own rule and the
    // reason this is an `Option` rather than a stub that answers yes.
    let confirmer = crate::tools::prompt::Prompt::from_process();
    let mut context = Context::opened(context::prefix_for(), layers::context_limits());

    run_one(
        version,
        report_at,
        resolution,
        &prepared,
        &session,
        1,
        task,
        confirmer
            .as_ref()
            .map(|prompt| prompt as &(dyn crate::tools::Confirm + Sync)),
        &mut [],
        &mut owed,
        &mut context,
    )
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
        // Unreachable: the inner loop is `None`, so no turn's body is an
        // iteration. Reported rather than unwrapped, because the day it
        // becomes reachable this arm is what a reader lands on.
        TurnOutcome::Iterated(_) => Exit::Failed(Surface::turn_iterated()),
    };
    if let Some(usage) = provider.client().usage() {
        lines.push(String::new());
        lines.push(crate::cli::render::usage(&usage));
    }
    Ran { lines, exit }
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
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime with the io and time drivers")
        .block_on(future)
}
