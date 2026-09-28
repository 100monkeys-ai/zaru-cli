// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! One projection per datum, as lines, rendered by whoever calls it.
//!
//! # Why a `Vec<String>` and not a `Display`
//!
//! [ADR-0015] D2's two entry points are one operation, so `/runtime` inside a
//! session and `zaru runtime` outside one must show the same thing. If the
//! binary composed its output with `println!` and the terminal composed its
//! own, there would be two statements of one datum and they would diverge —
//! which is the shape the `Layer` ruling of 2026-09-04 removed from this
//! crate, one level up.
//!
//! So each datum has exactly one projection here, it returns lines, and the
//! caller decides where they go: the binary writes them to standard output,
//! and the composer — when [ADR-0005]'s terminal exists — hands them to a
//! frame. **Nothing in this module prints.**
//!
//! # Nothing here composes a sentence a record already owns
//!
//! Every string a reader sees comes from the datum: [ADR-0001] D1's cells out
//! of `Engagement`, [ADR-0014] D3's `(not set)` and `← effective` out of
//! `config::explain`'s own constants, [ADR-0007] D8's apex marking out of
//! `Reach::marking`, a layer's name out of `Layer::label`. What this module
//! adds is column widths and the order of the lines.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::config::Explanation;
use crate::config::explain::NOT_SET;
use crate::credentials::Listing;
use crate::providers::{ModelTable, ProviderKind, ResolvedModel};
use crate::runtime::Runtime;

/// [ADR-0014] D3's block, as lines.
///
/// The block itself is [`Explanation`]'s own `Display`, which that module
/// wrote to reproduce D3 exactly. This splits it rather than re-rendering it,
/// so there is no second formatter for the one thing D3 spells out.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn explanation(explanation: &Explanation) -> Vec<String> {
    explanation.to_string().lines().map(str::to_owned).collect()
}

/// [ADR-0001] D2's datum, as lines.
///
/// D2: "`/runtime` prints the current tier and **what changing it would
/// alter**." The second half is arithmetic over D1's own table, computed by
/// [`Runtime`]; this walks it. A tier that differs in no column is rendered as
/// saying so, because [`Runtime::would_change`] carries every other tier
/// including one whose diff is empty, and dropping it would answer a
/// different question from the one D2 asks.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
///
/// # What is built comes first, and what is planned is marked as planned
///
/// D1's columns describe what each tier is **planned** to be. Until
/// 2026-09-28 this printed them as the current tier's state, so `contained`
/// read "membrane local containers" while a tool call at `contained` ran on
/// the machine exactly as at `bare`. Now the first lines say what is true
/// today, in the words the session's own warning uses, and D1's cells follow
/// under a line that says none of it is built.
#[must_use]
pub fn runtime(datum: &Runtime) -> Vec<String> {
    let mut lines = vec![
        format!(
            "{} = {} (from {})",
            crate::runtime::KEY,
            datum.tier,
            datum.supplied_by.label()
        ),
        format!("  {}", crate::compose::prose::not_a_sandbox_at(datum.tier)),
        "  Only the bare tier is built. At every tier a tool call runs directly on your \
         machine. The tier also sets the default for runtime.max_iterations."
            .to_owned(),
        String::new(),
        "What each tier is planned to change. None of it is built yet.".to_owned(),
    ];

    for (other, differences) in &datum.would_change {
        lines.push(String::new());
        if differences.is_empty() {
            lines.push(format!("changing to {other} would alter nothing"));
            continue;
        }
        lines.push(format!("changing to {other} would alter"));
        let width = differences
            .iter()
            .map(|difference| difference.column.chars().count())
            .max()
            .unwrap_or(0);
        for difference in differences {
            lines.push(format!(
                "  {:width$}  {} -> {}",
                difference.column, difference.here, difference.there
            ));
        }
    }

    lines
}

