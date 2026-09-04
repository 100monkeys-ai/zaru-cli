# Third-party material

ADR-0003 D1 permits lifting individual files or functions from Apache-2.0
sources where they solve a discrete problem well. ADR-0003 D6 is the discipline
that keeps that permission honest: **every file or function lifted from another
project keeps its original copyright header, is listed in `NOTICE`, and gets a
row in this table.**

The row is written in the same change that does the lifting. If upstream
relicenses, or a downstream consumer asks, the record has to already exist — it
cannot be reconstructed from memory or from `git log`. Without this table, the
claim that the harness was built from the ground up becomes an uncomfortable
question nobody can answer.

`scripts/check-license-headers.sh` enforces the half of this that a check can
reach: a Rust source file whose copyright holder is not 100monkeys must have a
row here naming its path. The other columns are not machine-checkable and are
owed by whoever writes the row.

## The table

| Repository path | Upstream project | Upstream file | Commit SHA | Date |
| --- | --- | --- | --- | --- |
| _(none)_ | | | | |

**Nothing has been lifted.** The table is empty because it is accurate, not
because it is unmaintained.

## What does not create a row

**Reading another project for architecture creates no row.** ADR-0003 D1 makes
study unrestricted and its Neutral section says so explicitly: Apache-2.0 makes
reading free, and only copying carries obligations. The skeleton arc read
[Goose](https://github.com/block/goose) at commit
`dce69009546ce5f20522d010fa3f1d57abbe2c3f` (2026-09-04) for its Cargo workspace
shape and its CI job decomposition, and copied no file, no function, and no
configuration block. Recording that reading as a row would make this table lie
about what it is for.

**The Apache-2.0 licence text in `LICENSE` creates no row.** It is the licence
this repository is under, fetched from
`https://www.apache.org/licenses/LICENSE-2.0.txt`, with the appendix's
`Copyright [yyyy] [name of copyright owner]` placeholder filled in and nothing
else changed. The same applies to the Developer Certificate of Origin 1.1 text
in `CONTRIBUTING.md`: a certification the repository asks contributors to make
has to be readable in the repository, and reproducing it verbatim is what makes
the sign-off mean something.
