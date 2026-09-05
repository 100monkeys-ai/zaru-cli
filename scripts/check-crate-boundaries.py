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
#
# Narrowed 2026-09-05 under directive 20, recorded as an accepted Update on
# ADR-0003 D8 and one sentence on ADR-0005's Status tracking. The property this
# gate holds is that the composer's search tier carries no network capability.
# Until now it was transcribed as "nothing in the closure that touches a
# socket", and that transcription was wider than the property: crossterm -- the
# terminal backend every in-session surface in ADR-0015 D2 needs -- brings mio
# and rustix, which are an OS interface rather than a network one. Measured:
# crossterm 0.28 takes rustix with default features off and exactly
# ["stdio", "termios"], so rustix::net is not compiled in at all.
#
# So the property is split in two, and BOTH halves are asserted. Nothing on
# NETWORK_CRATES may be reachable at all. Anything on BACKEND_ONLY may be
# reachable only THROUGH crossterm, which is asserted as a path rather than as
# an absence: the closure is walked a second time with crossterm treated as a
# leaf, and a BACKEND_ONLY crate still reachable in that reduced walk has a
# route that is not the terminal's. That is strictly stronger than deleting the
# two names from the list, which is what makes this a narrowing rather than a
# weakening.

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
#
# zaru-cli is deliberately NOT here and never will be: it is the composition
# root, it holds the terminal driver, and ADR-0003 D2 names reqwest for a
# provider client it will one day own. Its dependencies do not reach zaru-tui,
# because ADR-0003 D8 lets zaru-tui name only zaru-core -- a dependency edge
# runs from the dependent to the dependency, so zaru-cli taking tokio puts
# nothing whatever in zaru-tui's closure. Said here rather than feared.
NO_NETWORK = {"zaru-tui"}

# The one crate a BACKEND_ONLY name may be reached through.
#
# Named rather than derived: "whatever the terminal backend happens to pull"
# would be a rule that changes meaning when the backend does, which is the
# heuristic failure the comment above already rejects once.
TERMINAL_BACKEND = "crossterm"

# Crates a NO_NETWORK crate may reach ONLY through TERMINAL_BACKEND.
#
# Both are on NETWORK_CRATES' original list and both stay refused by every
# other route. mio is a poll/epoll reactor and rustix is a raw syscall wrapper;
# each can open a socket in the abstract, and neither does anything of the kind
# on the path that puts it here -- crossterm reads key events off a terminal
# file descriptor and restores termios. A direct edge to either from a
# NO_NETWORK crate is what this gate exists to catch, and it still is.
BACKEND_ONLY = {"mio", "rustix"}

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
    #
    # mio and rustix were on this list until 2026-09-05 and are now on
    # BACKEND_ONLY, which refuses them by every route except the terminal
    # backend's. They are not removed from this gate's reach; they moved to a
    # stricter question.
    "tokio",
    "async-std",
    "smol",
    "polling",
    "socket2",
    "async-io",
    "quinn",
    "quinn-proto",
    "quinn-udp",
    "hickory-resolver",
    "trust-dns-resolver",
    "nix",
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


def normal_closure(resolved: dict, root: str, leaves: set[str] | None = None) -> set[str]:
    """Every package reachable from `root` through normal dependencies.

    `cargo metadata` reports a dependency's kind as null for a normal one and
    as "dev" or "build" otherwise, so following only the null kind is what
    keeps a dev-dependency -- zaru-core's tokio, for one -- out of the answer.

    A package named in `leaves` is included in the answer and its own edges are
    not followed. That is what turns "is X reachable" into "is X reachable by
    some route other than through Y", which is the question BACKEND_ONLY asks:
    walk once normally, walk again with crossterm as a leaf, and anything still
    there in the second walk got there without the terminal backend.
    """
    leaves = leaves or set()
    by_id = {p["id"]: p["name"] for p in resolved["packages"]}
    nodes = {n["id"]: n for n in resolved["resolve"]["nodes"]}
    start = next((i for i, name in by_id.items() if name == root), None)
    if start is None:
        return set()

    seen = {start}
    frontier = [start]
    while frontier:
        current = frontier.pop()
        if by_id.get(current) in leaves:
            continue
        node = nodes.get(current)
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
    backends = []
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

        # The BACKEND_ONLY half. Walk again with the terminal backend as a
        # leaf; whatever is still reachable got there another way.
        without = normal_closure(resolved, name, leaves={TERMINAL_BACKEND})
        escaped = sorted((closure & BACKEND_ONLY) & without)
        for reachable in escaped:
            failures.append(
                "crate {!r} can reach {!r} by a route that does not pass through {!r}. "
                "That name is permitted in this closure only as part of the terminal "
                "backend ADR-0015 D2's in-session surface needs -- crossterm reads key "
                "events and restores termios -- and a second route to it is a socket "
                "capability arriving with no decision behind it. Adding it is an "
                "amendment to ADR-0003 D2 and to this gate, not an import."
                .format(name, reachable, TERMINAL_BACKEND)
            )
        through = sorted((closure & BACKEND_ONLY) - set(escaped))
        backends.append((name, through, TERMINAL_BACKEND in closure))

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
    for name, through, has_backend in backends:
        if not has_backend:
            print(
                "crate-boundaries: OK -- {!r} reaches no terminal backend at all, so none "
                "of the {} backend-only crate(s) this gate names can be there by any "
                "route.".format(name, len(BACKEND_ONLY))
            )
        else:
            print(
                "crate-boundaries: OK -- {!r} reaches {} through {!r} and by no other "
                "route; walked a second time with {!r} as a leaf and none of them was "
                "still reachable.".format(
                    name,
                    ", ".join(repr(c) for c in through) or "none of the backend-only crates",
                    TERMINAL_BACKEND,
                    TERMINAL_BACKEND,
                )
            )
    return 0


if __name__ == "__main__":
    sys.exit(main())
