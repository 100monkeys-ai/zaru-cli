// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Every sentence this harness states on a turn, transcribed from the records
//! and authored nowhere.
//!
//! # Three are said to a person and two are said to a model
//!
//! [`NOT_A_SANDBOX`], [`NO_VALIDATORS`] and [`DECLARE_ONE`] are read by the
//! person running the harness. [`SUMMARISE_SPAN`] and
//! [`ITERATION_IS_ONE_EXCHANGE`] are read by a model and never by a user, and
//! each says so where it is defined. [`NO_PERSONA`] is layer 1's own line
//! about what layer 1 does not have. The register differs between the two
//! kinds and the reason it differs is on each constant; what they share is
//! that no wording here was chosen by whoever typed it.
//!
//! # Why these are here and not at the places that print them
//!
//! Each of the user-facing three is a **required argument with no default** at
//! the module that uses it, and each of those modules says why in its own
//! words. From
//! [`crate::tools::notice`]:
//!
//! > What the user is told they are *not* getting is user-facing prose about
//! > a security posture, and [Autonomous Development] puts authoring that on
//! > the human side of the boundary … nothing here has a default, because a
//! > default would be the wording, chosen by whoever typed it.
//!
//! And from [`crate::manifest::absent`], for the other two halves: "What a
//! user is told they are not getting, and what they should do about it, is
//! user-facing prose about a capability."
//!
//! So the wording could not live there. It has to live *somewhere* the moment
//! a binary states one, and this module is that somewhere: one place, named
//! for what it holds, so that one search finds every sentence this harness
//! says to a person on a turn and each one says where it came from.
//!
//! # Nothing here is written by this arc
//!
//! Every constant below is **quoted verbatim from the record that drafted
//! it**, for that record's author to accept or replace. Not one word is
//! changed, and the doc comment on each carries the record, the clause and the
//! sentence the record uses to introduce it. Where the record drafted the
//! wording as a proposal, it is still a proposal — landing it in code does not
//! accept it, and the arc that landed it says so at the point a person reads.
//!
//! **The exceptions are [`NO_PERSONA`], and one sentence each of
//! [`SUMMARISE_SPAN`] and [`ITERATION_IS_ONE_EXCHANGE`]**, which no record
//! drafted. Each says exactly which of its words came from a record and which
//! were authored under directive 20 as a functional instruction, because a
//! constant that did not distinguish them would let an authored sentence be
//! read later as a record's.
//!
//! [Autonomous Development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development

/// [ADR-0011] D2's statement that Zaru is not a sandbox.
///
/// D2: "**At bare tier the harness states plainly, once at session start, that
/// it is not a sandbox.** A permission prompt that reads like containment
/// while being a suggestion is worse than no prompt, because it manufactures a
/// confidence the user has not earned."
///
/// Transcribed from that record's Update of 2026-09-04, which proposed it "for
/// this record's author to accept or replace", **with two changes made on
/// 2026-09-28** under a coordinator ruling open to Jeshua's veto. The last
/// sentence, "Run --runtime contained for a membrane that enforces rather than
/// asks.", is gone, because no tier enforces anything yet and the program
/// never recommends protection it does not give. And the rest was rewritten
/// in plain words, because the same warning is now said at every tier and
/// "a prompt is a question rather than a barrier" left a person to work out
/// what it meant. The wording is the coordinator's, open to Jeshua's veto.
///
/// At `contained` and `linked` [`not_built`] adds one sentence to it.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub const NOT_A_SANDBOX: &str = "Zaru is not a sandbox. Tool calls run on your machine with your \
                                 permissions. A permission prompt asks before a call runs; it \
                                 does not limit what an allowed call can do.";

/// The sentence said after [`NOT_A_SANDBOX`] at a tier that is not built.
///
/// `contained` and `linked` can be selected and enforce nothing. Until a tier
/// enforces something, selecting it prints the same warning as `bare` and
/// this. Ruled by the coordinator on 2026-09-28, open to Jeshua's veto.
#[must_use]
pub fn not_built(tier: crate::runtime::Tier) -> String {
    format!("The {tier} tier is not built yet and changes nothing about how tool calls run.")
}

/// The whole warning for `tier`: [`NOT_A_SANDBOX`], and at a tier that is not
/// built, [`not_built`] after it.
#[must_use]
pub fn not_a_sandbox_at(tier: crate::runtime::Tier) -> String {
    match tier {
        crate::runtime::Tier::Bare => NOT_A_SANDBOX.to_owned(),
        crate::runtime::Tier::Contained | crate::runtime::Tier::Linked => {
            format!("{NOT_A_SANDBOX} {}", not_built(tier))
        }
    }
}

