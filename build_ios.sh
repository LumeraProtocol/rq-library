#!/bin/bash
set -e

# Ensure Rust iOS target is installed
rustup target add aarch64-apple-ios

# Build the library for iOS
IPHONEOS_DEPLOYMENT_TARGET=16.0 cargo build --target aarch64-apple-ios --release

# Create the output directory if it doesn't exist
mkdir -p dist/lib/ios/arm64

# Copy the built library files
echo "Copying built files to dist/lib/ios/arm64/"
cp target/aarch64-apple-ios/release/librq_library.a dist/lib/ios/arm64/
cp target/aarch64-apple-ios/release/librq_library.dylib dist/lib/ios/arm64/ 2>/dev/null || echo "Note: Dynamic library not generated for iOS"

echo "iOS build completed. Output files in dist/lib/ios/arm64/"