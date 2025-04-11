mod processor;
mod platform;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use processor::{ProcessorConfig, RaptorQProcessor, ProcessError};
use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

// Global session counter for unique IDs
static SESSION_COUNTER: AtomicUsize = AtomicUsize::new(1);

// Global processor storage
static PROCESSORS: Lazy<Mutex<HashMap<usize, RaptorQProcessor>>> = Lazy::new(|| {
    // Initialize logging
    env_logger::init();
    Mutex::new(HashMap::new())
});

/// Initializes a RaptorQ session with the given configuration
/// Returns a session ID on success, or 0 on failure
#[unsafe(no_mangle)]
pub extern "C" fn raptorq_init_session(
    symbol_size: u16,
    redundancy_factor: u8,
    max_memory_mb: u32,
    concurrency_limit: u32,
) -> usize {
    let session_id = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);

    let config = ProcessorConfig {
        symbol_size,
        redundancy_factor,
        max_memory_mb,
        concurrency_limit,
    };

    let processor = RaptorQProcessor::new(config);

    let mut processors = PROCESSORS.lock();
    processors.insert(session_id, processor);

    session_id
}

/// Frees a RaptorQ session
#[unsafe(no_mangle)]
pub extern "C" fn raptorq_free_session(session_id: usize) -> bool {
    let mut processors = PROCESSORS.lock();
    processors.remove(&session_id).is_some()
}

