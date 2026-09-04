# Contributing

## Sign off every commit

Contributions to Zaru are accepted under the **Developer Certificate of Origin**
(ADR-0003 D4) rather than a contributor licence agreement: there is no copyright
assignment and nothing to sign, only a line on each commit certifying that you
have the right to contribute it. Add it with `git commit -s`, which appends
`Signed-off-by: Your Name <your@email>` using your git identity — the email must
match the commit author's, and every commit in a pull request needs one, not
just the last. If you forget on a branch, `git rebase --signoff <base>` fixes
the whole range. CI runs `scripts/check-dco.sh` over every commit in a pull
request and names the ones that are missing it; merge commits are skipped,
because a commit created by the merge button carries no trailer. Apache-2.0
already grants everything needed to distribute your patch, including a patent
grant, so the sign-off is the entire contribution barrier.

The text you are certifying is reproduced below in full, because being asked to
certify a document the repository does not carry is not a certification.

## Developer Certificate of Origin 1.1

```text
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.


Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same license (unless I am permitted to submit
    under a different license), as indicated in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

## What CI will check

Seven gates run on every pull request. Run them locally before opening one; each
prints what it checked, not only a verdict.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --locked --all-targets
cargo test --workspace --locked
scripts/check-license-headers.sh
scripts/check-dco.sh
scripts/check-crate-boundaries.py
```

## Licence headers

Every Rust source file starts with exactly these two lines:

```rust
// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0
```

A file lifted from another project is the exception: it keeps its **original**
copyright header, and it gets a row in [THIRD_PARTY.md](THIRD_PARTY.md) naming
its path, the upstream project, the upstream file, the commit SHA and the date.
The header check enforces both halves.

## Adding a dependency

Add it to `[workspace.dependencies]` in the root `Cargo.toml` with its version,
then take it in a crate with `workspace = true`. That is what stops two crates
pinning two versions of one dependency. ADR-0003 D2 governs what the harness may
depend on at all — notably, it may not carry a container library.

## Adding a crate, or an edge between crates

Do not. ADR-0003 D8 names six crates and the dependency edges between them, and
`scripts/check-crate-boundaries.py` fails on anything outside that table. A
seventh crate, or a new edge, is an amendment to that decision — raise it there
first.

## Where the rest of this lives

The engineering contract — how work is chosen, what counts as evidence, the
commit workflow, the testing tiers — is in the Zaru workspace at
<https://100monkeys-ai.cortex.page/zaru/>, not in this repository.
