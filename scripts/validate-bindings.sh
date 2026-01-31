#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
# Validate UniFFI bindings have all expected types
#
# This script checks that generated bindings contain all expected types.
# Run this after regenerating bindings or in CI to catch drift early.

set -e

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
    "MobileExchangeData"
    "MobileExchangeResult"
    "MobilePasswordCheck"
    "MobilePasswordStrength"
    "MobileProximityResult"
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
    "VauchiMobile"
    "MobileProximityVerifier"
    "PlatformAudioHandler"
)

# Minimum line counts (approximate, allows some variance)
MIN_SWIFT_LINES=3500
MIN_KOTLIN_LINES=5000

# Primary: check target/bindings/ (CI and local build output)
BINDINGS_DIR="$PROJECT_ROOT/target/bindings"
IOS_BINDINGS="$BINDINGS_DIR/ios/generated/vauchi_mobile.swift"
ANDROID_BINDINGS="$BINDINGS_DIR/android/kotlin/uniffi/vauchi_mobile/vauchi_mobile.kt"

# Fallback: check sibling repos (legacy local dev paths)
if [[ ! -f "$IOS_BINDINGS" && -f "$WORKSPACE_ROOT/ios/Vauchi/Generated/vauchi_mobile.swift" ]]; then
    IOS_BINDINGS="$WORKSPACE_ROOT/ios/Vauchi/Generated/vauchi_mobile.swift"
fi
if [[ ! -f "$ANDROID_BINDINGS" && -f "$WORKSPACE_ROOT/android/app/src/main/kotlin/uniffi/vauchi_mobile/vauchi_mobile.kt" ]]; then
    ANDROID_BINDINGS="$WORKSPACE_ROOT/android/app/src/main/kotlin/uniffi/vauchi_mobile/vauchi_mobile.kt"
fi

echo -e "${YELLOW}╔════════════════════════════════════════╗${NC}"
echo -e "${YELLOW}║     Vauchi Bindings Validation         ║${NC}"
echo -e "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""

ERRORS=0

# Function to check a binding file
check_bindings() {
    local file="$1"
    local platform="$2"
    local min_lines="$3"
    local missing=()

    echo -e "${YELLOW}Checking $platform bindings: $file${NC}"

    if [[ ! -f "$file" ]]; then
        echo -e "${RED}  ERROR: File not found!${NC}"
        return 1
    fi

    # Check line count
    local lines=$(wc -l < "$file")
    if [[ $lines -lt $min_lines ]]; then
        echo -e "${RED}  ERROR: File has $lines lines, expected at least $min_lines${NC}"
        echo -e "${RED}  This suggests bindings were generated from incomplete metadata.${NC}"
        echo -e "${RED}  Run: RUSTFLAGS=\"-Cstrip=none\" cargo build -p vauchi-mobile --release${NC}"
        ERRORS=$((ERRORS + 1))
    else
        echo -e "${GREEN}  Line count OK: $lines lines${NC}"
    fi

    # Check for expected types
    for type in "${EXPECTED_TYPES[@]}"; do
        if ! grep -q "$type" "$file"; then
            missing+=("$type")
        fi
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        echo -e "${RED}  ERROR: Missing types:${NC}"
        for type in "${missing[@]}"; do
            echo -e "${RED}    - $type${NC}"
        done
        ERRORS=$((ERRORS + 1))
    else
        echo -e "${GREEN}  All ${#EXPECTED_TYPES[@]} expected types present${NC}"
    fi

    echo ""
}

# Function to validate XCFramework structure
check_xcframework() {
    local xcfw_path="$1"

    echo -e "${YELLOW}Checking XCFramework: $xcfw_path${NC}"

    if [[ ! -d "$xcfw_path" ]]; then
        echo -e "${YELLOW}  XCFramework not found (skipping — may not be packaged yet)${NC}"
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
            echo -e "${RED}  ERROR: Missing CFBundleExecutable in: $plist${NC}"
            ERRORS=$((ERRORS + 1))
        else
            local fw_dir
            fw_dir=$(dirname "$plist")
            if [[ -f "$fw_dir/$exec_name" ]]; then
                valid_count=$((valid_count + 1))
            else
                echo -e "${RED}  ERROR: CFBundleExecutable '$exec_name' not found in $fw_dir${NC}"
                ERRORS=$((ERRORS + 1))
            fi
        fi
    done < <(find "$xcfw_path" -name "Info.plist" -path "*.framework/Info.plist")

    if [[ $slice_count -eq 0 ]]; then
        echo -e "${RED}  ERROR: No framework slices found in XCFramework${NC}"
        ERRORS=$((ERRORS + 1))
    elif [[ $valid_count -eq $slice_count ]]; then
        echo -e "${GREEN}  All $slice_count framework slices valid (CFBundleExecutable present)${NC}"
    fi

    echo ""
}

# Check iOS bindings
if [[ -f "$IOS_BINDINGS" ]]; then
    check_bindings "$IOS_BINDINGS" "iOS (Swift)" "$MIN_SWIFT_LINES"
else
    echo -e "${YELLOW}iOS bindings not found (skipping - may not be on macOS)${NC}"
    echo ""
fi

# Check Android bindings
check_bindings "$ANDROID_BINDINGS" "Android (Kotlin)" "$MIN_KOTLIN_LINES"

# Check library metadata (if we can build)
echo -e "${YELLOW}Checking library metadata...${NC}"
cd "$PROJECT_ROOT"

if [[ -f "target/release/libvauchi_mobile.so" ]]; then
    metadata_count=$(cargo run -p vauchi-mobile --bin uniffi-bindgen --release -- print-repr target/release/libvauchi_mobile.so 2>/dev/null | grep -c "Record\|Enum\|Object\|Interface" || true)
    if [[ $metadata_count -lt 20 ]]; then
        echo -e "${RED}  WARNING: Library has only $metadata_count metadata entries${NC}"
        echo -e "${RED}  Library may have been built with symbol stripping.${NC}"
        echo -e "${RED}  Rebuild with: RUSTFLAGS=\"-Cstrip=none\" cargo build -p vauchi-mobile --release${NC}"
    else
        echo -e "${GREEN}  Library metadata OK: $metadata_count entries${NC}"
    fi
else
    echo -e "${YELLOW}  Native library not found (run cargo build first)${NC}"
fi

# Check XCFramework structure (if packaged)
XCFRAMEWORK_PATH="$PROJECT_ROOT/target/xcframework-build/VauchiMobileFFI.xcframework"
if [[ -d "$XCFRAMEWORK_PATH" ]]; then
    check_xcframework "$XCFRAMEWORK_PATH"
fi

echo ""

# Summary
if [[ $ERRORS -gt 0 ]]; then
    echo -e "${RED}╔════════════════════════════════════════╗${NC}"
    echo -e "${RED}║     VALIDATION FAILED: $ERRORS error(s)      ║${NC}"
    echo -e "${RED}╚════════════════════════════════════════╝${NC}"
    echo ""
    echo "To fix:"
    echo "  1. cd $PROJECT_ROOT"
    echo "  2. RUSTFLAGS=\"-Cstrip=none\" cargo build -p vauchi-mobile --release"
    echo "  3. ./scripts/build-bindings.sh"
    exit 1
else
    echo -e "${GREEN}╔════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║     VALIDATION PASSED                  ║${NC}"
    echo -e "${GREEN}╚════════════════════════════════════════╝${NC}"
    exit 0
fi
