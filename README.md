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

Stored keys are encrypted with a key kept in your operating system's keyring. If the machine has no keyring (a server, WSL, a container) and a task needs a stored key, `zaru` tells you so and stops. Set `ZARU_CREDENTIAL_KEY` to 64 lower-case hexadecimal characters in your shell profile, for example the output of `openssl rand -hex 32`, and keep that value: without it, the keys you stored cannot be read again. A task that needs no stored key, such as one for Ollama, goes on without it and says that the stored keys could not be read.

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
Zaru is not a sandbox. Tool calls run on your machine with your permissions. A permission prompt asks before a call runs; it does not limit what an allowed call can do.

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

The transcript has one JSON object per line: your task, each model reply with the tool calls it asked for and their arguments, each tool call and whether it was allowed, what each call returned exactly as the model was given it, and the answer. Stored keys and tokens are replaced with a marker before a model reply or a tool result is written.

## Using Zaru

### Interactive sessions

Run `zaru` with no arguments in a terminal to open a session. Type a task and press Enter; each task you type continues the same conversation, and the model is sent every earlier task, reply, tool call and tool result in it. Type `/help` to list commands. Type `/exit`, or press Ctrl-C when nothing is running and the prompt is empty, to leave. Ctrl-C while a task runs, or at a permission prompt, stops the task and keeps the session open.

`zaru --continue` reopens the most recent session started in the current directory, and `zaru --resume <id>` reopens the session with that ID. The conversation is rebuilt from the transcript, so the model is sent the same conversation it had before. A session recorded before tool results were kept reopens with your tasks and the answers, and says once that the earlier tool results were not recorded. When the output is not a terminal, both print the session's transcript and exit instead. The line at the top of the screen shows the runtime tier, the model, the permission mode, how much of the model's context window is used, and the session ID.

**Selecting text.** A session takes over the mouse so that the scroll wheel scrolls the transcript. To select text, hold Shift while you drag (this works in most terminals, including Windows Terminal and VS Code). To give the mouse back to your terminal instead, set `terminal.mouse = false` in `~/.zaru/config.toml`; the scroll wheel then no longer scrolls the transcript.

### Permission prompts and modes

Before a tool call that needs permission, Zaru shows what the call will do (for a file write, the file and its new contents) and asks `[y/N/a · a allows this exact line for this session · esc says no to this call · ctrl-c stops the turn]`. `y` allows the call once. `a` allows the same call for the rest of the session. `N`, Enter or Esc says no to this call: the model is told you said no, and the task goes on. Ctrl-C stops the whole task: the call does not run, no other call runs, and the session stays open for your next task. The file, command or web address the prompt asks about is always on its first lines. On a small terminal a long path is shortened in the middle so its start and the file's name both show, a long command shows the program and says how many arguments it leaves out, and a long preview shows its first lines and says how many it leaves out. Before fetching a web page, Zaru shows the whole URL and also offers `h`, which allows every URL on that host for the rest of the session.

The permission mode decides what needs asking. Set it with `tools.mode` in `~/.zaru/config.toml`, the `ZARU_TOOLS_MODE` environment variable, or `--mode` on the command line:

| Mode | What it asks about |
| --- | --- |
| `ask` (default) | Every file write, file edit, command, web page fetch and Nuclear Notes call, and anything outside the working directory. Reading, listing and searching files inside it do not ask. |
| `allow` | Nothing on your allowlist; everything else. |
| `yolo` | Nothing. Every call the model makes runs. |

The allowlist is `tools.allowlist` in `~/.zaru/config.toml`: a list of the exact lines the prompt shows, for example `"cmd.run cargo test"`. For `web.fetch` an entry may also name a host with no `http://` or `https://`, for example `"web.fetch docs.rs"`, which allows every URL on exactly that host. A project's own files cannot set the mode or the allowlist.

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

A validator is a command in `./zaru.toml` that checks the model's work, such as a build or a test run. When a project declares validators, a task runs in a loop: the model makes a change, the validators run, and if any fails, its output goes back to the model for another attempt. The loop stops when every validator passes or when it reaches `runtime.max_iterations` attempts, which is one at the `bare` tier unless you set it.

