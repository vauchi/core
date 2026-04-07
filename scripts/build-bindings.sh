#!/bin/bash
# SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
#
# SPDX-License-Identifier: GPL-3.0-or-later
# Build UniFFI bindings for iOS and Android
#
# This script:
# 1. Builds vauchi-platform for iOS targets (ARM64, x86_64 simulator)
# 2. Builds vauchi-platform for Android targets (ARM64, x86_64)
# 3. Generates Swift bindings for iOS
# 4. Generates Kotlin bindings for Android
# 5. Copies artifacts to platform directories

set -euo pipefail

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

# macOS output directory
MACOS_LIBS_DIR="$BINDINGS_DIR/macos/libs"

# Optional local install directories (sibling repos for local dev)
LOCAL_IOS_DIR="$WORKSPACE_ROOT/ios"
LOCAL_ANDROID_DIR="$WORKSPACE_ROOT/android"
LOCAL_MACOS_DIR="$WORKSPACE_ROOT/macos"

# NDK paths (for Android) — auto-detect latest installed NDK if ANDROID_NDK_HOME not set
# Note: must not fail under `set -euo pipefail` when NDK dirs don't exist
NDK_DEFAULT=""
for _ndk_search in "$HOME/Library/Android/sdk/ndk" /opt/android-sdk/ndk /opt/android-ndk "$HOME/Android/Sdk/ndk"; do
    if [ -d "$_ndk_search" ]; then
        NDK_DEFAULT=$(ls -d "$_ndk_search"/*/ 2>/dev/null | sort -V | tail -1 || true)
        NDK_DEFAULT="${NDK_DEFAULT%/}"
        [ -n "$NDK_DEFAULT" ] && break
    fi
done
NDK_HOME="${ANDROID_NDK_HOME:-${NDK_DEFAULT:-}}"

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
BUILD_MACOS=false
BUILD_ALL=true
RELEASE_ONLY=false

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
        --macos)
            BUILD_MACOS=true
            BUILD_ALL=false
            shift
            ;;
        --apple)
            BUILD_IOS=true
            BUILD_MACOS=true
            BUILD_ALL=false
            shift
            ;;
        --release-only)
            RELEASE_ONLY=true
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [--ios] [--android] [--macos] [--apple] [--release-only]"
            echo ""
            echo "Options:"
            echo "  --ios           Build iOS bindings only"
            echo "  --android       Build Android bindings only"
            echo "  --macos         Build macOS bindings only"
            echo "  --apple         Build iOS + macOS bindings"
            echo "  --release-only  Skip simulator/Intel targets (arm64 only)"
            echo "  (no args)       Build all platforms, all targets"
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
    BUILD_MACOS=true
fi

# === iOS Build ===
if $BUILD_IOS; then
    echo ""
    echo -e "${BLUE}=== Building iOS Bindings ===${NC}"

    if [[ "$(uname)" != "Darwin" ]]; then
        echo -e "${YELLOW}SKIPPED: iOS build requires macOS${NC}"
    else
        # sccache works with cross-compilation targets since sccache 0.8+.
        # Previously disabled due to target discovery issues — resolved in modern versions.
        # RUSTC_WRAPPER inherited from environment (set by CI .self-hosted template).

        # Set iOS deployment target to match Rust's default (10.0) to prevent
        # ___chkstk_darwin linker errors. Without this, the cc crate picks up
        # the Xcode SDK version (e.g. 26.2), causing clang to emit stack probe
        # calls that don't exist at the Rust-side deployment target.
        export IPHONEOS_DEPLOYMENT_TARGET="10.0"

        # Show toolchain info for debugging
        echo "Active Rust toolchain:"
        rustup show active-toolchain
        echo "RUSTC_WRAPPER: ${RUSTC_WRAPPER:-<unset>}"
        echo ""

        # Build targets based on --release-only flag
        IOS_TARGETS="aarch64-apple-ios"
        if ! $RELEASE_ONLY; then
            IOS_TARGETS="$IOS_TARGETS aarch64-apple-ios-sim x86_64-apple-ios"
        fi

        # Ensure targets are installed
        echo "Installing iOS targets: $IOS_TARGETS"
        for t in $IOS_TARGETS; do rustup target add "$t" 2>/dev/null || true; done

        # Multi-target build: compile dependencies ONCE, cross-compile per target.
        # Cargo 1.64+ shares dep compilation across --target flags in a single invocation.
        TARGET_FLAGS=""
        for t in $IOS_TARGETS; do TARGET_FLAGS="$TARGET_FLAGS --target $t"; done
        echo -e "${YELLOW}Building iOS targets: $IOS_TARGETS${NC}"
        cargo build -p vauchi-platform $TARGET_FLAGS --release
        echo -e "${GREEN}iOS build complete ($(echo $IOS_TARGETS | wc -w | tr -d ' ') targets)${NC}"

        # Generate Swift bindings
        echo -e "${YELLOW}Generating Swift bindings...${NC}"
        mkdir -p "$IOS_GENERATED_DIR"

        cargo run -p vauchi-platform --bin uniffi-bindgen -- generate \
            --library target/aarch64-apple-ios/release/libvauchi_platform.a \
            --language swift \
            --out-dir "$IOS_GENERATED_DIR"

        # Strip trailing whitespace from generated Swift (UniFFI emits `/* ` with trailing space)
        sed -i.bak 's/[[:space:]]*$//' "$IOS_GENERATED_DIR/vauchi_platform.swift"
        rm -f "$IOS_GENERATED_DIR/vauchi_platform.swift.bak"
        echo -e "${GREEN}Swift bindings generated at: $IOS_GENERATED_DIR${NC}"

        # Package libraries
        mkdir -p "$IOS_LIBS_DIR"
        cp target/aarch64-apple-ios/release/libvauchi_platform.a "$IOS_LIBS_DIR/libvauchi_platform_device.a"

        if ! $RELEASE_ONLY; then
            # Create universal library for simulators (ARM64 + x86_64)
            echo -e "${YELLOW}Creating universal simulator library...${NC}"
            lipo -create \
                target/aarch64-apple-ios-sim/release/libvauchi_platform.a \
                target/x86_64-apple-ios/release/libvauchi_platform.a \
                -output "$IOS_LIBS_DIR/libvauchi_platform_sim.a"
        fi

        echo -e "${GREEN}iOS libraries:${NC}"
        ls -lh "$IOS_LIBS_DIR/"
    fi
fi

# === macOS Build ===
if $BUILD_MACOS; then
    echo ""
    echo -e "${BLUE}=== Building macOS Bindings ===${NC}"

    if [[ "$(uname)" != "Darwin" ]]; then
        echo -e "${YELLOW}SKIPPED: macOS build requires macOS${NC}"
    else
        # sccache works with cross-compilation targets since sccache 0.8+.
        # RUSTC_WRAPPER inherited from environment.

        # Set deployment target to match project.yml
        export MACOSX_DEPLOYMENT_TARGET="14.0"

        # Build targets based on --release-only flag
        MACOS_TARGETS="aarch64-apple-darwin"
        if ! $RELEASE_ONLY; then
            MACOS_TARGETS="$MACOS_TARGETS x86_64-apple-darwin"
        fi

        # Ensure targets are installed
        for t in $MACOS_TARGETS; do rustup target add "$t" 2>/dev/null || true; done

        # Multi-target build: dependencies compiled once, cross-compiled per target
        TARGET_FLAGS=""
        for t in $MACOS_TARGETS; do TARGET_FLAGS="$TARGET_FLAGS --target $t"; done
        echo -e "${YELLOW}Building macOS targets: $MACOS_TARGETS${NC}"
        cargo build -p vauchi-platform $TARGET_FLAGS --release
        echo -e "${GREEN}macOS build complete${NC}"

        # Package libraries
        mkdir -p "$MACOS_LIBS_DIR"

        if ! $RELEASE_ONLY; then
            # Create universal macOS library (ARM64 + x86_64)
            echo -e "${YELLOW}Creating universal macOS library...${NC}"
            lipo -create \
                target/aarch64-apple-darwin/release/libvauchi_platform.a \
                target/x86_64-apple-darwin/release/libvauchi_platform.a \
                -output "$MACOS_LIBS_DIR/libvauchi_platform_macos.a"
        else
            cp target/aarch64-apple-darwin/release/libvauchi_platform.a \
                "$MACOS_LIBS_DIR/libvauchi_platform_macos.a"
        fi

        echo -e "${GREEN}macOS libraries:${NC}"
        ls -lh "$MACOS_LIBS_DIR/"
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

    # Set up NDK toolchain environment for all Android targets
    export CC_aarch64_linux_android="$NDK_TOOLCHAIN/aarch64-linux-android24-clang"
    export AR_aarch64_linux_android="$NDK_TOOLCHAIN/llvm-ar"
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_TOOLCHAIN/aarch64-linux-android24-clang"
    export CC_x86_64_linux_android="$NDK_TOOLCHAIN/x86_64-linux-android24-clang"
    export AR_x86_64_linux_android="$NDK_TOOLCHAIN/llvm-ar"
    export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$NDK_TOOLCHAIN/x86_64-linux-android24-clang"

    # Build targets based on --release-only flag
    ANDROID_TARGETS="aarch64-linux-android"
    if ! $RELEASE_ONLY; then
        ANDROID_TARGETS="$ANDROID_TARGETS x86_64-linux-android"
    fi

    # Ensure targets are installed
    for t in $ANDROID_TARGETS; do rustup target add "$t" 2>/dev/null || true; done

    # Multi-target build: dependencies compiled once, cross-compiled per target
    TARGET_FLAGS=""
    for t in $ANDROID_TARGETS; do TARGET_FLAGS="$TARGET_FLAGS --target $t"; done
    echo -e "${YELLOW}Building Android targets: $ANDROID_TARGETS${NC}"
    cargo build -p vauchi-platform $TARGET_FLAGS --release
    echo -e "${GREEN}Android build complete ($(echo $ANDROID_TARGETS | wc -w | tr -d ' ') targets)${NC}"

    # Copy native libraries
    echo -e "${YELLOW}Copying native libraries...${NC}"
    mkdir -p "$ANDROID_JNI_DIR/arm64-v8a"
    cp target/aarch64-linux-android/release/libvauchi_platform.so "$ANDROID_JNI_DIR/arm64-v8a/"
    if ! $RELEASE_ONLY; then
        mkdir -p "$ANDROID_JNI_DIR/x86_64"
        cp target/x86_64-linux-android/release/libvauchi_platform.so "$ANDROID_JNI_DIR/x86_64/"
    fi

    # Generate Kotlin bindings
    # Note: uniffi-bindgen can't read metadata from cross-compiled libraries,
    # so we build a native library first and use that for binding generation.
    # We use --library mode to extract types from proc macros, matching iOS approach.
    # IMPORTANT: Build without symbol stripping to preserve UniFFI metadata!
    echo -e "${YELLOW}Generating Kotlin bindings...${NC}"
    mkdir -p "$ANDROID_KOTLIN_DIR"

    # Build native library for binding generation (without stripping to preserve metadata)
    echo -e "${YELLOW}Building native library for metadata extraction...${NC}"
    RUSTFLAGS="-Cstrip=none" cargo build -p vauchi-platform --release

    # Determine native library extension (.so on Linux, .dylib on macOS)
    if [[ "$(uname)" == "Darwin" ]]; then
        NATIVE_LIB="target/release/libvauchi_platform.dylib"
    else
        NATIVE_LIB="target/release/libvauchi_platform.so"
    fi

    cargo run -p vauchi-platform --bin uniffi-bindgen --release -- generate \
        --library "$NATIVE_LIB" \
        --language kotlin \
        --out-dir "$ANDROID_KOTLIN_DIR"

    echo -e "${GREEN}Kotlin bindings generated at: $ANDROID_KOTLIN_DIR${NC}"

    echo -e "${GREEN}Android libraries:${NC}"
    ls -lh "$ANDROID_JNI_DIR"/*/libvauchi_platform.so
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

if $BUILD_MACOS && [[ "$(uname)" == "Darwin" ]]; then
    echo -e "${GREEN}macOS:${NC}"
    echo "  Libraries:      $MACOS_LIBS_DIR/"
    echo "  (Swift bindings shared with iOS)"
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

    if $BUILD_MACOS && [[ -d "$LOCAL_MACOS_DIR" ]]; then
        echo -e "${YELLOW}Copying macOS bindings to $LOCAL_MACOS_DIR/...${NC}"
        mkdir -p "$LOCAL_MACOS_DIR/Vauchi/Generated"
        mkdir -p "$LOCAL_MACOS_DIR/Vauchi/Libs"
        # macOS shares Swift bindings with iOS (same UniFFI output)
        cp -r "$IOS_GENERATED_DIR/"* "$LOCAL_MACOS_DIR/Vauchi/Generated/" 2>/dev/null || true
        cp -r "$MACOS_LIBS_DIR/"* "$LOCAL_MACOS_DIR/Vauchi/Libs/" 2>/dev/null || true
        echo -e "${GREEN}  Installed to $LOCAL_MACOS_DIR/Vauchi/${NC}"
    fi

    if $BUILD_ANDROID && [[ -d "$LOCAL_ANDROID_DIR" ]]; then
        echo -e "${YELLOW}Copying Android bindings to $LOCAL_ANDROID_DIR/...${NC}"
        mkdir -p "$LOCAL_ANDROID_DIR/app/src/main/jniLibs"
        mkdir -p "$LOCAL_ANDROID_DIR/app/src/local-bindings/kotlin"
        cp -r "$ANDROID_JNI_DIR/"* "$LOCAL_ANDROID_DIR/app/src/main/jniLibs/" 2>/dev/null || true
        cp -r "$ANDROID_KOTLIN_DIR/"* "$LOCAL_ANDROID_DIR/app/src/local-bindings/kotlin/" 2>/dev/null || true
        echo -e "${GREEN}  Installed to $LOCAL_ANDROID_DIR/app/src/{main/jniLibs,local-bindings/kotlin}/${NC}"
    fi
fi
