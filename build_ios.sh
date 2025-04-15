#!/bin/bash
set -e

# Ensure Rust iOS target is installed
rustup target add aarch64-apple-ios

# Build the library for iOS
IPHONEOS_DEPLOYMENT_TARGET=16.0 cargo build --target aarch64-apple-ios