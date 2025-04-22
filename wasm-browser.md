# 🧾 Problem Description

The core problem is enabling a **WebAssembly (WASM)** build of a Rust-based application to process large input files in the browser **without exhausting memory** or **copying entire files** into JavaScript memory space.

The Rust application uses the [`raptorq`](https://crates.io/crates/raptorq) crate to encode large files (potentially >1GB) into erasure-coded symbols. In native environments, it reads input data from disk using `std::fs::File`, splits the file into **blocks**, and processes each block in sequence to avoid memory overload. The **layout file** (a side output describing all blocks and their symbol metadata) requires knowledge of all encoded blocks.

However, in the **browser**:
- `std::fs::File` and `Path::exists()` are unavailable.
- Reading entire files into memory via JavaScript leads to **poor UX and potential crashes**.
- File APIs in browsers are **asynchronous**, whereas Rust code (including `Encoder::new(&[u8])`) is **synchronous**.
- The WASM build must **not require changes** to the core encoder logic shared with native platforms.

Therefore, we need a solution that:
- Preserves native `std::fs` behavior.
- Enables chunked streaming reads in the browser.
- Avoids copying full files into memory.
- Maintains a consistent interface across platforms.

---

# ✅ Proposed Solution: Trait-Based Platform Abstraction

We define a `FileReader` trait with platform-specific implementations. The Rust encoder logic is written against this trait. This enables **native** and **browser** targets to share identical processing logic while hiding the I/O details.

## 1. Trait Definition

```rust
pub trait FileReader {
    fn file_size(&self) -> Result<usize, String>;
    fn read_chunk(&self, offset: usize, length: usize) -> Result<Vec<u8>, String>;
}
```

---

## 2. Native Implementation

```rust
pub struct NativeFileReader {
    file: std::fs::File,
    size: usize,
}

impl NativeFileReader {
    pub fn open(path: &str) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let size = file.metadata().map_err(|e| e.to_string())?.len() as usize;
        Ok(Self { file, size })
    }
}

impl FileReader for NativeFileReader {
    fn file_size(&self) -> Result<usize, String> {
        Ok(self.size)
    }

    fn read_chunk(&self, offset: usize, length: usize) -> Result<Vec<u8>, String> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = &self.file;
        file.seek(SeekFrom::Start(offset as u64)).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; length];
        file.read_exact(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    }
}
```

---

## 3. Browser Implementation (via `wasm-bindgen`)

```rust
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use js_sys::Uint8Array;

#[wasm_bindgen(module = "/js/browser_fs.js")]
extern "C" {
    #[wasm_bindgen(js_name = readFileChunk)]
    async fn js_read_chunk(path: &str, offset: u32, length: u32) -> Uint8Array;

    #[wasm_bindgen(js_name = getFileSize)]
    fn js_file_size(path: &str) -> u32;
}

pub struct BrowserFileReader {
    path: String,
}

impl BrowserFileReader {
    pub fn new(path: &str) -> Self {
        Self { path: path.to_string() }
    }
}

impl FileReader for BrowserFileReader {
    fn file_size(&self) -> Result<usize, String> {
        Ok(js_file_size(&self.path) as usize)
    }

    fn read_chunk(&self, offset: usize, length: usize) -> Result<Vec<u8>, String> {
        // Use `futures::executor::block_on` only if you're outside async context
        let promise = js_read_chunk(&self.path, offset as u32, length as u32);
        let future = wasm_bindgen_futures::JsFuture::from(promise);
        let bytes = wasm_bindgen_futures::block_on(future)
            .map_err(|e| format!("JS error: {:?}", e))?
            .dyn_into::<Uint8Array>()
            .map_err(|e| format!("Conversion error: {:?}", e))?;
        Ok(bytes.to_vec())
    }
}
```

---

## 4. JS Glue Code (`browser_fs.js`)

```js
window.vfs = {};

window.registerFile = function (path, file) {
    window.vfs[path] = file;
};

export function getFileSize(path) {
    const file = window.vfs[path];
    if (!file) throw new Error("File not found: " + path);
    return file.size;
}

export async function readFileChunk(path, offset, length) {
    const file = window.vfs[path];
    if (!file) throw new Error("File not found: " + path);
    const blob = file.slice(offset, offset + length);
    const arrayBuffer = await blob.arrayBuffer();
    return new Uint8Array(arrayBuffer);
}
```

---

## 5. Main Encoder Code (Platform-Independent)

```rust
pub fn encode_file<F: FileReader>(
    reader: &F,
    block_size: usize,
    repair_symbols: usize,
) -> Result<Layout, String> {
    let total_size = reader.file_size()?;
    let mut layout = Layout::new();

    let mut offset = 0;
    let mut block_index = 0;

    while offset < total_size {
        let chunk_len = std::cmp::min(block_size, total_size - offset);
        let data = reader.read_chunk(offset, chunk_len)?;
        let hash = get_hash_as_b58(&data);

        let encoder = Encoder::new(&data, RaptorQConfig::default());
        let symbols = encoder.get_encoded_packets(repair_symbols as u32);

        layout.add_block(block_index, offset, hash, symbols);
        offset += chunk_len;
        block_index += 1;
    }

    Ok(layout)
}
```

---

## ✅ Benefits of This Approach

- ✅ No memory explosion in the browser
- ✅ WASM memory is not overused — only one block at a time
- ✅ No need to change `raptorq` usage patterns
- ✅ Same Rust code works for both browser and native
- ✅ Clean abstraction with `FileReader` trait
- ✅ Compatible with multi-agent pipelines
