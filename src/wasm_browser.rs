//wasm_browser.rs
//! Public browser‑WASM API re‑exported to JavaScript.
#[cfg(all(target_arch = "wasm32", feature = "browser-wasm"))]
pub mod browser_wasm {
    use wasm_bindgen::prelude::*;
    use js_sys::{Uint8Array, Promise, Object};
    use wasm_bindgen_futures::future_to_promise;
    use crate::processor::{ProcessorConfig, RaptorQProcessor};
    use serde::Serialize;
    use std::sync::Arc;

    // Import the new file I/O abstractions
    use crate::file_io::{get_dir_manager, open_file_reader};
    
    // Import helpers from file_io/wasm module
    use crate::file_io::wasm::{
        log_to_console,
    };

    // Initialize panic hook for better error messages
    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
    }

    // RaptorQ Session for browser
    #[wasm_bindgen]
    pub struct RaptorQSession {
        processor: Arc<RaptorQProcessor>,
    }

    #[wasm_bindgen]
    impl RaptorQSession {
        // Create a new session
        #[wasm_bindgen(constructor)]
        pub fn new(symbol_size: u16, redundancy_factor: u8, max_memory_mb: u64, concurrency_limit: u64) -> Self {
            let config = ProcessorConfig {
                symbol_size,
                redundancy_factor,
                max_memory_mb,
                concurrency_limit,
            };

            let processor = RaptorQProcessor::new(config);

            Self {
                processor: Arc::new(processor),
            }
        }

        // Create metadata (layout) without generating symbols
        #[wasm_bindgen]
        pub fn create_metadata(&self, input_path: String, output_dir: String, block_size: usize, return_layout: bool) -> Promise {
            let processor = self.processor.clone();

            future_to_promise(async move {
                log_to_console(&format!("create_metadata called with input_path='{}', output_dir='{}', block_size={}, return_layout={}",
                    input_path, output_dir, block_size, return_layout));
                    
                // Create output directory using the new DirManager trait
                let dir_manager = get_dir_manager();
                dir_manager.create_dir_all(&output_dir)
                    .map_err(|e| JsValue::from_str(&format!("Failed to create directory: {}", e)))?;

                // Get file size using the new FileReader trait
                let reader = open_file_reader(&input_path)
                    .map_err(|e| JsValue::from_str(&format!("Failed to open file: {}", e)))?;
                let file_size = reader.file_size()
                    .map_err(|e| JsValue::from_str(&format!("Failed to get file size: {}", e)))?;

                // Calculate actual block size
                let actual_block_size = if block_size == 0 { processor.get_recommended_block_size(file_size as usize) } else { block_size };

                // Call the new create_metadata method
                let result = processor
                    .create_metadata(&input_path, &output_dir, actual_block_size, return_layout)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                Ok(build_result_object(&result)?)
            })
        }

        // Encode a file
        #[wasm_bindgen]
        pub fn encode_file(&self, input_path: String, output_dir: String, block_size: usize) -> Promise {
            let processor = self.processor.clone();

            future_to_promise(async move {
                // Create output directory using the new DirManager trait
                let dir_manager = get_dir_manager();
                dir_manager.create_dir_all(&output_dir)
                    .map_err(|e| JsValue::from_str(&format!("Failed to create directory: {}", e)))?;

                // Get file size using the new FileReader trait
                let reader = open_file_reader(&input_path)
                    .map_err(|e| JsValue::from_str(&format!("Failed to open file: {}", e)))?;
                let file_size = reader.file_size()
                    .map_err(|e| JsValue::from_str(&format!("Failed to get file size: {}", e)))?;

                // Calculate actual block size
                let actual_block_size = if block_size == 0 { processor.get_recommended_block_size(file_size as usize) } else { block_size };

                // Use generic encode_file method instead of browser-specific versions
                let result = processor
                    .encode_file(&input_path, &output_dir, actual_block_size, false)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;

                Ok(build_result_object(&result)?)
            })
        }

        // Decode symbols
        #[wasm_bindgen]
        pub fn decode_symbols(&self, symbols_dir: String, output_path: String, layout_path: String) -> Promise {
            let processor = self.processor.clone();

            future_to_promise(async move {
                // Use generic decode_symbols method
                match processor.decode_symbols(&symbols_dir, &output_path, &layout_path) {
                    Ok(_) => {},
                    Err(e) => return Err(JsValue::from_str(&format!("Error decoding symbols: {}", e))),
                };

                Ok(JsValue::from_bool(true))
            })
        }

        // Get recommended block size
        #[wasm_bindgen]
        pub fn get_recommended_block_size(&self, file_size: f64) -> usize {
            self.processor.get_recommended_block_size(file_size as usize)
        }

        // Get version
        #[wasm_bindgen]
        pub fn version() -> String {
            "RaptorQ Library v0.1.0 (WASM Browser Edition)".to_string()
        }
    }
    
    // helpers --------------------------------------------------------------
    use crate::processor::ProcessResult;

    // Helper function to convert Rust values to JS values
    fn to_value<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
        let serialized = serde_json::to_string(value)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))?;
        let value = js_sys::JSON::parse(&serialized)
            .map_err(|e| JsValue::from_str(&format!("JSON parse error: {:?}", e)))?;
        Ok(value)
    }

    fn build_result_object(res: &ProcessResult) -> Result<JsValue, JsValue> {
        let js = Object::new();

        let params = if let Some(blocks) = &res.blocks {
            blocks.first().map(|b| {
                let a = Uint8Array::new_with_length(b.encoder_parameters.len() as u32);
                a.copy_from(&b.encoder_parameters);
                a
            }).unwrap_or_else(|| Uint8Array::new_with_length(0))
        } else { Uint8Array::new_with_length(0) };

        js_sys::Reflect::set(&js, &JsValue::from_str("encoderParameters"), &params)?;
        js_sys::Reflect::set(&js, &JsValue::from_str("totalSymbolsCount"), &JsValue::from_f64(res.total_symbols_count as f64))?;
        js_sys::Reflect::set(&js, &JsValue::from_str("totalRepairSymbols"), &JsValue::from_f64(res.total_repair_symbols as f64))?;
        js_sys::Reflect::set(&js, &JsValue::from_str("symbolsDirectory"), &JsValue::from_str(&res.symbols_directory))?;
        js_sys::Reflect::set(&js, &JsValue::from_str("layoutFilePath"), &JsValue::from_str(&res.layout_file_path))?;

        if let Some(layout) = &res.layout_content {
            js_sys::Reflect::set(&js, &JsValue::from_str("layoutContent"), &JsValue::from_str(layout))?;
        }
        if let Some(blocks) = &res.blocks {
            js_sys::Reflect::set(&js, &JsValue::from_str("blocks"), &to_value(blocks)?)?;
        }

        Ok(js.into())
    }
}
