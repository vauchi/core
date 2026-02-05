#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
# Trigger downstream package repos after a release (local/manual use)
#
# In CI, the trigger: keyword in .gitlab-ci.yml handles this automatically.
# This script is for local/manual triggering when needed.
#
# This script triggers CI pipelines in:
# - vauchi/vauchi-mobile-swift
# - vauchi/vauchi-mobile-android
#
# Authentication:
#   - CI: Uses CI_JOB_TOKEN (auto-provisioned, zero management)
#   - Local: Uses GITLAB_TOKEN (personal access token)
#
# Usage:
#   ./trigger-downstream.sh <version>
#
# Example:
#   ./trigger-downstream.sh v0.1.0

set -euo pipefail

VERSION="${1:-}"

if [[ -z "$VERSION" ]]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 v0.1.0"
    exit 1
fi

# GitLab configuration
GITLAB_URL="${CI_SERVER_URL:-https://gitlab.com}"

# Project IDs
SWIFT_PROJECT_ID="77955316"   # vauchi/vauchi-mobile-swift
ANDROID_PROJECT_ID="77955319" # vauchi/vauchi-mobile-android

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}╔════════════════════════════════════════╗${NC}"
echo -e "${YELLOW}║     Trigger Downstream Pipelines       ║${NC}"
echo -e "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""
echo "Version: $VERSION"
echo ""

# Determine authentication method:
#   - CI: CI_JOB_TOKEN with JOB-TOKEN header (Pipeline API)
#   - Local: GITLAB_TOKEN with PRIVATE-TOKEN header (Pipeline API)
# Both use the same /pipeline endpoint — only the auth header differs.
AUTH_HEADER=""
if [[ -n "${CI_JOB_TOKEN:-}" ]]; then
    AUTH_HEADER="JOB-TOKEN: $CI_JOB_TOKEN"
    echo "Auth: CI_JOB_TOKEN (Pipeline API)"
elif [[ -n "${GITLAB_TOKEN:-}" ]]; then
    AUTH_HEADER="PRIVATE-TOKEN: $GITLAB_TOKEN"
    echo "Auth: GITLAB_TOKEN (Pipeline API)"
else
    echo -e "${RED}Error: No authentication token found${NC}"
    echo "In CI: CI_JOB_TOKEN is auto-provisioned (ensure downstream allowlist is configured)"
    echo "Locally: Set GITLAB_TOKEN environment variable"
    exit 1
fi
echo ""

trigger_pipeline() {
    local project_id="$1"
    local project_name="$2"

    echo -e "${YELLOW}Triggering $project_name...${NC}"

    local response
    response=$(curl -s -w "\n%{http_code}" \
        --request POST \
        --header "$AUTH_HEADER" \
        --header "Content-Type: application/json" \
        --data "{\"ref\":\"main\",\"variables\":[{\"key\":\"UPSTREAM_VERSION\",\"value\":\"$VERSION\"}]}" \
        "$GITLAB_URL/api/v4/projects/$project_id/pipeline")

    local http_code=$(echo "$response" | tail -n1)
    local body=$(echo "$response" | head -n -1)

    if [[ "$http_code" == "201" || "$http_code" == "200" ]]; then
        local pipeline_url=$(echo "$body" | jq -r '.web_url // "unknown"')
        echo -e "${GREEN}  ✓ Triggered: $pipeline_url${NC}"
        return 0
    else
        echo -e "${RED}  ✗ Failed ($http_code)${NC}"
        echo "  Response: $body"
        return 1
    fi
}

# Trigger both repos
FAILED=false

trigger_pipeline "$SWIFT_PROJECT_ID" "vauchi-mobile-swift" || FAILED=true
trigger_pipeline "$ANDROID_PROJECT_ID" "vauchi-mobile-android" || FAILED=true

echo ""

if $FAILED; then
    echo -e "${RED}Some triggers failed${NC}"
    exit 1
else
    echo -e "${GREEN}All downstream pipelines triggered${NC}"
fi
