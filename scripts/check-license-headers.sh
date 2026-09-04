#!/usr/bin/env bash
#
# Copyright 2026 100monkeys AI, Inc.
# SPDX-License-Identifier: Apache-2.0
#
# Enforces the licence header on every tracked Rust source file, and the half
# of ADR-0003 D6 that a header check can actually enforce.
#
# Two arms:
#
#   1. Every tracked *.rs file carries an SPDX licence line and a copyright
#      line in its first five lines.
#
#   2. Any file whose copyright holder is NOT 100monkeys has a row in
#      THIRD_PARTY.md naming its path. D6 requires a lifted file keep its
#      ORIGINAL copyright header, so this check must not force ours onto one --
#      what it must do instead is refuse a foreign header that nothing has
#      recorded the provenance of.
#
# The population comes from `git ls-files`, which is the index -- the thing
# that owns the answer to "which files are in this repository". A `find` would
# reach into target/ and into anything untracked, so it would be answering a
# different question. A gate that finds nothing looks exactly like a repository
# where everything is already correct, so a zero count is a failure here.

set -euo pipefail

readonly OWNER='100monkeys AI, Inc.'
readonly SPDX='// SPDX-License-Identifier: Apache-2.0'
readonly MANIFEST='THIRD_PARTY.md'
readonly HEADER_LINES=5

cd "$(git rev-parse --show-toplevel)"

checked=0
foreign=0
missing_spdx=()
missing_copyright=()
unlisted=()

while IFS= read -r -d '' file; do
    checked=$((checked + 1))
    header=$(head -n "$HEADER_LINES" "$file")

    if ! printf '%s\n' "$header" | grep -qxF -- "$SPDX"; then
        missing_spdx+=("$file")
    fi

    copyright=$(printf '%s\n' "$header" | grep -m1 -E '^// Copyright [0-9]{4} .+$' || true)
    if [ -z "$copyright" ]; then
        missing_copyright+=("$file")
        continue
    fi

    holder=${copyright#// Copyright ???? }
    if [ "$holder" != "$OWNER" ]; then
        foreign=$((foreign + 1))
        if ! grep -qF -- "$file" "$MANIFEST"; then
            unlisted+=("$file  (holder: $holder)")
        fi
    fi
done < <(git ls-files -z -- '*.rs')

if [ "$checked" -eq 0 ]; then
    echo "license-header: FAIL -- the discovery predicate matched no files." >&2
    echo "  \`git ls-files -- '*.rs'\` returned nothing, so this gate asserted nothing" >&2
    echo "  about anything. That is a broken gate, not a clean repository." >&2
    exit 1
fi

status=0

if [ ${#missing_spdx[@]} -gt 0 ]; then
    echo "license-header: FAIL -- ${#missing_spdx[@]} of $checked file(s) have no '$SPDX' line in their first $HEADER_LINES lines:" >&2
    printf '    %s\n' "${missing_spdx[@]}" >&2
    status=1
fi

if [ ${#missing_copyright[@]} -gt 0 ]; then
    echo "license-header: FAIL -- ${#missing_copyright[@]} of $checked file(s) have no '// Copyright <year> <holder>' line in their first $HEADER_LINES lines:" >&2
    printf '    %s\n' "${missing_copyright[@]}" >&2
    status=1
fi

if [ ${#unlisted[@]} -gt 0 ]; then
    echo "license-header: FAIL -- ${#unlisted[@]} of $foreign file(s) carrying a third-party copyright have no row in $MANIFEST naming their path:" >&2
    printf '    %s\n' "${unlisted[@]}" >&2
    echo "  ADR-0003 D6 requires every lifted file be recorded with its upstream project," >&2
    echo "  file, commit SHA and date. A foreign header nothing has recorded is the exact" >&2
    echo "  state D6 exists to prevent." >&2
    status=1
fi

if [ "$status" -eq 0 ]; then
    echo "license-header: OK -- $checked file(s) checked, $foreign carrying a third-party copyright, all of those listed in $MANIFEST."
fi

exit "$status"
