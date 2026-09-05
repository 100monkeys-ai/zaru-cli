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

**Pre-alpha. Nothing here is installable.** Six crates compile, CI enforces the
rules the repository is meant to hold itself to, and as of 2026-09-05 the
harness runs a task: `zaru <task>` starts a session, asks a model, runs the
tools it asks for, and writes every step to a transcript you can read with
`cat`.

`zaru` reads its arguments, resolves configuration, and prints what is already
on this machine. Seven commands run:

```sh
zaru runtime                  # the tier, what it engages, and what changing it would alter
zaru models                   # each model alias, what it resolved to, and which layer said so
zaru config explain <key>     # every layer's value for one key, with the effective one marked
zaru sessions list            # every session on this machine
zaru sessions rm <id>         # delete a session's directory, with no tombstone
zaru notes tokens             # the stored Nuclear Notes tokens and which is the composer's
zaru init                     # write ADR-0009 D1's manifest here, once, if there is none
```

`--runtime <tier>` and `--model <identifier>` set the two keys the flag layer
carries for one run. `--help` lists exactly what runs and nothing else.

`--resume <id>` and `--continue` restore a session, and **what they do next
depends on who is asking**. At a terminal they open the session: a status line
carrying the tier, a pane showing the last stretch of the transcript, and a
prompt that takes the same commands as the subcommands above, spelled with a
slash. Through a pipe they print the transcript's own bytes and exit, because
there is nobody there to be inside anything. `/exit` or `Ctrl-C` leaves, and
both exit 0.

Anything else you type is a **task**, and running one is what the harness is
for:

```sh
zaru providers keys add gemini   # reads the key from standard input, never an argument
zaru "read src/main.rs and tell me what it does"
```

One invocation is one turn. It creates `~/.zaru/sessions/<ulid>/` with three
plain files, asks the model, runs whatever of the seven built-in tools it asks
for — prompting you before a write or a command, unless you are not at a
terminal, in which case a call that needed asking is refused rather than
performed — and prints what the model answered and what the turn cost in
tokens. At `bare` tier it says, once, that it is not a sandbox, because it is
not.

**Two things it will refuse.** Four of ADR-0012 D3's five provider kinds have
no client, so a task against one is refused naming the kind that does. And a
project whose `zaru.toml` declares validators is refused rather than run,
because the iteration loop those validators are for does not exist yet and
running the tool-call loop instead would report work as done that nothing
checked.

Eleven pieces exist behind that binary and the command surface reaches eight of
them. The iteration loop and the tool-call loop are in `zaru-core`, headless,
driven through ports that nothing in any product tree implements — except the
three a declared validator calls out through, which all have one in `zaru-cli`
as of 2026-09-05. A validator's command runs as a real child process; a
`matches` pattern is compiled by an engine that cannot backtrack, so a pattern
from a repository you cloned cannot cost exponential time; and a `json_schema`
validator reads its schema inside the working directory and resolves no `$ref`
out of it. **No command this binary runs declares a validator or runs one**, so
none of that is reachable from the terminal yet.

The terminal is in `zaru-tui` as of 2026-09-05, and it is what every other
piece has been waiting on. A status line, a transcript pane and the composer,
all headless — the shell renders into a frame and reads a backend-agnostic
keystroke, and the terminal itself is `zaru-cli`'s, which is what keeps a
terminal backend out of the composer's search tier. The slash grammar is a
second grammar over one vocabulary: the eleven namespaces, their two spellings
each and the nearest-match rule are declared once, in `zaru-cli`, and handed
across, so a command reached with a slash runs the *same function* the
subcommand runs. Seven of the eleven namespaces answer; the other four need
things that do not exist and say so rather than guessing at a nearest.

The composer is in `zaru-tui` too, reaching its two search tiers through a port
and a request/response pair that nothing implements — **so the hint strip shows
nothing while you type**, and that is the largest missing piece of this
surface. The trie belongs to `zaru-notes` and is not built.

