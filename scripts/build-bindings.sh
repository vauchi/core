#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
# Build UniFFI bindings for iOS and Android
#
# This script:
# 1. Builds vauchi-mobile for iOS targets (ARM64, x86_64 simulator)
# 2. Builds vauchi-mobile for Android targets (ARM64, x86_64)
# 3. Generates Swift bindings for iOS
# 4. Generates Kotlin bindings for Android
# 5. Copies artifacts to platform directories

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
WORKSPACE_ROOT="$(dirname "$PROJECT_ROOT")"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Primary output: always within core/ (works in CI and locally)
BINDINGS_DIR="$PROJECT_ROOT/target/bindings"
IOS_GENERATED_DIR="$BINDINGS_DIR/ios/generated"
IOS_LIBS_DIR="$BINDINGS_DIR/ios/libs"
ANDROID_JNI_DIR="$BINDINGS_DIR/android/jniLibs"
ANDROID_KOTLIN_DIR="$BINDINGS_DIR/android/kotlin"

# Optional local install directories (sibling repos for local dev)
LOCAL_IOS_DIR="$WORKSPACE_ROOT/ios"
LOCAL_ANDROID_DIR="$WORKSPACE_ROOT/android"

# NDK paths (for Android)
NDK_HOME="${ANDROID_NDK_HOME:-$HOME/Library/Android/sdk/ndk/26.1.10909125}"

echo -e "${YELLOW}╔════════════════════════════════════════╗${NC}"
echo -e "${YELLOW}║     Vauchi UniFFI Bindings Build       ║${NC}"
echo -e "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""
echo "Project root: $PROJECT_ROOT"
echo "Bindings output: $BINDINGS_DIR"

cd "$PROJECT_ROOT"

# Parse arguments
BUILD_IOS=false
BUILD_ANDROID=false
BUILD_ALL=true

while [[ $# -gt 0 ]]; do
    case $1 in
        --ios)
            BUILD_IOS=true
            BUILD_ALL=false
            shift
            ;;
        --android)
            BUILD_ANDROID=true
            BUILD_ALL=false
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [--ios] [--android]"
            echo ""
            echo "Options:"
            echo "  --ios      Build iOS bindings only"
            echo "  --android  Build Android bindings only"
            echo "  (no args)  Build both platforms"
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            exit 1
            ;;
    esac
done

if $BUILD_ALL; then
    BUILD_IOS=true
    BUILD_ANDROID=true
fi

# === iOS Build ===
if $BUILD_IOS; then
    echo ""
    echo -e "${BLUE}=== Building iOS Bindings ===${NC}"

    if [[ "$(uname)" != "Darwin" ]]; then
        echo -e "${YELLOW}SKIPPED: iOS build requires macOS${NC}"
    else
        # Disable sccache for iOS cross-compilation (causes issues with target discovery)
        unset RUSTC_WRAPPER

        # Show toolchain info for debugging
        echo "Active Rust toolchain:"
        rustup show active-toolchain
        echo ""

        # Check for required iOS targets
        echo "Installing iOS targets..."
        rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios

        # Verify targets are installed
        echo "Installed iOS targets:"
        rustup target list --installed | grep -E "ios|iOS" || echo "WARNING: No iOS targets found!"
        echo ""

        # Build for iOS device (ARM64)
        echo -e "${YELLOW}Building for aarch64-apple-ios (device)...${NC}"
        cargo build -p vauchi-mobile --target aarch64-apple-ios --release
        echo -e "${GREEN}iOS device build complete${NC}"

        # Build for iOS simulator (ARM64 - Apple Silicon)
        echo -e "${YELLOW}Building for aarch64-apple-ios-sim (simulator ARM64)...${NC}"
        cargo build -p vauchi-mobile --target aarch64-apple-ios-sim --release
        echo -e "${GREEN}iOS simulator ARM64 build complete${NC}"

        # Build for iOS simulator (x86_64 - Intel)
        echo -e "${YELLOW}Building for x86_64-apple-ios (simulator x86_64)...${NC}"
        cargo build -p vauchi-mobile --target x86_64-apple-ios --release
        echo -e "${GREEN}iOS simulator x86_64 build complete${NC}"

        # Generate Swift bindings
        echo -e "${YELLOW}Generating Swift bindings...${NC}"
        mkdir -p "$IOS_GENERATED_DIR"

        cargo run -p vauchi-mobile --bin uniffi-bindgen -- generate \
            --library target/aarch64-apple-ios/release/libvauchi_mobile.a \
            --language swift \
            --out-dir "$IOS_GENERATED_DIR"

        echo -e "${GREEN}Swift bindings generated at: $IOS_GENERATED_DIR${NC}"

        # Create universal library for simulators
        echo -e "${YELLOW}Creating universal simulator library...${NC}"
        mkdir -p "$IOS_LIBS_DIR"

        lipo -create \
            target/aarch64-apple-ios-sim/release/libvauchi_mobile.a \
            target/x86_64-apple-ios/release/libvauchi_mobile.a \
            -output "$IOS_LIBS_DIR/libvauchi_mobile_sim.a"

        # Copy device library
        cp target/aarch64-apple-ios/release/libvauchi_mobile.a "$IOS_LIBS_DIR/libvauchi_mobile_device.a"

        echo -e "${GREEN}iOS libraries:${NC}"
        ls -lh "$IOS_LIBS_DIR/"
    fi
