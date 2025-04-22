/* RQ Library - C API
 * Generated with cbindgen
 */


#ifndef RQ_LIB_H
#define RQ_LIB_H

/* Generated with cbindgen:0.28.0 */

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
namespace RQLibrary {
#endif  // __cplusplus

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Initializes a RaptorQ session with the given configuration
 * Returns a session ID on success, or 0 on failure
 */
uintptr_t raptorq_init_session(uint16_t symbol_size,
                               uint8_t redundancy_factor,
                               uint64_t max_memory_mb,
                               uint64_t concurrency_limit);

/**
 * Frees a RaptorQ session
 */
bool raptorq_free_session(uintptr_t session_id);

/**
 * Encodes a file using RaptorQ - streaming implementation
 *
 * Arguments:
 * * `session_id` - Session ID returned from raptorq_init_session
 * * `input_path` - Path to the input file
 * * `output_dir` - Directory where symbols will be written
 * * `block_size` - Size of blocks to process at once (0 = auto)
 * * `result_buffer` - Buffer to store the result (JSON metadata)
 * * `result_buffer_len` - Length of the result buffer
 *
 * Returns:
 * *   0 on success
 * *  -1 on generic error
 * *  -2 on invalid parameters
 * *  -3 on invalid response
 * *  -4 on bad return buffer size
 * *  -4 on encoding failure
 * *  -5 on invalid session
 * * -11 on IO error
 * * -12 on File not found
 * * -13 on Invalid path
 * * -14 on Encoding failed
 * * -16 on Memory limit exceeded
 * * -17 on Concurrency limit reached
 */
int32_t raptorq_create_metadata(uintptr_t session_id,
                                const char *input_path,
                                const char *output_dir,
                                uintptr_t block_size,
                                bool return_layout,
                                char *result_buffer,
                                uintptr_t result_buffer_len);

/**
 * Encodes a file using RaptorQ - streaming implementation
 *
 * Arguments:
 * * `session_id` - Session ID returned from raptorq_init_session
 * * `input_path` - Path to the input file
 * * `output_dir` - Directory where symbols will be written
 * * `block_size` - Size of blocks to process at once (0 = auto)
 * * `result_buffer` - Buffer to store the result (JSON metadata)
 * * `result_buffer_len` - Length of the result buffer
 *
 * Returns:
 * *   0 on success
 * *  -1 on generic error
 * *  -2 on invalid parameters
 * *  -3 on invalid response
 * *  -4 on bad return buffer size
 * *  -4 on encoding failure
 * *  -5 on invalid session
 * * -11 on IO error
 * * -12 on File not found
 * * -13 on Invalid Path
 * * -14 on Encoding failed
 * * -16 on Memory limit exceeded
 * * -17 on Concurrency limit reached
 */
int32_t raptorq_encode_file(uintptr_t session_id,
                            const char *input_path,
                            const char *output_dir,
                            uintptr_t block_size,
                            char *result_buffer,
                            uintptr_t result_buffer_len);

/**
 * Gets the last error message from the processor
 *
 * Arguments:
 * * `session_id` - Session ID returned from raptorq_init_session
 * * `error_buffer` - Buffer to store the error message
 * * `error_buffer_len` - Length of the error buffer
 *
 * Returns:
 * * 0 on success
 * * -1 on error
 */
int32_t raptorq_get_last_error(uintptr_t session_id,
                               char *error_buffer,
                               uintptr_t error_buffer_len);

/**
 * Decodes RaptorQ symbols back to the original file
 *
 * Arguments:
 * * `session_id` - Session ID returned from raptorq_init_session
 * * `symbols_dir` - Directory containing the symbols
 * * `output_path` - Path where the decoded file will be written
 * * `layout_path` - Path to the layout file (containing encoder parameters and block information)
 *
 * Returns:
 * *   0 on success
 * *  -1 on generic error
 * *  -2 on invalid parameters
 * *  -3 on invalid response
 * *  -4 on bad return buffer size
 * *  -4 on encoding failure
 * *  -5 on invalid session
 * * -11 on IO error
 * * -12 on File not found
 * * -13 on Invalid Path
 * * -15 on Decoding failed
 * * -16 on Memory limit exceeded
 * * -17 on Concurrency limit reached
 */
int32_t raptorq_decode_symbols(uintptr_t session_id,
                               const char *symbols_dir,
                               const char *output_path,
                               const char *layout_path);

/**
 * Gets a recommended block size based on file size and available memory
 *
 * Arguments:
 * * `session_id` - Session ID returned from raptorq_init_session
 * * `file_size` - Size of the file to process
 *
 * Returns:
 * * Recommended block size in bytes
 * * 0 if it should not block or on error
 */
uintptr_t raptorq_get_recommended_block_size(uintptr_t session_id, uint64_t file_size);

/**
 * Version information
 */
int32_t raptorq_version(char *version_buffer, uintptr_t version_buffer_len);

extern uint32_t js_file_size(const str *path);

extern Promise js_read_chunk(const str *path, uint32_t offset, uint32_t length);

extern Promise js_write_chunk(const str *path, uint32_t offset, const Uint8Array *data);

extern Promise js_flush_file(const str *path);

extern Promise js_create_dir_all(const str *path);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#ifdef __cplusplus
}  // namespace RQLibrary
#endif  // __cplusplus

#endif  /* RQ_LIB_H */
