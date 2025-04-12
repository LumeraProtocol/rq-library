# Test Plan: rq-library


test processor::tests::test_decode_success_chunked_file ... FAILED
processor::tests::test_encode_memory_limit_exceeded

**Version:** 1.0
**Date:** 2025-04-11

**1. Introduction**

This document outlines the test plan for the `rq-library`, a Rust library providing RaptorQ encoding and decoding functionality with a C Foreign Function Interface (FFI). The plan covers unit tests for the core Rust logic and the FFI layer, system tests simulating end-to-end usage, and benchmarking tests to measure performance.

**2. Goals**

*   Ensure the correctness and robustness of the RaptorQ encoding and decoding logic (`src/processor.rs`).
*   Verify the C FFI layer (`src/lib.rs`) correctly handles parameters, manages sessions, propagates errors, and ensures memory safety.
*   Validate the library's ability to handle various file sizes, including chunking for large files.
*   Confirm the library functions correctly when used from both Rust and Go (via FFI).
*   Measure and document the performance characteristics of encoding and decoding.

**3. Scope**

*   **In Scope:**
    *   Unit testing of public and key internal functions in `src/processor.rs`.
    *   Unit testing of all `extern "C"` functions in `src/lib.rs`.
    *   System testing of encoding/decoding workflows using Rust and Go clients.
    *   Benchmarking of encoding/decoding performance using Rust and Go clients.
*   **Out of Scope:**
    *   Testing the underlying `raptorq` crate itself (assumed correct).
    *   Exhaustive testing of all possible OS/architecture combinations (focus on common platforms).
    *   UI testing (library has no UI).
    *   Security penetration testing.

**4. Test Strategy**

