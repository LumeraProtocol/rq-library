#!/bin/bash
set -e

echo "Building RQ Library for all supported platforms..."

# Check OS
OS=$(uname -s)
ARCH=$(uname -m)

# Make all shell scripts executable
chmod +x build_*.sh

# Build for the current platform first
if [ "$OS" = "Darwin" ]; then
    echo "Detected macOS system, building for macOS first..."
    ./build_macos.sh
    
    # Also build for iOS if on macOS
    echo "Building for iOS..."
    ./build_ios.sh
elif [ "$OS" = "Linux" ]; then
    echo "Detected Linux system, building for Linux first..."
    ./build_linux.sh
else
    echo "Unknown platform: $OS. Will try to continue with all builds..."
fi

# Build for WASM/Emscripten
echo "Building for WASM/Emscripten..."
./build_emscripten.sh

# Build for Android
if command -v javac &> /dev/null; then
    echo "Java detected, building for Android..."
    ./build_android.sh
else
    echo "Java not found, skipping Android build."
    echo "To build for Android, please install the Android SDK and NDK."
fi

# Print platform-specific instructions
echo ""
echo "Build process completed."
echo ""
echo "To build for Windows, run the PowerShell script on a Windows machine:"
echo "    ./build_windows.ps1"
echo ""
echo "Final libraries available in the following locations:"
echo "- MacOS:   dist/lib/darwin/amd64 (Intel) and dist/lib/darwin/arm64 (Apple Silicon)"
echo "- Linux:   dist/lib/linux/amd64 (x86_64) and dist/lib/linux/arm64 (aarch64)"
echo "- iOS:     dist/lib/ios/arm64"
echo "- Android: dist/lib/android/arm64"
echo "- WASM:    dist/lib/wasm/emscripten"
echo "- Windows: dist/lib/windows/x64 (64-bit) and dist/lib/windows/x86 (32-bit)"