The credential store is in `zaru-cli`, holding named tokens on disk with the
bearer value **sealed**: AES-256-GCM, a fresh nonce per seal, and the alias
bound in so a sealed value moved between entries will not open. The key comes
from the OS keyring where there is one and from `ZARU_CREDENTIAL_KEY` where
there is not — which is the ordinary case on a headless machine, not just in
CI. `zaru notes tokens` still lists nothing on any real machine, because
nothing here can *add* a token: that surface needs a Nuclear Notes server to
authenticate against. The configuration hierarchy is
in `zaru-cli`, resolving five layers over a schema whose keys arrive from the
records that own them; **all five layers have readers as of 2026-09-05** — the
built-in one, `~/.zaru/config.toml`, `./zaru.toml`, `ZARU_*`, and the command
line. The two that read files are the two that waited on a TOML parser, and a
file that does not parse is refused naming the file, the line and the column
and never the line's contents: the parser's own message renders the offending
source line, and a refusal that quoted it would publish whatever was on it. The local tool surface is in `zaru-cli` too: the seven built-in
tool names, the working-directory boundary, and the permission model. **Six
of the seven act**: `fs.read`, `fs.list`, `fs.write`, `fs.edit` and `fs.search`
through `std::fs` inside that boundary — the two that replace a file doing so
whole, at the file's own mode, and the one that searches never following a
link — and `cmd.run` as a real child process, started at the boundary's root
with a cleared environment and a wall-clock ceiling the caller supplies. A
command is not measured against the boundary as though it were a path — its
boundary is the directory it starts in — and **nothing contains that child**:
at `bare` tier the harness is not a sandbox and the decision record says so.
Every call's arguments arrive as one JSON object, read before the permission
decision because a path that has not been extracted is not yet a target.
`web.fetch` acts too, and it is the last of the seven to: `http` and `https`
only, no redirect followed across a host, this machine and the cloud
metadata range refused by name, no cookie kept and no header of the harness's
own added — and a response larger than the ceiling its caller supplies is
refused whole rather than cut short, because a document the harness stopped
reading is one no complete copy could be kept of. **The permission model
itself is now whole**: what the user pre-approved is read from
`~/.zaru/config.toml` as a list of the exact lines the prompt shows, matched
byte for byte and never by glob, and refused to a cloned repository; the
destructive-command categories the record names are recognised for the two of
the four whose shape its own words determine, with the two that name no
program matching nothing rather than a list nobody chose; and the prompt
itself is one line over the terminal, `y/N` with `N` the default, which
refuses the call rather than defaulting it when there is no terminal to ask.
**Nothing a user runs reaches any of that yet**, because no command reaches
the tool surface — `zaru config explain tools.allowlist` is what a person can
see of it today.

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
that reason. `meta.toml` is written and read since 2026-09-05, atomically and at
`0600`, and it records one thing ADR-0010 D1 does not name — the configuration
layer the tier came from, because a tier that cannot say where it came from is
not a record of the session's tier. **The binary starts no session**: it reads
the ones that are there and creates nothing by being asked a question, so no
`meta.toml` exists on any machine yet.

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
build needs a registry: `Cargo.lock` resolves 300 packages, six of which are
this workspace's own. The third-party set is `rmcp` for the Nuclear Notes
client, `ratatui` and `tui-textarea` for the composer, `serde` and `serde_json`
for the credential store, `aes-gcm` and `keyring` for sealing that store, `toml`
for `~/.zaru/config.toml`, `./zaru.toml` and `meta.toml`, `regex` and `boon` for
two of a declared validator's four `expect` kinds, `reqwest` for the provider
client **and for `web.fetch`, which share one builder** — a second caller for a
crate already carried rather than a new dependency, so the set is still
twelve — `tokio` for the client's channels, for polling that provider's futures,
for the binary crate's own check that drives a session end to end, and for
polling the loop's futures under `#[tokio::test]`, and what those twelve pull
in. `boon` needs the URL and Unicode machinery `$ref` resolution
asks for, and would be the largest single dependency here had `reqwest` not
already brought most of it. Which dependencies the harness may carry is
ADR-0003 D2's to decide, and `[workspace.dependencies]` is where each arrives
once it has a caller.

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