fi

# === Android Build ===
if $BUILD_ANDROID; then
    echo ""
    echo -e "${BLUE}=== Building Android Bindings ===${NC}"

    # Find NDK
    if [[ ! -d "$NDK_HOME" ]]; then
        # Try common locations
        for ndk_path in \
            "$HOME/Library/Android/sdk/ndk"/* \
            "$HOME/Android/Sdk/ndk"/* \
            "/opt/android-ndk" \
            ; do
            if [[ -d "$ndk_path" ]]; then
                NDK_HOME="$ndk_path"
                break
            fi
        done
    fi

    if [[ ! -d "$NDK_HOME" ]]; then
        echo -e "${RED}Error: Android NDK not found${NC}"
        echo "Set ANDROID_NDK_HOME environment variable or install NDK via Android Studio"
        exit 1
    fi

    echo "Using NDK: $NDK_HOME"

    # Determine NDK toolchain path based on OS
    if [[ "$(uname)" == "Darwin" ]]; then
        NDK_TOOLCHAIN="$NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin"
    else
        NDK_TOOLCHAIN="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
    fi

    if [[ ! -d "$NDK_TOOLCHAIN" ]]; then
        echo -e "${RED}Error: NDK toolchain not found at $NDK_TOOLCHAIN${NC}"
        exit 1
    fi

    # Check for required Android targets
    if ! rustup target list --installed | grep -q "aarch64-linux-android"; then
        echo "Installing Android targets..."
        rustup target add aarch64-linux-android
        rustup target add x86_64-linux-android
        rustup target add armv7-linux-androideabi
    fi

    # Build for ARM64 (real devices)
    echo -e "${YELLOW}Building for aarch64-linux-android (ARM64)...${NC}"
    export CC_aarch64_linux_android="$NDK_TOOLCHAIN/aarch64-linux-android24-clang"
    export AR_aarch64_linux_android="$NDK_TOOLCHAIN/llvm-ar"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_TOOLCHAIN/aarch64-linux-android24-clang"
    cargo build -p vauchi-mobile --target aarch64-linux-android --release
    echo -e "${GREEN}ARM64 build complete${NC}"

    # Build for x86_64 (emulator)
    echo -e "${YELLOW}Building for x86_64-linux-android (emulator)...${NC}"
    export CC_x86_64_linux_android="$NDK_TOOLCHAIN/x86_64-linux-android24-clang"
    export AR_x86_64_linux_android="$NDK_TOOLCHAIN/llvm-ar"
    export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$NDK_TOOLCHAIN/x86_64-linux-android24-clang"
    cargo build -p vauchi-mobile --target x86_64-linux-android --release
    echo -e "${GREEN}x86_64 build complete${NC}"

    # Copy native libraries
    echo -e "${YELLOW}Copying native libraries...${NC}"
    mkdir -p "$ANDROID_JNI_DIR/arm64-v8a"
    mkdir -p "$ANDROID_JNI_DIR/x86_64"
    cp target/aarch64-linux-android/release/libvauchi_mobile.so "$ANDROID_JNI_DIR/arm64-v8a/"
    cp target/x86_64-linux-android/release/libvauchi_mobile.so "$ANDROID_JNI_DIR/x86_64/"

    # Generate Kotlin bindings
    # Note: uniffi-bindgen can't read metadata from cross-compiled libraries,
    # so we build a native library first and use that for binding generation.
    # We use --library mode to extract types from proc macros, matching iOS approach.
    # IMPORTANT: Build without symbol stripping to preserve UniFFI metadata!
    echo -e "${YELLOW}Generating Kotlin bindings...${NC}"
    mkdir -p "$ANDROID_KOTLIN_DIR"

    # Build native library for binding generation (without stripping to preserve metadata)
    echo -e "${YELLOW}Building native library for metadata extraction...${NC}"
    RUSTFLAGS="-Cstrip=none" cargo build -p vauchi-mobile --release

    cargo run -p vauchi-mobile --bin uniffi-bindgen --release -- generate \
        --library target/release/libvauchi_mobile.so \
        --language kotlin \
        --out-dir "$ANDROID_KOTLIN_DIR"

    echo -e "${GREEN}Kotlin bindings generated at: $ANDROID_KOTLIN_DIR${NC}"

    echo -e "${GREEN}Android libraries:${NC}"
    ls -lh "$ANDROID_JNI_DIR"/*/libvauchi_mobile.so
