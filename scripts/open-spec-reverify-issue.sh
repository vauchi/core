#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: AGPL-3.0-or-later
# Open the spec's re-verify issue for a Core release (notify:spec,
# vauchi/private#874). The spec's chapters pin the Core commit they were
# read from and its tags map to Core releases; without a nudge the spec
# followed one-off audits. One issue per release: an open issue whose
# title names the tag is reused, not duplicated.
#
# Usage: open-spec-reverify-issue.sh <vX.Y.Z>
# Env:   PROJECT_ACCESS_TOKEN  group token: issues API on the spec project
#        CI_API_V4_URL         GitLab API base
#        SPEC_PROJECT          URL-encoded project path (default vauchi%2Fspec)
set -euo pipefail

VERSION="${1:?usage: open-spec-reverify-issue.sh <vX.Y.Z>}"
VERSION="v${VERSION#v}"
API="${CI_API_V4_URL:-https://gitlab.com/api/v4}"
PROJECT="${SPEC_PROJECT:-vauchi%2Fspec}"
TITLE="Re-verify the claims and chapters against Core ${VERSION} and tag"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# Status first, body second: an error body must never be parsed as data.
api() {
    local method=$1 url=$2 data=${3:-} status
    : > "$WORK/response.json"
    if [ -n "$data" ]; then
        status=$(curl -sS -o "$WORK/response.json" -w '%{http_code}' --request "$method" \
            --header "PRIVATE-TOKEN: ${PROJECT_ACCESS_TOKEN}" \
            --header "Content-Type: application/json" --data "$data" "$url") || status=000
    else
        status=$(curl -sS -o "$WORK/response.json" -w '%{http_code}' --request "$method" \
            --header "PRIVATE-TOKEN: ${PROJECT_ACCESS_TOKEN}" "$url") || status=000
    fi
    printf '%s' "$status"
}

json_field() {
    python3 -c 'import json, sys
data = json.load(open(sys.argv[1]))
if isinstance(data, list):
    data = data[0] if data else {}
print(data.get(sys.argv[2]) or "")' "$WORK/response.json" "$1"
}

encoded_title=$(python3 -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1], safe=""))' "$TITLE")
status=$(api GET "${API}/projects/${PROJECT}/issues?state=opened&search=${encoded_title}&in=title")
if [ "$status" != "200" ]; then
    echo "ERROR: listing open issues on ${PROJECT} returned HTTP ${status}" >&2
    exit 1
fi
existing=$(json_field web_url)
if [ -n "$existing" ]; then
    echo "re-verify issue for ${VERSION} already open: ${existing}"
    exit 0
fi

description=$(cat <<EOF
Core ${VERSION} is tagged. The spec's chapters pin the Core commit they were read from, and spec tags map to Core releases; this issue asks for both to follow the release.

1. Run the drift checks against the tag: \`check:vectors\`, \`check:claim-names\` and \`check:claim-annotations\` compare the book with Core \`main\`; a chapter re-extracted for the release pins \`${VERSION}\` instead.
2. Re-read every chapter's status line against \`${VERSION}\`: move the pinned commit or re-extract the section the release changed.
3. Audit the claims' statuses (shipped, partial, planned) against what the release ships, and the *Not claimed* list.
4. Add the changelog entry and tag the spec.

Opened by Core's \`notify:spec\` job on the release pipeline (vauchi/private#874).
EOF
)
payload=$(python3 -c 'import json, sys; print(json.dumps({"title": sys.argv[1], "description": sys.argv[2], "labels": "release"}))' "$TITLE" "$description")
status=$(api POST "${API}/projects/${PROJECT}/issues" "$payload")
if [ "$status" != "201" ]; then
    echo "ERROR: opening the re-verify issue on ${PROJECT} returned HTTP ${status}: $(json_field message)" >&2
    exit 1
fi
echo "opened re-verify issue for ${VERSION}: $(json_field web_url)"
