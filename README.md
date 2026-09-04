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

**Pre-alpha. Nothing here is installable, and no binary does any work yet.**
Six crates compile, a binary prints what it is composed of, and CI enforces the
rules the repository is meant to hold itself to.

Eight pieces exist behind that binary and it reaches exactly one of them.
The iteration loop is in `zaru-core`, headless, driven through five ports that
nothing in any product tree implements. The composer is in `zaru-tui`, rendered
under a test backend, reaching its two search tiers through a port and a
request/response pair that nothing implements either. The credential store is
in `zaru-cli`, holding named tokens on disk with no bearer value among them —
sealing is a port with no implementation, so no secret is written anywhere —
and caching each token's tool scope, which one Nuclear Notes session replaces
with a single fresh listing on each of the three signals a decision record
invalidates a cache on. The configuration hierarchy is in `zaru-cli`, resolving
five layers over a schema that names no key, reading each layer through a port
nothing implements. The local tool surface is in `zaru-cli` too: the seven built-in tool names, the
working-directory boundary, and the permission model that decides whether a
call is prompted for. **Nothing executes.** No tool runs a command, opens a
socket or touches a file, because the acting half sits behind ports with no
implementation, and the permission prompt itself is one of them.

The sixth is the Nuclear Notes client in `zaru-notes`: a session over MCP with
the workspace named on every read, a bearer value the type system will not
render, an attachment that cannot be constructed without saying where it lives,
and those three staleness signals. It reaches no network, because the transport
is a port with no implementation and there is therefore nothing in its
dependency tree that can open a socket. Every check against it exchanges real
protocol bytes over an in-memory pipe.

The seventh is the session lifecycle, in `zaru-cli`. A session is a directory
named by a ULID holding plain files, because a harness that shows its work
should not store the record of that work somewhere only it can read: an
append-only transcript of one event per line, a checkpoint rewritten
atomically beside it, a resume that restores and never re-executes, and
bounded retention whose deletion is real. Its `meta.toml` has no writer — the
dependency table names no TOML crate and an approximate emitter would be the
"for now" this harness forbids — so that file is a port with no
implementation, like the others above.

The eighth is the error taxonomy, also in `zaru-cli`, and it is the one the
binary reaches. Five classes of failure, each carrying by construction what its
class owes the reader; a mapping from every error the workspace already raises
to the class a decision record states for it; and a boundary around everything
the binary does, so that a bug in the harness is reported as a bug in the
harness — with the version and where to report it — rather than as a Rust
panic. The process exits with a documented code for what happened. Today the
binary does nothing that can fail, so the only code it can reach is `0`.

No provider, no SEAL, no terminal interface, and no command surface exists.
Seven of the eight pieces above are reachable only from their own tests.

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
