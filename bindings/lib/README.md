# Static RaptorQ Library

This directory contains the static version of the RaptorQ library (`librq_library.a`) for different platforms and architectures.

## Directory Structure

The library files are organized in platform-specific subdirectories following Go's standard naming convention:

```
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

To build the static library for your platform, navigate to the root of the RaptorQ project and run:

```bash
# Make sure staticlib is in the crate-type list in Cargo.toml
# [lib]
# name = "rq_library"
# crate-type = ["cdylib", "staticlib", "rlib"]

# Build static library (release mode)
cargo build --release

# Copy the static library to the appropriate platform directory
# On Linux amd64:
mkdir -p bindings/lib/linux/amd64
cp target/release/librq_library.a bindings/lib/linux/amd64/

# On macOS Intel (with deployment target for compatibility):
mkdir -p bindings/lib/darwin/amd64
MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --release
cp target/release/librq_library.a bindings/lib/darwin/amd64/

# On macOS Apple Silicon:
mkdir -p bindings/lib/darwin/arm64
MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --release --target aarch64-apple-darwin
cp target/aarch64-apple-darwin/release/librq_library.a bindings/lib/darwin/arm64/

# On Windows:
mkdir -p bindings/lib/windows/amd64
copy target\release\rq_library.lib bindings\lib\windows\amd64\librq_library.a
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
