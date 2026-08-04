#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later

set -eu

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/../.." && pwd)
PIPELINE="$ROOT/ci/package.yml"

job=$(
    awk '
        /^test:packaging-matrix:/ { capture = 1 }
        /^package:xcframework:/ { capture = 0 }
        capture { print }
    ' "$PIPELINE"
)

require() {
    pattern=$1
    description=$2
    if ! printf '%s\n' "$job" | grep -Fq -- "$pattern"; then
        echo "FAIL: packaging matrix must $description" >&2
        exit 1
    fi
}

require 'mktemp -d' "create a per-job build directory"
require 'git archive "$CI_COMMIT_SHA"' "copy only the tested revision"
require 'cd "$WORK_DIR"' "build outside the shared runner checkout"
require 'export RUSTC_WRAPPER=""' "avoid shared sccache ownership leaks"

echo "PASS: packaging matrix isolates each build from shared target permissions"
