//! WASM/browser implementations of FileReader, FileWriter, and DirManager traits.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use js_sys::Uint8Array;
use wasm_bindgen_futures::JsFuture;
use std::cell::RefCell;

use super::{FileReader, FileWriter, DirManager};

/// JS glue for browser file I/O (see browser_fs.js)
#[wasm_bindgen(module = "/js/browser_fs.js")]
extern "C" {
    #[wasm_bindgen(js_name = getFileSize)]
    fn js_file_size(path: &str) -> u32;

    #[wasm_bindgen(js_name = readFileChunk)]
    fn js_read_chunk(path: &str, offset: u32, length: u32) -> js_sys::Promise;

    #[wasm_bindgen(js_name = writeFileChunk)]
    fn js_write_chunk(path: &str, offset: u32, data: &Uint8Array) -> js_sys::Promise;

    #[wasm_bindgen(js_name = flushFile)]
    fn js_flush_file(path: &str) -> js_sys::Promise;

    #[wasm_bindgen(js_name = createDirAll)]
    fn js_create_dir_all(path: &str) -> js_sys::Promise;

    #[wasm_bindgen(js_name = syncDirExists)]
    fn js_dir_exists(path: &str) -> bool;
}

// Import console.log from web_sys instead of directly binding it
use web_sys::console;

// Additional JS bindings for filesystem access
#[wasm_bindgen]
extern "C" {
    type FileSystem;
    
    #[wasm_bindgen(js_namespace = window, js_name = FileSystem, thread_local_v2)]
    static FILE_SYSTEM: JsValue;
}

// Global filesystem access would be provided by the host
thread_local! {
    static FS: RefCell<Option<FileSystem>> = RefCell::new(None);
}

// Helper to log directly to JS console
pub fn log_to_console(msg: &str) {
    console::log_1(&JsValue::from_str(&format!("[RUST]: {}", msg)));
}

/// Browser implementation of FileReader.
pub struct BrowserFileReader {
    path: String,
}

impl BrowserFileReader {
    pub fn new(path: &str) -> Self {
        Self { path: path.to_string() }
    }
}

impl FileReader for BrowserFileReader {
    fn file_size(&self) -> Result<u64, String> {
        Ok(js_file_size(&self.path) as u64)
    }

    fn read_chunk(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, String> {
        // We need to use wasm_bindgen_futures::spawn_local for async operations in WASM
        // But since this function is synchronous, we'll need to use a different approach
        
        // Get a promise for the chunk read operation
        let promise = js_read_chunk(&self.path, offset as u32, buf.len() as u32);
        
        // Convert to a JsFuture for easier handling
        // Note: We're not using this future directly as we're using synchronous JS functions instead
        let _future = JsFuture::from(promise);
        
        // In WASM, we can't use block_on, so we need to make this operation synchronous
        // using a workaround. This is a simplified implementation for demonstration.
        // In a real-world application, you'd want to refactor to use async/await patterns.
        
        // This is a synchronous call to JavaScript, which works in WASM context
        let result = js_sys::Reflect::get(
            &js_sys::global(),
            &JsValue::from_str("syncReadChunk")
        ).map_err(|e| format!("Failed to get syncReadChunk: {:?}", e))?;
        
        let sync_read = result.dyn_ref::<js_sys::Function>()
            .ok_or_else(|| "syncReadChunk is not a function".to_string())?;
        
        let result = sync_read.call3(
            &JsValue::NULL,
            &JsValue::from_str(&self.path),
            &JsValue::from_f64(offset as f64),
            &JsValue::from_f64(buf.len() as f64),
        ).map_err(|e| format!("JS error: {:?}", e))?;
        
        let bytes = result.dyn_into::<Uint8Array>()
            .map_err(|e| format!("Conversion error: {:?}", e))?;
        
        let bytes_read = bytes.length() as usize;
        if bytes_read > 0 {
            // Copy data from JS array to Rust buffer
            let buf_len = buf.len(); // Store length to avoid double borrow
            if bytes_read <= buf_len {
                bytes.copy_to(&mut buf[0..bytes_read]);
                Ok(bytes_read)
            } else {
                // This should not happen if JS side respects the length we pass
                bytes.copy_to(&mut buf[0..buf_len]);
                Ok(buf_len)
            }
        } else {
            // No bytes read (EOF)
            Ok(0)
        }
    }
}

/// Browser implementation of FileWriter.
pub struct BrowserFileWriter {
    path: String,
}

impl BrowserFileWriter {
    pub fn new(path: &str) -> Self {
        Self { path: path.to_string() }
    }
}

impl FileWriter for BrowserFileWriter {
    fn write_chunk(&mut self, offset: usize, data: &[u8]) -> Result<(), String> {
        let array = Uint8Array::from(data);
        
        // Similar to read_chunk, we need a synchronous approach
        let result = js_sys::Reflect::get(
            &js_sys::global(),
            &JsValue::from_str("syncWriteChunk")
        ).map_err(|e| format!("Failed to get syncWriteChunk: {:?}", e))?;
        
        let sync_write = result.dyn_ref::<js_sys::Function>()
            .ok_or_else(|| "syncWriteChunk is not a function".to_string())?;
        
        sync_write.call3(
            &JsValue::NULL,
            &JsValue::from_str(&self.path),
            &JsValue::from_f64(offset as f64),
            &array,
        ).map_err(|e| format!("JS error: {:?}", e))?;
        
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        // Similar to other operations, we need a synchronous approach
        let result = js_sys::Reflect::get(
            &js_sys::global(),
            &JsValue::from_str("syncFlushFile")
        ).map_err(|e| format!("Failed to get syncFlushFile: {:?}", e))?;
        
        let sync_flush = result.dyn_ref::<js_sys::Function>()
            .ok_or_else(|| "syncFlushFile is not a function".to_string())?;
        
        sync_flush.call1(
            &JsValue::NULL,
            &JsValue::from_str(&self.path),
        ).map_err(|e| format!("JS error: {:?}", e))?;
        
        Ok(())
    }
}

/// Browser implementation of DirManager.
pub struct BrowserDirManager;

impl DirManager for BrowserDirManager {
    fn create_dir_all(&self, path: &str) -> Result<(), String> {
        // Similar to other operations, we need a synchronous approach
        let result = js_sys::Reflect::get(
            &js_sys::global(),
            &JsValue::from_str("syncCreateDirAll")
        ).map_err(|e| format!("Failed to get syncCreateDirAll: {:?}", e))?;
        
        let sync_create_dir = result.dyn_ref::<js_sys::Function>()
            .ok_or_else(|| "syncCreateDirAll is not a function".to_string())?;
        
        sync_create_dir.call1(
            &JsValue::NULL,
            &JsValue::from_str(path),
        ).map_err(|e| format!("JS error: {:?}", e))?;
        
        Ok(())
    }

    fn dir_exists(&self, path: &str) -> Result<bool, String> {
        // Call the JS function to check directory existence
        Ok(js_dir_exists(path))
    }

    fn count_files(&self, path: &str) -> Result<usize, String> {
        Ok(0) // Placeholder, as counting files in a directory is not implemented
    }
}