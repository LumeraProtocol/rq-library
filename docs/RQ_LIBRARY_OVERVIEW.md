# RQ Library Overview

## 1. Introduction

Erasure coding is a data protection method enabling the reconstruction of original data from a subset of encoded fragments. RaptorQ is a standardized, efficient algorithm for erasure coding.

The **`rq-library`** is a software library providing an interface to RaptorQ erasure coding functionalities. It serves as a high-level wrapper around the core `raptorq` Rust crate (https://github.com/cberner/raptorq), designed for direct integration into applications. Its primary function is to offer reliable data encoding and decoding capabilities across multiple platforms (desktop, mobile, web) while optimizing memory usage. This library facilitates the use of RaptorQ without requiring the management of a separate service.

## 2. Core Design and Architecture

The library's design emphasizes efficiency and cross-platform compatibility through several architectural principles:

* **Stream-Based File Processing:** To handle large files without excessive memory consumption, the library processes data in configurable blocks or chunks, rather than loading entire files into memory. This approach significantly reduces peak memory requirements during encoding and decoding operations.
   **Metadata File (`_raptorq_layout.json`):** During encoding, the library generates a metadata file named `_raptorq_layout.json`. This file contains essential parameters and structural information about how the original file was segmented and encoded. This metadata is required during the decoding phase to ensure accurate reconstruction of the original data from the encoded symbols.
* **Resource Management:** The library incorporates mechanisms for resource control, including configurable limits on memory usage and the number of concurrent encoding/decoding operations. These controls help maintain system stability when processing multiple requests or large datasets.
* **Cross-Platform Support:** The library is engineered for integration into applications targeting various operating systems and environments:
  * Desktop: macOS, Windows, Linux
  * Mobile: iOS, Android (via Foreign Function Interface)
  * Web: Modern web browsers (via WebAssembly)

* **Conceptual Architecture:**

    ```mermaid
    graph TD
        subgraph rq-library Integration
            C[Client Application] -- API Call --> D{rq-library};
            D -- Processes Data in Blocks --> M[Memory: Optimized Usage];
            D -- Generates --> E[_raptorq_layout.json Metadata];
            D -- Generates --> F[Encoded Symbols];
        end
        style M fill:#ccf,stroke:#333,stroke-width:2px
    ```

## 3. High-Level Workflow

Application integration involves two primary operations:

* **Encoding (Data Protection):**
  1. The client application specifies the input file path.
  2. The library reads and processes the file content in blocks.
  3. The library outputs:
     * Encoded data fragments, termed "symbols", containing original and redundant data.
     * The `_raptorq_layout.json` metadata file.
  4. The client application is responsible for storing the generated symbols and the layout file.

* **Decoding (Data Recovery):**
  1. The client application provides the library with:
     * A sufficient subset of the previously generated symbols.
     * The corresponding `_raptorq_layout.json` metadata file.
  2. The library parses the metadata file to determine the encoding structure.
  3. The library processes the provided symbols.
  4. The library reconstructs and outputs the original file data.

* **Developer Integration:** Native applications typically interact with the library via a C Foreign Function Interface (FFI). Web applications utilize a JavaScript interface exposed through WebAssembly.

## 4. Key Benefits

The `rq-library` offers the following advantages:

* **Memory Efficiency:** Stream-based processing minimizes peak RAM usage, preventing issues associated with loading large files entirely into memory.
* **Simplified Integration:** Direct library linkage removes the overhead and complexity of managing a separate network service for erasure coding.
* **Platform Flexibility:** Enables the use of consistent RaptorQ logic across diverse application environments (desktop, mobile, web).
* **Reliable Reconstruction:** The mandatory `_raptorq_layout.json` metadata file ensures the integrity and accuracy of the data decoding process.

## 5. Conclusion

The `rq-library` provides a robust and efficient mechanism for integrating RaptorQ erasure coding capabilities directly into software applications. Its design prioritizes memory efficiency, cross-platform compatibility, and reliable data reconstruction through stream processing and explicit metadata management.