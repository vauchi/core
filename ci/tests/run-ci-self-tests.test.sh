#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
#
# ci/run-ci-self-tests.sh must fail when a test fails or is missing, keep
# running the rest, and name every failure (vauchi/private#1179).

set -u
runner="$(cd "$(dirname "$0")/.." && pwd)/run-ci-self-tests.sh"
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
printf 'exit 0\n' > "$t/good.test.sh"
printf 'exit 4\n' > "$t/bad.test.sh"
fails=0

check() { # description, expected exit (0 or nonzero), output pattern, args...
    desc=$1 want=$2 pattern=$3
    shift 3
    out=$(CI_SELF_TESTS_DIR="$t" sh "$runner" "$@" 2>&1)
    rc=$?
    if { [ "$want" = 0 ] && [ "$rc" -ne 0 ]; } || { [ "$want" != 0 ] && [ "$rc" -eq 0 ]; }; then
        echo "FAIL: $desc: exit $rc"; fails=$((fails + 1)); return
    fi
    if ! printf '%s\n' "$out" | grep -q "$pattern"; then
        echo "FAIL: $desc: output lacks '$pattern'"; fails=$((fails + 1)); return
    fi
    echo "PASS: $desc"
}

check "passing tests pass" 0 "All CI self-tests passed." good
check "a failing test fails the run" 1 "Failed: bad" bad
check "a test after a failing one still runs" 1 "=== good: pass" bad good
check "a missing test fails the run" 1 "MISSING: .*/absent.test.sh" absent good
check "every failure is named" 1 "Failed: bad absent" bad absent good

[ "$fails" -eq 0 ] || { echo "$fails failure(s)"; exit 1; }
