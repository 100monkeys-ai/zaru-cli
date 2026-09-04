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

for sha in "${commits[@]}"; do
    checked=$((checked + 1))
    author=$(git log -1 --format='%ae' "$sha")
    trailers=$(git log -1 --format='%(trailers:key=Signed-off-by,valueonly)' "$sha")
    if ! printf '%s\n' "$trailers" | grep -qiF -- "<$author>"; then
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

if [ ${#unsigned[@]} -gt 0 ]; then
    echo "dco: FAIL -- ${#unsigned[@]} of $checked commit(s) in $range carry no Signed-off-by matching their author:" >&2
    printf '    %s\n' "${unsigned[@]}" >&2
    echo "  Every commit needs a Developer Certificate of Origin sign-off (ADR-0003 D4)." >&2
    echo "  Add one with 'git commit -s', or 'git rebase --signoff <base>' for a branch." >&2
    exit 1
fi

echo "dco: OK -- $checked commit(s) checked in $range, $merges merge commit(s) skipped."
