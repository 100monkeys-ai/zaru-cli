// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0013]'s layered context, as the port the loop calls it through.
//!
//! # The value and the port are two different things, and that is D7
//!
//! `zaru-core`'s [`Context`] is a value with two operations and the whole of
//! D7 is the difference between their signatures: `assemble` takes `&self` and
//! `compact` takes `&mut self`, so "a `ContextPolicy` implementation holding a
//! `Context` behind the shared borrow `assemble` gives it therefore *cannot*
//! compact — the method is not callable from there."
//!
//! [`TurnContext`] is that implementation, and it holds the context by shared
//! borrow. **The mutation that would break D7 does not compile here either**,
//! which is the property being inherited rather than re-established.
//!
//! # Compaction is somewhere else, and that is the whole of D7
//!
//! D2's compaction is a turn-boundary act, and the type that can perform one
//! is [`SessionContext`](crate::compose::SessionContext) — which owns the
//! context mutably and hands out one of these for the turn. So the borrow this
//! type holds is what makes D7 structural at both layers: while a `TurnContext`
//! exists, nothing can compact, because `at_turn_boundary` needs `&mut` and
//! this has the shared half.
//!
//! A context that will not fit still **refuses**, with [ADR-0013] D7's own
//! answer — "an iteration that would exceed the window fails as exhausted with
//! a clear reason rather than continuing on a rewritten context" — carried out
//! as `ContextRefusal::WindowExceeded` with both numbers on it. That is the
//! answer *inside* a turn, where D7 forbids rewriting; relieving the pressure
//! is what the boundary before the next turn is for.
//!
//! # What is in each of D1's seven layers today
//!
//! | Layer | This composition |
//! | --- | --- |
//! | 1 system prompt and persona | [ADR-0027]'s served page where one was read, the harness's own [`system_prompt`] where none was — see below |
//! | 2 grounding, session-start | empty: [ADR-0006]'s client reaches no network, so nothing is read at session start |
//! | 3 relationship memory | empty, and **for a new reason since 2026-09-15**: [ADR-0031] D3 delivers it *inside* the served prompt and forbids a second fetch path, so now that layer 1 has a page it rides that page — see below |
//! | 4 project manifest summary | empty: no record says what a manifest summary is, and inventing a shape would settle it |
//! | 5 user attachments | empty: [ADR-0005] D5's attachments are not built and the trie is `zaru-notes`' |
//! | 6 conversation and tool results | empty on the first turn; every earlier turn as the messages it was, rebuilt from the transcript at each turn boundary by [`crate::compose::boundary`]; the turn's own messages ride on `ModelRequest.turn` rather than here, which is [ADR-0013] D7 as `tool_call::run` reads it |
//! | 7 iteration history | empty: an iteration's memory is [ADR-0008]'s refinement prompt, which arrives as the turn's own tail rather than as a layer |
//!
//! **Six of the seven are empty and the prefix says so about the one that
//! matters.** An empty layer contributes nothing to the rendered text rather
//! than a blank section, which is `StablePrefix`' own rule, so what a model
//! actually receives is the one absence line and the task. That is a small
//! prompt and it is an honest one; the layers exist, they are reached, and
//! what fills them is other records' work.
//!
//! **Dated 2026-09-15, beside the paragraph above rather than in it.** Layer 1
//! can now hold a page, so on a machine with a pinned workspace, a stored
//! token and a persona page the count is five empty rather than six and the
//! prefix is the page. **Where any of those three is missing it is exactly
//! what the paragraph above describes**, which is every machine that has not
//! set `persona.path` and every machine with no cortex at all.
//!
//! # Layer 3 is empty under a page, and the reason changed
//!
//! [ADR-0031] D3 appends the relationship memory to the served prompt **before
//! it is returned**, and that record forbids the harness a second fetch — "a
//! second fetch path is a second thing that can disagree". Under a served
//! *page*, the memory therefore arrives **inside layer 1's body**, appended by
//! whatever serves the page, and the harness's fetch count stays one.
//!
//! **The cost is that layer 3 stays empty for ever under this default, and it
//! is stated rather than left to be found.** Filling it would need either a
//! section grammar over a document this harness does not own — an authored
//! parse of somebody else's prose — or a second read, which ADR-0031 forbids
//! in as many words. So layer 3's emptiness is no longer "absent exactly when
//! layer 1 is"; it is "the memory is in layer 1 when it exists at all", which
//! is a different fact about the same empty string and is on [ADR-0013]'s
//! amendments page.
//!
//! Layer 7's line said "no iteration runs, because there is no inner loop"
//! until 2026-09-05, and the `iteration-wiring` arc's landing made it false
//! while leaving the layer genuinely empty. It is corrected here rather than
//! left, because a reason that is false is worse than no reason: it tells the
//! next reader the layer is waiting on a capability that already exists.
//!
//! # One sentence rides the tail, and only on the iterating branch
//!
//! [`prose::ITERATION_IS_ONE_EXCHANGE`] is prepended by
//! [`TurnContext::assemble`] when the project declared validators. It is not a
//! layer: it is [ADR-0008] D1's statement of what an iteration is, true of the
//! inner loop and **false of the outer one**, so a prefix layer carrying it
//! would state a falsehood on every turn that declares no validators. That
//! function's own documentation carries the whole argument.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//!
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
//! [ADR-0031]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0031-relationship-memory
//! [`Context`]: zaru_core::context::Context

