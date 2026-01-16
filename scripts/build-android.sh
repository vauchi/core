#!/bin/bash
# Build script for WebBook Android native libraries
#
# This script:
# 1. Builds webbook-mobile for Android targets (ARM64, x86_64)
# 2. Generates Kotlin bindings
# 3. Copies native libraries to the Android project

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# NDK paths
NDK_HOME="${ANDROID_NDK_HOME:-/opt/android-ndk}"
NDK_TOOLCHAIN="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"

# Target directories
JNI_LIBS_DIR="$PROJECT_ROOT/webbook-android/app/src/main/jniLibs"
KOTLIN_OUT_DIR="$PROJECT_ROOT/webbook-android/app/src/main/kotlin"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}=== WebBook Android Build ===${NC}"
echo "NDK: $NDK_HOME"
echo "Project: $PROJECT_ROOT"
echo ""

# Check NDK exists
if [ ! -d "$NDK_TOOLCHAIN" ]; then
    echo -e "${RED}Error: NDK toolchain not found at $NDK_TOOLCHAIN${NC}"
    echo "Set ANDROID_NDK_HOME environment variable or install NDK at /opt/android-ndk"
    exit 1
fi

cd "$PROJECT_ROOT"

# Build for ARM64 (real devices)
echo -e "${YELLOW}Building for aarch64-linux-android (ARM64)...${NC}"
export CC_aarch64_linux_android="$NDK_TOOLCHAIN/aarch64-linux-android24-clang"
export AR_aarch64_linux_android="$NDK_TOOLCHAIN/llvm-ar"
cargo build -p webbook-mobile --target aarch64-linux-android --release
echo -e "${GREEN}ARM64 build complete${NC}"

# Build for x86_64 (emulator)
echo -e "${YELLOW}Building for x86_64-linux-android (emulator)...${NC}"
export CC_x86_64_linux_android="$NDK_TOOLCHAIN/x86_64-linux-android24-clang"
export AR_x86_64_linux_android="$NDK_TOOLCHAIN/llvm-ar"
cargo build -p webbook-mobile --target x86_64-linux-android --release
echo -e "${GREEN}x86_64 build complete${NC}"

# Copy native libraries
echo -e "${YELLOW}Copying native libraries...${NC}"
mkdir -p "$JNI_LIBS_DIR/arm64-v8a"
mkdir -p "$JNI_LIBS_DIR/x86_64"
cp target/aarch64-linux-android/release/libwebbook_mobile.so "$JNI_LIBS_DIR/arm64-v8a/"
cp target/x86_64-linux-android/release/libwebbook_mobile.so "$JNI_LIBS_DIR/x86_64/"
echo -e "${GREEN}Libraries copied${NC}"

# Generate Kotlin bindings
echo -e "${YELLOW}Generating Kotlin bindings...${NC}"
cargo run -p webbook-mobile --bin uniffi-bindgen -- generate \
    --library target/aarch64-linux-android/release/libwebbook_mobile.so \
    --language kotlin \
    --out-dir "$KOTLIN_OUT_DIR"
echo -e "${GREEN}Kotlin bindings generated${NC}"

echo ""
echo -e "${GREEN}=== Build Complete ===${NC}"
echo "Native libraries: $JNI_LIBS_DIR/"
echo "Kotlin bindings:  $KOTLIN_OUT_DIR/uniffi/webbook_mobile/"
echo ""
echo "Library sizes:"
ls -lh "$JNI_LIBS_DIR"/*/libwebbook_mobile.so