`zaru init` writes an example `./zaru.toml` to edit. A small one:

```toml
[[validator]]
name = "test"
run = "cargo test"
expect = "exit-zero"
```

`expect` can be `"exit-zero"`, `{ exit-code = 2 }`, `{ matches = "<regex>" }` or `{ json_schema = "<file>" }`. If the validators never all pass, `zaru` exits with code 1.

Validators are commands, so none runs until you approve them. Before the first one runs in a project, Zaru shows every validator's name and command and asks. Your answer is kept in `~/.zaru/approved-validators.jsonl`, for that directory and that exact set of commands. If `zaru.toml` changes them, Zaru asks again and shows what changed. When there is no terminal to ask on, the task is refused: run `zaru validators approve` in the project's directory to see the commands and approve them, and `zaru validators list` to see what you have approved. The permission mode does not change this, `yolo` included.

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
| `zaru validators approve` | Show the validators in `./zaru.toml` and ask to approve them. |
| `zaru validators list` | List every project whose validators you approved, and their commands. |
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

- **What the model can do.** It has seven built-in tools: `fs.read`, `fs.list`, `fs.search`, `fs.write`, `fs.edit`, `cmd.run` and `web.fetch`. `fs.read` returns at most 500 numbered lines at a time, fewer when the lines are long, and says how many lines the file has and where to start to read on; it refuses a binary file or one that is not UTF-8, naming its size. `cmd.run` starts a program directly, without a shell, in the working directory, with only `PATH`, `HOME`, `LANG`, `LC_ALL` and `TMPDIR` from your environment, and stops it after two minutes. Nothing limits what that program does while it runs. `web.fetch` fetches `http` and `https` URLs only. In `ask` mode it asks first and shows the whole URL, because a URL can carry data off your machine. A redirect to another host is asked about again, and refused when there is no terminal to ask on. It refuses this machine's own addresses and the link-local range, which includes the cloud metadata address.
- **Permission.** The permission mode decides what Zaru asks about. In `yolo` mode it asks nothing, so a model can also change which validators are approved; use `yolo` only for a directory and a task you trust entirely.
- **Validators run only after you approve them.** The commands in a project's `./zaru.toml` are shown to you and run only once you say yes, and you are asked again when they change. The approval is kept under `~/.zaru/`, never in the project, and the permission mode does not skip it.
- **Runtime tiers.** A runtime tier is how much of the 100monkeys platform Zaru uses. The default is `bare`, which uses none. `contained` and `linked` can be selected but are not built yet: at every tier, tool calls run directly on your machine. Zaru prints the not-a-sandbox warning at every tier, and at `contained` and `linked` it adds that the tier is not built yet and changes nothing about how tool calls run. `zaru runtime` says the same.
- **What the model provider receives.** A short system prompt, your task, the conversation so far, the tool descriptions, and the result of every tool call, including the contents of files read, command output and fetched pages. The system prompt states the working directory, the operating system, the date, the tool names and the permission mode; if a system prompt is read from a Nuclear Notes page (`persona.path`), that page is sent instead. Zaru removes the values of the keys and tokens it has stored from this. When it cannot read them (no keyring and no `ZARU_CREDENTIAL_KEY`) and the task needs none of them, it says so and goes on without removing them. It does not look for any other secret.
- **What stays on your machine.** Settings, stored keys and sessions are under `~/.zaru/`. Keys and tokens are in `~/.zaru/credentials.json`, encrypted with AES-256-GCM; the encryption key is in the OS keyring or in `ZARU_CREDENTIAL_KEY`. Session files are readable only by you and are kept until you delete them with `zaru sessions rm`.

### Known limitations

- Only three provider kinds work: `gemini`, `ollama` and `openai-compatible`. `anthropic` and `aegis` are recognised but have no client.
- `zaru learned` and `zaru inbox` are placeholders.
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
| 128 + n | The session or task was ended by signal n: 143 for `SIGTERM`, 130 for `SIGINT`, and 129 for `SIGHUP` or a terminal that was closed. What was running is stopped first, and the session can be resumed. |

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
