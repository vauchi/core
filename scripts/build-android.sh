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

# Parse arguments
BUILD_TYPE="${1:-release}"
if [[ "$BUILD_TYPE" == "debug" ]]; then
    CARGO_FLAG=""
    BUILD_DIR="debug"
    echo -e "${YELLOW}Building DEBUG version${NC}"
else
    CARGO_FLAG="--release"
    BUILD_DIR="release"
    echo -e "${YELLOW}Building RELEASE version (size-optimized)${NC}"
fi

# Build for ARM64 (modern devices - 90%+ of market)
echo -e "${YELLOW}Building for aarch64-linux-android (ARM64)...${NC}"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_TOOLCHAIN/aarch64-linux-android26-clang"
export CC_aarch64_linux_android="$NDK_TOOLCHAIN/aarch64-linux-android26-clang"
export AR_aarch64_linux_android="$NDK_TOOLCHAIN/llvm-ar"
# 16 KB page alignment required for Android 15+ (API 35)
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-z -C link-arg=max-page-size=16384"
cargo build -p vauchi-mobile --target aarch64-linux-android $CARGO_FLAG
echo -e "${GREEN}ARM64 build complete${NC}"

# Build for ARMv7 (older 32-bit devices - ~10% of market)
echo -e "${YELLOW}Building for armv7-linux-androideabi (ARM32)...${NC}"
export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="$NDK_TOOLCHAIN/armv7a-linux-androideabi26-clang"
export CC_armv7_linux_androideabi="$NDK_TOOLCHAIN/armv7a-linux-androideabi26-clang"
export AR_armv7_linux_androideabi="$NDK_TOOLCHAIN/llvm-ar"
cargo build -p vauchi-mobile --target armv7-linux-androideabi $CARGO_FLAG
echo -e "${GREEN}ARM32 build complete${NC}"

# Copy native libraries (production ABIs only: arm64-v8a, armeabi-v7a)
echo -e "${YELLOW}Copying native libraries...${NC}"
mkdir -p "$JNI_LIBS_DIR/arm64-v8a"
mkdir -p "$JNI_LIBS_DIR/armeabi-v7a"
cp target/aarch64-linux-android/$BUILD_DIR/libvauchi_mobile.so "$JNI_LIBS_DIR/arm64-v8a/"
cp target/armv7-linux-androideabi/$BUILD_DIR/libvauchi_mobile.so "$JNI_LIBS_DIR/armeabi-v7a/"
echo -e "${GREEN}Libraries copied${NC}"

# Generate Kotlin bindings
echo -e "${YELLOW}Generating Kotlin bindings...${NC}"
cargo run -p vauchi-mobile --bin uniffi-bindgen $CARGO_FLAG -- generate \
    --library target/aarch64-linux-android/$BUILD_DIR/libvauchi_mobile.so \
    --language kotlin \
    --out-dir "$KOTLIN_OUT_DIR"
echo -e "${GREEN}Kotlin bindings generated${NC}"

echo ""
echo -e "${GREEN}=== Build Complete ===${NC}"
echo "Build type:       $BUILD_TYPE"
echo "Native libraries: $JNI_LIBS_DIR/"
echo "Kotlin bindings:  $KOTLIN_OUT_DIR/uniffi/vauchi_mobile/"
echo ""
echo "Library sizes:"
ls -lh "$JNI_LIBS_DIR"/*/libvauchi_mobile.so
echo ""
echo "Usage:"
echo "  $0          # Release build (size-optimized)"
echo "  $0 release  # Release build (size-optimized)"
echo "  $0 debug    # Debug build (fast compile, symbols)"
