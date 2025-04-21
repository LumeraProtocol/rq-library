#!/bin/bash
set -e

# Install wasm-bindgen-cli if not already installed
if ! command -v wasm-bindgen &> /dev/null; then
    echo "Installing wasm-bindgen-cli..."
    cargo install wasm-bindgen-cli
fi

# Ensure Rust WebAssembly target is installed
rustup target add wasm32-unknown-unknown

# Build the library with wasm32-unknown-unknown target
cargo build --target wasm32-unknown-unknown --release --features browser-wasm

# Create output directory if it doesn't exist
mkdir -p dist/lib/wasm/browser

# Run wasm-bindgen to generate JavaScript bindings
wasm-bindgen --target web \
    --out-dir dist/lib/wasm/browser \
    --out-name rq_library \
    target/wasm32-unknown-unknown/release/rq_library.wasm

echo "Browser WASM build completed. Output files in dist/lib/wasm/browser"