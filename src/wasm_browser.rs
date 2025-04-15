//wasm_browser.rs
#[cfg(all(target_arch = "wasm32", feature = "browser-wasm"))]
mod wasm_browser {
    use wasm_bindgen::prelude::*;
    use js_sys::{Uint8Array, Promise, Object};
    use web_sys::{File, Blob};
    use wasm_bindgen_futures::future_to_promise;
    use crate::processor::{ProcessorConfig, RaptorQProcessor, ProcessResult};
    use serde::Serialize;
    use crate::platform::browser;
    use std::sync::Arc;

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
        pub fn new(symbol_size: u16, redundancy_factor: u8, max_memory_mb: u32, concurrency_limit: u32) -> Self {
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

        // Set filesystem access 
        #[wasm_bindgen]
        pub fn set_filesystem(&self, fs: JsValue) {
            browser::set_filesystem(fs.into());
        }

        // Encode a file
        #[wasm_bindgen]
        pub fn encode_file(&self, input_path: String, output_dir: String, block_size: usize) -> Promise {
            let processor = self.processor.clone();

            future_to_promise(async move {
                // Create output directory
                browser::create_dir_all_async(&output_dir).await?;

                // Get file size
                let file_size = browser::file_size_async(&input_path).await?;

                // Calculate actual block size
                let actual_block_size = if block_size == 0 {
                    processor.get_recommended_block_size(file_size)
                } else {
                    block_size
                };

                // Use generic encode_file method instead of browser-specific versions
                let result = processor.encode_file(&input_path, &output_dir, actual_block_size, false)?;

                // Convert result to JS object
                let js_result = Object::new();

                // Extract encoder parameters from the first block if available
                let encoder_params = if let Some(blocks) = &result.blocks {
                    if let Some(first_block) = blocks.first() {
                        let params_array = Uint8Array::new_with_length(first_block.encoder_parameters.len() as u32);
                        params_array.copy_from(&first_block.encoder_parameters);
                        params_array
                    } else {
                        Uint8Array::new_with_length(0)
                    }
                } else {
                    Uint8Array::new_with_length(0)
                };

                // Set properties
                js_sys::Reflect::set(&js_result, &JsValue::from_str("encoderParameters"), &encoder_params)?;
                js_sys::Reflect::set(&js_result, &JsValue::from_str("totalSymbolsCount"), &JsValue::from_f64(result.total_symbols_count as f64))?;
                js_sys::Reflect::set(&js_result, &JsValue::from_str("totalRepairSymbols"), &JsValue::from_f64(result.total_repair_symbols as f64))?;
                js_sys::Reflect::set(&js_result, &JsValue::from_str("symbolsDirectory"), &JsValue::from_str(&result.symbols_directory))?;
                js_sys::Reflect::set(&js_result, &JsValue::from_str("layoutFilePath"), &JsValue::from_str(&result.layout_file_path))?;

                if let Some(blocks) = &result.blocks {
                    let js_blocks = to_value(&blocks)?;
                    js_sys::Reflect::set(&js_result, &JsValue::from_str("blocks"), &js_blocks)?;
                } else {
                    js_sys::Reflect::set(&js_result, &JsValue::from_str("blocks"), &JsValue::null())?;
                }

                Ok(js_result.into())
            })
        }

        // Decode symbols
        #[wasm_bindgen]
        pub fn decode_symbols(&self, symbols_dir: String, output_path: String, layout_path: String) -> Promise {
            let processor = self.processor.clone();

            future_to_promise(async move {
                // Use generic decode_symbols method
                processor.decode_symbols(&symbols_dir, &output_path, &layout_path)?;

                Ok(JsValue::from_bool(true))
            })
        }

        // Get recommended block size
        #[wasm_bindgen]
        pub fn get_recommended_block_size(&self, file_size: f64) -> usize {
            self.processor.get_recommended_block_size(file_size as u64)
        }

        // Get version
        #[wasm_bindgen(static_method_of = RaptorQSession)]
        pub fn version() -> String {
            "RaptorQ Library v0.1.0 (WASM Browser Edition)".to_string()
        }
    }
    
    // Helper function to convert Rust values to JS values
    fn to_value<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
        let serialized = serde_json::to_string(value)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))?;
        let value = js_sys::JSON::parse(&serialized)
            .map_err(|e| JsValue::from_str(&format!("JSON parse error: {:?}", e)))?;
        Ok(value)
    }
}