*   **Unit Tests (Rust):** Use Rust's built-in testing framework (`#[test]`). Focus on isolating components and testing individual functions/methods with various inputs, including edge cases and error conditions. Mocking will be used where necessary (e.g., file system interactions if feasible, though integration tests might be more practical here).
*   **FFI Unit Tests (Rust):** Test the `extern "C"` functions from within Rust, simulating C-like calls with raw pointers and checking return codes, buffer contents, and error messages.
*   **System Tests (Rust & Go):** Create separate test suites in Rust and Go. These tests will use the library (Rust directly, Go via FFI) to perform end-to-end encoding and decoding of generated files of varying sizes. File integrity will be checked after decoding.
*   **Benchmarking Tests (Rust & Go):** Use appropriate benchmarking frameworks (e.g., `criterion` for Rust, Go's built-in benchmarking) to measure encoding/decoding times for different file sizes.

**5. Test Environment**

*   **Operating Systems:** macOS (primary), Linux (secondary), Windows (secondary)
*   **Languages/Runtimes:** Rust (latest stable), Go (latest stable)
*   **Dependencies:** `raptorq` crate, standard Rust/Go build tools.

**6. Test Cases**

**6.1. Unit Tests (`src/processor.rs`)**

*   **`ProcessorConfig`:**
    *   `test_config_default`: Verify `ProcessorConfig::default()` returns expected values.
    *   `test_config_custom`: Verify creating `ProcessorConfig` with custom values works.
*   **`RaptorQProcessor::new`:**
    *   `test_new_processor`: Verify successful creation with a given config.
    *   `test_new_processor_initial_state`: Verify `active_tasks` is 0 and `last_error` is empty.
*   **`RaptorQProcessor::get_last_error`:**
    *   `test_get_last_error_empty`: Verify returns empty string initially.
    *   `test_get_last_error_after_error`: Verify returns the correct error message after a simulated failure (requires triggering an error in another method).
*   **`RaptorQProcessor::get_recommended_chunk_size`:**
    *   `test_chunk_size_small_file`: File size < calculated safe memory -> returns 0.
    *   `test_chunk_size_large_file`: File size > calculated safe memory -> returns a non-zero chunk size multiple of symbol size.
    *   `test_chunk_size_edge_memory`: Test with `max_memory_mb` values causing chunk size calculation changes.
    *   `test_chunk_size_zero_file`: File size 0 -> returns 0 (though `open_and_validate_file` should prevent this).
*   **`RaptorQProcessor::encode_file_streamed`:**
    *   `test_encode_file_not_found`: Input path does not exist -> returns `ProcessError::FileNotFound`.
    *   `test_encode_empty_file`: Input file is empty -> returns `ProcessError::EncodingFailed` (via `open_and_validate_file`).
    *   `test_encode_success_no_chunking`: Small file, `chunk_size = 0` -> encodes successfully, `result.chunks` is `None`.
    *   `test_encode_success_manual_no_chunking`: Small file, `chunk_size` > file size -> encodes successfully, `result.chunks` is `None`.
    *   `test_encode_success_auto_chunking`: Large file, `chunk_size = 0` -> encodes successfully, `result.chunks` is `Some`.
    *   `test_encode_success_manual_chunking`: Large file, `chunk_size` specified -> encodes successfully, `result.chunks` is `Some`.
    *   `test_encode_output_dir_creation`: Output directory doesn't exist -> it gets created.
    *   `test_encode_memory_limit_exceeded`: Small `max_memory_mb`, large file, no chunking requested -> returns `ProcessError::MemoryLimitExceeded`.
    *   `test_encode_concurrency_limit`: Simulate max tasks running -> returns `ProcessError::ConcurrencyLimitReached`.
    *   `test_encode_verify_result`: Successful encoding -> check `ProcessResult` fields (symbol counts, dir, chunk info if applicable).
    *   `test_encode_verify_symbols`: Successful encoding -> check symbol files are created in the output directory (count, naming convention).
*   **`RaptorQProcessor::decode_symbols`:**
    *   `test_decode_symbols_dir_not_found`: Symbols directory does not exist -> returns `ProcessError::FileNotFound`.
    *   `test_decode_no_symbols_in_dir`: Symbols directory exists but is empty -> returns `ProcessError::DecodingFailed`.
    *   `test_decode_success_single_file`: Decode symbols for a non-chunked file -> successful, output file matches original.
    *   `test_decode_success_chunked_file`: Decode symbols for a chunked file -> successful, output file matches original.
    *   `test_decode_insufficient_symbols`: Provide fewer symbols than required -> returns `ProcessError::DecodingFailed`.
    *   `test_decode_corrupted_symbol`: Provide a corrupted/invalid symbol file -> returns `ProcessError::DecodingFailed` or IO error during read.
    *   `test_decode_invalid_encoder_params`: Provide incorrect `encoder_params` -> returns `ProcessError::DecodingFailed` (likely during `Decoder::new`).
    *   `test_decode_concurrency_limit`: Simulate max tasks running -> returns `ProcessError::ConcurrencyLimitReached`.
    *   `test_decode_output_file_creation`: Output file path -> file is created/overwritten.
*   **Internal Helpers (Selected):**
    *   `test_open_validate_file_ok`: Valid file -> returns `Ok((File, size))`.
    *   `test_open_validate_file_not_found`: Invalid path -> returns `ProcessError::IOError`.
    *   `test_open_validate_file_empty`: Empty file -> returns `ProcessError::EncodingFailed`.
    *   `test_calculate_repair_symbols_logic`: Test calculation with different `data_len` and config values.
    *   `test_estimate_memory_logic`: Test calculation with different `data_size`.
    *   `test_is_memory_available_logic`: Test check against `max_memory_mb`.
    *   `test_read_symbol_files_ok`: Valid dir with symbols -> returns `Ok(Vec<Vec<u8>>)` with correct data.
    *   `test_read_symbol_files_empty`: Empty dir -> returns `ProcessError::DecodingFailed`.
    *   `test_read_symbol_files_io_error`: Simulate read error -> returns `ProcessError::IOError`.
    *   `test_decode_chunked_sorting`: Ensure chunks are processed in the correct order based on directory name (`chunk_0`, `chunk_1`, ...).

**6.2. Unit Tests (`src/lib.rs` - FFI Layer)**

*   **`raptorq_init_session`:**
    *   `test_ffi_init_success`: Call with valid config -> returns non-zero session ID. Verify processor exists in `PROCESSORS` map with correct config.
    *   `test_ffi_init_multiple`: Call multiple times -> returns unique session IDs.
*   **`raptorq_free_session`:**
    *   `test_ffi_free_valid_session`: Init then free -> returns `true`. Verify processor removed from map.
    *   `test_ffi_free_invalid_session`: Free non-existent ID -> returns `false`.
    *   `test_ffi_free_double_free`: Free same ID twice -> second call returns `false`.
*   **`raptorq_encode_file`:**
    *   `test_ffi_encode_null_pointers`: Pass NULL for paths/buffer -> returns -1.
    *   `test_ffi_encode_invalid_session`: Pass invalid session ID -> returns -4.
    *   `test_ffi_encode_success`: Valid params, successful encode -> returns 0. Verify `result_buffer` contains valid JSON matching `ProcessResult`.
    *   `test_ffi_encode_file_not_found`: Input file doesn't exist -> returns -2. Check `raptorq_get_last_error`.
    *   `test_ffi_encode_encoding_failed`: Simulate encoding error (e.g., empty file) -> returns -3. Check `raptorq_get_last_error`.
    *   `test_ffi_encode_result_buffer_too_small`: Provide buffer too small for JSON result -> returns -5.
    *   `test_ffi_encode_path_conversion_error`: Pass invalid UTF-8 paths -> returns -1.
*   **`raptorq_get_last_error`:**
    *   `test_ffi_get_error_null_buffer`: Pass NULL buffer -> returns -1.
    *   `test_ffi_get_error_invalid_session`: Pass invalid session ID -> returns -1.
    *   `test_ffi_get_error_success_empty`: No prior error -> returns 0, buffer contains empty string.
    *   `test_ffi_get_error_success_message`: After an error -> returns 0, buffer contains correct error message.
    *   `test_ffi_get_error_buffer_too_small`: Buffer too small for error message -> returns 0, buffer contains truncated, null-terminated message.
*   **`raptorq_decode_symbols`:**
    *   `test_ffi_decode_null_pointers`: Pass NULL for paths/params -> returns -1.
    *   `test_ffi_decode_invalid_session`: Pass invalid session ID -> returns -4.
    *   `test_ffi_decode_invalid_params_len`: Pass `encoder_params_len` != 12 -> returns -1.
    *   `test_ffi_decode_success`: Valid params, successful decode -> returns 0. Verify output file is correct.
    *   `test_ffi_decode_file_not_found`: Symbols dir doesn't exist -> returns -2. Check `raptorq_get_last_error`.
    *   `test_ffi_decode_decoding_failed`: Simulate decoding error (e.g., insufficient symbols) -> returns -3. Check `raptorq_get_last_error`.
    *   `test_ffi_decode_path_conversion_error`: Pass invalid UTF-8 paths -> returns -1.
*   **`raptorq_get_recommended_chunk_size`:**
    *   `test_ffi_chunk_size_invalid_session`: Pass invalid session ID -> returns 0.
    *   `test_ffi_chunk_size_success`: Valid session, various file sizes -> returns expected chunk size (or 0).
*   **`raptorq_version`:**
    *   `test_ffi_version_null_buffer`: Pass NULL buffer -> returns -1.
    *   `test_ffi_version_buffer_too_small`: Buffer too small -> returns -1.
    *   `test_ffi_version_success`: Valid buffer -> returns 0, buffer contains correct version string.

**6.3. System Tests (Rust & Go)**

*   **Setup:** Create helper functions to generate random binary files of specified sizes.
*   **Test Cases (Apply to both Rust direct usage and Go via FFI):**
    *   `test_sys_encode_decode_small_file`: Encode/decode 1KB file. Verify decoded matches original.
    *   `test_sys_encode_decode_medium_file`: Encode/decode 10MB file (likely no chunking). Verify.
    *   `test_sys_encode_decode_large_file_auto_chunk`: Encode/decode 100MB file (expect auto-chunking). Verify.
    *   `test_sys_encode_decode_large_file_manual_chunk`: Encode/decode 100MB file (specify chunk size). Verify.
    *   `test_sys_encode_decode_very_large_file`: Encode/decode 1GB file. Verify.
    *   `test_sys_decode_minimum_symbols`: Encode file, delete repair symbols, decode with only source symbols. Verify.
    *   `test_sys_decode_redundant_symbols`: Encode file, decode with all source + repair symbols. Verify.
    *   `test_sys_decode_random_subset`: Encode file, decode with a random subset of symbols (>= source symbols). Verify.
    *   `test_sys_error_handling_encode`: Trigger encoding error (e.g., non-existent input) -> verify error reported correctly.
    *   `test_sys_error_handling_decode`: Trigger decoding error (e.g., non-existent symbols dir) -> verify error reported correctly.
    *   **(Go Specific):** Test passing Go strings/slices to C functions and handling returned C strings/buffers.

**6.4. Benchmarking Tests (Rust & Go)**

*   **Setup:** Use standard benchmarking frameworks. Generate test files beforehand.
*   **Benchmarks (Apply to both Rust direct usage and Go via FFI):**
    *   `bench_encode_1MB`: Measure encoding time for a 1MB file.
    *   `bench_encode_10MB`: Measure encoding time for a 10MB file.
    *   `bench_encode_100MB`: Measure encoding time for a 100MB file.
    *   `bench_encode_1GB`: Measure encoding time for a 1GB file.
    *   `bench_decode_1MB`: Measure decoding time for a 1MB file.
    *   `bench_decode_10MB`: Measure decoding time for a 10MB file.
    *   `bench_decode_100MB`: Measure decoding time for a 100MB file.
    *   `bench_decode_1GB`: Measure decoding time for a 1GB file.
    *   **(Optional):** Benchmark encoding/decoding with varying `redundancy_factor` or `symbol_size`.

**7. Reporting**

*   Test results will be logged during execution.
*   A summary report will be generated, highlighting pass/fail status for each test category and any significant findings.
*   Benchmarking results will be tabulated for easy comparison.

**8. Conclusion**

This test plan provides a comprehensive approach to verifying the functionality, reliability, and performance of the `rq-library`. Execution of this plan will build confidence in the library's readiness for use.