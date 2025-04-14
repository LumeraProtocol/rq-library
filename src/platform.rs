// platform.rs
#[cfg(all(target_arch = "wasm32", feature = "browser-wasm"))]
pub mod browser {
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;
    use js_sys::{Uint8Array, Promise};
    use wasm_bindgen_futures::JsFuture;

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
        static FS: RefCell<Option<FileSystem>> = RefCell::new(None);
    }

    #[allow(dead_code)] // Called via JS or specific setup
    pub async fn read_file_async(path: &str) -> Result<Vec<u8>, JsValue> {
        let future = FS.with(|cell| {
            let fs_opt_ref = cell.borrow();
            match *fs_opt_ref {
                Some(ref fs) => Ok(JsFuture::from(fs.read_file(path))),
                None => Err(JsValue::from_str("FileSystem not initialized")),
            }
        })?;
        let result = future.await?;

        let array = Uint8Array::new(&result);
        let mut vec = vec![0; array.length() as usize];
        array.copy_to(&mut vec);
        Ok(vec)
    }

    #[allow(dead_code)] // Called via JS or specific setup
    pub async fn write_file_async(path: &str, data: &[u8]) -> Result<(), JsValue> {
        let array = Uint8Array::new_with_length(data.len() as u32);
        array.copy_from(data);

        let future = FS.with(|cell| {
            let fs_opt_ref = cell.borrow();
            match *fs_opt_ref {
                Some(ref fs) => Ok(JsFuture::from(fs.write_file(path, &array))),
                None => Err(JsValue::from_str("FileSystem not initialized")),
            }
        })?;
        future.await?;

        Ok(())
    }

    #[allow(dead_code)] // Called via JS or specific setup
    pub async fn create_dir_all_async(path: &str) -> Result<(), JsValue> {
        let future = FS.with(|cell| {
            let fs_opt_ref = cell.borrow();
            match *fs_opt_ref {
                Some(ref fs) => Ok(JsFuture::from(fs.mkdir(path))),
                None => Err(JsValue::from_str("FileSystem not initialized")),
            }
        })?;
        future.await?;

        Ok(())
    }

    #[allow(dead_code)] // Called via JS or specific setup
    pub async fn file_size_async(path: &str) -> Result<u64, JsValue> {
        let future = FS.with(|cell| {
            let fs_opt_ref = cell.borrow();
            match *fs_opt_ref {
                Some(ref fs) => Ok(JsFuture::from(fs.stat(path))),
                None => Err(JsValue::from_str("FileSystem not initialized")),
            }
        })?;
        let result = future.await?;

        // The result is a JS object with a size property
        let size = js_sys::Reflect::get(&result, &JsValue::from_str("size"))?;
        Ok(size.as_f64().unwrap_or(0.0) as u64)
    }

    // Function to set the file system
    #[allow(dead_code)] // Called via JS or specific setup
    fn set_filesystem(fs: FileSystem) { // Make private to the module
        FS.with(|cell| {
            *cell.borrow_mut() = Some(fs); // Now cell is &RefCell<Option<FileSystem>>
        });
    }
}