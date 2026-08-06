#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
# Validate UniFFI bindings have all expected types
#
# This script checks that generated bindings contain all expected types.
# Run this after regenerating bindings or in CI to catch drift early.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
WORKSPACE_ROOT="$(dirname "$PROJECT_ROOT")"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Expected types that must be present in bindings
# Update this list when adding new UniFFI-exported types
EXPECTED_TYPES=(
    "MobileContact"
    "MobileContactCard"
    "MobileContactField"
    "MobileExchangeResult"
    "MobileExchangeState"
    "MobileRecoveryClaim"
    "MobileRecoveryProgress"
    "MobileRecoveryVerification"
    "MobileRecoveryVoucher"
    "MobileSocialNetwork"
    "MobileSyncResult"
    "MobileSyncStatus"
    "MobileVisibilityLabel"
    "MobileVisibilityLabelDetail"
    "MobileFieldType"
    "MobileError"
    "VauchiPlatform"
)

# Minimum line counts (approximate, allows some variance)
MIN_SWIFT_LINES=3500
MIN_KOTLIN_LINES=5000

# Primary: check target/bindings/ (CI and local build output)
BINDINGS_DIR="$PROJECT_ROOT/target/bindings"
IOS_BINDINGS="$BINDINGS_DIR/ios/generated/vauchi_platform.swift"
ANDROID_BINDINGS="$BINDINGS_DIR/android/kotlin/uniffi/vauchi_platform/vauchi_platform.kt"
CABI_HEADER="$PROJECT_ROOT/vauchi-cabi/include/vauchi.h"

# Fallback: check sibling repos (legacy local dev paths)
if [[ ! -f "$IOS_BINDINGS" && -f "$WORKSPACE_ROOT/ios/Vauchi/Generated/vauchi_platform.swift" ]]; then
    IOS_BINDINGS="$WORKSPACE_ROOT/ios/Vauchi/Generated/vauchi_platform.swift"
fi
if [[ ! -f "$ANDROID_BINDINGS" && -f "$WORKSPACE_ROOT/android/app/src/main/kotlin/uniffi/vauchi_platform/vauchi_platform.kt" ]]; then
    ANDROID_BINDINGS="$WORKSPACE_ROOT/android/app/src/main/kotlin/uniffi/vauchi_platform/vauchi_platform.kt"
fi

printf '%b\n' "${YELLOW}╔════════════════════════════════════════╗${NC}"
printf '%b\n' "${YELLOW}║     Vauchi Bindings Validation         ║${NC}"
printf '%b\n' "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""

ERRORS=0

# Function to check a binding file
check_bindings() {
    local file="$1"
    local platform="$2"
    local min_lines="$3"
    local expected_export="$4"
    local missing=()

    printf '%b\n' "${YELLOW}Checking $platform bindings: $file${NC}"

    if [[ ! -f "$file" ]]; then
        printf '%b\n' "${RED}  ERROR: File not found!${NC}"
        return 1
    fi

    # Check line count
    local lines
    lines=$(wc -l < "$file")
    if [[ $lines -lt $min_lines ]]; then
        printf '%b\n' "${RED}  ERROR: File has $lines lines, expected at least $min_lines${NC}"
        printf '%b\n' "${RED}  This suggests bindings were generated from incomplete metadata.${NC}"
        printf '%b\n' "${RED}  Run: RUSTFLAGS=\"-Cstrip=none\" cargo build -p vauchi-platform --release${NC}"
        ERRORS=$((ERRORS + 1))
    else
        printf '%b\n' "${GREEN}  Line count OK: $lines lines${NC}"
    fi

    # Check for expected types
    for type in "${EXPECTED_TYPES[@]}"; do
        if ! grep -Fq "$type" "$file"; then
            missing+=("$type")
        fi
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        printf '%b\n' "${RED}  ERROR: Missing types:${NC}"
        for type in "${missing[@]}"; do
            printf '%b\n' "${RED}    - $type${NC}"
        done
        ERRORS=$((ERRORS + 1))
    else
        printf '%b\n' "${GREEN}  All ${#EXPECTED_TYPES[@]} expected types present${NC}"
    fi

    if ! grep -Fq "$expected_export" "$file"; then
        printf '%b\n' "${RED}  ERROR: Missing presentation export: $expected_export${NC}"
        ERRORS=$((ERRORS + 1))
    else
        printf '%b\n' "${GREEN}  Presentation export present${NC}"
    fi

    echo ""
}

check_cabi_header() {
    local expected_export="vauchi_presentation_contract_fixture(void)"

    printf '%b\n' "${YELLOW}Checking C ABI header: $CABI_HEADER${NC}"
    if [[ ! -f "$CABI_HEADER" ]]; then
        printf '%b\n' "${RED}  ERROR: File not found!${NC}"
        ERRORS=$((ERRORS + 1))
    elif ! grep -Fq "$expected_export" "$CABI_HEADER"; then
        printf '%b\n' "${RED}  ERROR: Missing presentation export: $expected_export${NC}"
        ERRORS=$((ERRORS + 1))
    else
        printf '%b\n' "${GREEN}  Presentation export present${NC}"
    fi
    echo ""
}

