#!/bin/bash
# Build script for Vauchi Android native libraries
#
# This script:
# 1. Builds vauchi-mobile for Android targets (ARM64, x86_64)
# 2. Generates Kotlin bindings
# 3. Copies native libraries to the Android project

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Colors for output (defined early for error messages)
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# NDK paths - auto-detect from Android Studio SDK or environment
# Check for toolchain existence, not just directory
find_ndk() {
    local ndk_path="$1"
    if [ -d "$ndk_path/toolchains/llvm/prebuilt/linux-x86_64/bin" ]; then
        echo "$ndk_path"
        return 0
    fi
    return 1
}

NDK_HOME=""
if [ -n "$ANDROID_NDK_HOME" ] && find_ndk "$ANDROID_NDK_HOME" >/dev/null; then
    NDK_HOME="$ANDROID_NDK_HOME"
elif [ -d "$HOME/Android/Sdk/ndk" ]; then
    # Find latest NDK version in Android Studio SDK
    for ndk in $(ls -d "$HOME/Android/Sdk/ndk/"* 2>/dev/null | sort -V -r); do
        if find_ndk "$ndk" >/dev/null; then
            NDK_HOME="$ndk"
            break
        fi
    done
fi

if [ -z "$NDK_HOME" ]; then
    echo -e "${RED}Error: Android NDK not found${NC}"
    echo "Install NDK via Android Studio SDK Manager or set ANDROID_NDK_HOME"
    exit 1
fi
NDK_TOOLCHAIN="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"

# Target directories (android/ is at workspace root, not inside code/)
WORKSPACE_ROOT="$(dirname "$PROJECT_ROOT")"
JNI_LIBS_DIR="$WORKSPACE_ROOT/android/app/src/main/jniLibs"
KOTLIN_OUT_DIR="$WORKSPACE_ROOT/android/app/src/main/kotlin"

echo -e "${YELLOW}=== Vauchi Android Build ===${NC}"
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
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_TOOLCHAIN/aarch64-linux-android24-clang"
export CC_aarch64_linux_android="$NDK_TOOLCHAIN/aarch64-linux-android24-clang"
export AR_aarch64_linux_android="$NDK_TOOLCHAIN/llvm-ar"
cargo build -p vauchi-mobile --target aarch64-linux-android --release
echo -e "${GREEN}ARM64 build complete${NC}"

# Build for x86_64 (emulator)
echo -e "${YELLOW}Building for x86_64-linux-android (emulator)...${NC}"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$NDK_TOOLCHAIN/x86_64-linux-android24-clang"
export CC_x86_64_linux_android="$NDK_TOOLCHAIN/x86_64-linux-android24-clang"
export AR_x86_64_linux_android="$NDK_TOOLCHAIN/llvm-ar"
cargo build -p vauchi-mobile --target x86_64-linux-android --release
echo -e "${GREEN}x86_64 build complete${NC}"

# Copy native libraries
echo -e "${YELLOW}Copying native libraries...${NC}"
mkdir -p "$JNI_LIBS_DIR/arm64-v8a"
mkdir -p "$JNI_LIBS_DIR/x86_64"
cp target/aarch64-linux-android/release/libvauchi_mobile.so "$JNI_LIBS_DIR/arm64-v8a/"
cp target/x86_64-linux-android/release/libvauchi_mobile.so "$JNI_LIBS_DIR/x86_64/"
echo -e "${GREEN}Libraries copied${NC}"

# Generate Kotlin bindings
echo -e "${YELLOW}Generating Kotlin bindings...${NC}"
cargo run -p vauchi-mobile --bin uniffi-bindgen -- generate \
    --library target/aarch64-linux-android/release/libvauchi_mobile.so \
    --language kotlin \
    --out-dir "$KOTLIN_OUT_DIR"
echo -e "${GREEN}Kotlin bindings generated${NC}"

echo ""
echo -e "${GREEN}=== Build Complete ===${NC}"
echo "Native libraries: $JNI_LIBS_DIR/"
echo "Kotlin bindings:  $KOTLIN_OUT_DIR/uniffi/vauchi_mobile/"
echo ""
echo "Library sizes:"
ls -lh "$JNI_LIBS_DIR"/*/libvauchi_mobile.so
