#!/bin/bash
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
TOKEN="${CI_JOB_TOKEN:-${GITLAB_TOKEN:-}}"

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

if [[ -z "$TOKEN" ]]; then
    echo -e "${RED}Error: No authentication token found${NC}"
    echo "Set CI_JOB_TOKEN (in CI) or GITLAB_TOKEN (local) environment variable"
    exit 1
fi

# Determine auth header
if [[ -n "${CI_JOB_TOKEN:-}" ]]; then
    AUTH_HEADER="JOB-TOKEN: $TOKEN"
else
    AUTH_HEADER="PRIVATE-TOKEN: $TOKEN"
fi

trigger_pipeline() {
    local project_id="$1"
    local project_name="$2"

    echo -e "${YELLOW}Triggering $project_name...${NC}"

    local response
    response=$(curl -s -w "\n%{http_code}" \
        --request POST \
        --header "$AUTH_HEADER" \
        --form "ref=main" \
        --form "variables[UPSTREAM_VERSION]=$VERSION" \
        "$GITLAB_URL/api/v4/projects/$project_id/trigger/pipeline")

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
