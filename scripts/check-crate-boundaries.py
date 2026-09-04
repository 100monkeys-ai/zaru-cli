#!/usr/bin/env python3
#
# Copyright 2026 100monkeys AI, Inc.
# SPDX-License-Identifier: Apache-2.0
#
# Enforces ADR-0003 D8's crate boundaries as an edge set, and publish = false
# on every crate.
#
# D8 names six crates, one per responsibility, and states one dependency rule
# explicitly: zaru-core must not depend on zaru-tui. That one clause is already
# enforced by Cargo, because zaru-tui depends on zaru-core and Cargo refuses
# dependency cycles. Nothing enforces the rest -- that zaru-seal and zaru-notes
# stay dependency-free so they remain publishable and arm's-length, or that
# zaru-aegis reaches only zaru-seal. A boundary nothing checks is a boundary
# that erodes one convenient import at a time.
#
# The population comes from `cargo metadata`, which is the workspace's own
# answer to "which crates are here and what does each depend on" -- not from a
# list retyped beside this check. A crate present in the workspace and absent
# from the table below is an error rather than something to skip: that is the
# only way a seventh crate fails loudly instead of arriving unnoticed.

import json
import subprocess
import sys

# The rule. Each crate maps to the set of SIBLING crates it may depend on.
# Changing this table is changing ADR-0003 D8, which is an ADR-level act.
ALLOWED = {
    "zaru-core": set(),
    "zaru-seal": set(),
    "zaru-notes": set(),
    "zaru-aegis": {"zaru-seal"},
    "zaru-tui": {"zaru-core"},
    "zaru-cli": {"zaru-core", "zaru-tui", "zaru-notes", "zaru-seal", "zaru-aegis"},
}


def main() -> int:
    raw = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    packages = {p["name"]: p for p in json.loads(raw)["packages"]}

    failures = []
    on_disk = set(packages)
    declared = set(ALLOWED)

    for extra in sorted(on_disk - declared):
        failures.append(
            "crate {!r} exists in the workspace but has no row in this checks "
            "table. ADR-0003 D8 names six crates; adding a seventh is an "
            "amendment to that decision, not an implementation detail.".format(extra)
        )
    for missing in sorted(declared - on_disk):
        failures.append(
            "crate {!r} is required by ADR-0003 D8 and is not in the "
            "workspace.".format(missing)
        )

    checked = 0
    edges = 0
    for name in sorted(on_disk & declared):
        checked += 1
        pkg = packages[name]

        siblings = {
            d["name"] for d in pkg["dependencies"] if d["name"].startswith("zaru-")
        }
        edges += len(siblings)
        for dep in sorted(siblings - ALLOWED[name]):
            permitted = sorted(ALLOWED[name]) or "nothing"
            failures.append(
                "crate {!r} depends on sibling {!r}, which ADR-0003 D8 does not "
                "allow it. Permitted for {!r}: {}.".format(name, dep, name, permitted)
            )

        # cargo metadata reports publish = false as an empty registry list, and
        # a publishable crate as null.
        if pkg.get("publish") != []:
            failures.append(
                "crate {!r} does not carry publish = false. Publishing is a "
                "released artefact and human-owned; nothing here is ready to be "
                "published.".format(name)
            )

    if checked == 0:
        print(
            "crate-boundaries: FAIL -- cargo metadata reported no workspace "
            "packages, so this gate asserted nothing about anything.",
            file=sys.stderr,
        )
        return 1

    if failures:
        print(
            "crate-boundaries: FAIL -- {} violation(s) across {} crate(s):".format(
                len(failures), checked
            ),
            file=sys.stderr,
        )
        for failure in failures:
            print("    " + failure, file=sys.stderr)
        return 1

    print(
        "crate-boundaries: OK -- {} crate(s) checked, {} sibling edge(s), all "
        "within the ADR-0003 D8 table; all carry publish = false.".format(
            checked, edges
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
