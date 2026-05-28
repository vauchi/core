#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
# Package iOS bindings into an XCFramework
#
# This script:
# 1. Creates XCFramework from device and simulator static libraries
# 2. Bundles Swift bindings alongside the framework
# 3. Creates a distributable zip archive
#
# Prerequisites:
#   - Run build-bindings.sh --ios first
#   - macOS with Xcode command line tools
#
# Usage:
#   ./package-xcframework.sh [version]
#
# Output:
#   dist/VauchiPlatformFFI.xcframework.zip

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
WORKSPACE_ROOT="$(dirname "$PROJECT_ROOT")"

# Version from argument or Cargo.toml (strip v prefix from tags like v0.1.0)
RAW_VERSION="${1:-$(grep -m1 'version = ' "$PROJECT_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')}"
VERSION="${RAW_VERSION#v}"

# Note: pre-release versions (dev/rc) are allowed — CI rules control which tags
# reach this job. Dev tags need packaging for test bindings.

# Paths — read from target/bindings/ (output of build-bindings.sh)
BINDINGS_DIR="$PROJECT_ROOT/target/bindings"
IOS_LIBS_DIR="$BINDINGS_DIR/ios/libs"
IOS_GENERATED_DIR="$BINDINGS_DIR/ios/generated"
MACOS_LIBS_DIR="$BINDINGS_DIR/macos/libs"
DIST_DIR="$PROJECT_ROOT/dist"
BUILD_DIR="$PROJECT_ROOT/target/xcframework-build"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}╔════════════════════════════════════════╗${NC}"
echo -e "${YELLOW}║     Package XCFramework v$VERSION         ${NC}"
echo -e "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""

# Check prerequisites
if [[ "$(uname)" != "Darwin" ]]; then
    echo -e "${RED}Error: XCFramework packaging requires macOS${NC}"
    exit 1
fi

if [[ ! -f "$IOS_LIBS_DIR/libvauchi_platform_device.a" ]]; then
    echo -e "${RED}Error: iOS libraries not found. Run build-bindings.sh --ios first${NC}"
    exit 1
fi

if [[ ! -f "$IOS_GENERATED_DIR/vauchi_platform.swift" ]]; then
    echo -e "${RED}Error: Swift bindings not found. Run build-bindings.sh --ios first${NC}"
    exit 1
fi

# Clean and create directories (rm dist/ to prevent stale artifacts from prior
# pipeline runs accumulating on shell runners — see 2026-03-16 problem record)
rm -rf "$BUILD_DIR"
rm -rf "$DIST_DIR"
mkdir -p "$BUILD_DIR"
mkdir -p "$DIST_DIR"

# Create module map for the FFI layer
echo -e "${YELLOW}Creating module map...${NC}"
HEADERS_DIR="$BUILD_DIR/Headers"
mkdir -p "$HEADERS_DIR"

# Copy the generated C header (UniFFI generates this)
if [[ -f "$IOS_GENERATED_DIR/vauchi_platformFFI.h" ]]; then
    cp "$IOS_GENERATED_DIR/vauchi_platformFFI.h" "$HEADERS_DIR/"
else
    # Generate a minimal header if not present
    cat > "$HEADERS_DIR/vauchi_platformFFI.h" << 'EOF'
// VauchiPlatformFFI - UniFFI generated C bindings
// This header is auto-generated. Do not edit.

#ifndef VAUCHI_PLATFORM_FFI_H
#define VAUCHI_PLATFORM_FFI_H

#include <stdint.h>
#include <stdbool.h>

// UniFFI scaffolding types are defined in the Swift bindings
// This header exists for XCFramework module map requirements

#endif // VAUCHI_PLATFORM_FFI_H
EOF
fi

# Create module map
cat > "$HEADERS_DIR/module.modulemap" << 'EOF'
framework module VauchiPlatformFFI {
    umbrella header "vauchi_platformFFI.h"
    export *
    module * { export * }
    link "vauchi_platform"
}
EOF

# Create XCFramework structure for device
echo -e "${YELLOW}Preparing device slice...${NC}"
DEVICE_DIR="$BUILD_DIR/ios-arm64"
mkdir -p "$DEVICE_DIR/VauchiPlatformFFI.framework"
cp "$IOS_LIBS_DIR/libvauchi_platform_device.a" "$DEVICE_DIR/VauchiPlatformFFI.framework/VauchiPlatformFFI"
cp -r "$HEADERS_DIR" "$DEVICE_DIR/VauchiPlatformFFI.framework/Headers"
mkdir -p "$DEVICE_DIR/VauchiPlatformFFI.framework/Modules"
cp "$HEADERS_DIR/module.modulemap" "$DEVICE_DIR/VauchiPlatformFFI.framework/Modules/"

