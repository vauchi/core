#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
# Trigger downstream package repos after a release
#
# This script triggers CI pipelines in:
# - vauchi/vauchi-mobile-swift
# - vauchi/vauchi-mobile-android
#
# Prerequisites:
#   - CI_JOB_TOKEN or GITLAB_TOKEN environment variable
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

# Project IDs and their trigger tokens
# In CI: use dedicated trigger tokens (can set variables)
# Locally: use GITLAB_TOKEN with pipeline API
SWIFT_PROJECT_ID="77955316"   # vauchi/vauchi-mobile-swift
ANDROID_PROJECT_ID="77955319" # vauchi/vauchi-mobile-android
SWIFT_TOKEN="${SWIFT_TRIGGER_TOKEN:-${GITLAB_TOKEN:-}}"
ANDROID_TOKEN="${ANDROID_TRIGGER_TOKEN:-${GITLAB_TOKEN:-}}"

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

# Determine which API endpoint and auth to use:
#   - CI: /trigger/pipeline with dedicated trigger tokens (can set variables)
#   - Local: /pipeline with PRIVATE-TOKEN header
USE_TRIGGER_API=false
if [[ -n "${SWIFT_TRIGGER_TOKEN:-}" && -n "${ANDROID_TRIGGER_TOKEN:-}" ]]; then
    USE_TRIGGER_API=true
elif [[ -z "${GITLAB_TOKEN:-}" ]]; then
    echo -e "${RED}Error: No authentication token found${NC}"
    echo "In CI: Set SWIFT_TRIGGER_TOKEN and ANDROID_TRIGGER_TOKEN"
    echo "Locally: Set GITLAB_TOKEN environment variable"
    exit 1
fi

trigger_pipeline() {
    local project_id="$1"
    local project_name="$2"
    local token="$3"

    echo -e "${YELLOW}Triggering $project_name...${NC}"

    local response
    if $USE_TRIGGER_API; then
        # CI: use /trigger/pipeline with dedicated trigger token
        response=$(curl -s -w "\n%{http_code}" \
            --request POST \
            --form "token=$token" \
            --form "ref=main" \
            --form "variables[UPSTREAM_VERSION]=$VERSION" \
            "$GITLAB_URL/api/v4/projects/$project_id/trigger/pipeline")
    else
        # Local: use /pipeline with PRIVATE-TOKEN header
        response=$(curl -s -w "\n%{http_code}" \
            --request POST \
            --header "PRIVATE-TOKEN: $token" \
            --header "Content-Type: application/json" \
            --data "{\"ref\":\"main\",\"variables\":[{\"key\":\"UPSTREAM_VERSION\",\"value\":\"$VERSION\"}]}" \
            "$GITLAB_URL/api/v4/projects/$project_id/pipeline")
    fi

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

trigger_pipeline "$SWIFT_PROJECT_ID" "vauchi-mobile-swift" "$SWIFT_TOKEN" || FAILED=true
trigger_pipeline "$ANDROID_PROJECT_ID" "vauchi-mobile-android" "$ANDROID_TOKEN" || FAILED=true

echo ""

if $FAILED; then
    echo -e "${RED}Some triggers failed${NC}"
    exit 1
else
    echo -e "${GREEN}All downstream pipelines triggered${NC}"
fi
