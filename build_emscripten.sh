#!/bin/bash
set -e

# Install Emscripten if not already installed
if ! command -v emcc &> /dev/null; then
    echo "Installing Emscripten..."
    git clone https://github.com/emscripten-core/emsdk.git
    cd emsdk
    ./emsdk install latest
    ./emsdk activate latest
    source ./emsdk_env.sh
    cd ..
fi

# Ensure Rust Emscripten target is installed
rustup target add wasm32-unknown-emscripten

# Build the library with Emscripten
cargo build --target wasm32-unknown-emscripten --release --features browser-wasm

# Copy output files to dist directory
#mkdir -p dist/browser
#cp target/wasm32-unknown-emscripten/release/raptorq_lib.js dist/browser/
#cp target/wasm32-unknown-emscripten/release/raptorq_lib.wasm dist/browser/
#
#echo "Browser WASM build completed. Output files in dist/browser/"