# Create Info.plist for device framework
cat > "$DEVICE_DIR/VauchiPlatformFFI.framework/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>VauchiPlatformFFI</string>
    <key>CFBundleIdentifier</key>
    <string>com.vauchi.VauchiPlatformFFI</string>
    <key>CFBundleName</key>
    <string>VauchiPlatformFFI</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundlePackageType</key>
    <string>FMWK</string>
</dict>
</plist>
EOF

# Create XCFramework structure for simulator
echo -e "${YELLOW}Preparing simulator slice...${NC}"
SIM_DIR="$BUILD_DIR/ios-arm64_x86_64-simulator"
mkdir -p "$SIM_DIR/VauchiPlatformFFI.framework"
cp "$IOS_LIBS_DIR/libvauchi_platform_sim.a" "$SIM_DIR/VauchiPlatformFFI.framework/VauchiPlatformFFI"
cp -r "$HEADERS_DIR" "$SIM_DIR/VauchiPlatformFFI.framework/Headers"
mkdir -p "$SIM_DIR/VauchiPlatformFFI.framework/Modules"
cp "$HEADERS_DIR/module.modulemap" "$SIM_DIR/VauchiPlatformFFI.framework/Modules/"
cp "$DEVICE_DIR/VauchiPlatformFFI.framework/Info.plist" "$SIM_DIR/VauchiPlatformFFI.framework/Info.plist"

# Create XCFramework structure for macOS (if macOS libs exist)
MACOS_FRAMEWORK_ARG=""
if [[ -f "$MACOS_LIBS_DIR/libvauchi_platform_macos.a" ]]; then
    echo -e "${YELLOW}Preparing macOS slice (versioned bundle)...${NC}"
    MACOS_DIR="$BUILD_DIR/macos-arm64_x86_64"
    MACOS_FW="$MACOS_DIR/VauchiPlatformFFI.framework"
    # macOS requires versioned framework bundles (not flat iOS-style)
    mkdir -p "$MACOS_FW/Versions/A/Headers"
    mkdir -p "$MACOS_FW/Versions/A/Modules"
    mkdir -p "$MACOS_FW/Versions/A/Resources"
    cp "$MACOS_LIBS_DIR/libvauchi_platform_macos.a" "$MACOS_FW/Versions/A/VauchiPlatformFFI"
    cp "$HEADERS_DIR/vauchi_platformFFI.h" "$MACOS_FW/Versions/A/Headers/"
    cp "$HEADERS_DIR/module.modulemap" "$MACOS_FW/Versions/A/Headers/"
    cp "$HEADERS_DIR/module.modulemap" "$MACOS_FW/Versions/A/Modules/"
    cp "$DEVICE_DIR/VauchiPlatformFFI.framework/Info.plist" "$MACOS_FW/Versions/A/Resources/"
    # Create required symlinks
    (cd "$MACOS_FW/Versions" && ln -sf A Current)
    (cd "$MACOS_FW" && ln -sf Versions/Current/Headers Headers)
    (cd "$MACOS_FW" && ln -sf Versions/Current/Modules Modules)
    (cd "$MACOS_FW" && ln -sf Versions/Current/Resources Resources)
    (cd "$MACOS_FW" && ln -sf Versions/Current/VauchiPlatformFFI VauchiPlatformFFI)
    MACOS_FRAMEWORK_ARG="-framework $MACOS_FW"
else
    echo -e "${YELLOW}No macOS libraries found — XCFramework will be iOS-only${NC}"
fi

# Create XCFramework
echo -e "${YELLOW}Creating XCFramework...${NC}"
XCFRAMEWORK_PATH="$BUILD_DIR/VauchiPlatformFFI.xcframework"

xcodebuild -create-xcframework \
    -framework "$DEVICE_DIR/VauchiPlatformFFI.framework" \
    -framework "$SIM_DIR/VauchiPlatformFFI.framework" \
    $MACOS_FRAMEWORK_ARG \
    -output "$XCFRAMEWORK_PATH"

echo -e "${GREEN}XCFramework created at: $XCFRAMEWORK_PATH${NC}"

# Post-process: Re-inject CFBundleExecutable into framework slice plists
# xcodebuild -create-xcframework regenerates Info.plists and strips
# CFBundleExecutable for static-library frameworks. iOS requires this key.
echo -e "${YELLOW}Post-processing: Ensuring CFBundleExecutable in framework slices...${NC}"
while IFS= read -r plist; do
    if ! /usr/libexec/PlistBuddy -c "Print :CFBundleExecutable" "$plist" 2>/dev/null; then
        echo "  Adding CFBundleExecutable to: $plist"
        /usr/libexec/PlistBuddy -c "Add :CFBundleExecutable string VauchiPlatformFFI" "$plist"
    else
        echo "  CFBundleExecutable already present in: $plist"
    fi