use crate::compose::prose;
use crate::providers::capacity::Calibration;
use zaru_core::context::{Context, PrefixParts, StablePrefix};
use zaru_core::iteration::{ContextPolicy, ContextRefusal, Prompt, Turn};
use zaru_core::redaction::Redactor;

/// What a model is told about the session it works in, and cannot find out
/// for itself.
///
/// Every field is a fact the harness holds and the model does not: where the
/// tools run, on what system, on what day, with which tools, under which
/// permission mode. [`system_prompt`] turns them into layer 1's text when no
/// persona is served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The working directory every tool call is resolved against, where it
    /// resolved.
    pub directory: Option<String>,
    /// The operating system, as Rust names it (`linux`).
    pub system: String,
    /// Today's date, `YYYY-MM-DD`, read once when the session's prefix is
    /// built. Layer 1 is never rewritten mid-session, so a session that runs
    /// past midnight keeps the date it opened on.
    pub date: String,
    /// The names of the tools the model is offered.
    pub tools: Vec<String>,
    /// [ADR-0011] D3's permission mode, where it resolved.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub mode: Option<crate::tools::Mode>,
}

impl Facts {
    /// The facts of a session opened now, in `directory`, offering `tools`
    /// under `resolution`'s permission mode.
    ///
    /// A mode that does not resolve is left out rather than guessed: such a
    /// session runs no turn, because `crate::compose::turn::prepare` refuses
    /// on the same reading.
    #[must_use]
    pub fn of_this_session(
        directory: Option<&std::path::Path>,
        resolution: &crate::config::Resolution,
        tools: &[zaru_core::tool_call::ToolDescriptor],
    ) -> Self {
        Self {
            directory: directory.map(|directory| directory.display().to_string()),
            system: std::env::consts::OS.to_owned(),
            date: crate::commands::date::today(),
            tools: tools.iter().map(|tool| tool.name.clone()).collect(),
            mode: crate::tools::Mode::from_configuration(resolution).ok(),
        }
    }
}

