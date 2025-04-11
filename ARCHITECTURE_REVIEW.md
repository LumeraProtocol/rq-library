# RQ-Library Architecture Review (April 10, 2025)

## 1. Objective

Review the current architecture and implementation of the `rq-library` project against the goals outlined in `RQ redesign.md`, identify potential issues, and propose a plan for refinement.

## 2. Analysis Summary

The library aims to implement the architecture described in `RQ redesign.md`, focusing on replacing a memory-intensive gRPC service with an embeddable Rust library, using chunking for memory efficiency when handling large files, and providing resource controls (memory/concurrency limits).

*   **Redesign Goals Alignment:** The library structure (FFI layer in `lib.rs`, core logic in `processor.rs`) generally aligns with the goal of creating an embeddable library.
*   **FFI Layer (`lib.rs`):** Provides a standard C interface using session management via a global map, which is appropriate for this type of library.
*   **Concurrency Control (`processor.rs`):** Correctly implemented using `AtomicUsize` and an RAII `TaskGuard` to limit simultaneous operations based on the configured `concurrency_limit`.
*   **Chunking Logic (`processor.rs`):** The code correctly determines *when* to chunk based on file size and configured memory limits (`get_recommended_chunk_size`). It processes files either as a whole (`encode_single_file`) or in parts (`encode_file_in_chunks`).
*   **Underlying `raptorq` Crate (v2.0.0):** Analysis confirmed that the `raptorq` crate API (specifically `Encoder::new` and `Decoder::decode`) requires complete data segments (the entire file or chunk) to be held in memory. It **does not support true streaming** for encoding input or decoding output.

## 3. Identified Issues & API Limitations

The primary issues stem from the limitations of the `raptorq` v2.0.0 API:

1.  **Encoding Memory Usage (API Limitation):** The `encode_stream` function reads the input stream but must accumulate the **entire file or chunk content into a `Vec<u8>`** before passing it to `raptorq::Encoder::new`. This means peak memory usage during encoding is dictated by the chunk size (or file size if not chunking), not constant memory as true streaming would provide.
2.  **Chunked Decoding Memory Usage (API Limitation):** `decode_chunked_file` uses `decode_symbols_to_memory`, which loads each **entire decoded chunk** into a `Vec<u8>` in memory before writing it to the output file. This is necessary because `Decoder::decode` returns the full block at once. While better than loading the whole file, it can still cause memory spikes for large chunks.
3.  **Memory Estimation Accuracy:** The `estimate_memory_requirements` heuristic (`data_size * 2.5`) might be inaccurate given Issue #1. A poor estimate could lead to suboptimal chunking decisions or potential OOMs if the estimate is too low compared to the actual memory needed for the `Vec<u8>` plus `raptorq` overhead.

## 4. Refined Plan

Given that true streaming isn't possible with `raptorq` v2.0.0, the focus is on ensuring the existing chunking mechanism is robust and well-configured:

1.  **Acknowledge API Limitation:** Accept that memory usage will scale with chunk size due to the `raptorq` API.
2.  **Validate Chunking Strategy:** Confirm the logic in `get_recommended_chunk_size` and its interaction with `max_memory_mb` correctly limits chunk size to prevent exceeding available memory.
3.  **Refine Memory Estimation:** Re-evaluate the `estimate_memory_requirements` heuristic (`data_size * 2.5`). Consider profiling or using a more conservative estimate to better reflect the actual peak memory usage (Vec allocation + RaptorQ overhead).
4.  **Review Chunked Decoding:** Confirm the current approach (`decode_symbols_to_memory` then write) is the necessary implementation path given the `Decoder::decode` API returning the full block.
5.  **Documentation:** Update internal and external documentation to clearly state that the library operates on full chunks in memory due to the underlying `raptorq` dependency. Explain how `max_memory_mb` controls this behavior and relates to chunk size.

## 5. Call Flow Diagram (Highlighting API Constraints)

```mermaid
sequenceDiagram
    participant C_Client as C Client
    participant FFI as rq-library (lib.rs)
    participant Processor as rq-library (processor.rs)
    participant RaptorQ as raptorq crate API (v2.0.0)
    participant FS as File System

    C_Client->>+FFI: raptorq_encode_file(session_id, input, output, chunk_size=N)
    FFI->>+Processor: encode_file_streamed(input, output, N)
    Processor->>Processor: Determine actual_chunk_size based on file_size, N, max_memory_mb
    alt Chunking (actual_chunk_size > 0)
        Processor->>Processor: encode_file_in_chunks(...)
        loop For Each Chunk
            Processor->>FS: Read chunk data into Vec<u8> `chunk_data`
            Note over Processor: Memory Usage ≈ chunk_size + overhead
            Processor->>+RaptorQ: Encoder::new(&chunk_data, config)
            Note over RaptorQ: API requires full chunk data in memory
            Processor->>RaptorQ: get_encoded_packets(...)
            RaptorQ-->>-Processor: symbols
            Processor->>FS: Write symbols
        end
    else No Chunking (actual_chunk_size == 0)
        Processor->>Processor: encode_single_file(...)
        Processor->>FS: Read full file data into Vec<u8> `file_data`
        Note over Processor: Memory Usage ≈ file_size + overhead
        Processor->>+RaptorQ: Encoder::new(&file_data, config)
        Note over RaptorQ: API requires full file data in memory
        Processor->>RaptorQ: get_encoded_packets(...)
        RaptorQ-->>-Processor: symbols
        Processor->>FS: Write symbols
    end
    Processor-->>-FFI: ProcessResult
    FFI-->>-C_Client: Result Code

    C_Client->>+FFI: raptorq_decode_symbols(session_id, symbols_dir, output_path, params)
    FFI->>+Processor: decode_symbols(...)
    Processor->>FS: Check for chunk dirs
    alt Chunked Directory
        Processor->>Processor: decode_chunked_file(...)
        Processor->>FS: Create output file writer
        loop For Each Chunk Dir
            Processor->>FS: Read symbol files for chunk
            Processor->>+Processor: decode_symbols_to_memory(decoder, symbols, &mut chunk_data_vec)
            Processor->>+RaptorQ: Decoder::new(config)
            loop Feed packets
                Processor->>RaptorQ: decode(packet)
                alt Decoded
                    RaptorQ-->>Processor: Decoded chunk Vec<u8> `result`
                    Note over RaptorQ: API returns full decoded chunk in memory
                    Processor->>Processor: chunk_data_vec.extend_from_slice(&result)
                    Note over Processor: Memory Usage ≈ decoded chunk size
                    break
                end
            end
            Processor-->>-Processor:
            Processor->>FS: output_file.write_all(&chunk_data_vec)
        end
    else Single Directory
        Processor->>Processor: decode_single_file(...)
        Processor->>FS: Read symbol files
        Processor->>FS: Create output file writer
        Processor->>+Processor: decode_symbols_to_file(decoder, symbols, output_writer)
        Processor->>+RaptorQ: Decoder::new(config)
        loop Feed packets
            Processor->>RaptorQ: decode(packet)
            alt Decoded
                RaptorQ-->>Processor: Decoded block Vec<u8> `result`
                Note over RaptorQ: API returns full decoded block in memory
                Processor->>FS: output_writer.write_all(&result)
                Note over Processor: Writes decoded block directly (more efficient than chunked path)
                break
            end
        end
        Processor-->>-Processor:
    end
    Processor-->>-FFI: Ok
    FFI-->>-C_Client: Result Code