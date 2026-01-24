#!/bin/bash
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
#   dist/VauchiMobileFFI.xcframework.zip

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
WORKSPACE_ROOT="$(dirname "$PROJECT_ROOT")"

# Version from argument or Cargo.toml
VERSION="${1:-$(grep -m1 'version = ' "$PROJECT_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')}"

# Paths
IOS_DIR="$WORKSPACE_ROOT/ios"
IOS_LIBS_DIR="$IOS_DIR/Vauchi/Libs"
IOS_GENERATED_DIR="$IOS_DIR/Vauchi/Generated"
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

if [[ ! -f "$IOS_LIBS_DIR/libvauchi_mobile_device.a" ]]; then
    echo -e "${RED}Error: iOS libraries not found. Run build-bindings.sh --ios first${NC}"
    exit 1
fi

if [[ ! -f "$IOS_GENERATED_DIR/vauchi_mobile.swift" ]]; then
    echo -e "${RED}Error: Swift bindings not found. Run build-bindings.sh --ios first${NC}"
    exit 1
fi

# Clean and create directories
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"
mkdir -p "$DIST_DIR"

# Create module map for the FFI layer
echo -e "${YELLOW}Creating module map...${NC}"
HEADERS_DIR="$BUILD_DIR/Headers"
mkdir -p "$HEADERS_DIR"

# Copy the generated C header (UniFFI generates this)
if [[ -f "$IOS_GENERATED_DIR/vauchi_mobileFFI.h" ]]; then
    cp "$IOS_GENERATED_DIR/vauchi_mobileFFI.h" "$HEADERS_DIR/"
else
    # Generate a minimal header if not present
    cat > "$HEADERS_DIR/vauchi_mobileFFI.h" << 'EOF'
// VauchiMobileFFI - UniFFI generated C bindings
// This header is auto-generated. Do not edit.

#ifndef VAUCHI_MOBILE_FFI_H
#define VAUCHI_MOBILE_FFI_H

#include <stdint.h>
#include <stdbool.h>

// UniFFI scaffolding types are defined in the Swift bindings
// This header exists for XCFramework module map requirements

#endif // VAUCHI_MOBILE_FFI_H
EOF
fi

# Create module map
cat > "$HEADERS_DIR/module.modulemap" << 'EOF'
framework module VauchiMobileFFI {
    umbrella header "vauchi_mobileFFI.h"
    export *
    module * { export * }
    link "vauchi_mobile"
}
EOF

# Create XCFramework structure for device
echo -e "${YELLOW}Preparing device slice...${NC}"
DEVICE_DIR="$BUILD_DIR/ios-arm64"
mkdir -p "$DEVICE_DIR/VauchiMobileFFI.framework"
cp "$IOS_LIBS_DIR/libvauchi_mobile_device.a" "$DEVICE_DIR/VauchiMobileFFI.framework/VauchiMobileFFI"
cp -r "$HEADERS_DIR" "$DEVICE_DIR/VauchiMobileFFI.framework/Headers"
mkdir -p "$DEVICE_DIR/VauchiMobileFFI.framework/Modules"
cp "$HEADERS_DIR/module.modulemap" "$DEVICE_DIR/VauchiMobileFFI.framework/Modules/"

# Create Info.plist for device framework
cat > "$DEVICE_DIR/VauchiMobileFFI.framework/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.vauchi.VauchiMobileFFI</string>
    <key>CFBundleName</key>
    <string>VauchiMobileFFI</string>
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
mkdir -p "$SIM_DIR/VauchiMobileFFI.framework"
cp "$IOS_LIBS_DIR/libvauchi_mobile_sim.a" "$SIM_DIR/VauchiMobileFFI.framework/VauchiMobileFFI"
cp -r "$HEADERS_DIR" "$SIM_DIR/VauchiMobileFFI.framework/Headers"
mkdir -p "$SIM_DIR/VauchiMobileFFI.framework/Modules"
cp "$HEADERS_DIR/module.modulemap" "$SIM_DIR/VauchiMobileFFI.framework/Modules/"
cp "$DEVICE_DIR/VauchiMobileFFI.framework/Info.plist" "$SIM_DIR/VauchiMobileFFI.framework/Info.plist"

# Create XCFramework
echo -e "${YELLOW}Creating XCFramework...${NC}"
XCFRAMEWORK_PATH="$BUILD_DIR/VauchiMobileFFI.xcframework"

xcodebuild -create-xcframework \
    -framework "$DEVICE_DIR/VauchiMobileFFI.framework" \
    -framework "$SIM_DIR/VauchiMobileFFI.framework" \
    -output "$XCFRAMEWORK_PATH"

echo -e "${GREEN}XCFramework created at: $XCFRAMEWORK_PATH${NC}"

# Create distribution package
echo -e "${YELLOW}Creating distribution package...${NC}"
PACKAGE_DIR="$BUILD_DIR/VauchiMobile-$VERSION"
mkdir -p "$PACKAGE_DIR"

# Copy XCFramework
cp -r "$XCFRAMEWORK_PATH" "$PACKAGE_DIR/"

# Copy Swift bindings
mkdir -p "$PACKAGE_DIR/Sources"
cp "$IOS_GENERATED_DIR/vauchi_mobile.swift" "$PACKAGE_DIR/Sources/"

# Create README
cat > "$PACKAGE_DIR/README.md" << EOF
# VauchiMobile v$VERSION

UniFFI bindings for Vauchi iOS apps.

## Contents

- \`VauchiMobileFFI.xcframework/\` - Native library (device + simulator)
- \`Sources/vauchi_mobile.swift\` - Swift bindings

## Integration

### Swift Package Manager (Binary Target)

\`\`\`swift
.binaryTarget(
    name: "VauchiMobileFFI",
    url: "https://gitlab.com/api/v4/projects/vauchi%2Fcore/packages/generic/vauchi-mobile/$VERSION/VauchiMobileFFI.xcframework.zip",
    checksum: "CHECKSUM_HERE"
)
\`\`\`

### Manual Integration

1. Drag \`VauchiMobileFFI.xcframework\` into your Xcode project
2. Add \`Sources/vauchi_mobile.swift\` to your target
3. Import and use: \`import VauchiMobile\`

## License

MIT License - see https://gitlab.com/vauchi/core
EOF

# Create zip archive
ZIP_PATH="$DIST_DIR/VauchiMobile-$VERSION.zip"
cd "$BUILD_DIR"
zip -r "$ZIP_PATH" "VauchiMobile-$VERSION"

# Also create framework-only zip for SPM binary target
XCFRAMEWORK_ZIP="$DIST_DIR/VauchiMobileFFI.xcframework.zip"
cd "$BUILD_DIR"
zip -r "$XCFRAMEWORK_ZIP" "VauchiMobileFFI.xcframework"

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
echo "$CHECKSUM" > "$DIST_DIR/VauchiMobileFFI.xcframework.zip.sha256"
