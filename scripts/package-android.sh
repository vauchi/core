#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
# Package Android bindings into a distributable archive
#
# This script:
# 1. Bundles JNI native libraries (.so files)
# 2. Includes Kotlin bindings
# 3. Creates a distributable zip archive
#
# Prerequisites:
#   - Run build-bindings.sh --android first
#
# Usage:
#   ./package-android.sh [version]
#
# Output:
#   dist/vauchi-platform-kotlin-{version}.zip

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
ANDROID_JNI_DIR="$BINDINGS_DIR/android/jniLibs"
ANDROID_KOTLIN_DIR="$BINDINGS_DIR/android/kotlin"
DIST_DIR="$PROJECT_ROOT/dist"
BUILD_DIR="$PROJECT_ROOT/target/android-build"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${YELLOW}╔════════════════════════════════════════╗${NC}"
echo -e "${YELLOW}║     Package Android Bindings v$VERSION    ${NC}"
echo -e "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""

# Check prerequisites
if [[ ! -d "$ANDROID_JNI_DIR/arm64-v8a" ]]; then
    echo -e "${RED}Error: Android JNI libraries not found. Run build-bindings.sh --android first${NC}"
    exit 1
fi

if [[ ! -d "$ANDROID_KOTLIN_DIR/uniffi" ]]; then
    echo -e "${RED}Error: Kotlin bindings not found. Run build-bindings.sh --android first${NC}"
    exit 1
fi

# Clean and create directories
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"
mkdir -p "$DIST_DIR"

# Create package structure
echo -e "${YELLOW}Creating package structure...${NC}"
PACKAGE_DIR="$BUILD_DIR/vauchi-platform-kotlin-$VERSION"
mkdir -p "$PACKAGE_DIR/jniLibs"
mkdir -p "$PACKAGE_DIR/kotlin"

# Copy JNI libraries
echo -e "${YELLOW}Copying JNI libraries...${NC}"
for arch_dir in "$ANDROID_JNI_DIR"/*/; do
    arch=$(basename "$arch_dir")
    if [[ -f "$arch_dir/libvauchi_platform.so" ]]; then
        mkdir -p "$PACKAGE_DIR/jniLibs/$arch"
        cp "$arch_dir/libvauchi_platform.so" "$PACKAGE_DIR/jniLibs/$arch/"
        echo "  - $arch: $(ls -lh "$arch_dir/libvauchi_platform.so" | awk '{print $5}')"
    fi
done

# Copy Kotlin bindings
echo -e "${YELLOW}Copying Kotlin bindings...${NC}"
cp -r "$ANDROID_KOTLIN_DIR/uniffi" "$PACKAGE_DIR/kotlin/"

# Count lines in Kotlin bindings
KOTLIN_LINES=$(wc -l < "$PACKAGE_DIR/kotlin/uniffi/vauchi_platform/vauchi_platform.kt" 2>/dev/null || echo "0")
echo "  - vauchi_platform.kt: $KOTLIN_LINES lines"

# Create README
cat > "$PACKAGE_DIR/README.md" << EOF
# VauchiPlatform Android v$VERSION

UniFFI bindings for Vauchi Android apps.

## Contents

- \`jniLibs/\` - Native libraries per ABI
  - \`arm64-v8a/\` - ARM64 (most modern devices)
  - \`x86_64/\` - x86_64 (emulators)
- \`kotlin/uniffi/vauchi_platform/\` - Kotlin bindings

## Integration

### Gradle (from Maven repository)

\`\`\`kotlin
// settings.gradle.kts
dependencyResolutionManagement {
    repositories {
        maven {
            url = uri("https://gitlab.com/api/v4/projects/vauchi%2Fvauchi-platform-kotlin/packages/maven")
        }
    }
}

// app/build.gradle.kts
dependencies {
    implementation("com.vauchi:vauchi-platform:$VERSION")
}
\`\`\`

### Manual Integration

1. Copy \`jniLibs/\` contents to \`app/src/main/jniLibs/\`
2. Copy \`kotlin/uniffi/\` to \`app/src/main/kotlin/uniffi/\`
3. Add to build.gradle.kts:
   \`\`\`kotlin
   android {
       sourceSets {
           getByName("main") {
               jniLibs.srcDir("src/main/jniLibs")
           }
       }
   }
   \`\`\`
4. Import and use: \`import uniffi.vauchi_platform.*\`

## ABI Support

| ABI | Description | Support |
|-----|-------------|---------|
| arm64-v8a | 64-bit ARM | Primary |
| x86_64 | 64-bit x86 | Emulator |

## License

MIT License - see https://gitlab.com/vauchi/core
EOF

# Create build info
cat > "$PACKAGE_DIR/BUILD_INFO" << EOF
version=$VERSION
build_date=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
rust_version=$(rustc --version | cut -d' ' -f2)
EOF

# Create zip archive
echo -e "${YELLOW}Creating zip archive...${NC}"
ZIP_PATH="$DIST_DIR/vauchi-platform-kotlin-$VERSION.zip"
cd "$BUILD_DIR"
zip -r "$ZIP_PATH" "vauchi-platform-kotlin-$VERSION"

# Calculate checksum (cross-platform: shasum on macOS, sha256sum on Linux)
if command -v sha256sum >/dev/null 2>&1; then
    CHECKSUM=$(sha256sum "$ZIP_PATH" | cut -d' ' -f1)
else
    CHECKSUM=$(shasum -a 256 "$ZIP_PATH" | cut -d' ' -f1)
fi

echo ""
echo -e "${GREEN}╔════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║         Packaging Complete             ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════╝${NC}"
echo ""
echo "Version: $VERSION"
echo ""
echo "Artifact: $ZIP_PATH"
echo "Size: $(ls -lh "$ZIP_PATH" | awk '{print $5}')"
echo ""
echo "Checksum (SHA-256):"
echo "  $CHECKSUM"

# Write checksum to file for CI
echo "$CHECKSUM" > "$DIST_DIR/vauchi-platform-kotlin-$VERSION.zip.sha256"

# Sign checksum with cosign (T1-5: required in CI, optional locally)
# GitLab masked file variables store base64-encoded content — decode if needed.
if [[ -n "${COSIGN_KEY:-}" ]]; then
    COSIGN_KEY_FILE="$COSIGN_KEY"
    if ! head -1 "$COSIGN_KEY" | grep -q -- "-----BEGIN"; then
        COSIGN_KEY_FILE=$(mktemp)
        base64 -d "$COSIGN_KEY" > "$COSIGN_KEY_FILE"
    fi
    echo -e "${YELLOW}Signing checksum with cosign...${NC}"
    cosign sign-blob --yes --key "$COSIGN_KEY_FILE" \
        --bundle "$DIST_DIR/vauchi-platform-kotlin-$VERSION.zip.sha256.bundle" \
        "$DIST_DIR/vauchi-platform-kotlin-$VERSION.zip.sha256"
    [[ "$COSIGN_KEY_FILE" != "$COSIGN_KEY" ]] && rm -f "$COSIGN_KEY_FILE"
    echo -e "${GREEN}Checksum signed${NC}"
elif [[ -n "${CI:-}" ]]; then
    echo -e "${RED}ERROR: COSIGN_KEY is required in CI for release signing${NC}"
    exit 1
else
    echo -e "${YELLOW}COSIGN_KEY not set — skipping checksum signing (local build)${NC}"
fi
