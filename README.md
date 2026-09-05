# Zaru

Zaru is the companion layer of the 100monkeys platform: a personal AI that
works alongside its user, remembers them, and shows its work. This repository
is the local development harness — the terminal side of that companion.

Every other AI harness asks you to trust it. Zaru shows you: the execution loop
is visible, the membrane is physics rather than promises, retrieved knowledge is
surfaced for you to choose rather than silently injected, and the harness is
open source. Where a design choice would make behaviour less legible in exchange
for a smoother surface, the legible option wins. That trade is the product.

## Status

**Pre-alpha. Nothing here is installable, and the harness cannot run a task.**
Six crates compile, CI enforces the rules the repository is meant to hold
itself to, and as of 2026-09-05 the binary does something a person can see.

`zaru` reads its arguments, resolves configuration, and prints what is already
on this machine. Six commands run:

```sh
zaru runtime                  # the tier, what it engages, and what changing it would alter
zaru models                   # each model alias, what it resolved to, and which layer said so
zaru config explain <key>     # every layer's value for one key, with the effective one marked
zaru sessions list            # every session on this machine
zaru sessions rm <id>         # delete a session's directory, with no tombstone
zaru notes tokens             # the stored Nuclear Notes tokens and which is the composer's
```

`--runtime <tier>` and `--model <identifier>` set the two keys the flag layer
carries for one run. `--resume <id>` and `--continue` restore a session and
print its transcript. `--help` lists exactly what runs and nothing else.

**It cannot run a task**, and it says so rather than letting you find out: no
provider client exists anywhere in this workspace, so there is nothing for the
agent loop to ask. A task invocation is refused, naming what is missing.

Ten pieces exist behind that binary and the command surface reaches six of them.
The iteration loop and the tool-call loop are in `zaru-core`, headless, driven
through ports that nothing in any product tree implements. The composer is in
`zaru-tui`, rendered under a test backend, reaching its two search tiers through
a port and a request/response pair that nothing implements either — **nothing
this binary prints goes through it**; the terminal is unbuilt and every command
above writes plain lines to standard output.

The credential store is in `zaru-cli`, holding named tokens on disk with no
bearer value among them — sealing is a port with no implementation, so no
secret is written anywhere, and so `zaru notes tokens` lists nothing on a
machine that has not been given one by a test. The configuration hierarchy is
in `zaru-cli`, resolving five layers over a schema whose keys arrive from the
records that own them; **three of the five layers have readers** — the built-in
one, `ZARU_*`, and the command line — and the two that read files wait on a
TOML parser. The local tool surface is in `zaru-cli` too: the seven built-in
tool names, the working-directory boundary, and the permission model. **Three
of the seven act**: `fs.read` and `fs.list` through `std::fs` inside that
boundary, and `cmd.run` as a real child process, started at the boundary's root
with a cleared environment and a wall-clock ceiling the caller supplies. A
command is not measured against the boundary as though it were a path — its
boundary is the directory it starts in — and **nothing contains that child**:
at `bare` tier the harness is not a sandbox and the decision record says so.
The other four sit behind ports with no implementation, as does the permission
prompt itself.

The Nuclear Notes client is in `zaru-notes`: a session over MCP with the
workspace named on every read, a bearer value the type system will not render,
and three staleness signals. It reaches no network, because the transport is a
port with no implementation. Every check against it exchanges real protocol
bytes over an in-memory pipe.

The session lifecycle is in `zaru-cli`. A session is a directory named by a
ULID holding plain files, because a harness that shows its work should not
store the record of that work somewhere only it can read: an append-only
transcript of one event per line, a checkpoint rewritten atomically beside it,
a resume that restores and never re-executes, and bounded retention whose
deletion is real. `zaru --resume` prints the transcript's own bytes for exactly
that reason. Its `meta.toml` has no writer — the dependency table's `toml` row
has no caller yet — so that file is a port with no implementation, like the
others above. **The binary starts no session**: it reads the ones that are
there and creates nothing by being asked a question.