/// Encodes a file using RaptorQ - streaming implementation
///
/// Arguments:
/// * `session_id` - Session ID returned from raptorq_init_session
/// * `input_path` - Path to the input file
/// * `output_dir` - Directory where symbols will be written
/// * `chunk_size` - Size of chunks to process at once (0 = auto)
/// * `result_buffer` - Buffer to store the result (JSON metadata)
/// * `result_buffer_len` - Length of the result buffer
///
/// Returns:
/// * 0 on success
/// * -1 on generic error
/// * -2 on file not found
/// * -3 on encoding failure
/// * -4 on invalid session
/// * -5 on memory allocation error
#[unsafe(no_mangle)]
pub extern "C" fn raptorq_encode_file(
    session_id: usize,
    input_path: *const c_char,
    output_dir: *const c_char,
    chunk_size: usize,
    result_buffer: *mut c_char,
    result_buffer_len: usize,
) -> i32 {
    // Basic null pointer checks
    if input_path.is_null() || output_dir.is_null() || result_buffer.is_null() {
        return -1;
    }

    let input_path_str = match unsafe { CStr::from_ptr(input_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let output_dir_str = match unsafe { CStr::from_ptr(output_dir) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let processors = PROCESSORS.lock();
    let processor = match processors.get(&session_id) {
        Some(p) => p,
        None => return -4,
    };

    match processor.encode_file_streamed(input_path_str, output_dir_str, chunk_size) {
        Ok(result) => {
            // Serialize result to JSON
            let result_json = match serde_json::to_string(&result) {
                Ok(j) => j,
                Err(_) => return -3,
            };

            // Copy result to result buffer
            let c_result = match CString::new(result_json) {
                Ok(s) => s,
                Err(_) => return -5,
            };

            let result_bytes = c_result.as_bytes_with_nul();
            if result_bytes.len() > result_buffer_len {
                return -5;
            }

            unsafe {
                ptr::copy_nonoverlapping(
                    result_bytes.as_ptr() as *const c_char,
                    result_buffer,
                    result_bytes.len(),
                );
            }

            0
        },
        Err(e) => match e {
            ProcessError::FileNotFound(_) => -2,
            ProcessError::EncodingFailed(_) => -3,
            _ => -1,
        },
    }
}

/// Gets the last error message from the processor
///
/// Arguments:
/// * `session_id` - Session ID returned from raptorq_init_session
/// * `error_buffer` - Buffer to store the error message
/// * `error_buffer_len` - Length of the error buffer
///
/// Returns:
/// * 0 on success
/// * -1 on error
#[unsafe(no_mangle)]
pub extern "C" fn raptorq_get_last_error(
    session_id: usize,
    error_buffer: *mut c_char,
    error_buffer_len: usize,
) -> i32 {
    if error_buffer.is_null() {
        return -1;
    }

    let processors = PROCESSORS.lock();
    let processor = match processors.get(&session_id) {
        Some(p) => p,
        None => return -1,
    };

    let error_msg = processor.get_last_error();
    let c_error = match CString::new(error_msg) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let error_bytes = c_error.as_bytes_with_nul();
    if error_bytes.len() > error_buffer_len {
        // Error message too long, truncate
        unsafe {
            ptr::copy_nonoverlapping(
                error_bytes.as_ptr() as *const c_char,
                error_buffer,
                error_buffer_len - 1,
            );
            *error_buffer.add(error_buffer_len - 1) = 0;
        }
    } else {
        unsafe {
            ptr::copy_nonoverlapping(
                error_bytes.as_ptr() as *const c_char,
                error_buffer,
                error_bytes.len(),
            );
        }
    }

    0
}

/// Decodes RaptorQ symbols back to the original file
///
/// Arguments:
/// * `session_id` - Session ID returned from raptorq_init_session
/// * `symbols_dir` - Directory containing the symbols
/// * `output_path` - Path where the decoded file will be written
/// * `encoder_params` - Encoder parameters (12 bytes)
/// * `encoder_params_len` - Length of encoder parameters
///
/// Returns:
/// * 0 on success
/// * -1 on generic error
/// * -2 on file not found
/// * -3 on decoding failure
/// * -4 on invalid session
#[unsafe(no_mangle)]
pub extern "C" fn raptorq_decode_symbols(
    session_id: usize,
    symbols_dir: *const c_char,
    output_path: *const c_char,
    encoder_params: *const u8,
    encoder_params_len: usize,
) -> i32 {
    // Basic null pointer checks
    if symbols_dir.is_null() || output_path.is_null() || encoder_params.is_null() {
        return -1;
    }

    if encoder_params_len != 12 {
        return -1;
    }

    let symbols_dir_str = match unsafe { CStr::from_ptr(symbols_dir) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let output_path_str = match unsafe { CStr::from_ptr(output_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    // Copy encoder params
    let mut params = [0u8; 12];
    unsafe {
        ptr::copy_nonoverlapping(encoder_params, params.as_mut_ptr(), 12);
    }

    let processors = PROCESSORS.lock();
    let processor = match processors.get(&session_id) {
        Some(p) => p,
        None => return -4,
    };

    match processor.decode_symbols(symbols_dir_str, output_path_str, &params) {
        Ok(_) => 0,
        Err(e) => match e {
            ProcessError::FileNotFound(_) => -2,
            ProcessError::DecodingFailed(_) => -3,
            _ => -1,
        },
    }
}

/// Gets a recommended chunk size based on file size and available memory
///
/// Arguments:
/// * `session_id` - Session ID returned from raptorq_init_session
/// * `file_size` - Size of the file to process
///
/// Returns:
/// * Recommended chunk size in bytes
/// * 0 if it should not chunk or on error
#[unsafe(no_mangle)]
pub extern "C" fn raptorq_get_recommended_chunk_size(
    session_id: usize,
    file_size: u64,
) -> usize {
    let processors = PROCESSORS.lock();
    let processor = match processors.get(&session_id) {
        Some(p) => p,
        None => return 0,
    };

    processor.get_recommended_chunk_size(file_size)
}

/// Version information
#[unsafe(no_mangle)]
pub extern "C" fn raptorq_version(
    version_buffer: *mut c_char,
    version_buffer_len: usize,
) -> i32 {
    if version_buffer.is_null() {
        return -1;
    }

    let version = "RaptorQ Library v0.1.0";
    let c_version = match CString::new(version) {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let version_bytes = c_version.as_bytes_with_nul();
    if version_bytes.len() > version_buffer_len {
        return -1;
    }

    unsafe {
        ptr::copy_nonoverlapping(
            version_bytes.as_ptr() as *const c_char,
            version_buffer,
            version_bytes.len(),
        );
    }

    0
}
