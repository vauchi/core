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
PACKAGE_NAME="vauchi-platform"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Note: pre-release versions (dev/rc) are allowed — CI rules control which tags
# reach this job. Dev tags need publishing for test bindings.

printf '%b\n' "${YELLOW}╔════════════════════════════════════════╗${NC}"
printf '%b\n' "${YELLOW}║     Publish Packages v$VERSION            ${NC}"
printf '%b\n' "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""

# Validate environment
if [[ -z "$TOKEN" ]]; then
    printf '%b\n' "${RED}Error: No authentication token found${NC}"
    echo "Set CI_JOB_TOKEN (in CI) or GITLAB_TOKEN (local) environment variable"
    exit 1
fi

if [[ -z "$PROJECT_ID" ]]; then
    # Try to get project ID from API
    printf '%b\n' "${YELLOW}Fetching project ID...${NC}"
    PROJECT_BODY=$(mktemp)
    PROJECT_STATUS=$(curl --silent --show-error -o "$PROJECT_BODY" -w '%{http_code}' \
        --header "PRIVATE-TOKEN: $TOKEN" \
        "$GITLAB_URL/api/v4/projects/vauchi%2Fcore") || PROJECT_STATUS="000"
    PROJECT_ID=""
    if [[ "$PROJECT_STATUS" == "200" ]]; then
        PROJECT_ID=$(jq -r '.id // empty' "$PROJECT_BODY")
    fi
    rm -f "$PROJECT_BODY"

    if [[ -z "$PROJECT_ID" ]]; then
        printf '%b\n' "${RED}Error: Could not determine project ID (GitLab API returned HTTP ${PROJECT_STATUS})${NC}"
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
    local filename
    filename=$(basename "$file")

    if [[ ! -f "$file" ]]; then
        printf '%b\n' "${YELLOW}Skipping $filename (not found)${NC}"
        return 0
    fi

    printf '%b\n' "${YELLOW}Uploading $filename...${NC}"

    local response
    response=$(curl -s -w "\n%{http_code}" \
        --header "$AUTH_HEADER" \
        --upload-file "$file" \
        "$PACKAGE_URL/$filename")

    local http_code body
    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | sed '$d')

    if [[ "$http_code" == "201" || "$http_code" == "200" ]]; then
        printf '%b\n' "${GREEN}  ✓ Uploaded: $filename${NC}"
        return 0
    elif [[ "$http_code" == "409" ]]; then
        printf '%b\n' "${YELLOW}  ⚠ Already exists: $filename${NC}"
        return 0
    elif [[ "$http_code" == "400" && "$body" == *"Duplicate package is not allowed"* ]]; then
        # GitLab returns 400 (not 409) when the *generic-package*
        # `duplicates_allowed` toggle is off and the file already
        # exists at this path. Treat as the idempotent case so a
        # retried `publish:packages` job (e.g. after a partial failure
        # in another artifact group) doesn't fail on already-uploaded
        # files. Body match is required — bare 400 is a real "Bad
        # Request" we still want to fail loudly on. Recurrence note:
        # this is what skipped `update:swift-bindings` for core
        # v0.49.0 — see _private/docs/problems/2026-04-28-platform-swift-v0.28.1-tag-mismatch/.
        printf '%b\n' "${YELLOW}  ⚠ Already exists (400 dup): $filename${NC}"
        return 0
    else
        printf '%b\n' "${RED}  ✗ Failed ($http_code): $filename${NC}"
        echo "  Response: $body"
        return 1
    fi
}

# Track success
UPLOAD_SUCCESS=true

# Upload iOS artifacts
printf '%b\n' "${YELLOW}=== iOS Artifacts ===${NC}"
upload_file "$DIST_DIR/VauchiPlatformFFI.xcframework.zip" || UPLOAD_SUCCESS=false
upload_file "$DIST_DIR/VauchiPlatformFFI.xcframework.zip.sha256" || UPLOAD_SUCCESS=false
upload_file "$DIST_DIR/VauchiPlatformFFI.xcframework.zip.sha256.bundle" || UPLOAD_SUCCESS=false
upload_file "$DIST_DIR/VauchiPlatform-$VERSION.zip" || UPLOAD_SUCCESS=false

# Upload Android artifacts
echo ""
printf '%b\n' "${YELLOW}=== Android Artifacts ===${NC}"
upload_file "$DIST_DIR/vauchi-platform-kotlin-$VERSION.zip" || UPLOAD_SUCCESS=false
upload_file "$DIST_DIR/vauchi-platform-kotlin-$VERSION.zip.sha256" || UPLOAD_SUCCESS=false
upload_file "$DIST_DIR/vauchi-platform-kotlin-$VERSION.zip.sha256.bundle" || UPLOAD_SUCCESS=false

# Upload SBOM artifacts (T0-2)
echo ""
printf '%b\n' "${YELLOW}=== SBOM Artifacts ===${NC}"
for sbom in "$DIST_DIR"/*.sbom.json; do
    [ -f "$sbom" ] || continue
    upload_file "$sbom" || UPLOAD_SUCCESS=false
    upload_file "${sbom}.sha256" || UPLOAD_SUCCESS=false
    upload_file "${sbom}.sha256.bundle" || UPLOAD_SUCCESS=false
done

# Upload SLSA provenance (G3)
if [ -f "$DIST_DIR/provenance.intoto.jsonl" ]; then
    echo ""
    printf '%b\n' "${YELLOW}=== SLSA Provenance ===${NC}"
    upload_file "$DIST_DIR/provenance.intoto.jsonl" || UPLOAD_SUCCESS=false
    upload_file "$DIST_DIR/provenance.intoto.jsonl.bundle" || UPLOAD_SUCCESS=false
fi

echo ""

if $UPLOAD_SUCCESS; then
    printf '%b\n' "${GREEN}╔════════════════════════════════════════╗${NC}"
    printf '%b\n' "${GREEN}║         Publish Complete               ║${NC}"
    printf '%b\n' "${GREEN}╚════════════════════════════════════════╝${NC}"
    echo ""
    echo "Package URL:"
    echo "  $GITLAB_URL/vauchi/core/-/packages"
    echo ""
    echo "Direct download URLs:"
    echo "  iOS XCFramework:"
    echo "    $PACKAGE_URL/VauchiPlatformFFI.xcframework.zip"
    echo ""
    echo "  Android:"
    echo "    $PACKAGE_URL/vauchi-platform-kotlin-$VERSION.zip"
else
    printf '%b\n' "${RED}╔════════════════════════════════════════╗${NC}"
    printf '%b\n' "${RED}║         Publish Failed                 ║${NC}"
    printf '%b\n' "${RED}╚════════════════════════════════════════╝${NC}"
    exit 1
fi