# Function to validate XCFramework structure
check_xcframework() {
    local xcfw_path="$1"

    printf '%b\n' "${YELLOW}Checking XCFramework: $xcfw_path${NC}"

    if [[ ! -d "$xcfw_path" ]]; then
        printf '%b\n' "${YELLOW}  XCFramework not found (skipping — may not be packaged yet)${NC}"
        echo ""
        return 0
    fi

    local slice_count=0
    local valid_count=0
    while IFS= read -r plist; do
        slice_count=$((slice_count + 1))
        local exec_name
        exec_name=$(/usr/libexec/PlistBuddy -c "Print :CFBundleExecutable" "$plist" 2>/dev/null || true)
        if [[ -z "$exec_name" ]]; then
            printf '%b\n' "${RED}  ERROR: Missing CFBundleExecutable in: $plist${NC}"
            ERRORS=$((ERRORS + 1))
        else
            local fw_dir
            fw_dir=$(dirname "$plist")
            if [[ -f "$fw_dir/$exec_name" ]]; then
                valid_count=$((valid_count + 1))
            else
                printf '%b\n' "${RED}  ERROR: CFBundleExecutable '$exec_name' not found in $fw_dir${NC}"
                ERRORS=$((ERRORS + 1))
            fi
        fi
    done < <(find "$xcfw_path" -name "Info.plist" -path "*.framework/Info.plist")

    if [[ $slice_count -eq 0 ]]; then
        printf '%b\n' "${RED}  ERROR: No framework slices found in XCFramework${NC}"
        ERRORS=$((ERRORS + 1))
    elif [[ $valid_count -eq $slice_count ]]; then
        printf '%b\n' "${GREEN}  All $slice_count framework slices valid (CFBundleExecutable present)${NC}"
    fi

    echo ""
}

# Check iOS bindings
if [[ -f "$IOS_BINDINGS" ]]; then
    check_bindings \
        "$IOS_BINDINGS" \
        "iOS (Swift)" \
        "$MIN_SWIFT_LINES" \
        "func presentationContractFixtureJson("
else
    printf '%b\n' "${YELLOW}iOS bindings not found (skipping - may not be on macOS)${NC}"
    echo ""
fi

# Check Android bindings
check_bindings \
    "$ANDROID_BINDINGS" \
    "Android (Kotlin)" \
    "$MIN_KOTLIN_LINES" \
    "fun presentationContractFixtureJson("

# Check the public C ABI declaration used by native frontends
check_cabi_header

# Check library metadata (if we can build)
printf '%b\n' "${YELLOW}Checking library metadata...${NC}"
cd "$PROJECT_ROOT"

if [[ -f "target/release/libvauchi_platform.so" ]]; then
    metadata_count=$(cargo run -p vauchi-platform --bin uniffi-bindgen --release -- print-repr target/release/libvauchi_platform.so 2>/dev/null | grep -c "Record\|Enum\|Object\|Interface" || true)
    if [[ $metadata_count -lt 20 ]]; then
        printf '%b\n' "${RED}  WARNING: Library has only $metadata_count metadata entries${NC}"
        printf '%b\n' "${RED}  Library may have been built with symbol stripping.${NC}"
        printf '%b\n' "${RED}  Rebuild with: RUSTFLAGS=\"-Cstrip=none\" cargo build -p vauchi-platform --release${NC}"
    else
        printf '%b\n' "${GREEN}  Library metadata OK: $metadata_count entries${NC}"
    fi
else
    printf '%b\n' "${YELLOW}  Native library not found (run cargo build first)${NC}"
fi

# Check XCFramework structure (if packaged)
XCFRAMEWORK_PATH="$PROJECT_ROOT/target/xcframework-build/VauchiPlatformFFI.xcframework"
if [[ -d "$XCFRAMEWORK_PATH" ]]; then
    check_xcframework "$XCFRAMEWORK_PATH"
fi

echo ""

# Summary
if [[ $ERRORS -gt 0 ]]; then
    printf '%b\n' "${RED}╔════════════════════════════════════════╗${NC}"
    printf '%b\n' "${RED}║     VALIDATION FAILED: $ERRORS error(s)      ║${NC}"
    printf '%b\n' "${RED}╚════════════════════════════════════════╝${NC}"
    echo ""
    echo "To fix:"
    echo "  1. cd $PROJECT_ROOT"
    echo "  2. RUSTFLAGS=\"-Cstrip=none\" cargo build -p vauchi-platform --release"
    echo "  3. ./scripts/build-bindings.sh"
    exit 1
else
    printf '%b\n' "${GREEN}╔════════════════════════════════════════╗${NC}"
    printf '%b\n' "${GREEN}║     VALIDATION PASSED                  ║${NC}"
    printf '%b\n' "${GREEN}╚════════════════════════════════════════╝${NC}"
    exit 0
fi
