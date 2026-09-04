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

**Pre-alpha. This repository currently contains the workspace skeleton and
nothing else.** Six crates compile, a binary prints what it is composed of, and
CI enforces the rules the repository is meant to hold itself to. No agent loop,
no composer, no provider, no SEAL, and no terminal interface exists yet. Nothing
here is installable and nothing here does any work.

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

The toolchain is pinned in `rust-toolchain.toml` and rustup will honour it. The
workspace currently has no third-party dependencies at all, so a build needs no
registry.

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