/// The first half of [ADR-0009] D4's line: what is unavailable.
///
/// D4: "A project with no `zaru.toml` runs the tool-call loop only. The
/// harness says so once — **a single line naming what is unavailable and how
/// to get it**, appended to the end of the session's first turn — and never
/// mentions it again."
///
/// **Transcribed verbatim from that record's Update of 2026-09-04**, which
/// introduces both halves as "Proposed wording, for the author to accept or
/// replace".
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
pub const NO_VALIDATORS: &str = "no validators are declared, so the iteration loop cannot run";

/// The second half of [ADR-0009] D4's line: how to get it.
///
/// See [`NO_VALIDATORS`]. Both halves are required arguments of
/// [`MissingManifest`](crate::manifest::absent::MissingManifest), because "a
/// recommendation naming what is unavailable and not how to get it is half of
/// what D4 asks for, and there is no constructor that builds one".
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
pub const DECLARE_ONE: &str = "declare one in `./zaru.toml`";

/// What the pane says when a turn was interrupted and the session was kept.
///
/// # This one is authored, and here is exactly what it is
///
/// **No record supplies it**, and that is stated rather than implied.
/// [ADR-0015]'s ruling of 2026-09-05 gave `Ctrl-C` one meaning — leave — and
/// recorded that "interrupt-and-stay was considered and **not** built, because
/// giving one key two meanings depending on whether a turn is running is a
/// decision no record makes". That decision was made on 2026-09-06 as an
/// accepted Update on the same record, under directive 25's "it should look
/// and feel similar to harnesses such as Claude Code", and the line below came
/// with it: **a person who interrupts and sees nothing cannot tell an
/// interrupt from a hang**, which is the failure the whole survey was about.
///
/// It is a delegated coordinator ruling open to Jeshua's veto, in the shape
/// [`ITERATION_IS_ONE_EXCHANGE`] and [`NO_PERSONA`] already have, and its text
/// is quoted verbatim on ADR-0015's Updates so the record owns the words
/// rather than this file.
///
/// **Both halves are facts and neither is reassurance.** That the turn stopped
/// is what the key did; that what it had already written is on disk is
/// [ADR-0010] D2's own promise — "a crash loses at most the event in flight" —
/// arriving without a crash, which is the sentence `terminal-source` recorded
/// for the drop this line now narrates.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
pub const INTERRUPTED: &str = "turn interrupted · the session stays open, and what the turn had \
                               already written is in the transcript";

/// What the pane says when an exchange has generated nothing yet.
///
/// # The gap this closes, measured twice
///
/// [operations/harness-look-and-feel-audit-2] row 2: "the pane does not
/// change for most of a reasoning turn", 34.0 s of a 40.8-second turn and
/// 14.2 s of a 17.5-second one. Re-measured by `turn-liveness` from the
/// release binary at `793712c` on 2026-09-15: **37.55 s of a 40.73-second
/// turn at a hundred columns, 92.2%**, and 56.4 s of about 59 s at forty.
/// A person cannot tell that from a hang.
///
/// **The cause is the provider and not the harness.** Two probes of
/// `streamGenerateContent?alt=sse` the same day timestamped every line of
/// the response: the first SSE byte arrived at 46.0 s of a 55.8-second
/// request and at 92.7 s of a 106.4-second one, with **no frame of any kind**
/// before it. So there is nothing to render, and the honest thing to render
/// is that there is nothing.
///
/// # Why a sentence and not a spinner
///
/// [ADR-0028] D1 refuses a spinner in as many words. This is a narrative row
/// like the ones beside it: painted **once** per exchange that goes quiet,
/// in [`Register::Plain`](zaru_tui::shell::port::Register::Plain) whose
/// marker is the absence of a marker, never repainted and never taken away.
/// `crate::terminal::source::QUIET` is how long an exchange may say nothing
/// first, and `crate::terminal::driver::Pane` is what says it.
///
/// **At forty columns this line is the whole of it.** `Rank::Tokens` and
/// `Rank::Elapsed` are both dropped before `Rank::Context` at that width, so
/// the status row carries no token figure and no clock at all — measured on
/// the same binary, where a 9,201-token turn left the row reading
/// `runtime.tier = bare · 3.8k/1048.5k` throughout. Nothing else on that
/// screen moves.
///
/// # Its subject is the model
///
/// [ADR-0002] D7 forbids a line about the user's own behaviour, and
/// [`crate::compose::emission::Unprompted::StillGenerating`] declares
/// [`Subject::TheModel`](crate::compose::emission::Subject::TheModel) for
/// that reason: what is reported is what the provider is doing, not how long
/// somebody has been sitting there.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
/// [operations/harness-look-and-feel-audit-2]: https://100monkeys-ai.cortex.page/zaru/p/operations/harness-look-and-feel-audit-2
pub const STILL_GENERATING: &str = "the model is generating; nothing has come back yet";

