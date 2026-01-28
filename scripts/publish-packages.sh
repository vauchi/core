#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
# Publish packaged bindings to GitLab Generic Packages
#
# This script uploads:
# - iOS XCFramework to GitLab Generic Packages
# - Android bindings to GitLab Generic Packages
#
# Prerequisites:
#   - Run package-xcframework.sh and/or package-android.sh first
#   - CI_JOB_TOKEN or GITLAB_TOKEN environment variable set
#   - CI_PROJECT_ID or GITLAB_PROJECT_ID environment variable set
#
# Usage:
#   ./publish-packages.sh [version]
#
# Environment:
#   CI_JOB_TOKEN     - GitLab CI job token (set automatically in CI)
#   GITLAB_TOKEN     - Personal access token (for local use)
#   CI_PROJECT_ID    - GitLab project ID (set automatically in CI)
#   GITLAB_PROJECT_ID - Project ID (for local use, default: vauchi/core)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
DIST_DIR="$PROJECT_ROOT/dist"

# Version from argument or Cargo.toml (strip v prefix from tags like v0.1.0)
RAW_VERSION="${1:-$(grep -m1 'version = ' "$PROJECT_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')}"
VERSION="${RAW_VERSION#v}"

# GitLab configuration
GITLAB_URL="${CI_SERVER_URL:-https://gitlab.com}"
PROJECT_ID="${CI_PROJECT_ID:-${GITLAB_PROJECT_ID:-}}"
TOKEN="${CI_JOB_TOKEN:-${GITLAB_TOKEN:-}}"
PACKAGE_NAME="vauchi-mobile"

# Colors
RED='[0;31m'
GREEN='[0;32m'
YELLOW='[1;33m'
NC='[0m'

echo -e "${YELLOW}╔════════════════════════════════════════╗${NC}"
echo -e "${YELLOW}║     Publish Packages v$VERSION            ${NC}"
echo -e "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""

# Validate environment
if [[ -z "$TOKEN" ]]; then
    echo -e "${RED}Error: No authentication token found${NC}"
    echo "Set CI_JOB_TOKEN (in CI) or GITLAB_TOKEN (local) environment variable"
    exit 1
fi

if [[ -z "$PROJECT_ID" ]]; then
    # Try to get project ID from API
    echo -e "${YELLOW}Fetching project ID...${NC}"
    PROJECT_ID=$(curl -s --header "PRIVATE-TOKEN: $TOKEN" \
        "$GITLAB_URL/api/v4/projects/vauchi%2Fcore" | jq -r '.id')

    if [[ "$PROJECT_ID" == "null" || -z "$PROJECT_ID" ]]; then
        echo -e "${RED}Error: Could not determine project ID${NC}"
        echo "Set CI_PROJECT_ID or GITLAB_PROJECT_ID environment variable"
        exit 1
    fi
fi

echo "GitLab URL: $GITLAB_URL"
echo "Project ID: $PROJECT_ID"
echo "Package: $PACKAGE_NAME"
echo "Version: $VERSION"
echo ""

# Determine auth header
if [[ -n "${CI_JOB_TOKEN:-}" ]]; then
    AUTH_HEADER="JOB-TOKEN: $TOKEN"
else
    AUTH_HEADER="PRIVATE-TOKEN: $TOKEN"
fi

PACKAGE_URL="$GITLAB_URL/api/v4/projects/$PROJECT_ID/packages/generic/$PACKAGE_NAME/$VERSION"

upload_file() {
    local file="$1"
    local filename=$(basename "$file")

    if [[ ! -f "$file" ]]; then
        echo -e "${YELLOW}Skipping $filename (not found)${NC}"
        return 0
    fi

    echo -e "${YELLOW}Uploading $filename...${NC}"

    local response
    response=$(curl -s -w "
%{http_code}" \
        --header "$AUTH_HEADER" \
        --upload-file "$file" \
        "$PACKAGE_URL/$filename")

    local http_code=$(echo "$response" | tail -n1)
    local body=$(echo "$response" | head -n -1)

    if [[ "$http_code" == "201" || "$http_code" == "200" ]]; then
        echo -e "${GREEN}  ✓ Uploaded: $filename${NC}"
        return 0
    elif [[ "$http_code" == "409" ]]; then
        echo -e "${YELLOW}  ⚠ Already exists: $filename${NC}"
        return 0
    else
        echo -e "${RED}  ✗ Failed ($http_code): $filename${NC}"
        echo "  Response: $body"
        return 1
    fi
}

# Track success
UPLOAD_SUCCESS=true

# Upload iOS artifacts
echo -e "${YELLOW}=== iOS Artifacts ===${NC}"
upload_file "$DIST_DIR/VauchiMobileFFI.xcframework.zip" || UPLOAD_SUCCESS=false
upload_file "$DIST_DIR/VauchiMobileFFI.xcframework.zip.sha256" || UPLOAD_SUCCESS=false
upload_file "$DIST_DIR/VauchiMobile-$VERSION.zip" || UPLOAD_SUCCESS=false

# Upload Android artifacts
echo ""
echo -e "${YELLOW}=== Android Artifacts ===${NC}"
upload_file "$DIST_DIR/vauchi-mobile-android-$VERSION.zip" || UPLOAD_SUCCESS=false
upload_file "$DIST_DIR/vauchi-mobile-android-$VERSION.zip.sha256" || UPLOAD_SUCCESS=false

echo ""

if $UPLOAD_SUCCESS; then
    echo -e "${GREEN}╔════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║         Publish Complete               ║${NC}"
    echo -e "${GREEN}╚════════════════════════════════════════╝${NC}"
    echo ""
    echo "Package URL:"
    echo "  $GITLAB_URL/vauchi/core/-/packages"
    echo ""
    echo "Direct download URLs:"
    echo "  iOS XCFramework:"
    echo "    $PACKAGE_URL/VauchiMobileFFI.xcframework.zip"
    echo ""
    echo "  Android:"
    echo "    $PACKAGE_URL/vauchi-mobile-android-$VERSION.zip"
else
    echo -e "${RED}╔════════════════════════════════════════╗${NC}"
    echo -e "${RED}║         Publish Failed                 ║${NC}"
    echo -e "${RED}╚════════════════════════════════════════╝${NC}"
    exit 1
fi
