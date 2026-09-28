# Zaru

Zaru is an AI assistant that works in your terminal. You give it a task, it asks a language model, and it runs the tools the model asks for: it reads and edits files, runs commands and fetches web pages, asking your permission first. Every step is written to a plain-text transcript you can read with `cat`.

This repository is the `zaru` command-line program. It is pre-alpha: there are no releases and no packages, you build it from source, and commands, files and settings can change without notice.

## Requirements

- Linux. The program is built and tested on Linux only. It uses Unix-only APIs, so it does not build on Windows. macOS is not tested.
- Rust 1.98.1. The version is pinned in [`rust-toolchain.toml`](rust-toolchain.toml), and `rustup` installs it for you on the first build.
- A C compiler (`cc`). Some dependencies compile C code.
- Network access to crates.io for the first build.
- A model to talk to: a Google Gemini API key, an [Ollama](https://ollama.com) server, or any server with an OpenAI-compatible chat completions API.

## Build and install

```sh
git clone https://github.com/100monkeys-ai/zaru-cli.git
cd zaru-cli
cargo install --path crates/zaru-cli --locked
```

This builds the `zaru` binary and copies it to `~/.cargo/bin`; `zaru --version` checks that it runs. To build without installing, run `cargo build --locked`; the binary is then `target/debug/zaru`.

## Quick start

### 1. Choose a model

Zaru has no default model. You name one in `~/.zaru/config.toml`, which you create yourself.

**Gemini.** Store your API key. It is read from standard input: paste it, press Enter, then press Ctrl-D.

```console
$ zaru providers keys add gemini
stored a `gemini` key under the alias `provider.gemini`.
  the value is sealed and is not printed by any command.
```

Then name the model in `~/.zaru/config.toml`:

```toml
[model]
default = "gemini-3.6-flash"
```

Stored keys are encrypted with a key kept in your operating system's keyring. If the machine has no keyring (a server, WSL, a container), `zaru` tells you so and stops. Set `ZARU_CREDENTIAL_KEY` to 64 lower-case hexadecimal characters in your shell profile, for example the output of `openssl rand -hex 32`, and keep that value: without it, the keys you stored cannot be read again.

**Ollama.** No key is needed. Start the server, then write `~/.zaru/config.toml`:

```toml
[model]
default = "llama3.2"

[provider.default]
kind = "ollama"
```

Zaru connects to `http://localhost:11434`. Set `provider.ollama.endpoint` if your server is elsewhere.

**An OpenAI-compatible server.** Give the base URL (Zaru adds `/chat/completions` to it) and the model's context window in tokens:

```toml
[model]
default = "your-model-name"

[provider.default]
kind = "openai-compatible"

[provider.openai_compatible]
endpoint = "http://localhost:8080/v1"
context_tokens = 32768
```

If the server needs a key, store it with `zaru providers keys add openai-compatible`.

### 2. Run a task

Run `zaru` with the task in quotes, from the directory you want it to work in:

```console
$ zaru "read src/main.rs and tell me what it does"
bare tier has no membrane. Zaru is not a sandbox here: a tool call runs with your permissions, on your machine, and a prompt is a question rather than a barrier. ...

<the model's answer>

tokens: 120 prompt + 7 completion = 127

no validators are declared, so the iteration loop cannot run · declare one in `./zaru.toml`
```

It prints a warning that it is not a sandbox (see [Safety](#safety)), the model's answer, the tokens the task used, and a note about validators (see [Validators](#validators-and-the-iteration-loop)).

When the model wants to write a file or run a command, Zaru asks you first. If there is no terminal to ask (for example, when `zaru` runs in a script), the call is refused and the model is told so.

### 3. Read the transcript

Each task creates a session: a directory under `~/.zaru/sessions/` named by a session ID.

```sh
zaru sessions list
cat ~/.zaru/sessions/<id>/transcript.jsonl
```

The transcript has one JSON object per line: your task, each model reply, each tool call and whether it was allowed, and the answer.

## Using Zaru

### Interactive sessions

Run `zaru` with no arguments in a terminal to open a session. Type a task and press Enter; each task you type continues the same conversation. Type `/help` to list commands. Type `/exit` or press Ctrl-C to leave.

`zaru --continue` reopens the most recent session started in the current directory, and `zaru --resume <id>` reopens the session with that ID. When the output is not a terminal, both print the session's transcript and exit instead. The line at the top of the screen shows the runtime tier, the model, the permission mode, how much of the model's context window is used, and the session ID.

**Selecting text.** A session takes over the mouse so that the scroll wheel scrolls the transcript. To select text, hold Shift while you drag (this works in most terminals, including Windows Terminal and VS Code). To give the mouse back to your terminal instead, set `terminal.mouse = false` in `~/.zaru/config.toml`; the scroll wheel then no longer scrolls the transcript.

### Permission prompts and modes

Before a tool call that needs permission, Zaru shows what the call will do (for a file write, the file and its new contents) and asks `[y/N/a · a allows this exact line for this session · esc declines]`. `y` allows the call once. `N`, Enter or Esc declines it. `a` allows the same call for the rest of the session.

The permission mode decides what needs asking. Set it with `tools.mode` in `~/.zaru/config.toml`, the `ZARU_TOOLS_MODE` environment variable, or `--mode` on the command line:

| Mode | What it asks about |
| --- | --- |
| `ask` (default) | Every file write, file edit, command and Nuclear Notes call, and anything outside the working directory. Reading, listing and searching files inside it, and fetching web pages, do not ask. |
| `allow` | Nothing on your allowlist; everything else. |
| `yolo` | Nothing. Every call the model makes runs. |

The allowlist is `tools.allowlist` in `~/.zaru/config.toml`: a list of the exact lines the prompt shows, for example `"cmd.run cargo test"`. A project's own files cannot set the mode or the allowlist.

### Configuration

Zaru reads settings from these places. A later one overrides an earlier one:

1. built-in defaults
2. `~/.zaru/config.toml`, your settings
3. `./zaru.toml`, the project's settings (only `[project]`, `[runtime]` and `[[validator]]`)
4. environment variables: the key in upper case with `ZARU_` in front, for example `ZARU_MODEL_DEFAULT`
5. command-line flags

`zaru config explain <key>` shows the value from every place and marks the one in effect. The keys most people need:

| Key | Meaning |
| --- | --- |
| `model.default` | The model to use. `--model` sets it for one run. |
| `provider.default.kind` | `gemini`, `ollama` or `openai-compatible`. |
| `provider.ollama.endpoint`, `provider.openai_compatible.endpoint` | Where the model server listens. |
| `provider.openai_compatible.context_tokens` | The OpenAI-compatible model's context window, in tokens. Required for that kind. |
| `tools.mode`, `tools.allowlist` | See [Permission prompts and modes](#permission-prompts-and-modes). |
| `runtime.max_iterations` | How many attempts the validator loop makes. |
| `runtime.max_tool_exchanges` | A limit on model replies per task. Unlimited if unset. |
| `terminal.mouse` | `false` gives mouse selection back to the terminal. |

### Validators and the iteration loop

A validator is a command in `./zaru.toml` that checks the model's work, such as a build or a test run. When a project declares validators, a task runs in a loop: the model makes a change, the validators run, and if any fails, its output goes back to the model for another attempt. The loop stops when every validator passes or when it reaches `runtime.max_iterations` attempts, which is one unless you set it.

`zaru init` writes an example `./zaru.toml` to edit. A small one:

```toml
[[validator]]
name = "test"
run = "cargo test"
expect = "exit-zero"
```

`expect` can be `"exit-zero"`, `{ exit-code = 2 }`, `{ matches = "<regex>" }` or `{ json_schema = "<file>" }`. If the validators never all pass, `zaru` exits with code 1.

### Nuclear Notes

Nuclear Notes is 100monkeys' notes service. `zaru notes tokens add <alias> <host>` stores an access token for it, read from standard input. When a session opens with a stored token, Zaru suggests matching page and atom names from your notes as you type. The model can call a Nuclear Notes instance's tools only if you list them under `notes.<alias>.agent_tools` in `~/.zaru/config.toml`.

## Command reference

`zaru "<task>"` runs one task. `zaru` with no arguments opens a session in a terminal and prints help otherwise.

| Command | What it does |
| --- | --- |
| `zaru runtime` | Show the runtime tier and what the other tiers would change. |
| `zaru models` | Show each model alias (`default`, `fast`, `smart`, `cheap`, `local`) and the model it resolves to. |
| `zaru config explain <key>` | Show one setting's value at every level, marking the one in effect. |
| `zaru init` | Write an example `./zaru.toml`, if there is none. |
| `zaru providers keys` | List the provider keys stored on this machine. |
| `zaru providers keys add <kind>` | Store a provider key, read from standard input. |
| `zaru providers keys rm <kind>` | Delete a stored provider key. |
| `zaru sessions list` | List the sessions on this machine. |
| `zaru sessions rm <id>` | Delete a session's directory. |
| `zaru notes tokens` | List the stored Nuclear Notes tokens. |
| `zaru notes tokens add <alias> <host>` | Store a Nuclear Notes token, read from standard input. |
| `zaru notes tokens describe <alias> <text>` | Add a description to a stored token. |
| `zaru notes tokens rm <alias>` | Delete a stored token. |
| `zaru notes use <alias>` | Choose which token the session's suggestions use. |
| `zaru learned` | Not built yet; says there is nothing to show. |
| `zaru inbox` | Not built yet; says there is nothing to show. |
| `zaru help` | Print the help text. |

| Flag | What it does |
| --- | --- |
| `--model <identifier>` | Use this model for one run. |
| `--mode <mode>` | Use this permission mode for one run: `ask`, `allow` or `yolo`. |
| `--runtime <tier>` | Use this runtime tier for one run. |
| `--resume <id>` | Reopen a session. |
| `--continue` | Reopen the most recent session started in this directory. |
| `--help` | Print the help text. |
| `--version` | Print the version and the libraries it is built from. |

## Safety

Zaru acts on your machine with your user account's permissions. It is not a sandbox.

- **What the model can do.** It has seven built-in tools: `fs.read`, `fs.list`, `fs.search`, `fs.write`, `fs.edit`, `cmd.run` and `web.fetch`. `cmd.run` starts a program directly, without a shell, in the working directory, with only `PATH`, `HOME`, `LANG`, `LC_ALL` and `TMPDIR` from your environment, and stops it after two minutes. Nothing limits what that program does while it runs. `web.fetch` fetches `http` and `https` URLs only, without asking, and refuses this machine's own addresses and the link-local range, which includes the cloud metadata address.
- **Permission.** The permission mode decides what Zaru asks about. In `yolo` mode it asks nothing.
- **Validators run without asking.** The commands in `./zaru.toml` run whenever you give a task in that directory, in every mode. Read the `zaru.toml` of a project you cloned before you run `zaru` in it.
- **Runtime tiers.** A runtime tier is how much of the 100monkeys platform Zaru uses. The default is `bare`, which uses none. `contained` and `linked` can be selected but enforce nothing yet: at every tier, tool calls run directly on your machine.
- **What the model provider receives.** Your task, the conversation so far, the tool descriptions, and the result of every tool call, including the contents of files read, command output and fetched pages. Zaru removes the values of the keys and tokens it has stored from this. It does not look for any other secret.
- **What stays on your machine.** Settings, stored keys and sessions are under `~/.zaru/`. Keys and tokens are in `~/.zaru/credentials.json`, encrypted with AES-256-GCM; the encryption key is in the OS keyring or in `ZARU_CREDENTIAL_KEY`. Session files are readable only by you and are kept until you delete them with `zaru sessions rm`.

### Known limitations

- Only three provider kinds work: `gemini`, `ollama` and `openai-compatible`. `anthropic` and `aegis` are recognised but have no client.
- `zaru learned` and `zaru inbox` are placeholders.
- Unless a system prompt is read from a Nuclear Notes page (`persona.path`), none is sent, and the model receives your task with a line saying so.
- The `contained` and `linked` tiers contain nothing (see above).

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | The command succeeded. |
| 1 | The work failed: the validators never all passed. |
| 2 | Something you can fix: a missing key or model, a bad setting, an unknown command. The message says what to change. |
| 3 | A problem outside Zaru, such as the network or the provider. The message says whether to wait. |
| 4 | The current runtime tier cannot do what was asked. |
| 70 | A bug in Zaru. The message says how to report it. |
| 128 + n | The session was ended by signal n. |

## Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked
scripts/check-license-headers.sh
python3 scripts/check-crate-boundaries.py
```

These are the checks CI runs, together with `cargo doc` and a sign-off check. Unset `NO_COLOR` before you run the tests, because one test checks colour output. Every commit needs a `Signed-off-by` line (`git commit -s`); see [CONTRIBUTING.md](CONTRIBUTING.md). Contributors and coding agents start at [CLAUDE.md](CLAUDE.md).

## Licence

Apache-2.0. See [LICENSE](LICENSE), [NOTICE](NOTICE) and [THIRD_PARTY.md](THIRD_PARTY.md).