/// [ADR-0012] D4's listing, as lines.
///
/// D4: "`zaru models` prints each alias, what it resolved to, and **which
/// layer supplied it**." An alias no layer set is `(not set)` — D3's own
/// spelling, taken from that module's constant rather than retyped, because
/// ADR-0012's own documentation says this listing renders an unresolved alias
/// "the way ADR-0014 D3's block renders `(not set)`".
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[must_use]
pub fn models(table: &ModelTable) -> Vec<String> {
    let rows: Vec<(String, String, String)> = table
        .rows()
        .map(|(alias, resolved)| match resolved {
            ResolvedModel::Resolved {
                model,
                supplied_by,
                named,
            } => (
                alias.to_string(),
                model.as_str().to_owned(),
                // A `--model` value that named an alias says so, because the
                // alias won over any model whose identifier is that name.
                match named {
                    Some(named) => format!(
                        "{}, from --model {named}, which names the alias `{named}`",
                        supplied_by.label()
                    ),
                    None => supplied_by.label().to_owned(),
                },
            ),
            ResolvedModel::Unresolved => (alias.to_string(), NOT_SET.to_owned(), String::new()),
        })
        .collect();

    let alias_width = rows
        .iter()
        .map(|(alias, _, _)| alias.chars().count())
        .max()
        .unwrap_or(0);
    let model_width = rows
        .iter()
        .map(|(_, model, _)| model.chars().count())
        .max()
        .unwrap_or(0);

    rows.iter()
        .map(|(alias, model, layer)| {
            format!("  {alias:alias_width$}  {model:model_width$}  {layer}")
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// The context window a session would use, and which of the three places
/// it came from: the provider, the reader's configuration, or this build's
/// default. See [`crate::providers::window`].
#[must_use]
pub fn window(window: crate::providers::window::Window) -> String {
    format!("  context window: {window}")
}

/// Every session on this machine, as lines.
///
/// The id and nothing else. [ADR-0010] D1 makes a ULID sort lexically by
/// creation time — "so listing sessions in order costs a directory read" — and
/// [`SessionStore::ids`](crate::session::SessionStore::ids) sorts, so the
/// order is the record's rather than a second reading of the clock. A machine
/// with no sessions is told so, because an empty listing and a listing that
/// failed look identical.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[must_use]
pub fn sessions(ids: &[crate::session::SessionId]) -> Vec<String> {
    if ids.is_empty() {
        return vec!["no sessions".to_owned()];
    }
    ids.iter().map(|id| format!("  {id}")).collect()
}

/// What a resumed session restored, as lines.
///
/// **The transcript's own bytes.** [ADR-0010] D1's argument for plain files is
/// that "a harness that shows its work should not store the record of that
/// work somewhere only it can read", so what this shows is the file, not a
/// rendering of the parsed records — which would be a second description of
/// one line. D4's *re-render* is a different act and belongs to [ADR-0005]'s
/// terminal.
///
/// The whole transcript rather than a tail, because no record names a number
/// and a number invented by the thing it bounds is not a number anybody chose.
/// The terminal will pick its own when it re-renders.
///
/// A trailing fragment is reported rather than hidden: D2's promise is that a
/// crash costs at most the event in flight, and a resume that silently dropped
/// it would be the invisible truncation [ADR-0011] D5 forbids one layer up.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[must_use]
pub fn resumed(id: &crate::session::SessionId, resumed: &crate::session::Resumed) -> Vec<String> {
    let mut lines = vec![format!("session {id}")];

    lines.push(match &resumed.checkpoint {
        Some(_) => "  checkpoint restored".to_owned(),
        None => "  no checkpoint was written".to_owned(),
    });
    lines.push(format!(
        "  {} record(s) in the transcript",
        resumed.tail.len()
    ));
    if let Some(bytes) = resumed.fragment {
        lines.push(format!(
            "  and {bytes} byte(s) of an event that was in flight when the process died"
        ));
    }
    if let Some(interrupted) = &resumed.interrupted {
        lines.push(format!(
            "  one tool call did not complete: {}",
            interrupted.call.line
        ));
    }

    lines.push(String::new());
    lines.extend(resumed.tail_lines.iter().cloned());
    lines
}

/// [ADR-0007] D7's `tokens` listing, as lines.
///
/// D7: "`/notes tokens` — list aliases, descriptions, workspace, tool count",
/// and "**`/notes tokens` shows the composer role explicitly.** A user must be
/// able to answer 'which token is my search using' without inspecting
/// configuration."
///
/// D8's apex marking is one of the three places that record requires it, and
/// the marking is [`Reach::APEX_MARKING`](crate::credentials::Reach) rather
/// than a spelling composed here — D8 wants a token marked identically
/// wherever it appears, and a second spelling is how three renderings come to
/// differ.
///
/// **No bearer value is reachable from here by construction.** Until
/// 2026-09-05 that was because the type this walks had no field one could go
/// in; since sealing landed it has exactly one, and the argument narrows
/// rather than lapsing. [`Record::sealed`](crate::credentials::Record) is
/// AES-256-GCM ciphertext whose type has no accessor yielding a plaintext
/// without a key and an alias, this renderer holds neither, and the row it
/// builds does not name that field at all. D3 stays a property of the store
/// rather than a rule this renderer follows.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[must_use]
pub fn tokens(store: &crate::credentials::CredentialStore) -> Vec<String> {
    use crate::credentials::{Reach, Role, StoredReach};

    // **The emptiness this asks about is the emptiness of the *listing*, not
    // of the store**, and the difference is a defect found by running rather
    // than by reading. `store.is_empty()` was the condition until 2026-09-05;
    // once the store could hold a provider key, a machine holding one and no
    // Notes token fell through to a row set that filtered down to nothing and
    // printed **nothing at all** -- no rows, no explanation, exit 0. A
    // command that answers a question with silence is indistinguishable from
    // one that crashed quietly.
    let listed = store.listed(Listing::Notes);
    if listed.is_empty() {
        return vec![
            "no tokens".to_owned(),
            // **Rewritten twice on 2026-09-05, and the second rewrite is the
            // one that matters.** It first said sealing was a port with no
            // implementation; then that `notes tokens add` "is not built, and
            // it needs a Nuclear Notes server to authenticate against". Both
            // were true when written and both stopped being true the same day.
            // A line telling a user why they cannot do something, on a surface
            // where they now can, is worse than no line -- so this one stopped
            // being an excuse and became the command, which is ADR-0016 D2's
            // rule that a remedy names something the binary runs.
            "  add one with: zaru notes tokens add <alias> <host>".to_owned(),
        ];
    }

    // Filtered to Nuclear Notes tokens, and that filter is what stops this
    // listing lying about what it lists. The store has held provider keys
    // since 2026-09-05, and ADR-0007 D7 names this surface `notes tokens`:
    // every column below -- workspace, tool count, instance, composer role --
    // is a Nuclear Notes token's, and a provider key rendered here would show
    // four empty cells under headings that do not apply to it. The other
    // listing is `zaru providers keys`, and the two share
    // `CredentialStore::listed`, which both listings are built from.
    let rows: Vec<[String; 6]> = listed
        .into_iter()
        .zip(store.records().filter(|(_, record)| record.is_notes()))
        .map(|(listed, (_, record))| {
            [
                listed.alias,
                listed.description,
                record
                    .workspace()
                    .map_or_else(|| NOT_SET.to_owned(), str::to_owned),
                format!("{} tool(s)", record.tools().len()),
                match record.reach() {
                    Some(StoredReach::Apex) => Reach::APEX_MARKING.to_owned(),
                    Some(StoredReach::InstanceLocked(instance)) => instance.clone(),
                    None => String::new(),
                },
                if record.role() == Some(Role::Composer.as_str()) {
                    Role::Composer.as_str().to_owned()
                } else {
                    String::new()
                },
            ]
        })
        .collect();

    let mut widths = [0usize; 6];
    for row in &rows {
        for (slot, cell) in widths.iter_mut().zip(row) {
            *slot = (*slot).max(cell.chars().count());
        }
    }

    rows.iter()
        .map(|row| {
            let padded: Vec<String> = row
                .iter()
                .zip(widths)
                .map(|(cell, width)| format!("{cell:width$}"))
                .collect();
            format!("  {}", padded.join("  ")).trim_end().to_owned()
        })
        .collect()
}

/// What [ADR-0009] D6's `zaru init` says it did.
///
/// The path it wrote, so a user in a deep directory can see *which* file
/// appeared, and the one sentence that says the template is the record's
/// example rather than a guess about this project — which is that record's
/// rejected Alternative 1 stated where a user will meet it.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[must_use]
pub fn initialised(path: &std::path::Path) -> Vec<String> {
    vec![
        format!("wrote {}", path.display()),
        String::new(),
        "It is the manifest's worked example, not a guess about this project: nothing here"
            .to_owned(),
        "infers a name, a language or a build command, because a validator set nobody can"
            .to_owned(),
        "read is not a contract. Edit it before running anything against it.".to_owned(),
    ]
}

/// `zaru providers keys` — which providers this machine holds a key for.
///
/// # What it prints, and the one thing it cannot
///
/// The alias, the kind and the description, built from
/// [`Listed`](crate::credentials::Listed) — which has no field a bearer value
/// could occupy. So "this listing never prints a key" is a property of the
/// type both listings are built from rather than a rule each renderer keeps
/// separately.
///
/// It deliberately does **not** print a length, a prefix, a fingerprint or a
/// masked form of the key. Every one of those is a fact about the value, and
/// a fact about a value is what a listing exists to avoid carrying: a length
/// tells an onlooker which of two keys is stored, and a masked prefix is the
/// part of a credential that most often identifies the account.
#[must_use]
pub fn provider_keys(store: &crate::credentials::CredentialStore) -> Vec<String> {
    let listed = store.listed(Listing::Providers);
    if listed.is_empty() {
        return vec![
            "no provider key is stored on this machine.".to_owned(),
            format!(
                "  add one with `zaru providers keys add <kind>`, which reads the key from \
                 standard input: {}",
                ProviderKind::ALL
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ];
    }

    let rows: Vec<[String; 3]> = listed
        .into_iter()
        .map(|entry| [entry.alias, entry.kind, entry.description])
        .collect();

    let mut widths = [0usize; 3];
    for row in &rows {
        for (slot, cell) in widths.iter_mut().zip(row) {
            *slot = (*slot).max(cell.chars().count());
        }
    }

    rows.iter()
        .map(|row| {
            let padded: Vec<String> = row
                .iter()
                .zip(widths)
                .map(|(cell, width)| format!("{cell:width$}"))
                .collect();
            padded.join("  ").trim_end().to_owned()
        })
        .collect()
}

/// [ADR-0012] D7's line, on the status row and on the way out.
///
/// D7: "Every request records prompt tokens, completion tokens, and — where
/// the provider publishes pricing — cost. Per turn in the status line, per
/// session on exit." **This one function is both halves**, as of the
/// `status-line` arc of 2026-09-05: `compose::turn::rendered` prints it when
/// the session ends and `terminal::driver::refresh_status` puts the same
/// string on [`zaru_tui::shell::Status`], so the two spellings D7 asks for
/// cannot disagree about a word.
///
/// **Nothing here computes a cost.** `TokenUsage` carries one only when a
/// provider reported it, and `providers::usage` refuses to invent a rate: "no
/// rate, no currency, no rounding". So a run against a provider that publishes
/// no pricing prints two numbers and their sum, and says nothing about money.
///
/// # The sum is this function's arithmetic, and that is worth stating exactly
///
/// This paragraph read "The total is `TokenUsage`'s own, not this function's
/// arithmetic" until 2026-09-05, and it was **false**: `providers::TokenUsage`
/// has no `total` and cannot have one, because a check in that module forbids
/// the string `fn total` there — D7 names two quantities and a third would be
/// one more thing to keep consistent. So the sum below is computed here.
///
/// What the retired sentence was protecting is real and is unaffected: the
/// `gemini` client reports `completion_tokens` as candidates **plus**
/// thoughts, so `prompt + completion` equals the provider's own
/// `totalTokenCount` including the thinking tokens D7 knows nothing about.
/// That equality is a property of the **mapping**, checked in
/// `providers::gemini::tests`, and it was never a property of this addition.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[must_use]
pub fn usage(usage: &crate::providers::TokenUsage) -> String {
    let mut line = format!(
        "tokens: {} prompt + {} completion = {}, counted by the provider",
        usage.prompt_tokens(),
        usage.completion_tokens(),
        usage.prompt_tokens() + usage.completion_tokens(),
    );
    if let Some(cost) = usage.cost() {
        line.push_str(&format!(" · {cost}"));
    }
    line
}

/// [ADR-0013] D3's and D4's lines, **without** the leading marker.
///
/// # Both lines are the record's own, transcribed rather than composed
///
/// D3: `◈ compacted 34 earlier turns · 18.2k → 2.1k tokens · full history in
/// transcript`. D4: `◈ dropped attachment: adrs/0117-aegis-edge-mode ·
/// re-attach with [[`. Every word, both separators and the arrow are quoted
/// from those two examples; nothing here is worded by this arc.
///
/// **The marker is deliberately not here.** `zaru-tui`'s
/// [`Register::Announced`](zaru_tui::shell::Register) already owns `◈` and its
/// contract is that "the text is the producer's… The shell chooses the glyph
/// and nothing else", so a producer emitting the glyph would put it on the
/// line twice inside the pane. The out-of-session path, which has no
/// register, prepends [`ANNOUNCEMENT_MARKER`].
///
/// # The one reading, named rather than slipped in
///
/// D3 renders `18.2k` and `2.1k` and states no rule for the abbreviation.
/// [`thousands`] is that rule read off those two examples: below a thousand
/// the integer, at or above it one decimal place and `k`. Accepted under
/// directive 20 of 2026-09-05 and recorded on ADR-0013 D3, open to Jeshua's
/// veto — it is the only thing on either line this arc chose.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[must_use]
pub fn announcement(announcement: &zaru_core::context::Announcement) -> String {
    use zaru_core::context::Announcement;
    match announcement {
        Announcement::Compacted {
            turns,
            before,
            after,
        } => format!(
            "compacted {turns} earlier turns · ~{} → ~{} tokens, estimated · full history in \
             transcript",
            thousands(*before),
            thousands(*after),
        ),
        // The workspace is rendered with the path and is not decoration:
        // `ItemId`'s own documentation is that "two workspaces may each hold
        // `architecture/bounded-contexts` and they are different pages", so a
        // line naming the path alone would name two attachments identically.
        // D4's own example, `adrs/0117-aegis-edge-mode`, reads as exactly this
        // pair on the instance that holds an `adrs` workspace.
        Announcement::AttachmentDropped {
            identity,
            how_to_reattach,
        } => format!(
            "dropped attachment: {}/{} · {how_to_reattach}",
            identity.workspace(),
            identity.path(),
        ),
    }
}

/// The marker [ADR-0013] D3 and D4 open both announcement lines with.
///
/// Used only where there is no register to carry it — the out-of-session
/// turn, which prints to standard output. Inside the shell the pane's
/// `Register::Announced` supplies the same character, and this constant is
/// **not** what it reads: two spellings of one glyph would be a rule in two
/// places, and the one that renders in the pane is `zaru-tui`'s because that
/// is where ADR-0008 D3 puts rendering.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub const ANNOUNCEMENT_MARKER: &str = "◈";

/// A token count as [ADR-0013] D3's line abbreviates one.
///
/// Below a thousand the integer; at or above it, one decimal place and `k`.
/// Read off D3's own `18.2k` and `2.1k` — see [`announcement`] for why that
/// reading is named rather than assumed. Truncating rather than rounding, so
/// the abbreviation never reports more than was measured.
///
/// Why the inner loop stopped, in [ADR-0008] D5's "`ExhaustionReason`'s own
/// words".
///
/// # One wording, two readers
///
/// This sentence was inside [`crate::cli::classify`]'s `loop_exhausted` until
/// 2026-09-05, where it was reached only by the classified failure the binary
/// exits on. The pane reached none of it and rendered `{reason:?}` instead, so
/// a run that stopped had one explanation at the exit code and a Rust struct
/// dump on the screen. It is here now for the reason [`announcement`] is —
/// this module is the one projection per datum, and both callers come through
/// it.
///
/// **The words are not chosen here.** `CeilingReached` is [ADR-0008] D5's own
/// account of what happened; the window arm is [ADR-0013] D7's, and it names
/// both numbers because D3's own argument for the variant carrying them is
/// that "D7 asks for a *clear* reason and a reader cannot act on 'the window
/// was exceeded' without knowing by how much".
///
/// `iterations` is the count the event carries rather than the ceiling, which
/// no event holds. On the ceiling route they are the same number by
/// construction — the machine checks `n >= ceiling` and reports `n`.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[must_use]
pub fn exhaustion(iterations: u32, reason: zaru_core::iteration::ExhaustionReason) -> String {
    use zaru_core::iteration::ExhaustionReason as Why;
    match reason {
        Why::CeilingReached => format!("the ceiling of {iterations} iteration(s) was reached"),
        Why::ContextWindowExceeded { needed, window } => format!(
            "assembling the next iteration needed an estimated {needed} tokens and the window \
             allows {window}"
        ),
    }
}

/// What one declared validator reported, in [ADR-0009] D2's own words.
///
/// Three outcomes rather than a boolean, because that record "makes `skipped`
/// distinct from `passed` and `failed` on purpose: a validator whose
/// prerequisite failed did not run, and reporting that as a pass is exactly
/// the silent green that decision exists to prevent". So the third arm says
/// the validator did not run rather than saying anything about whether it
/// would have held.
///
/// The verbs are `ValidatorOutcome`'s own documentation, which is that
/// record's text: "ran and its expectation held", "ran and its expectation did
/// not hold", "did not run, because a validator it declared `after` failed".
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[must_use]
pub const fn validator_outcome(outcome: zaru_core::iteration::ValidatorOutcome) -> &'static str {
    use zaru_core::iteration::ValidatorOutcome as What;
    match outcome {
        What::Passed => "passed",
        What::Failed => "failed",
        What::Skipped => "did not run",
    }
}

/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[must_use]
pub fn thousands(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    // Integer arithmetic throughout: a `u64` past 2^53 is not representable
    // as an `f64`, and a context window is a number a provider chooses.
    let whole = tokens / 1_000;
    let tenth = (tokens % 1_000) / 100;
    format!("{whole}.{tenth}k")
}

/// [ADR-0013] D6's context usage, as the status line carries it.
///
/// D6: "The status line carries context usage continuously. Approaching the
/// threshold is not an event to announce — it is a number that has been
/// visible all along." Trigger clause 5 is the same sentence as a check:
/// "Context usage is present in the status line throughout."
///
/// # Both numbers, because one of them cannot be approached
///
/// [`Usage`] carries what is used and the window it is measured against, and
/// **both are rendered**. D6's whole claim is that a user can see pressure
/// building before it becomes an event, and a bare count of what is used is a
/// number nobody can read as near or far — approaching is a relation, so it
/// needs the thing being approached.
///
/// **The pressure threshold itself is deliberately not shown.** It is what
/// [`crate::cli::layers::context_limits`] derives from the kind's window, it
/// is not on `Usage`, and putting it here would be a third number on a row
/// two records already share. What D6 asks for is that the number be visible and rising; where
/// compaction begins is [ADR-0013] D3's announcement's job, which says so at
/// the moment it happens. Ruled 2026-09-05 under directive 20, open to
/// Jeshua's veto.
///
/// # The abbreviation is D3's and the unit is D3's word
///
/// [`thousands`] is the record's own, read off `18.2k` and `2.1k`, and
/// `tokens` is the word D3's line uses. The leading word `context` names
/// which of the row's segments this is — the row carries two.
///
/// # The used figure is an estimate, and it says so
///
/// Since 2026-09-28 (`token-accounting`) what is used is **estimated in
/// tokens**: the request's bytes divided by the bytes-per-token ratio
/// [`Calibration`](crate::providers::capacity::Calibration) learned from the
/// provider's own counts, or its starting ratio before the first answer.
/// Before that it was a byte count printed as tokens. So the figure carries a
/// `~` at every width and the full form ends `estimated`; the provider's own
/// count is the row's other segment, [`usage_row`], which says `counted`.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [`Usage`]: zaru_core::context::Usage
#[must_use]
pub fn context_usage(usage: zaru_core::context::Usage, window: Window) -> String {
    match window {
        Window::Known => format!(
            "context ~{}/{} tokens, estimated",
            thousands(usage.used()),
            thousands(usage.window())
        ),
        Window::Unknown => format!("context ~{} tokens, estimated", thousands(usage.used())),
    }
}

/// Whether this session knows the window its context is measured against.
///
/// # Why the row needs this and the usage figure cannot carry it
///
/// [`Usage`](zaru_core::context::Usage) always carries a window, because a
/// [`Context`](zaru_core::context::Context) is built with limits and a session
/// that resolved no provider is built with
/// [`crate::cli::layers::WINDOW_WHEN_NO_PROVIDER`] — the smallest window any
/// kind in this binary states, chosen so that the shape claims no more room
/// than something real. That is the right number for the *shape*, and it is
/// not a claim the row may make: no provider answered, so nothing on this
/// machine has said how much room there is.
///
/// Row 6 of [the second look-and-feel audit] measured the alternative, which
/// was to show nothing at all: with no provider the row read
/// `runtime.tier = bare · session …` for the whole session, at every width,
/// while [ADR-0013] D6's trigger clause 5 asks for the figure "throughout".
///
/// **The used figure alone is not the narrowing [ADR-0001]'s amendment
/// refused.** That refusal is "narrowing the context figure past its window
/// destroys D6's relation", and the relation is approaching: a session that
/// resolved no provider can run no turn, its layer 6 never grows, and there is
/// nothing to approach. Where a window is known both numbers stay, at both
/// widths, exactly as they were.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [the second look-and-feel audit]: https://100monkeys-ai.cortex.page/zaru/p/operations/harness-look-and-feel-audit-2
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    /// [ADR-0012] D3's capability descriptor for the kind that resolved.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    Known,
    /// No provider resolved, so the second figure would be a number nobody on
    /// this machine has claimed.
    Unknown,
}

/// The token count as [ADR-0001] D2's row carries it, in both its spellings.
///
/// # Two spellings of one datum, and the total is computed once
///
/// The full form is [`usage`] — the very line the session prints on exit, so
/// the row and that line cannot disagree about a word. The narrow form is the
/// total and the unit, which is what a row 40 columns wide has room for, and
/// **it is the same sum**: both come from `tokens_total`, so the two
/// spellings cannot disagree about the number either. `469 tokens` against
/// `tokens: 390 prompt + 79 completion = 469` costs thirty columns and drops
/// no datum a narrow row could have shown anyway.
///
/// This is [`Rank::Tokens`](zaru_tui::shell::Rank::Tokens), which is
/// [ADR-0012] clause 6's, and it is where the narrow spelling was accepted on
/// that record's amendments page on 2026-09-06.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[must_use]
pub fn usage_row(spent: &crate::providers::TokenUsage) -> zaru_tui::shell::Segment {
    zaru_tui::shell::Segment::new(
        usage(spent),
        format!("{} tokens counted", tokens_total(spent)),
    )
}

/// What a request spent, in the one place both spellings read it.
///
/// A second addition at a second call site is how the row and the exit line
/// would come to disagree about a number while agreeing about every word.
fn tokens_total(spent: &crate::providers::TokenUsage) -> u64 {
    spent.prompt_tokens() + spent.completion_tokens()
}

/// The one line a session opens on, naming where it is and how to ask.
///
/// # Why there is a line here at all
///
/// Row 9 of [the second look-and-feel audit] measured the opening frame: one
/// status row and twenty-nine blank rows at 100×30, twenty-three at 40×24. No
/// greeting, nothing saying what to type or how to leave, and **the working
/// directory — which decides what every tool call may touch and which session
/// `--continue` resumes — on no surface at all**. The first survey's row 14
/// refused a path on the *status row* for a reason that still stands, that a
/// path is unbounded and would compete with two clauses for one row's width;
/// this is not the status row.
///
/// # It is caused output, and that is a reading rather than a claim
///
/// [ADR-0002] D1 is "Zaru never emits output the user did not cause", and its
/// causes are "The user sent a message, **ran a command**, or is in a turn
/// Zaru is currently serving". Running `zaru` is running a command, and
/// [`Cause::UserMessage`](crate::compose::emission::Cause::UserMessage)
/// already reads "including the command that opened the session this line is
/// painted in". **Accepted 2026-09-15 under directive 20 and open to Jeshua's
/// veto**, written on that record's amendments page; if the reading is refused
/// the row closes as measured-and-correct and this line goes.
///
/// # One sentence and one path
///
/// [`OPENING`] is the whole of what is authored — one constant, one edit to
/// change — and the directory is the canonical root [ADR-0011] D4 bounds every
/// tool call by, the same value `meta.toml` records. It is painted in
/// [`Register::Plain`](zaru_tui::shell::port::Register::Plain): no marker, no
/// colour and no seventh register, because no record gives an opening line
/// one and inventing one would be authoring.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [the second look-and-feel audit]: https://100monkeys-ai.cortex.page/zaru/p/operations/harness-look-and-feel-audit-2
#[must_use]
pub fn opening(here: &std::path::Path) -> String {
    // The separator is authored here rather than read off
    // `zaru_tui::shell::SEPARATOR`: that constant is the *status row's*
    // structure, which is why a field carrying it paints as two, and a pane
    // line borrowing it would tie one to the other for the sake of a glyph.
    format!("{} · {OPENING}", here.display())
}

/// The authored half of [`opening`], and the only sentence this arc adds to
/// the door.
///
/// A constant so that changing what a session says at its first frame is one
/// edit in one place, which is the discipline
/// [`crate::compose::prose`] already keeps for every line a person reads
/// unasked. It lives here rather than there because the line a person meets
/// carries a path as well, so its wording is **composed** rather than quoted —
/// see [`crate::compose::emission::Unprompted::Opening`], which records it
/// that way.
pub const OPENING: &str = "type a task, or `/help` for what this harness can do";

/// [ADR-0013] D6's context figure as the row carries it, in both spellings.
///
/// The full form is [`context_usage`]. The narrow form drops the leading word
/// and D3's unit and **keeps both numbers**, because D6's whole claim is that
/// a user can watch pressure build and "approaching is a relation" — a figure
/// without its window is one nobody can read as near or far, so the window is
/// the one thing a narrow spelling may not drop.
///
/// This is [`Rank::Context`](zaru_tui::shell::Rank::Context), the best rank
/// after the tier, because D6 says "continuously" and its trigger clause 5
/// says "throughout".
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[must_use]
pub fn context_row(usage: zaru_core::context::Usage, window: Window) -> zaru_tui::shell::Segment {
    // **The unit stays in the narrow form where the window is gone, and it is
    // dropped where the window is there.** Measured at forty columns: with
    // both numbers `1.4k/1048.5k` reads as a figure over its room and needs no
    // noun, and with one `1.4k` is a number with nothing saying what it counts.
    let narrow = match window {
        Window::Known => format!("~{}/{}", thousands(usage.used()), thousands(usage.window())),
        Window::Unknown => format!("~{} tokens", thousands(usage.used())),
    };
    zaru_tui::shell::Segment::new(context_usage(usage, window), narrow)
}

/// Which model is answering, as [ADR-0001] D2's row carries it.
///
/// # The identifier and not the alias
///
/// [operations/harness-look-and-feel] row 14 measured that the model is "the
/// field a person checks most" and is not on the row, and [ADR-0012] D4's
/// surface for it is `zaru models`, which is out of session. What that row is
/// asking is *which model is answering*, and an alias answers it only for
/// somebody who already knows the resolution — so this is the resolved
/// identifier.
///
/// # It is the one field a cloned repository chooses, so it cannot forge one
///
/// `model.<alias>` is **free at every configuration layer** — [ADR-0012] D4
/// lists project configuration among the five that resolve an alias — so the
/// string here is one `./zaru.toml` can set. [`ModelId`](crate::providers::ModelId)
/// refuses control
/// characters, an empty value and surrounding whitespace, and bounds nothing
/// else, so an identifier containing [`zaru_tui::shell::SEPARATOR`] would
/// paint as **two** fields and the second could read as a tier: a repository
/// setting `x · runtime.tier = linked` would put a second membrane claim on
/// the one row [ADR-0001] D2 exists to make unambiguous.
///
/// **An identifier that could say something the row itself says is not painted,
/// and the field is absent instead.** Replacing the separator with a space was
/// tried first and is not enough — it was caught by this arc's own corpus
/// check, which printed `runtime.tier = bare · x runtime.tier = linked ·
/// session …`: the forged field stopped being a *field* and went on reading as
/// a tier. So the rule is a refusal over both of the row's structural
/// spellings, [`zaru_tui::shell::SEPARATOR`] and
/// [`zaru_tui::shell::TIER_PREFIX`], and losing the field is the direction to
/// be wrong in on a security boundary. It cannot be done in the row: a field
/// that legitimately carries the separator exists — [`usage`] appends a cost
/// after one — so a blanket rule there would corrupt the token line.
///
/// **No provider names a model this way**, so nothing real is refused; and
/// nothing is altered either, so `zaru models` still prints exactly what
/// configuration said. The mode needs no such rule: `Mode::from_layer` refuses
/// the project layer outright under [ADR-0014] D6's escalation ceiling.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ModelId]: crate::providers::ModelId
/// [operations/harness-look-and-feel]: https://100monkeys-ai.cortex.page/zaru/p/operations/harness-look-and-feel
#[must_use]
pub fn model_row(model: &crate::providers::ModelId) -> Option<String> {
    let rendered = model.to_string();
    let forgeable = [zaru_tui::shell::SEPARATOR, zaru_tui::shell::TIER_PREFIX];
    forgeable
        .iter()
        .all(|spelling| !rendered.contains(spelling))
        .then_some(rendered)
}

/// [ADR-0011] D3's permission mode, as the row carries it.
///
/// The three names are that record's own, through [`Mode::as_str`], and the
/// leading word says which of the row's fields this is. **A mode on the row is
/// always the resolved one**, which D3 fixes for the life of a session, and it
/// can never be a word a cloned repository chose: `Mode::from_layer` refuses
/// the project layer under [ADR-0014] D6's escalation ceiling.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
/// [`Mode::as_str`]: crate::tools::Mode::as_str
#[must_use]
pub fn mode_row(mode: crate::tools::Mode) -> String {
    format!("mode {}", mode.as_str())
}
