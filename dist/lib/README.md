# Static RaptorQ Library

This directory contains the static version of the RaptorQ library (`librq_library.a`) for different platforms and architectures.

## Directory Structure

The library files are organized in platform-specific subdirectories following Go's standard naming convention:

```shell
lib/
├── README.md             # This file
├── android/              # Android libraries
├── darwin/               # macOS libraries
│   ├── amd64/            # Intel macOS
│   │   ├── librq_library.a
│   └── arm64/            # Apple Silicon
│       └── librq_library.a
├── ios/                  # iOS libraries
├── linux/                # Linux libraries
│   ├── amd64/            # amd64
│   │   └── librq_library.a
│   └── arm64/            # ARM64
│       └── librq_library.a
├── wasm/                 # WebAssembly libraries
└── windows/              # Windows libraries
    └── amd64/            # amd64
        └── librq_library.a
```

## Building the Static Library

To build the static library for your platform, navigate to the root of the RaptorQ project and use the provided build scripts:

```bash
# For all supported platforms on the current OS
./build_all.sh

# For specific platforms:
./build_macos.sh     # Build for macOS (Intel and ARM64)
./build_linux.sh     # Build for Linux (x86_64 and aarch64)
./build_ios.sh       # Build for iOS
./build_android.sh   # Build for Android
./build_wasm_browser.sh # Build for WebAssembly

# On Windows (using PowerShell):
.\build_windows.ps1  # Build for Windows (x64 and x86)
```

The build scripts will automatically:

1. Install any required Rust targets
2. Build the library for the specified platform(s)
3. Create the necessary directory structure
4. Copy the built libraries to the appropriate directories in `dist/lib/`

### Manual Building

If you prefer to build manually, make sure staticlib is in the crate-type list in Cargo.toml:

```toml
[lib]
name = "rq_library"
crate-type = ["cdylib", "staticlib", "rlib"]
```

Then build for your target and copy to the appropriate directory:

```bash
# Build static library (release mode)
cargo build --release --target <target-triple>

# Copy the static library to the appropriate platform directory
mkdir -p dist/lib/<platform>/<arch>
cp target/<target-triple>/release/librq_library.a dist/lib/<platform>/<arch>/
```

### macOS Deployment Target

For macOS compatibility across different versions, set the deployment target when building:

```bash
# For macOS 15.0+ compatibility:
MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --release
```

Without setting this, the library will target the macOS version you're building on, which may cause linking warnings when used with applications targeting older versions.

## Static Linking Notes

The Go module is configured to statically link with this library. This approach offers several advantages:

1. **Self-contained binaries**: No need to distribute separate shared libraries with your Go application.
2. **Predictable behavior**: Avoids compatibility issues with system libraries at runtime.
3. **Simplified deployment**: Single binary deployment without external dependencies.

However, statically linked applications may be larger than dynamically linked ones, as the library code is included in the executable.

### Platform-Specific Dependencies

When using the Go module with static linking, your application may still need certain system libraries at **build time**:

- **Linux**: libc, libdl, libpthread, libm
- **macOS**: Security, CoreFoundation, libm
- **Windows**: ws2_32, userenv, advapi32 libraries

For more detailed build instructions, refer to the main project README and documentation.
