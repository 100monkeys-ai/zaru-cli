// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The three sentences a turn shows a person, transcribed from the records
//! and authored nowhere.
//!
//! # Why these are here and not at the places that print them
//!
//! Each of the three is a **required argument with no default** at the module
//! that uses it, and each of those modules says why in its own words. From
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
//! **The exception is [`NO_PERSONA`]**, which no record drafted, and its own
//! documentation says exactly what it is and is not.
//!
//! [Autonomous Development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development

/// [ADR-0011] D2's statement that `bare` tier is not a sandbox.
///
/// D2: "**At bare tier the harness states plainly, once at session start, that
/// it is not a sandbox.** A permission prompt that reads like containment
/// while being a suggestion is worse than no prompt, because it manufactures a
/// confidence the user has not earned."
///
/// **Transcribed verbatim from that record's Update of 2026-09-04**, which
/// introduces it as "The exact text proposed, for this record's author to
/// accept or replace". It is still a proposal; this constant is where it is
/// said, not where it was decided.
///
/// [`SessionNotice::for_tier`](crate::tools::SessionNotice::for_tier) returns
/// nothing at `contained` and `linked`, where a membrane exists and the
/// sentence would be false — so the line is not stated where it would be
/// untrue by absence rather than by a branch here.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub const NOT_A_SANDBOX: &str = "bare tier has no membrane. Zaru is not a sandbox here: a tool \
                                 call runs with your permissions, on your machine, and a prompt \
                                 is a question rather than a barrier. Run --runtime contained for \
                                 a membrane that enforces rather than asks.";

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
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
pub const NO_PERSONA: &str = "[no persona: this harness reached no prompt server, so it has no \
                              system prompt and no persona for this session, and this line is \
                              here so the absence is visible rather than inferred.]";