/// Layer 1 where no persona is served: the harness's own system prompt.
///
/// # What it says, and why each line is there
///
/// Only facts the model needs and cannot know. It has no personality and
/// makes no claim about the product, because a persona is [ADR-0027]'s and is
/// served, not written here.
///
/// - The working directory, and that a relative path is resolved against it:
///   every session measured by the survey of 2026-09-28 opened with an
///   `fs.list` because nothing said where it was.
/// - The operating system and the date, which a model cannot learn from
///   anything it is sent.
/// - The tools by name, and that `cmd.run` has no shell: a pipe or a
///   redirect is refused, and the refusal is a wasted exchange.
/// - The permission mode, and what it means for a call.
/// - That earlier turns' tool results are in the conversation, so the model
///   looks there before calling a tool again.
///
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
#[must_use]
pub fn system_prompt(facts: &Facts) -> String {
    let mut lines = vec![
        "You are working with a person through Zaru, a tool harness running on their machine."
            .to_owned(),
        String::new(),
    ];
    if let Some(directory) = &facts.directory {
        lines.push(format!(
            "- Working directory: {directory}. A relative path in a tool call is resolved \
             against it."
        ));
    }
    lines.extend([
        format!("- Operating system: {}.", facts.system),
        format!("- Today's date: {}.", facts.date),
        format!(
            "- Tools: {}. cmd.run runs one program with its arguments and no shell, so pipes, \
             redirection, globs and && do not work.",
            facts.tools.join(", ")
        ),
    ]);
    if let Some(mode) = facts.mode {
        lines.push(format!(
            "- Permission mode: {}. {}",
            mode.as_str(),
            match mode {
                crate::tools::Mode::Ask => {
                    "The person is asked before a call writes a file, runs a command or \
                     fetches a URL, and may say no."
                }
                crate::tools::Mode::Allow => {
                    "Calls the person has allowed run without asking; anything else is asked \
                     first, and the person may say no."
                }
                crate::tools::Mode::Yolo => "Calls run without asking the person.",
            }
        ));
    }
    lines.push(
        "- The results of tool calls in earlier turns are part of this conversation; read them \
         there before calling a tool again."
            .to_owned(),
    );
    lines.join("\n")
}

/// [ADR-0013] D1's layers 1 to 4 for a session this harness can actually
/// assemble.
///
/// Layer 1 carries the served persona where `persona` is `Some`, and the
/// harness's own [`system_prompt`] over `facts` where it is `None`; the other
/// three are empty — see the module documentation for what each is waiting
/// on. The prefix is built **once** and has no method that changes it, which
/// is that record's trigger clause 1 held by the type rather than by a rule
/// anybody keeps.
///
/// # The argument is taken here rather than fetched here, and that is clause 1
///
/// [ADR-0013] trigger clause 1 — "layers 1 to 4 are byte-identical across
/// every turn of a long session" — is satisfied, and its two checks were
/// **watched red by rewriting the prefix mid-session**. So this function takes
/// values that have already been resolved: the resolution happens before the
/// prefix exists, on the caller's own thread, and nothing after it can reach
/// back in. A fetch *inside* here, or a background task that landed in layer 1
/// afterwards, would be exactly the mutation that clause forbids and would
/// destroy the prompt caching [ADR-0013] D1 calls "an architectural constraint
/// rather than an optimisation". See [`crate::compose::persona`].
///
/// # An empty body is not a persona
///
/// `Some("")` would put an empty layer 1 in the prefix, which renders as no
/// layer at all — so a model would receive no system text whatever. A page
/// that came back empty is therefore treated as no page: **the absence is the
/// same absence however it arose**, which is what [ADR-0027]'s 2026-09-05
/// Update decided.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
#[must_use]
pub fn prefix_for(persona: Option<&str>, facts: &Facts) -> StablePrefix {
    let layer_one = match persona {
        Some(served) if !served.is_empty() => served.to_owned(),
        _ => system_prompt(facts),
    };
    StablePrefix::assembled_once(PrefixParts {
        system_prompt_and_persona: layer_one,
        grounding: String::new(),
        relationship_memory: String::new(),
        project_manifest_summary: String::new(),
    })
}

/// [ADR-0013]'s context, as [ADR-0008]'s loop reaches it.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub struct TurnContext<'a> {
    context: &'a Context,
    counter: &'a Calibration,
    redactor: &'a (dyn Redactor + Sync),
    /// Whether this turn's body is [ADR-0008] D1's iteration.
    ///
    /// The composition already computes it — `let iterating =
    /// !declared.is_empty()` — and it arrives here rather than being derived
    /// a second time, because two answers to "is this an iteration" is how
    /// the prefix and the branch would drift apart.
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    iterating: bool,
}

impl core::fmt::Debug for TurnContext<'_> {
    /// Names what it holds and renders none of it.
    ///
    /// A context is the whole of what a model is about to be shown, so a
    /// derived `Debug` would put a session's conversation into a panic
    /// message. The redactor cannot be rendered at all — it holds the
    /// harness's own bearer values in memory, which is why
    /// [`HeldSecrets`](crate::redaction::HeldSecrets) writes its own `Debug`
    /// by hand — so this reports the usage instead, which is a number.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TurnContext")
            .field("usage", &self.usage())
            .finish_non_exhaustive()
    }
}