done < <(find "$XCFRAMEWORK_PATH" -name "Info.plist" -path "*/VauchiPlatformFFI.framework/*")
echo -e "${GREEN}Post-processing complete${NC}"

# Create distribution package
echo -e "${YELLOW}Creating distribution package...${NC}"
PACKAGE_DIR="$BUILD_DIR/VauchiPlatform-$VERSION"
mkdir -p "$PACKAGE_DIR"

# Copy XCFramework
cp -r "$XCFRAMEWORK_PATH" "$PACKAGE_DIR/"

# Copy Swift bindings
mkdir -p "$PACKAGE_DIR/Sources"
cp "$IOS_GENERATED_DIR/vauchi_platform.swift" "$PACKAGE_DIR/Sources/"

# Copy golden fixtures for frontend contract tests
GOLDEN_SRC="$PROJECT_ROOT/vauchi-core/tests/fixtures/golden"
if [[ -d "$GOLDEN_SRC" ]]; then
    echo -e "${YELLOW}Copying golden fixtures...${NC}"
    mkdir -p "$PACKAGE_DIR/GoldenFixtures"
    cp "$GOLDEN_SRC"/*.json "$PACKAGE_DIR/GoldenFixtures/"
    if [[ -f "$GOLDEN_SRC/.version" ]]; then
        cp "$GOLDEN_SRC/.version" "$PACKAGE_DIR/GoldenFixtures/"
    fi
    echo "  $(ls "$PACKAGE_DIR/GoldenFixtures/"*.json | wc -l | tr -d ' ') fixtures copied"
fi

# Create README
cat > "$PACKAGE_DIR/README.md" << EOF
# VauchiPlatform v$VERSION

UniFFI bindings for Vauchi iOS apps.

## Contents

- \`VauchiPlatformFFI.xcframework/\` - Native library (iOS device + simulator + macOS)
- \`Sources/vauchi_platform.swift\` - Swift bindings

## Integration

### Swift Package Manager (Binary Target)

\`\`\`swift
.binaryTarget(
    name: "VauchiPlatformFFI",
    url: "https://gitlab.com/api/v4/projects/vauchi%2Fcore/packages/generic/vauchi-platform/$VERSION/VauchiPlatformFFI.xcframework.zip",
    checksum: "CHECKSUM_HERE"
)
\`\`\`

### Manual Integration

1. Drag \`VauchiPlatformFFI.xcframework\` into your Xcode project
2. Add \`Sources/vauchi_platform.swift\` to your target
3. Import and use: \`import VauchiPlatform\`

## License

MIT License - see https://gitlab.com/vauchi/core
EOF

# Create zip archive (-y preserves macOS framework symlinks)
ZIP_PATH="$DIST_DIR/VauchiPlatform-$VERSION.zip"
cd "$BUILD_DIR"
zip -ry "$ZIP_PATH" "VauchiPlatform-$VERSION"

# Also create framework-only zip for SPM binary target
# -y preserves symlinks (macOS versioned framework bundles use symlinks
# like Versions/Current → A; without -y, zip follows them and stores
# duplicate files, which Xcode can't resolve on extraction)
XCFRAMEWORK_ZIP="$DIST_DIR/VauchiPlatformFFI.xcframework.zip"
cd "$BUILD_DIR"
zip -ry "$XCFRAMEWORK_ZIP" "VauchiPlatformFFI.xcframework"

# Calculate checksums
echo -e "${YELLOW}Calculating checksums...${NC}"
CHECKSUM=$(swift package compute-checksum "$XCFRAMEWORK_ZIP" 2>/dev/null || shasum -a 256 "$XCFRAMEWORK_ZIP" | cut -d' ' -f1)

echo ""
echo -e "${GREEN}╔════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║         Packaging Complete             ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════╝${NC}"
echo ""
echo "Version: $VERSION"
echo ""
echo "Artifacts:"
echo "  Full package:  $ZIP_PATH"
echo "  XCFramework:   $XCFRAMEWORK_ZIP"
echo ""
echo "XCFramework checksum (SHA-256):"
echo "  $CHECKSUM"
echo ""
echo "Save this checksum for Package.swift binaryTarget!"

# Write checksum to file for CI
echo "$CHECKSUM" > "$DIST_DIR/VauchiPlatformFFI.xcframework.zip.sha256"

# Sign checksum with cosign (T1-5: required in CI, optional locally)
# GitLab masked file variables store base64-encoded content — decode if needed.
if [[ -n "${COSIGN_KEY:-}" ]]; then
    # GitLab file-type variable: env holds a path to a staged file. The
    # nell runner has been observed to drop staging intermittently — env
    # is set but file is missing. Retry to absorb the staging race; if
    # still missing, fail fast with an actionable message instead of
    # cascading head/base64 errors.
    #
    # Empirical (2026-05-28, pipelines 2553603199 + 2558069282): the
    # earlier 3×2s = 4s budget consistently lost the race on both
    # consecutive release tag runs (manual retries succeeded ~8 min
    # later, suggesting the staging window is wider than 4s). Bumped to
    # 10×3s = up to 27s to cover it. Job-level `retry: {when:
    # [script_failure]}` in `ci/package.yml` is the belt-and-braces.
    for attempt in 1 2 3 4 5 6 7 8 9 10; do
        [[ -f "$COSIGN_KEY" ]] && break
        if [[ "$attempt" -lt 10 ]]; then
            echo -e "${YELLOW}COSIGN_KEY path '$COSIGN_KEY' not present (attempt $attempt/10) — retrying in 3s${NC}" >&2
            sleep 3
        fi
    done
    if [[ ! -f "$COSIGN_KEY" ]]; then
        echo -e "${RED}ERROR: COSIGN_KEY env is set to '$COSIGN_KEY' but file does not exist after 10 attempts.${NC}" >&2
        echo -e "${RED}This is a GitLab Runner file-variable staging failure (runner-side issue).${NC}" >&2
        echo -e "${RED}Retry the job; if it persists, check runner config on the host.${NC}" >&2
        exit 1
    fi
    if [[ ! -s "$COSIGN_KEY" ]]; then
        echo -e "${RED}ERROR: COSIGN_KEY file '$COSIGN_KEY' is empty — staging produced a zero-byte file.${NC}" >&2
        exit 1
    fi
    if ! command -v cosign >/dev/null 2>&1; then
        echo -e "${RED}ERROR: cosign not found on PATH — install via .cosign-install template${NC}" >&2
        exit 1
    fi
    COSIGN_KEY_FILE="$COSIGN_KEY"
    DECODED_KEY_FILE=""
    # Plaintext key files start with "-----BEGIN". Anything else is treated
    # as base64-encoded (GitLab's masked file-variable encoding).
    if ! head -1 "$COSIGN_KEY" | grep -q -- "-----BEGIN"; then
        DECODED_KEY_FILE=$(mktemp)
        # Cleanup trap: secret material must not linger on disk if cosign fails.
        trap 'rm -f "$DECODED_KEY_FILE"' EXIT INT TERM
        if ! base64 -d < "$COSIGN_KEY" > "$DECODED_KEY_FILE" 2>/dev/null; then
            echo -e "${RED}ERROR: failed to base64-decode COSIGN_KEY at '$COSIGN_KEY'${NC}" >&2
            exit 1
        fi
        if ! head -1 "$DECODED_KEY_FILE" | grep -q -- "-----BEGIN"; then
            echo -e "${RED}ERROR: decoded COSIGN_KEY is not a PEM-formatted key (no -----BEGIN header)${NC}" >&2
            exit 1
        fi
        COSIGN_KEY_FILE="$DECODED_KEY_FILE"
    fi
    echo -e "${YELLOW}Signing checksum with cosign...${NC}"
    cosign sign-blob --yes --key "$COSIGN_KEY_FILE" \
        --bundle "$DIST_DIR/VauchiPlatformFFI.xcframework.zip.sha256.bundle" \
        "$DIST_DIR/VauchiPlatformFFI.xcframework.zip.sha256"
    if [[ -n "$DECODED_KEY_FILE" ]]; then
        rm -f "$DECODED_KEY_FILE"
        trap - EXIT INT TERM
    fi
    echo -e "${GREEN}Checksum signed${NC}"
elif [[ -n "${CI:-}" ]] && [[ "$VERSION" != dev-* ]]; then
    echo -e "${RED}ERROR: COSIGN_KEY is required in CI for release signing${NC}"
    exit 1
else
    echo -e "${YELLOW}COSIGN_KEY not set — skipping checksum signing (local/dev build)${NC}"
fi

# Ensure all dist artifacts are world-readable — shell runners may have restrictive
# umask (0077), producing 0600 files that downstream Docker jobs can't read.
chmod 644 "$DIST_DIR"/*