Cutting across three of the pieces above is one port rather than an eleventh:
every path from captured bytes into a model prompt passes a `Redactor`, and the
type a prompt is built from cannot be made any other way. The single
implementation removes the bearer values the harness is itself holding in the
credential store, by exact value and by the ASCII core an escaping formatter
would leave intact. It matches no patterns and looks for nothing it does not
hold, so a secret the harness never saw is out of scope and said to be. The
transcript, the checkpoint and the preserved output of an oversized command
keep the raw bytes: redaction is on what a model reads, not on the record.
**Nothing this binary prints passes through it**, because nothing it prints is
a prompt.

The error taxonomy is in `zaru-cli`, and it is what every command exits
through. Five classes of failure, each carrying by construction what its class
owes the reader; a mapping from every error the workspace already raises to the
class a decision record states for it; and a boundary around everything the
binary does, so that a bug in the harness is reported as a bug in the harness
rather than as a Rust panic. Three of its six exit codes are now reachable from
the real artefact: `0`, `2` for anything the reader can change, and `4` for a
model that resolved to a provider this build cannot reach.

The runtime tiers are in `zaru-cli` as well, and `zaru runtime` is the first
thing that shows one. **The tier names are ADR-0001's and this file
deliberately does not restate them**, because they become effectively permanent
at first publication and the record wants its review before then.

The harness is pre-alpha in the load-bearing sense too: it carries no
backward-compatibility shims and no legacy code paths, and anything that looks
like one should be removed rather than preserved.

## Layout

| Crate | Holds |
| --- | --- |
| `zaru-core` | The agent loop, the iteration state machine, the validator contract, and the event stream. Headless. |
| `zaru-tui` | The terminal interface and the composer. Subscribes to the event stream. |
| `zaru-notes` | The Nuclear Notes client. |
| `zaru-seal` | Zaru's own SEAL implementation, written against the SEAL v1 RFC. |
| `zaru-aegis` | The AEGIS orchestrator client, over MCP and SEAL across a process boundary. |
| `zaru-cli` | The `zaru` binary: configuration, session lifecycle, and the error taxonomy. |

The crate boundaries and the dependency edges between them are decided by
ADR-0003 D8 and enforced by `scripts/check-crate-boundaries.py`, which fails on
any edge the decision does not allow.

The harness runs at three levels of platform engagement. **The runtime tiers are
named and defined by ADR-0001**, which is the authority for them; this file
deliberately does not restate the names, because they become effectively
permanent at first publication and the record wants its review before then.

## Building

```sh
cargo build --workspace
cargo test --workspace
```

The toolchain is pinned in `rust-toolchain.toml` and rustup will honour it. A
build needs a registry: `Cargo.lock` resolves 109 packages, six of which are
this workspace's own. The third-party set is `rmcp` for the Nuclear Notes
client, `ratatui` and `tui-textarea` for the composer, `serde` and `serde_json`
for the credential store, `tokio` for the client's channels, for the
binary crate's own check that drives a session end to end, and for polling the
loop's futures under `#[tokio::test]`, and what those six pull in. Which dependencies the harness may carry is ADR-0003 D2's
to decide, and `[workspace.dependencies]` is where each arrives once it has a
caller.

## Where the knowledge is

**A repository holds code. It does not hold knowledge.** The architecture, the
decision records, the operating principles, the testing contract, and the commit
workflow live in the Zaru workspace at
<https://100monkeys-ai.cortex.page/zaru/>. `CLAUDE.md` in this directory is a
bootstrap that points there and nothing more; where it and the workspace
disagree, the workspace wins.

## Contributing

Contributions are accepted under the Developer Certificate of Origin — a
`Signed-off-by` line on every commit, no copyright assignment, nothing to sign.
See [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

Apache-2.0. See [LICENSE](LICENSE), [NOTICE](NOTICE), and
[THIRD_PARTY.md](THIRD_PARTY.md).