/// What [ADR-0013] D1's layer 1 says when there is no persona to put in it.
///
/// # This one is not transcribed, and here is exactly what it is
///
/// [ADR-0027] D1 serves the system prompt and the persona from a prompt
/// server, and this build reaches one at no tier. That record's Status
/// tracking reserves the question — "whether it ships a cached universal
/// prompt, degrades to a minimal one, or refuses is an open question … it
/// belongs in a record before it is written in code" — and it was decided on
/// 2026-09-05 under directive 20 as the third of those three answers: the
/// harness assembles no layer-1 identity text, and the prefix **says so in one
/// line** so that a reader of the transcript sees the absence rather than
/// inferring it from a prompt that looks short.
///
/// **This sentence is not persona and contains none.** It is a statement about
/// what the harness did not have, in the harness's own voice, of the same kind
/// as [`NOT_A_SANDBOX`]. The persona itself is content Jeshua owns and arrives
/// in a record; nothing here invents a sentence of it, and the day ADR-0027's
/// fetch exists this constant is deleted rather than edited.
///
/// # No reader of the transcript sees it, corrected 2026-09-15
///
/// The paragraph above says the prefix states the absence "so that a reader of
/// the transcript sees the absence rather than inferring it from a prompt that
/// looks short". **That is left as the arc that wrote it left it, and this is
/// the correction beside it: the transcript does not carry the prefix.**
/// [`crate::compose::context::prefix_for`] puts this line in [ADR-0013] D1's
/// layer 1, which is assembled into the prompt at every turn and persisted
/// nowhere — measured on a real session at 100×30 on 2026-09-15, whose
/// `context.json` holds only `exchanges` and whose `transcript.jsonl` holds no
/// prefix of any kind.
///
/// So this line reaches a **model** and no person, which is why
/// [`crate::compose::emission`] holds it as an exemption rather than as a
/// member of [ADR-0002]'s emission set, beside [`SUMMARISE_SPAN`] and
/// [`ITERATION_IS_ONE_EXCHANGE`]. Nothing is changed here but this paragraph:
/// whether the absence *should* be visible to a person is the decision
/// ADR-0027's author and ADR-0013's own reserve, and it is recorded rather
/// than built.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
pub const NO_PERSONA: &str = "[no persona: this harness reached no prompt server, so it has no \
                              system prompt and no persona for this session, and this line is \
                              here so the absence is visible rather than inferred.]";

/// What [ADR-0013] D2's compaction asks the model to do.
///
/// # Two of its three sentences are the record's own words
///
/// D2 says only that "the oldest span of layer 6 is replaced by a **generated
/// summary**" and names no wording, so an instruction had to come from
/// somewhere. It comes from the record rather than from an author:
///
/// - **"keeping the decisions made and the constraints established"** is
///   Alternative 1's own sentence, which is why that alternative was
///   *rejected*: a sliding window "loses decisions made early in a session —
///   which in a coding session is where the constraints were established".
///   That is the record stating what a summary must preserve, and it is the
///   only such statement in it.
/// - **"It replaces these exchanges in your own context; the full text stays
///   on disk"** is D2's, compressed: "the raw span stays in the transcript.
///   History is preserved on disk; only the model's view is compacted."
/// - **"Write the summary and nothing else"** has no source. It was authored
///   under directive 20 of 2026-09-05 as a **functional instruction** rather
///   than persona — it says what shape the answer takes, not who is speaking
///   — because without it the three response arms cannot be told apart: a
///   model that prefaces its answer would put its preamble into layer 6 as
///   though the preamble were the conversation.
///
/// Recorded as an accepted Update on ADR-0013 D2 and quoted for Jeshua at
/// READY, open to his veto exactly as [`NOT_A_SANDBOX`] and [`NO_VALIDATORS`]
/// are. **This is what a model reads and never what a user reads**, which is
/// the one way it differs from every other constant here.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub const SUMMARISE_SPAN: &str = "Summarise the conversation below into a replacement for it, keeping the decisions made and \
     the constraints established. It replaces these exchanges in your own context; the full text \
     stays on disk. Write the summary and nothing else.";

