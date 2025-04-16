#!/bin/bash
set -e

# Configuration
NDK_VERSION=29.0.13113456
NDK_HOME="$HOME/Android/Sdk/ndk/$NDK_VERSION"
API_LEVEL=35
TARGET=aarch64-linux-android
TOOLCHAIN="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64"

# Check existence
if [ ! -d "$TOOLCHAIN" ]; then
  echo "Error: NDK toolchain not found at $TOOLCHAIN"
  exit 1
fi

# Export toolchain paths
export AR="$TOOLCHAIN/bin/llvm-ar"
export CC="$TOOLCHAIN/bin/${TARGET}${API_LEVEL}-clang"
export CXX="$TOOLCHAIN/bin/${TARGET}${API_LEVEL}-clang++"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CC"

# Add Rust target if not installed
if ! rustup target list --installed | grep -q "$TARGET"; then
  echo "Installing Rust target: $TARGET"
  rustup target add "$TARGET"
fi

# Build
echo "Building Rust project for $TARGET (API $API_LEVEL)"
cargo build --target "$TARGET" --release
