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

# Create the output directory if it doesn't exist
mkdir -p dist/lib/android/arm64

# Copy the built library files
echo "Copying built files to dist/lib/android/arm64/"
cp target/$TARGET/release/librq_library.so dist/lib/android/arm64/
cp target/$TARGET/release/librq_library.a dist/lib/android/arm64/

echo "Android build completed. Output files in dist/lib/android/arm64/"
