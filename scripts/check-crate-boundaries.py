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
#
# It also enforces one property that is not an edge between siblings at all:
# ADR-0005 D3 says of the composer's fast tier that it is "zero network,
# instant, works offline", and the strongest form of that claim is not a check
# on a code path but the absence of anything to call. So zaru-tui's whole
# transitive dependency closure is walked and refused if it contains a crate
# that can open a socket, speak HTTP, or terminate TLS. The list is named
# below rather than matched by a heuristic, because a heuristic over crate
# names silently stops covering whatever gets named differently next.

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

# Crates whose transitive NORMAL dependencies must contain nothing that can
# reach a network. Dev-dependencies are excluded on purpose: a check harness
# that reaches a network would be its own problem, and the claim here is about
# what the shipped library can do. Build-dependencies are excluded for the same
# reason and are worth revisiting if one ever appears.
NO_NETWORK = {"zaru-tui"}

# Named, not matched. Every entry is a crate that provides an HTTP client or
# server, terminates TLS, opens a socket, resolves DNS, or drives an I/O
# reactor that can do any of those. Adding a dependency that belongs on this
# list and is not on it is how the gate goes quiet, so the list grows in the
# same change as any dependency that makes it necessary.
NETWORK_CRATES = {
    # HTTP
    "reqwest",
    "hyper",
    "hyper-util",
    "hyper-rustls",
    "tower-http",
    "h2",
    "ureq",
    "attohttpc",
    "isahc",
    "surf",
    "curl",
    "curl-sys",
    "actix-web",
    "axum",
    "warp",
    "tiny_http",
    # TLS
    "rustls",
    "rustls-webpki",
    "rustls-pki-types",
    "webpki-roots",
    "tokio-rustls",
    "native-tls",
    "openssl",
    "openssl-sys",
    "boring",
    "schannel",
    "security-framework",
    # sockets, reactors, DNS
    "tokio",
    "async-std",
    "smol",
    "mio",
    "polling",
    "socket2",
    "async-io",
    "quinn",
    "quinn-proto",
    "quinn-udp",
    "hickory-resolver",
    "trust-dns-resolver",
    "nix",
    "rustix",
    # MCP and websockets, which arrive over one of the above
    "rmcp",
    "tungstenite",
    "tokio-tungstenite",
}


def metadata(*flags: str) -> dict:
    raw = subprocess.run(
        ["cargo", "metadata", *flags, "--format-version", "1"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return json.loads(raw)


def normal_closure(resolved: dict, root: str) -> set[str]:
    """Every package reachable from `root` through normal dependencies.

    `cargo metadata` reports a dependency's kind as null for a normal one and
    as "dev" or "build" otherwise, so following only the null kind is what
    keeps a dev-dependency -- zaru-core's tokio, for one -- out of the answer.
    """
    by_id = {p["id"]: p["name"] for p in resolved["packages"]}
    nodes = {n["id"]: n for n in resolved["resolve"]["nodes"]}
    start = next((i for i, name in by_id.items() if name == root), None)
    if start is None:
        return set()

    seen = {start}
    frontier = [start]
    while frontier:
        node = nodes.get(frontier.pop())
        if node is None:
            continue
        for dep in node["deps"]:
            normal = any(kind["kind"] is None for kind in dep["dep_kinds"])
            if normal and dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                frontier.append(dep["pkg"])
    return {by_id[i] for i in seen}


def main() -> int:
    resolved = metadata()
    packages = {p["name"]: p for p in metadata("--no-deps")["packages"]}
    members = set(packages)

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

    closures = []
    for name in sorted(NO_NETWORK & on_disk):
        closure = normal_closure(resolved, name)
        third_party = closure - members
        if not third_party:
            failures.append(
                "crate {!r} resolved a dependency closure containing no third-party "
                "package at all, so this gate asserted nothing about it. That is a "
                "broken walk, not a clean crate.".format(name)
            )
            continue
        closures.append((name, len(closure), len(third_party)))
        for reachable in sorted(closure & NETWORK_CRATES):
            failures.append(
                "crate {!r} can reach {!r} through its normal dependencies. ADR-0005 D3 "
                "says the composer's fast tier is \"zero network, instant, works "
                "offline\", and the form of that claim this gate holds is that there is "
                "nothing there to call. If the dependency is genuinely wanted, that is "
                "an amendment to ADR-0003 D2 and to this gate, not an import."
                .format(name, reachable)
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
    for name, total, third_party in closures:
        print(
            "crate-boundaries: OK -- {!r}'s normal dependency closure is {} package(s), "
            "{} of them third-party, and carries none of the {} network-capable crates "
            "this gate names.".format(name, total, third_party, len(NETWORK_CRATES))
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
