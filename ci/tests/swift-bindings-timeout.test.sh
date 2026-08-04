#!/bin/sh
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
# SPDX-License-Identifier: GPL-3.0-or-later

set -eu

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/../.." && pwd)
PIPELINE="$ROOT/ci/publish-and-trigger.yml"

job=$(
    awk '
        /^update:swift-bindings:/ { capture = 1 }
        /^release:verify-published:/ { capture = 0 }
        capture { print }
    ' "$PIPELINE"
)

if ! printf '%s\n' "$job" | grep -Fq '.timeout-standard'; then
    echo "FAIL: update:swift-bindings must allow enough time to download the XCFramework artifact" >&2
    exit 1
fi

if printf '%s\n' "$job" | grep -Fq '.timeout-quick'; then
    echo "FAIL: update:swift-bindings must not use the 10-minute quick timeout" >&2
    exit 1
fi

echo "PASS: Swift binding publication has a download-safe timeout"
