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
//!   [`Classifying`](crate::compose::Classifying) adapter kept.
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
//! **No second turn.** One invocation is one turn. A session that holds a
//! conversation is what the in-session shell will drive, and the checkpoint
//! this writes is what it will restore.
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
use crate::compose::{Classifying, NoFetch, Records, TurnContext, context, prose};
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
    fn refused(classified: Classified) -> Self {
        Self {
            lines: Vec::new(),
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

/// Run one turn for `task`, and say what the process exits with.
///
/// `version` and `report_at` are the binary's own package metadata, for
/// [ADR-0016] D3's report.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "\
    the composition is one dependency order and splitting it into functions \
    would put the order in the call graph rather than on the page, where the \
    only thing that makes it reviewable is reading it top to bottom"
)]
pub fn task(version: &str, report_at: &str, resolution: &Resolution, task: &str) -> Ran {
    let surface = Surface::new(version, report_at);

    // --- ADR-0001 D2's tier, resolved once ---------------------------------
    let tier = match ResolvedTier::from_configuration(resolution) {
        Ok(tier) => tier,
        Err(refusal) => return Ran::refused(surface.tier(&refusal)),
    };

    // --- ADR-0012 D4's model, through ADR-0014's five layers ---------------
    let model = match ModelTable::from_configuration(resolution) {
        Err(refusal) => return Ran::refused(Classified::from(refusal)),
        Ok(table) => match table.row(ModelAlias::Default) {
            ResolvedModel::Unresolved => {
                return Ran::refused(Surface::no_model_for_the_default_alias());
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
        Err(failure) => return Ran::refused(Surface::working_directory(&failure)),
    };

    // --- ADR-0009 D1's manifest, and D4's branch ---------------------------
    //
    // Read before the session is created, so a project that cannot be served
    // is refused without writing a directory for a turn that never ran.
    let manifest_file =
        crate::manifest::ManifestFile::in_directory(here.clone(), layers::file_ceiling());
    let manifest = match manifest_file.parse() {
        Ok(manifest) => manifest,
        Err(refusal) => return Ran::refused(Surface::manifest(&refusal)),
    };
    if let Some(declared) = manifest.as_ref()
        && !declared.validators().is_empty()
    {
        return Ran::refused(Surface::no_inner_loop(declared.validators().len()));
    }

    // --- ADR-0007's store, and which kind this machine can reach -----------
    let store_root = match CredentialStore::default_root() {
        Ok(root) => root,
        Err(failure) => {
            return Ran::refused(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            );
        }
    };
    let store = match CredentialStore::reading(store_root.clone()) {
        Ok(store) => store,
        Err(failure) => {
            return Ran::refused(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            );
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
            return Ran::refused(Surface::no_key_for(&KINDS_WITH_A_CLIENT, &model));
        }
        None => return Ran::refused(surface.no_client_for_the_kinds_held(&model, &held_kinds)),
    };

    // --- The key, and the redactor over everything the store holds ---------
    let keyring = OsKeyring::for_store(&store_root);
    let keys = HarnessKeys::from_process(&keyring);
    let alias = kind.credential_alias();
    let secret = match store.secret(&alias, &keys) {
        Ok(secret) => secret,
        Err(failure) => {
            return Ran::refused(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            );
        }
    };
    // ADR-0008 clause 6's port, over every value the store holds -- including
    // the one this turn is about to send, which is why it is built from the
    // store rather than from the secret above.
    let held: HeldSecrets = match held_secrets_for_redaction(&store, &keys) {
        Ok(held) => held,
        Err(failure) => {
            return Ran::refused(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            );
        }
    };

    // --- ADR-0012 D5's endpoint: configuration, then this kind's default ---
    let endpoint = match resolution.get(&kind.endpoint_key()) {
        Some(crate::config::Value::Text(configured)) => {
            match crate::providers::ProviderEndpoint::new(configured) {
                Ok(endpoint) => endpoint,
                Err(refusal) => return Ran::refused(Surface::endpoint(kind, &refusal)),
            }
        }
        _ => Endpoint::default_endpoint(),
    };

    let client = match GeminiClient::new(endpoint, model.clone(), alias, secret) {
        Ok(client) => client,
        Err(failure) => return Ran::refused(Surface::provider(&failure)),
    };
    let provider = Classifying::over(&client);

    // --- ADR-0012 clause 3: the model is asked before the loop starts ------
    let witness = match ToolCalling::required(&provider, model.as_str()) {
        Ok(witness) => witness,
        Err(refusal) => return Ran::refused(Surface::model_cannot_call_tools(&refusal)),
    };

    // --- ADR-0010 D1's session, and the first `meta.toml` a product writes --
    let session_store = match SessionStore::open(store_root) {
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
        tier,
        // ADR-0001 D1 gives `bare` no cortex, and nothing here attaches one.
        None,
        Some(kind.to_string()),
        id.minted_at(),
    );
    if let Err(failure) = MetaFile::at(session.meta_path()).write(&meta) {
        return Ran::refused(Surface::meta(&failure, evidence));
    }
    let checkpoint = Checkpoint::at(session.checkpoint_path());
    if let Err(failure) = checkpoint.write(&serde_json::json!({ "exchanges": [] })) {
        return Ran::refused(Surface::checkpoint(&failure, evidence));
    }
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
    if let Some(mut notice) = SessionNotice::for_tier(tier.tier(), prose::NOT_A_SANDBOX)
        && let Some(sentence) = notice.state_once()
    {
        lines.push(sentence);
        lines.push(String::new());
    }

    // --- ADR-0011's acting half, over every port it needs ------------------
    let mut overflow = crate::tools::SessionOverflow::in_session(session.directory());
    let allowlist = match crate::tools::Allowed::from_configuration(resolution) {
        Ok(allowlist) => allowlist,
        Err(refusal) => return Ran::refused(Surface::allowlist(&refusal)),
    };
    let destructive = crate::tools::Shapes;
    let verdicts = crate::tools::NoMembrane;
    // ADR-0011 D3's prompt over the terminal. `None` when standard input is
    // not one, at which point a call that needed a confirmation is refused
    // rather than performed -- which is `Decision::permit`'s own rule and the
    // reason this is an `Option` rather than a stub that answers yes.
    let confirmer = crate::tools::prompt::Prompt::from_process();
    let environment = match crate::process::Environment::inherited_minimum() {
        Ok(environment) => environment,
        Err(refusal) => return Ran::refused(Surface::child_environment(&refusal)),
    };
    let spawn = crate::process::Spawn::new(&here, environment, layers::process_ceiling());
    let fetch = NoFetch;

    let executor = Executor {
        working_directory: &here,
        // ADR-0011 D3's default, and no key is declared for it: that record
        // names none and ADR-0014's Neutral consequence leaves each record its
        // own. A user cannot change the mode from the terminal, which is a
        // real limitation and is recorded rather than closed by inventing one.
        mode: Mode::default(),
        allowlist: &allowlist,
        destructive: &destructive,
        confirmer: confirmer
            .as_ref()
            .map(|prompt| prompt as &(dyn crate::tools::Confirm + Sync)),
        verdicts: &verdicts,
        budget: layers::output_budget(),
        search_ceiling: layers::search_ceiling(),
        overflow: &mut overflow,
        transcript: &mut transcript,
        redactor: &held,
        subprocess: &spawn,
        fetch: &fetch,
    };

    // --- ADR-0013's context, assembled once at the turn boundary -----------
    let context = Context::opened(context::prefix_for(), layers::context_limits());
    let policy = TurnContext::over(&context, &held);
    let clock = SystemClock::started_now();

    let mut executor = executor;
    let outcome = block_on(tool_call::run(
        1,
        Start::Task(task),
        layers::tool_call_ceiling(),
        witness,
        Ports {
            model: &provider,
            tools: &mut executor,
            context: &policy,
            clock: &clock,
            redactor: &held,
        },
        // ADR-0009 D4's branch. `None` always: see the module documentation
        // and `Surface::no_inner_loop`, which is what a project that declared
        // validators was refused with above.
        Option::<&crate::compose::NoInnerLoop>::None,
        &mut [&mut events],
    ));

    // A transcript that lost an event has not recorded what happened, whatever
    // the loop returned, and ADR-0010 D2 makes this file the replayable record.
    if let Some(failure) = events.first_failure() {
        return Ran::refused(Surface::transcript(failure, evidence));
    }

    let mut ran = match outcome {
        Ok(outcome) => rendered(&provider, &outcome, &mut lines),
        Err(error) => {
            return Ran::refused(surface.turn(&error, provider.taken().as_ref(), evidence));
        }
    };

    // --- ADR-0009 D4's line, at the end of the first turn -------------------
    //
    // ADR-0002 D8's event-anchored kind, settled under directive 20: appended
    // to the end of the triggering turn rather than stated at session start,
    // because "a manifest is absent before the user does anything" and a line
    // at session start would be a timer wearing a costume by D8's own test.
    if let Some(mut owed) = crate::manifest::MissingManifest::for_manifest(
        manifest.as_ref(),
        crate::failure::Statement::sanitised(prose::NO_VALIDATORS),
        crate::failure::Statement::sanitised(prose::DECLARE_ONE),
    ) && let Some(recommendation) = owed.state_once()
    {
        ran.lines.push(String::new());
        ran.lines.push(recommendation.to_string());
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