impl<'a> TurnContext<'a> {
    /// Assemble against this context, estimating tokens through `counter`,
    /// redacting held secrets.
    ///
    /// `iterating` is the composition's own boolean and decides one thing:
    /// whether the assembled prompt carries
    /// [`prose::ITERATION_IS_ONE_EXCHANGE`]. See [`Self::assemble`].
    #[must_use]
    pub const fn over(
        context: &'a Context,
        counter: &'a Calibration,
        redactor: &'a (dyn Redactor + Sync),
        iterating: bool,
    ) -> Self {
        Self {
            context,
            counter,
            redactor,
            iterating,
        }
    }

    /// What the context costs right now. [ADR-0013] D6's continuous number.
    ///
    /// Measured through the same counter and the same redactor the assembly
    /// uses, because "a marker is not the same length as the value it replaced
    /// and the threshold is compared against that count".
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    #[must_use]
    pub fn usage(&self) -> zaru_core::context::Usage {
        self.context.usage(self.counter, self.redactor)
    }
}

impl ContextPolicy for TurnContext<'_> {
    /// Assemble the prompt for the turn about to begin.
    ///
    /// The tail is whichever of [`Turn`]'s two variants the loop passed, and
    /// **this function adds no prose to either**.
    ///
    /// # The one sentence this function does add, and where it does not
    ///
    /// **The rule above is about the two tails and it is unchanged**: no
    /// variant is wrapped, re-described or annotated. What is prepended, when
    /// and only when `iterating` is set, is
    /// [`prose::ITERATION_IS_ONE_EXCHANGE`] — [ADR-0008] D1's statement of
    /// what an iteration is, which is a property of the loop the prompt is
    /// being assembled for rather than a gloss on the task inside it.
    ///
    /// It is prepended **here** rather than in `refinement::construct`, which
    /// is `zaru-core`'s fixed prose, for two reasons that are both structural.
    /// The first is that the sentence has to reach [`Turn::Initial`] — the
    /// first iteration is the one that decides whether the model explores or
    /// acts, and a refinement prompt arrives too late to change it. The second
    /// is [ADR-0008] D4: that prompt carries the validator's output verbatim
    /// and "never paraphrased", and a harness sentence inside those bytes is
    /// the paraphrase the clause exists to forbid, arriving through a side
    /// door — the same argument the `process-runner` arc already made once
    /// when it refused to append a sentence to a captured stream.
    ///
    /// **Where it is not added**: a turn with no declared validators, where
    /// the sentence would be false. There the results of a call come back
    /// inside the turn on `ModelRequest.turn`, so telling a model they do
    /// not would be a falsehood stated on every `bare`-tier turn that declares
    /// nothing.
    ///
    /// The prepend goes through the same assembly the tail does, so it is
    /// counted by the same [`Calibration`] against [ADR-0013] D6's window and
    /// passes the same [`Redactor`] — it is not a second path into a prompt
    /// and [ADR-0008]'s clause-6 enumeration gains no row, because a static
    /// constant of this harness's own is not captured bytes.
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        let tail = match turn {
            Turn::Initial { task } => (*task).to_owned(),
            Turn::Refinement { refinement } => refinement.as_str().to_owned(),
        };
        let tail = if self.iterating {
            format!("{}\n\n{tail}", prose::ITERATION_IS_ONE_EXCHANGE)
        } else {
            tail
        };
        let assembled = self
            .context
            .assemble(self.counter, self.redactor, &tail)
            .map_err(ContextRefusal::from)?;
        Ok(assembled.into_prompt())
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    /// The facts a check's layer 1 is built from: fixed, so a prompt a check
    /// compares is the same on every machine and every day.
    pub(crate) fn facts() -> super::Facts {
        super::Facts {
            directory: Some("/work".to_owned()),
            system: "linux".to_owned(),
            date: "2026-09-28".to_owned(),
            tools: vec!["fs.read".to_owned()],
            mode: None,
        }
    }
}
