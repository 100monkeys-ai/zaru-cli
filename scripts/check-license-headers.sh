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
#
# EVERY VERDICT BELOW IS READ FROM AN EXPLICIT EXIT STATUS, NEVER FROM AN `if`
# CONDITION, AND NEVER OUT OF A PIPELINE.
#
# `grep` answers 0 for found, 1 for not found, and 2 or above for "I could not
# look" -- an unreadable haystack, a refused allocation, a process that never
# started. An `if ! ...` collapses every non-zero into the second answer, so the
# gate reports a file as missing a header it carries. That is the shape library
# verification lessons volume 4 §85 names, and reading it as absence is what
# this script did until 2026-09-05.
#
# The pipeline is gone for a second reason, and it is the one that was actually
# firing. `printf ... | grep -q` under `set -o pipefail` reports the pipeline's
# status as the last non-zero of ANY element. `grep -q` exits the instant it
# matches, so when the SPDX line is not the LAST line of the header -- it is
# line 1 or line 2 of five -- `grep` can be gone while `printf` is still
# writing; `printf` dies of SIGPIPE with status 141; `pipefail` hands that 141
# to the `if`, which reads it as "the line is absent". The gate then reported a
# file as missing the header at the moment it successfully found it. Measured on
# this tree at 4000 pipelines per cell under load: 41 SIGPIPEs with the match on
# line 1, 76 with it on line 2, 0 with it on the last line, 0 with no match at
# all, and 0 without `-q`. Three arcs lost an afternoon to it on 2026-09-05
# before it was reproduced. A here-string removes the second process, so there
# is nothing left to lose the race with.
#
# Note which way that defect could fail: a file that genuinely lacks the line
# takes the no-match path, where `grep` reads to the end and nothing races. So
# it could only ever produce a false FAIL on a correct file, never a false PASS
# on a wrong one. That is luck rather than design, and it is not relied on.

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
unreadable=()

while IFS= read -r -d '' file; do
    checked=$((checked + 1))

    # `set -e` would abort the whole run on a failing command substitution, and
    # abort it silently: the verdict is printed after the loop, so the gate
    # would exit non-zero having said nothing at all about anything. Capturing
    # the status is what turns that into a named file and a sentence.
    header_status=0
    header=$(head -n "$HEADER_LINES" -- "$file") || header_status=$?
    if [ "$header_status" -ne 0 ]; then
        unreadable+=("$file  (head exited $header_status)")
        continue
    fi

    spdx_status=0
    grep -qxF -- "$SPDX" <<<"$header" || spdx_status=$?
    case "$spdx_status" in
        0) ;;
        1) missing_spdx+=("$file") ;;
        *) unreadable+=("$file  (the SPDX grep exited $spdx_status)") ; continue ;;
    esac

    # This arm read `grep`'s stdout rather than its status, behind a `|| true`
    # that discarded every failure. Reading the output is why the SIGPIPE race
    # above never showed here -- by the time `printf` died, `grep -m1` had
    # already printed the line it matched -- and it is also why a `grep` that
    # could not run at all reported the file as having no copyright line. The
    # status is load-bearing now, and the output is still what is parsed.
    copyright_status=0
    copyright=$(grep -m1 -E -- '^// Copyright [0-9]{4} .+$' <<<"$header") || copyright_status=$?
    case "$copyright_status" in
        0) ;;
        1) missing_copyright+=("$file") ; continue ;;
        *) unreadable+=("$file  (the copyright grep exited $copyright_status)") ; continue ;;
    esac

    holder=${copyright#// Copyright ???? }
    if [ "$holder" != "$OWNER" ]; then
        foreign=$((foreign + 1))
        # Not a pipeline -- `grep` reads the manifest directly, so nothing can
        # SIGPIPE here. The status still has three meanings, and an unreadable
        # THIRD_PARTY.md must not read as "this file has no row in it": that
        # would be a fabricated ADR-0003 D6 violation.
        manifest_status=0
        grep -qF -- "$file" "$MANIFEST" || manifest_status=$?
        case "$manifest_status" in
            0) ;;
            1) unlisted+=("$file  (holder: $holder)") ;;
            *) unreadable+=("$file  (the $MANIFEST grep exited $manifest_status)") ;;
        esac
    fi
done < <(git ls-files -z -- '*.rs')

if [ "$checked" -eq 0 ]; then
    echo "license-header: FAIL -- the discovery predicate matched no files." >&2
    echo "  \`git ls-files -- '*.rs'\` returned nothing, so this gate asserted nothing" >&2
    echo "  about anything. That is a broken gate, not a clean repository." >&2
    exit 1
fi

status=0

# This block is first on purpose. "I could not look" is not a finding about the
# tree, and a reader who meets the missing-header list first will go and open a
# file that is perfectly correct -- which is exactly the afternoon this gate
# cost three arcs before the cause was found.
if [ ${#unreadable[@]} -gt 0 ]; then
    echo "license-header: FAIL -- ${#unreadable[@]} of $checked file(s) could not be checked at all, which is not the same as failing the check:" >&2
    printf '    %s\n' "${unreadable[@]}" >&2
    echo "  A sub-process this gate reads a verdict from did not run to completion." >&2
    echo "  \`grep\` answers 1 for 'not found' and 2 or above for 'I could not look';" >&2
    echo "  a status of 128 or more is a signal, and 141 is SIGPIPE. Reading any of" >&2
    echo "  those as 'not found' reports a file as missing a header it carries." >&2
    echo "  Nothing above is a claim about the file. Re-run; if it persists, the" >&2
    echo "  machine could not start a process or read a file, and that is the defect." >&2
    status=1
fi

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