/// What [ADR-0008] D1's iteration is, told to the model that is inside one.
///
/// # Why this exists: a measurement, twice
///
/// The `iteration-wiring` arc ran four tasks against `gemini-3.6-flash` on
/// 2026-09-05 and found that three of them, whose task needed the model to
/// look before acting, spent **every** iteration exploring and never wrote.
/// The `gemini-read-loop` arc then found a defect in this harness's own client
/// — the model's turns were not being resent — and **withdrew** that
/// inference pending re-measurement, because a model handed its own tool
/// output under a name matching no declaration asked again six times in eight,
/// which is a property of the client rather than of the model.
///
/// **The re-measurement was made on the fixed client and the finding stands.**
/// Twenty-four runs, the same four tasks six times each: the task naming what
/// the file must contain succeeded five times of six on its first candidate,
/// and the three that need a look exhausted **eighteen times out of eighteen**.
/// All nineteen exhaustions were one shape — `fs.list`, then `fs.read`, then
/// `fs.list` **again**, never a write — and in eleven of them every byte the
/// model needed was already in the prompt it was reading, carried there by
/// [ADR-0008] D4. So the sentence below is written against a measured failure
/// rather than an anticipated one.
///
/// # Two of its three sentences are the record's own words
///
/// - **"it is one exchange: the tool calls you return are applied together as
///   your whole proposed change"** is that record's own amendment: "An
///   iteration is **one** exchange: the generator asks once, the calls that
///   come back are the candidate, they are applied", with [ADR-0012]'s
///   "`Calls` are the candidate" beside it.
/// - **"it reaches you in the next iteration's prompt, in full, together with
///   what the validators said"** is the rest of the same amendment — "their
///   output reaches the model only in the *next* iteration's refinement
///   prompt" — over D1's own sentence that `Refine` "constructs the next
///   prompt from the previous candidate, the execution output, and the
///   validator's failure text". "In full" is D4's "verbatim, truncated but
///   never paraphrased".
/// - **"So propose the change itself rather than looking first, and never ask
///   again for output this prompt has already given you"** has no source. It
///   was authored under directive 20 of 2026-09-05 as a **functional
///   instruction** rather than persona — it says what shape the answer takes,
///   not who is speaking — and the record names it as the missing sentence in
///   as many words: "nothing in D1, D4 or [ADR-0013] D1's layer 1 says a
///   candidate is a complete proposed change or that an iteration is one
///   shot". Its second half is what the re-measurement above added and what
///   nothing before it could have known.
///
/// Recorded as an accepted Update on [ADR-0008] D1 and quoted for Jeshua at
/// READY, open to his veto exactly as [`NOT_A_SANDBOX`] and [`SUMMARISE_SPAN`]
/// are. **This is what a model reads and never what a user reads**, which it
/// shares with [`SUMMARISE_SPAN`] alone.
///
/// # It is prepended only where it is true
///
/// [`TurnContext::assemble`](crate::compose::TurnContext) adds it when the
/// project declared validators and not otherwise. In the **outer** tool-call
/// loop the sentence is false — there the results of a call do come back
/// inside the turn, on `ModelRequest.results` — so a layer-1 prefix carrying
/// it unconditionally would state a falsehood on every `bare`-tier turn that
/// declares no validators. That asymmetry is the argument for [ADR-0008] D1
/// owning this sentence rather than [ADR-0013] D1's layer 1, and
/// `a_turn_with_no_declared_validators_is_not_told_an_iteration_is_one_exchange`
/// is where it is pinned.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub const ITERATION_IS_ONE_EXCHANGE: &str = "This is one iteration of a validated loop, and it is \
     one exchange: the tool calls you return are applied together as your whole proposed change. \
     Their output does not come back to you inside this exchange; it reaches you in the next \
     iteration's prompt, in full, together with what the validators said. So propose the change \
     itself rather than looking first, and never ask again for output this prompt has already \
     given you.";

/// What a task says when stored keys could not be read and it needs none of
/// them.
///
/// Stored keys are removed from everything sent to a model, and that needs
/// their values. When the store cannot be opened (no keyring and no
/// `ZARU_CREDENTIAL_KEY`) and the task needs nothing from it, the task goes
/// on and says this once. Ruled by the coordinator on 2026-09-28, open to
/// Jeshua's veto.
pub const STORED_KEYS_UNREAD: &str = "Your stored keys could not be read, so Zaru cannot remove \
                                      them from what it sends to the model; this task needs none \
                                      of them, so it goes on.";
