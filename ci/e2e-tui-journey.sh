#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# e2e's TUI journeys against a Core merge request's own commit
# (vauchi/private#1297, ADR-082 item 3). The smoke suite's Core device runs
# no app engine; `vauchi-tui` does. With the sync call removed from the
# engine, the smoke suite and e2e's 279 CLI tests passed and these journeys
# did not. e2e's own job builds the TUI on the Core tag tui pins, so it sees
# a Core change only after a release and a pin bump; this one builds it on
# the merge request's Core.
#
# A failure is attributed as in ci/e2e-smoke.sh (vauchi/private#1034): if
# the journeys fail on the MR's Core they are tried on the target branch's.
# When the target fails too, tui or e2e is behind Core, not this MR's
# doing: exit 3, which the job allows as a warning.
#
# Usage: ci/e2e-tui-journey.sh <e2e-checkout> <mr-core-dir> <target-core-dir>
# Exit:  0 the MR passes; 1 only the MR fails; 3 the target fails too.

set -eu

e2e=${1:?usage: e2e-tui-journey.sh <e2e-checkout> <mr-core-dir> <target-core-dir>}
mr_core=${2:?mr core dir}
target_core=${3:?target core dir}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

journeys='test(/cross_platform::test_tui_create_identity/) + test(/cross_platform::test_tui_list_contacts_when_contacts_are_already_visible/) + test(/cross_platform::integration_tui_to_tui_link_exchange/) + test(/cross_platform::integration_tui_to_tui_device_link/)'

journeys_against() { # core-dir name
    copy="$work/$2"
    cp -R "$e2e" "$copy"
    sh "$copy/ci/point-core-at.sh" "$1" "$copy/Cargo.toml" &&
        (cd "$copy" && E2E_CORE_DIR="$1" sh ci/tui-journey.sh "$work/$2-job" -E "$journeys")
}

if journeys_against "$mr_core" mr; then
    echo "the TUI journeys pass against this merge request's Core"
    exit 0
fi
echo "the TUI journeys failed against this merge request's Core; trying the target branch's Core"
if journeys_against "$target_core" target; then
    echo "FAIL: the TUI journeys pass against the target branch and fail against this merge request" >&2
    exit 1
fi
echo "WARN: the TUI journeys fail against the target branch's Core too: tui or e2e is behind Core." >&2
echo "      Not this merge request's doing; land their Core bump." >&2
exit 3
