# RaptorQ Go Bindings (Static Linking)

This Go package provides bindings to the RaptorQ erasure coding library. It uses CGO to interface with the Rust implementation and is configured for **static linking**.

## Installation

This package uses a static library for the RaptorQ implementation. To use this package, add it to your Go module:

```
go get github.com/LumeraProtocol/rq-go
```

## Usage

```go
package main

import (
	"fmt"
	"github.com/LumeraProtocol/rq-go"
)

func main() {
	// Create a new RaptorQ processor with default settings
	processor, err := raptorq.NewDefaultRaptorQProcessor()
	if err != nil {
		panic(err)
	}
	defer processor.Free()

	// Get library version
	version := raptorq.GetVersion()
	fmt.Printf("RaptorQ library version: %s\n", version)

	// Use the processor to encode/decode files
	// See the test file for examples
}
```

## Static Linking

This module uses static linking by default, which means:

1. The Rust library code is compiled into your Go binary directly
2. You don't need to distribute separate shared libraries with your application
3. Your final binary will be larger but more portable

### Requirements

The Go bindings require:

- Go 1.17 or newer
- The static RaptorQ library (`librq_library.a` in the appropriate platform directory under `../lib`)
- A C compiler (gcc, clang, or MSVC)

Additionally, your system will need the following development dependencies:

- **Linux**: GCC, libc-dev, and other base development tools
- **macOS**: Xcode Command Line Tools
- **Windows**: Visual Studio or MinGW with appropriate C/C++ development tools

## Testing

To run the tests from the `bindings/go` directory:

```bash
go test ./...
```

The CGO flags required for static linking are embedded in the source code, so no external environment variables are needed for building or testing.

## Platforms Supported

The static library can be built and used on:

- Linux amd64
- macOS (Intel/Apple Silicon)
- Windows amd64

Static building for other platforms may require additional configuration. See the `bindings/lib/README.md` file for build instructions.

## Building Applications

When building applications that use this package, you'll create a fully self-contained binary that can be deployed without external dependencies. No need to ship separate `.so`, `.dylib`, or `.dll` files.

```bash
# Build a completely static Go binary
go build -o myapp main.go
```

## Platform-Specific Considerations

### macOS Deployment Target

When building for macOS, you might see warnings like:

```
ld: warning: object file was built for newer 'macOS' version (15.4) than being linked (15.0)
```

These warnings are generally safe to ignore as they don't affect functionality. However, if you need to eliminate them or ensure compatibility with older macOS versions, you can:

1. **When building the Rust library:**
   ```bash
   MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --release
   ```

2. **When building with Go:**
   ```bash
   CGO_CFLAGS="-mmacosx-version-min=15.0" go build
   ```

3. **Use the Go minimum version flag:**
   ```bash
   go build -ldflags "-linkmode external -extldflags '-mmacosx-version-min=15.0'"
   ```

## License

This project is licensed under the same terms as the main RaptorQ library.