fi

# === Summary ===
echo ""
echo -e "${YELLOW}╔════════════════════════════════════════╗${NC}"
echo -e "${YELLOW}║           Build Complete               ║${NC}"
echo -e "${YELLOW}╚════════════════════════════════════════╝${NC}"
echo ""

if $BUILD_IOS && [[ "$(uname)" == "Darwin" ]]; then
    echo -e "${GREEN}iOS:${NC}"
    echo "  Swift bindings: $IOS_GENERATED_DIR/"
    echo "  Libraries:      $IOS_LIBS_DIR/"
fi

if $BUILD_ANDROID; then
    echo -e "${GREEN}Android:${NC}"
    echo "  Kotlin bindings: $ANDROID_KOTLIN_DIR/"
    echo "  JNI libraries:   $ANDROID_JNI_DIR/"
fi

# === Local Install (copy to sibling repos for local development) ===
if [[ -z "${CI:-}" ]]; then
    echo ""
    echo -e "${BLUE}=== Local Install ===${NC}"

    if $BUILD_IOS && [[ -d "$LOCAL_IOS_DIR" ]]; then
        echo -e "${YELLOW}Copying iOS bindings to $LOCAL_IOS_DIR/...${NC}"
        mkdir -p "$LOCAL_IOS_DIR/Vauchi/Generated"
        mkdir -p "$LOCAL_IOS_DIR/Vauchi/Libs"
        cp -r "$IOS_GENERATED_DIR/"* "$LOCAL_IOS_DIR/Vauchi/Generated/" 2>/dev/null || true
        cp -r "$IOS_LIBS_DIR/"* "$LOCAL_IOS_DIR/Vauchi/Libs/" 2>/dev/null || true
        echo -e "${GREEN}  Installed to $LOCAL_IOS_DIR/Vauchi/${NC}"
    fi

    if $BUILD_ANDROID && [[ -d "$LOCAL_ANDROID_DIR" ]]; then
        echo -e "${YELLOW}Copying Android bindings to $LOCAL_ANDROID_DIR/...${NC}"
        mkdir -p "$LOCAL_ANDROID_DIR/app/src/main/jniLibs"
        mkdir -p "$LOCAL_ANDROID_DIR/app/src/main/kotlin"
        cp -r "$ANDROID_JNI_DIR/"* "$LOCAL_ANDROID_DIR/app/src/main/jniLibs/" 2>/dev/null || true
        cp -r "$ANDROID_KOTLIN_DIR/"* "$LOCAL_ANDROID_DIR/app/src/main/kotlin/" 2>/dev/null || true
        echo -e "${GREEN}  Installed to $LOCAL_ANDROID_DIR/app/src/main/${NC}"
    fi
fi
