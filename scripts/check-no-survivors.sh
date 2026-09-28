#!/usr/bin/env bash
#
# Copyright 2026 100monkeys AI, Inc.
# SPDX-License-Identifier: Apache-2.0
#
# Fails when any process is still running from a build directory after the
# test suite that built it has ended.
#
# Usage:
#   scripts/check-no-survivors.sh <directory>
#
# CI runs it on `target/` after `cargo test`. Every process a check starts is
# owned by `crates/zaru-cli/tests/support/owned.rs` and is gone before the
# check returns, so at this point nothing may be running from `target/` at
# all: not the harness, not a test binary, not a copy of either under a
# deleted path.
#
# Why this exists, measured on 2026-09-28: the machine's watchdog sent SIGTERM
# to eight `zaru` processes running from the `target/debug/` of a worktree
# deleted five hours earlier. They were left by a passing run of the suite
# under `nohup`, and nothing in that run's output said so, because a process a
# check leaves behind is not a failure of any check. This makes it one.
#
# A process is matched by `/proc/<pid>/exe`, the executable the kernel is
# running, rather than by its command line, which a process can rewrite and
# which names whatever path it was started by. A binary deleted since it
# started still resolves, with " (deleted)" after its path, and still matches.
#
# Three answers, never two, for the reason `scripts/check-license-headers.sh`
# sets out: 0 when it looked and found nothing, 1 when it found a survivor, and
# 2 when it could not look -- a directory that is not there, or a `/proc` in
# which it could read no process at all. "Could not look" read as "found
# nothing" is the green this gate exists to refuse.

set -uo pipefail

if [ "$#" -ne 1 ]; then
    echo "no-survivors: usage: scripts/check-no-survivors.sh <directory>" >&2
    exit 2
fi
if [ ! -d "$1" ]; then
    echo "no-survivors: cannot look: $1 is not a directory" >&2
    exit 2
fi
root=$(cd -- "$1" && pwd -P) || {
    echo "no-survivors: cannot look: $1 does not resolve" >&2
    exit 2
}

examined=0
survivors=()
for entry in /proc/[0-9]*; do
    pid=${entry#/proc/}
    [ "$pid" = "$$" ] && continue
    # Another user's process, or one that ended between the glob and here, is
    # not readable and is not ours to judge.
    exe=$(readlink -- "$entry/exe" 2>/dev/null) || continue
    examined=$((examined + 1))
    case "$exe" in
        "$root"/*) survivors+=("$pid") ;;
    esac
done

if [ "$examined" -eq 0 ]; then
    echo "no-survivors: cannot look: read the executable of no process in /proc" >&2
    exit 2
fi

if [ "${#survivors[@]}" -gt 0 ]; then
    echo "no-survivors: FAIL -- ${#survivors[@]} process(es) still running from $root after the suite ended:" >&2
    for pid in "${survivors[@]}"; do
        listed=$(ps -o pid=,ppid=,etime=,rss=,args= -p "$pid" 2>/dev/null) \
            || listed="$pid (ended while being listed)"
        echo "  $listed" >&2
    done
    exit 1
fi

echo "no-survivors: ok -- none of the $examined process(es) this user can read runs from $root"
exit 0
