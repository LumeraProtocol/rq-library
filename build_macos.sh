#!/bin/bash
set -e

echo "Building RQ Library for macOS (Intel and ARM64)..."

# Build for Intel (x86_64-apple-darwin)
echo "Building for Intel Mac (x86_64-apple-darwin)..."
rustup target add x86_64-apple-darwin
cargo build --target x86_64-apple-darwin --release

# Create the output directory if it doesn't exist
mkdir -p dist/lib/darwin/amd64

# Copy the built library files for Intel Mac
echo "Copying built files to dist/lib/darwin/amd64/"
cp target/x86_64-apple-darwin/release/librq_library.a dist/lib/darwin/amd64/
cp target/x86_64-apple-darwin/release/librq_library.dylib dist/lib/darwin/amd64/
cp target/x86_64-apple-darwin/release/librq_library.rlib dist/lib/darwin/amd64/

# Build for ARM64 (aarch64-apple-darwin)
echo "Building for Apple Silicon (aarch64-apple-darwin)..."
rustup target add aarch64-apple-darwin
cargo build --target aarch64-apple-darwin --release

# Create the output directory if it doesn't exist
mkdir -p dist/lib/darwin/arm64

# Copy the built library files for ARM64 Mac
echo "Copying built files to dist/lib/darwin/arm64/"
cp target/aarch64-apple-darwin/release/librq_library.a dist/lib/darwin/arm64/
cp target/aarch64-apple-darwin/release/librq_library.dylib dist/lib/darwin/arm64/
cp target/aarch64-apple-darwin/release/librq_library.rlib dist/lib/darwin/arm64/

echo "macOS build completed. Output files in dist/lib/darwin/"