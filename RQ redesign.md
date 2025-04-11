# From Memory-Intensive Service to Efficient Multi-Platform Library

## 1. Original Architecture and Problem Statement

### 1.1 Service-Based Architecture

Our original implementation consisted of two backend services:

- **Supernode** (Go): User-facing service receiving files and requests
- **RQ-Service** (Rust): Specialized gRPC server wrapping the RaptorQ erasure coding library

The workflow operated as follows:
1. Supernode received files from users and stored them on disk
2. Supernode made gRPC calls to RQ-Service with file paths
3. RQ-Service loaded entire files into memory
4. RaptorQ encoding was applied to create encoded symbols
5. Symbols were written to disk in a subdirectory

### 1.2 Critical Memory Issues

This architecture encountered severe memory management problems:

- **Complete File Loading**: RQ-Service loaded entire files into memory via `file.read_to_end(&mut data)`
- **Concurrent Processing**: Multiple large files processed simultaneously quickly exhausted RAM
- **No Resource Limits**: Absence of throttling for concurrent requests or memory usage
- **Out-of-Memory (OOM) Killer**: System would terminate services under memory pressure
- **Service Recovery**: Required manual intervention or watchdog processes

Files exceeding several gigabytes were particularly problematic, leading to unpredictable service behavior and failures.

## 2. Proposed Solution: Multi-Target Library Architecture

### 2.1 Architectural Transformation

We redesigned the system to eliminate the separate RQ-Service by converting it to a library that could be directly embedded in the Supernode (or any client application):

- **RQ-Library**: Core RaptorQ processing logic compiled for multiple targets
- **Direct Integration**: No interprocess communication or separate service
- **Resource Control**: Memory management handled within the calling application

### 2.2 Key Design Principles

1. **Stream Processing**: Process files in chunks rather than loading entirely into memory
2. **Resource Management**: Built-in limits for concurrent operations and memory usage
3. **Pre-chunking**: Ability to split very large files before RaptorQ processing
4. **Multiple Compilation Targets**: Support for various platforms and integration methods

### 2.3 Multi-Target Approach

The library supports multiple compilation targets for maximum flexibility:

- **Native Platforms**:
  - macOS (x86_64, ARM64)
  - Windows (x86_64)
  - Linux (x86_64, ARM64)
  - iOS (ARM64)
  - Android (ARM64)

- **WebAssembly Options** (alternative integration approach):
  - wasm32-unknown-unknown (for browser and basic embedding)
  - wasm32-wasip1 (for WASI-compatible environments)

## 3. Benefits and Results

### 3.1 Memory Efficiency

- **Controlled Memory Usage**: Processing in manageable chunks prevents memory exhaustion
- **Resource Limits**: Built-in concurrency control prevents system overload
- **Adaptive Chunking**: Automatic adjustment based on file size and available resources
- **Elimination of OOM Issues**: Predictable memory usage patterns

### 3.2 Architectural Advantages

- **Simplified Deployment**: Single-process architecture eliminates complex service orchestration
- **Reduced Latency**: No inter-service communication overhead
- **Enhanced Security**: Fewer network interfaces and attack surfaces
- **Improved Error Handling**: Direct error propagation without gRPC serialization

### 3.3 Flexibility Improvements

- **Cross-Platform Support**: Same core logic available on multiple platforms
- **Integration Options**: FFI, native libraries, or WebAssembly as needed
- **Unified Codebase**: Maintain single implementation with conditional compilation
- **Customizable Memory Model**: Clients can control memory allocation based on device capabilities

## 4. Implementation Strategies

### 4.1 Core Processing Logic

- **Streaming API**: Process data incrementally without loading entire files
- **Chunking Strategies**: Multiple approaches for different file sizes
- **Memory Monitoring**: Track usage and apply backpressure when needed

### 4.2 Integration Methods

- **FFI for Native Targets**: C-compatible API for seamless integration with host languages
- **WebAssembly**: Alternative integration for environments that support WASM
- **Platform-Specific Wrappers**: Optional convenience APIs for Swift, Kotlin, etc.

## 5. Future Considerations

### 5.1 Enhancements

- **Parallel Processing**: Further optimization for multi-core architectures
- **GPU Acceleration**: Evaluate potential for offloading computation to GPUs
- **Statistical Tuning**: Optimize chunk sizes and concurrency based on performance metrics

### 5.2 Extensibility

- **Additional Targets**: Support for new platforms as they emerge
- **Feature Toggles**: Compile-time options for specialized use cases
- **Configuration API**: Runtime tuning of processing parameters
