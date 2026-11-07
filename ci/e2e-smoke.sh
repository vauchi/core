#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# e2e's smoke suite against a Core merge request's own commit
# (vauchi/private#1032, D8), with the failure attributed
# (vauchi/private#1034): e2e's pinned set is built against the MR's Core;
# if that fails, the same is tried against the target branch's Core.
# When the target fails too, the pinned set is behind Core (a Core minor
# moved and e2e's bump has not landed), not this MR's doing: exit 3,
# which the job allows as a warning.
#
# Usage: ci/e2e-smoke.sh <e2e-checkout> <mr-core-dir> <target-core-dir>
# Exit:  0 the MR passes; 1 only the MR fails; 3 the target fails too.

set -eu

# e2e's Core device calls this Core in-process and no CLI is built, so a
# Core change the CLI has yet to follow can still merge; the CLI's own
# pipeline runs the suite on the CLI at its pin bump (vauchi/private#1118).
export E2E_DEVICE_BACKEND=core

e2e=${1:?usage: e2e-smoke.sh <e2e-checkout> <mr-core-dir> <target-core-dir>}
mr_core=${2:?mr core dir}
target_core=${3:?target core dir}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# One attempt in a fresh copy of the e2e checkout, so the rewritten
# manifest and the build of one attempt never leak into the other.
smoke_against() { # core-dir name
    copy="$work/$2"
    cp -R "$e2e" "$copy"
    sh "$copy/ci/point-core-at.sh" "$1" "$copy/Cargo.toml" &&
        E2E_CORE_DIR="$1" sh "$copy/ci/build-pinned-binaries.sh" "$copy/.bin" &&
        (cd "$copy" && E2E_BIN_DIR="$copy/.bin" cargo nextest run --profile ci \
            --run-ignored default -j 4 -- smoke_)
}

if smoke_against "$mr_core" mr; then
    echo "e2e smoke passes against this merge request's Core"
    exit 0
fi

echo "e2e smoke failed against this merge request's Core; trying the target branch's Core"
if smoke_against "$target_core" target; then
    echo "FAIL: e2e smoke passes against the target branch and fails against this merge request" >&2
    exit 1
fi

echo "WARN: e2e smoke fails against the target branch's Core too: e2e's pinned set is behind Core." >&2
echo "      Not this merge request's doing; land e2e's Core bump (ci/pins.env and Cargo.toml)." >&2
exit 3
