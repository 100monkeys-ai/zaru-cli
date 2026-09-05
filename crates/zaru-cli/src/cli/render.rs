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
#[must_use]
pub fn runtime(datum: &Runtime) -> Vec<String> {
    let engagement = datum.engagement;
    let mut lines = vec![
        format!(
            "{} = {} (from {})",
            crate::runtime::KEY,
            datum.tier,
            datum.supplied_by.label()
        ),
        format!("  membrane  {}", engagement.membrane.as_str()),
        format!("  loop      {}", engagement.r#loop.as_str()),
        format!("  cortex    {}", engagement.cortex.as_str()),
        format!("  network   {}", engagement.network.as_str()),
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
    let rows: Vec<(String, String, &str)> = table
        .rows()
        .map(|(alias, resolved)| match resolved {
            ResolvedModel::Resolved { model, supplied_by } => (
                alias.to_string(),
                model.as_str().to_owned(),
                supplied_by.label(),
            ),
            ResolvedModel::Unresolved => (alias.to_string(), NOT_SET.to_owned(), ""),
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
            // Rewritten 2026-09-05: this said sealing was a port with no
            // implementation and that no secret is written anywhere, which
            // stopped being true the day sealing landed. A line telling a user
            // why they cannot do something has to name the reason that is
            // actually stopping them, and that reason is now the surface
            // rather than the store.
            "  nothing in this harness can add one yet: ADR-0007 D7's `notes tokens add` is not \
             built, and it needs a Nuclear Notes server to authenticate against"
                .to_owned(),
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
        "It is ADR-0009 D1's worked example, not a guess about this project: nothing here"
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
        "tokens: {} prompt + {} completion = {}",
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
            "compacted {turns} earlier turns · {} → {} tokens · full history in transcript",
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
/// **The pressure threshold itself is deliberately not shown.** It is
/// [`crate::cli::layers::PRESSURE_THRESHOLD_TOKENS`], it is not on `Usage`,
/// and putting it here would be a third number on a row two records already
/// share. What D6 asks for is that the number be visible and rising; where
/// compaction begins is [ADR-0013] D3's announcement's job, which says so at
/// the moment it happens. Ruled 2026-09-05 under directive 20, open to
/// Jeshua's veto.
///
/// # The abbreviation is D3's and the unit is D3's word
///
/// [`thousands`] is the record's own, read off `18.2k` and `2.1k`, and
/// `tokens` is the word D3's line uses. **Nothing here is authored except the
/// separator** between the two numbers and the leading word `context`, which
/// name which of the row's segments this is — the row carries two.
///
/// The count is honest about what it counted: `compose::count::ByteCounter`
/// measures **bytes** against a window stated in tokens, deliberately and with
/// its reasons in that module. This renders the number the harness actually
/// holds rather than one it would like to.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [`Usage`]: zaru_core::context::Usage
#[must_use]
pub fn context_usage(usage: zaru_core::context::Usage) -> String {
    format!(
        "context {}/{} tokens",
        thousands(usage.used()),
        thousands(usage.window())
    )
}
