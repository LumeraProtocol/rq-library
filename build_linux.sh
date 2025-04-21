#!/bin/bash
set -e

echo "Building RQ Library for Linux (x86_64 and aarch64)..."

# Build for x86_64 (amd64)
echo "Building for x86_64-unknown-linux-gnu..."
rustup target add x86_64-unknown-linux-gnu
cargo build --target x86_64-unknown-linux-gnu --release

# Create the output directory if it doesn't exist
mkdir -p dist/lib/linux/amd64

# Copy the built library files for x86_64
echo "Copying built files to dist/lib/linux/amd64/"
cp target/x86_64-unknown-linux-gnu/release/librq_library.so dist/lib/linux/amd64/
cp target/x86_64-unknown-linux-gnu/release/librq_library.a dist/lib/linux/amd64/
cp target/x86_64-unknown-linux-gnu/release/librq_library.rlib dist/lib/linux/amd64/

# Build for aarch64 (arm64)
# Check if we have the capability to build for ARM64
if rustup target list | grep -q "aarch64-unknown-linux-gnu"; then
    echo "Building for aarch64-unknown-linux-gnu..."
    rustup target add aarch64-unknown-linux-gnu
    
    # For cross-compilation, we might need additional tools
    if command -v aarch64-linux-gnu-gcc &> /dev/null; then
        # Set up cross-compilation environment
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
        cargo build --target aarch64-unknown-linux-gnu --release
        
        # Create the output directory if it doesn't exist
        mkdir -p dist/lib/linux/arm64
        
        # Copy the built library files for ARM64
        echo "Copying built files to dist/lib/linux/arm64/"
        cp target/aarch64-unknown-linux-gnu/release/librq_library.so dist/lib/linux/arm64/
        cp target/aarch64-unknown-linux-gnu/release/librq_library.a dist/lib/linux/arm64/
        cp target/aarch64-unknown-linux-gnu/release/librq_library.rlib dist/lib/linux/arm64/
    else
        echo "Warning: aarch64-linux-gnu-gcc is not installed. Skipping ARM64 build."
        echo "To build for ARM64, install gcc-aarch64-linux-gnu package."
    fi
else
    echo "Warning: aarch64-unknown-linux-gnu target is not available. Skipping ARM64 build."
fi

echo "Linux build completed. Output files in dist/lib/linux/"