#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# A merge request pipeline holding a job that needs build:tests, without
# build:tests itself, is not created: GitLab fails it with no jobs. That
# happened for twelve main pipelines after #1212, and again for core!2370,
# whose only change, two .spthy headers, matched test:core's and
# lint:semver's vauchi-core/**/* but no build:tests path (private#796).
# So every merge-request trigger path of a job that needs build:tests
# (lint:semver, and every job extending .run-archive) must be covered by a
# build:tests trigger path. Paths come from the job's own rules and from
# the in-file templates its rules reference (`!reference [.x, rules]`).
#
# Not checked: rules inherited from the scripts templates (.validation-rust,
# .validation-platform), which build:tests mirrors by hand; and an MR rule
# with no `changes:` at all.
#
# Coverage compares globs as strings: a dependent glob is covered when it
# matches a build:tests glob read as a shell pattern (`**` and `*` both
# matching `/`), which holds for the prefix and suffix globs used here.
#
# Usage: ci/tests/build-tests-needs.test.sh [ci-config]
#        (from the core repo root)

set -eu
# Globs are data here; word-splitting them must not expand them.
set -f

ci_config="${1:-.gitlab-ci.yml}"
failures=0

fail() {
    echo "FAIL: $1" >&2
    failures=$((failures + 1))
}

job_block() {
    awk -v job="$1" '
        $0 == job ":" { in_job = 1; next }
        in_job && /^[^[:space:]#]/ { exit }
        in_job { print }
    ' "$ci_config"
}

# The list items under each `changes:` (directly, or under its `paths:`).
change_paths() {
    printf '%s\n' "$1" | awk '
        function indent(s) { match(s, /^ */); return RLENGTH }
        BEGIN { c_ind = -1; p_ind = -1 }
        /^[[:space:]]*$/ { next }
        {
            ind = indent($0)
            if (p_ind >= 0 && ind <= p_ind) p_ind = -1
            if (c_ind >= 0 && ind <= c_ind) { c_ind = -1; p_ind = -1 }
            if ($0 ~ /^ *- / && ((p_ind >= 0 && ind > p_ind) || (c_ind >= 0 && p_ind < 0 && ind > c_ind))) {
                item = $0
                sub(/^ *- */, "", item); sub(/ *$/, "", item); gsub(/"/, "", item)
                print item
                next
            }
            if ($0 ~ /^ *changes: *$/) { c_ind = ind; next }
            if (c_ind >= 0 && $0 ~ /^ *paths: *$/) { p_ind = ind; next }
        }
    '
}

# The in-file templates whose rules a block references.
referenced_rules() {
    printf '%s\n' "$1" | sed -n 's/^ *- *!reference *\[ *\(\.[A-Za-z0-9_-]*\) *, *rules *\].*$/\1/p'
}

covered() { # glob, globs
    c_glob=$1
    for c_build in $2; do
        c_pat=$(printf '%s' "$c_build" | sed 's#\*\*/#*#g; s#\*\*#*#g')
        # shellcheck disable=SC2254 # the pattern is meant to glob
        case "$c_glob" in $c_pat) return 0 ;; esac
    done
    return 1
}

build_block=$(job_block "build:tests")
[ -n "$build_block" ] || { echo "FAIL: build:tests is not in $ci_config" >&2; exit 1; }
build_paths=$(change_paths "$build_block")
[ -n "$build_paths" ] || { echo "FAIL: build:tests has no trigger paths" >&2; exit 1; }

dependents=$(awk '
    /^[a-z][^ ]*:$/ { job = substr($0, 1, length($0) - 1); next }
    /^[^[:space:]]/ { job = "" }
    job != "" && (/^ *- *job: *build:tests *$/ || /^ *- *build:tests *$/ || /^ *extends:.*\.run-archive/) { print job }
' "$ci_config" | sort -u)
[ -n "$dependents" ] || { echo "FAIL: no job needs build:tests; is the parse broken?" >&2; exit 1; }

checked=0
for job in $dependents; do
    block=$(job_block "$job")
    paths=$(change_paths "$block")
    for ref in $(referenced_rules "$block"); do
        paths="$paths
$(change_paths "$(job_block "$ref")")"
    done
    for p in $paths; do
        checked=$((checked + 1))
        covered "$p" "$build_paths" ||
            fail "$job triggers on '$p', which starts no build:tests: its pipeline would fail with no jobs"
    done
done

[ "$checked" -gt 0 ] || { echo "FAIL: no dependent trigger path was read; is the parse broken?" >&2; exit 1; }
if [ "$failures" -gt 0 ]; then
    echo "$failures path(s) uncovered; add them to build:tests' rules" >&2
    exit 1
fi
echo "PASS: $checked trigger path(s) of $(printf '%s\n' "$dependents" | wc -l | tr -d ' ') job(s) are covered by build:tests"
