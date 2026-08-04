#!/bin/sh

# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later

set -eu

pipeline="ci/publish-and-trigger.yml"
job=$(
    awk '
        /^trigger:windows:/ { capture = 1 }
        /^trigger:ios:/ { capture = 0 }
        capture { print }
    ' "$pipeline"
)

require() {
    pattern=$1
    description=$2
    if ! printf '%s\n' "$job" | grep -Fq -- "$pattern"; then
        echo "FAIL: trigger:windows must $description" >&2
        exit 1
    fi
}

require_count() {
    pattern=$1
    expected=$2
    description=$3
    actual=$(printf '%s\n' "$job" | grep -Fc -- "$pattern" || true)
    if [ "$actual" -ne "$expected" ]; then
        echo "FAIL: trigger:windows must $description (expected $expected, found $actual)" >&2
        exit 1
    fi
}

require 'BRANCH="chore/bump-vauchi-cabi-${CLEAN_VERSION}"' \
    "create a versioned update branch"
require 'git push -u origin "$BRANCH"' \
    "push only the versioned update branch"
require 'Draft: chore: update CABI DLL to ${CLEAN_VERSION}' \
    "open the update as a Draft merge request"
require '/merge_requests"' \
    "create the merge request through the GitLab API"
require_count '--header "PRIVATE-TOKEN: ${PROJECT_ACCESS_TOKEN}"' 2 \
    "use the API-capable group token for both merge-request API calls"
require '--data-urlencode "source_branch=$BRANCH"' \
    "verify an existing branch still has an open merge request"
require 'exit 1' \
    "fail when an existing branch has no open merge request"

if printf '%s\n' "$job" | grep -Fq 'git push origin main'; then
    echo "FAIL: trigger:windows must not update protected main directly" >&2
    exit 1
fi

echo "windows-trigger-mr: OK"
