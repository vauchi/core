#!/bin/bash
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

IOS_BINDINGS="$WORKSPACE_ROOT/ios/Vauchi/Generated/vauchi_mobile.swift"
ANDROID_BINDINGS="$WORKSPACE_ROOT/android/app/src/main/kotlin/uniffi/vauchi_mobile/vauchi_mobile.kt"

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
