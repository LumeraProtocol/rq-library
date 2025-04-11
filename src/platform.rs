// platform.rs
#[cfg(all(target_arch = "wasm32", feature = "browser-wasm"))]
pub mod browser {
    use wasm_bindgen::prelude::*;
    use js_sys::{Array, Uint8Array, Promise};
    use web_sys::{File, Blob, FileReader};
    use wasm_bindgen_futures::JsFuture;
    use std::io;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console)]
        fn log(s: &str);

        type FileSystem;
        #[wasm_bindgen(method, js_name = readFile)]
        fn read_file(this: &FileSystem, path: &str) -> Promise;

        #[wasm_bindgen(method, js_name = writeFile)]
        fn write_file(this: &FileSystem, path: &str, data: &Uint8Array) -> Promise;

        #[wasm_bindgen(method, js_name = mkdir)]
        fn mkdir(this: &FileSystem, path: &str) -> Promise;

        #[wasm_bindgen(method, js_name = stat)]
        fn stat(this: &FileSystem, path: &str) -> Promise;
    }

    // Global filesystem access would be provided by the host
    thread_local! {
        static FS: Option<FileSystem> = None;
    }

    pub async fn read_file_async(path: &str) -> Result<Vec<u8>, JsValue> {
        let result = FS.with(|fs| {
            match fs {
                Some(fs) => JsFuture::from(fs.read_file(path)),
                None => Err(JsValue::from_str("FileSystem not initialized")).into(),
            }
        })?;

        let array = Uint8Array::new(&result);
        let mut vec = vec![0; array.length() as usize];
        array.copy_to(&mut vec);
        Ok(vec)
    }

    pub async fn write_file_async(path: &str, data: &[u8]) -> Result<(), JsValue> {
        let array = Uint8Array::new_with_length(data.len() as u32);
        array.copy_from(data);

        FS.with(|fs| {
            match fs {
                Some(fs) => JsFuture::from(fs.write_file(path, &array)),
                None => Err(JsValue::from_str("FileSystem not initialized")).into(),
            }
        })?;

        Ok(())
    }

    pub async fn create_dir_all_async(path: &str) -> Result<(), JsValue> {
        FS.with(|fs| {
            match fs {
                Some(fs) => JsFuture::from(fs.mkdir(path)),
                None => Err(JsValue::from_str("FileSystem not initialized")).into(),
            }
        })?;

        Ok(())
    }

    pub async fn file_size_async(path: &str) -> Result<u64, JsValue> {
        let result = FS.with(|fs| {
            match fs {
                Some(fs) => JsFuture::from(fs.stat(path)),
                None => Err(JsValue::from_str("FileSystem not initialized")).into(),
            }
        })?;

        // The result is a JS object with a size property
        let size = js_sys::Reflect::get(&result, &JsValue::from_str("size"))?;
        Ok(size.as_f64().unwrap_or(0.0) as u64)
    }

    // Function to set the file system
    pub fn set_filesystem(fs: FileSystem) {
        FS.with(|cell| {
            *cell.borrow_mut() = Some(fs);
        });
    }
}