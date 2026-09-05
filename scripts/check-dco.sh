#!/usr/bin/env bash
#
# Copyright 2026 100monkeys AI, Inc.
# SPDX-License-Identifier: Apache-2.0
#
# Enforces ADR-0003 D4: contributions are accepted under the Developer
# Certificate of Origin, which means every commit carries a Signed-off-by
# trailer whose email is the commit author's.
#
# Usage:
#   scripts/check-dco.sh [<range>] [--require-nonempty]
#
# With no range, checks everything on this branch since it diverged from the
# default branch. CI passes the range explicitly, because the range a push or a
# pull request covers is a fact the event carries and not one to re-derive.
#
# Merge commits are skipped. A commit created by GitHub's merge button carries
# no trailer and is authored by whoever pressed it, so a check that failed on
# those would fail on every merge the moment it became a required status check.
# The count of skipped merges is printed rather than left implicit.
#
# What this check is NOT: it is not the enforcement on its own. It makes the
# failure visible on every pull request. Making it BLOCKING is branch
# protection marking this job a required status check, which is human-owned.
#
# The trailer match reads an explicit exit status and is not a pipeline, for
# the reasons `scripts/check-license-headers.sh` sets out at length and does
# not repeat here. The short form: `grep` answers 0 for found, 1 for not found,
# and 2 or above for "I could not look", and `if ! ...` collapses the third
# into the second -- so a `grep` that could not run reported every correctly
# signed commit in the range as unsigned. Forced on 2026-09-05 with a `grep`
# shimmed to exit 2: this gate reported 3 of 3 signed commits as unsigned and
# printed the sign-off it had just failed to match in the same sentence that
# denied it existed.
#
# This script carried the same `printf ... | grep -q` pipeline the licence gate
# reddened on, and it has never been seen to redden: its needle spans a whole
# single-line trailer, so `grep` must consume everything to match and there is
# no early exit to race with -- 0 SIGPIPEs in 9,200 measured pipelines against
# the licence gate's 6 reddened runs in 20. That is a property of the payload,
# not of the code, and a payload is not where a guarantee belongs. The shape is
# gone from both scripts.

set -euo pipefail

range=''
require_nonempty=0

for arg in "$@"; do
    case "$arg" in
        --require-nonempty) require_nonempty=1 ;;
        *) range="$arg" ;;
    esac
done

if [ -z "$range" ]; then
    if base=$(git merge-base HEAD main 2>/dev/null); then
        range="$base..HEAD"
    else
        range="$(git rev-list --max-parents=0 HEAD | tail -n 1)..HEAD"
    fi
    echo "dco: no range given, using $range (merge-base with the default branch)."
fi

mapfile -t commits < <(git rev-list --no-merges "$range")
merges=$(git rev-list --merges "$range" | wc -l | tr -d ' ')

checked=0
unsigned=()
unreadable=()

for sha in "${commits[@]}"; do
    checked=$((checked + 1))
    author=$(git log -1 --format='%ae' "$sha")
    trailers=$(git log -1 --format='%(trailers:key=Signed-off-by,valueonly)' "$sha")

    match_status=0
    grep -qiF -- "<$author>" <<<"$trailers" || match_status=$?
    if [ "$match_status" -ge 2 ]; then
        unreadable+=("${sha:0:12}  (the sign-off grep exited $match_status)")
        continue
    fi

    if [ "$match_status" -eq 1 ]; then
        subject=$(git log -1 --format='%s' "$sha")
        if [ -z "$trailers" ]; then
            unsigned+=("${sha:0:12}  $subject  -- no Signed-off-by trailer at all")
        else
            signers=$(printf '%s' "$trailers" | tr '\n' ' ' | sed 's/ *$//')
            unsigned+=("${sha:0:12}  $subject  -- signed off by $signers, but authored by <$author>")
        fi
    fi
done

if [ "$checked" -eq 0 ] && [ "$require_nonempty" -eq 1 ]; then
    echo "dco: FAIL -- the range $range contains no non-merge commits." >&2
    echo "  A pull request always contains at least one, so this means the range was" >&2
    echo "  computed wrongly and the gate asserted nothing. $merges merge commit(s) were skipped." >&2
    exit 1
fi

if [ ${#unreadable[@]} -gt 0 ]; then
    echo "dco: FAIL -- ${#unreadable[@]} of $checked commit(s) in $range could not be checked at all, which is not the same as failing the check:" >&2
    printf '    %s\n' "${unreadable[@]}" >&2
    echo "  The sign-off match did not run to completion, so nothing above is a claim" >&2
    echo "  about whether those commits are signed. \`grep\` answers 1 for 'not found'" >&2
    echo "  and 2 or above for 'I could not look'; reading the second as the first" >&2
    echo "  reports a correctly signed commit as unsigned. Re-run; if it persists, the" >&2
    echo "  machine could not start a process, and that is the defect." >&2
    exit 1
fi

if [ ${#unsigned[@]} -gt 0 ]; then
    echo "dco: FAIL -- ${#unsigned[@]} of $checked commit(s) in $range carry no Signed-off-by matching their author:" >&2
    printf '    %s\n' "${unsigned[@]}" >&2
    echo "  Every commit needs a Developer Certificate of Origin sign-off (ADR-0003 D4)." >&2
    echo "  Add one with 'git commit -s', or 'git rebase --signoff <base>' for a branch." >&2
    exit 1
fi

echo "dco: OK -- $checked commit(s) checked in $range, $merges merge commit(s) skipped."
