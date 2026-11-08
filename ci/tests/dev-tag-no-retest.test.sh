#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
#
# A dev tag is cut by auto-tag:version in main's last stage, on the commit
# whose every blocking main job has just passed. Re-running lint and tests
# on the tag's pipeline tested the same SHA twice: ~10 h of allyson a week
# for 84 dev tags (vauchi/private#1179). The tag pipeline keeps what main
# does not run (reproducibility, binary size, dev bindings).

set -eu
cd "$(dirname "$0")/../.."

file=ci/auto-tag-and-dev-rc.yml
failures=0
for job in dev:lint:format dev:lint:clippy dev:test; do
    if grep -qx "$job:" "$file"; then
        echo "FAIL: $job re-runs on a dev tag what main passed on the same commit"
        failures=$((failures + 1))
    fi
done
grep -q 'without a `needs` DAG shortcut so every blocking main job' "$file" || {
    echo "FAIL: auto-tag:version no longer waits for every blocking main job;"
    echo "      a dev tag would then need its own lint and test again"
    failures=$((failures + 1))
}
[ "$failures" -eq 0 ] || exit 1
echo "PASS: dev tags do not re-test the commit main already passed"
