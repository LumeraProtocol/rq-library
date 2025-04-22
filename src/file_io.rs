//! Unified File I/O Traits for Cross-Platform (Native & WASM) Support
//!
//! This module defines the core traits for platform-abstracted file and directory I/O:
//! - `FileReader`: For efficient, chunked file reading
//! - `FileWriter`: For efficient, chunked file writing
//! - `DirManager`: For directory creation
//!
//! Implementations are provided in platform-specific modules.
#[cfg(all(not(target_arch = "wasm32"), not(feature = "browser-wasm")))]
pub mod native;
#[cfg(all(not(target_arch = "wasm32"), not(feature = "browser-wasm")))]
pub use native::*;

#[cfg(all(target_arch = "wasm32", feature = "browser-wasm"))]
pub mod wasm;
#[cfg(all(target_arch = "wasm32", feature = "browser-wasm"))]
pub use wasm::*;


/// Trait for platform-abstracted, memory-efficient file reading.
pub trait FileReader {
    /// Returns the total size of the file in bytes.
    fn file_size(&self) -> Result<u64, String>;

    /// Reads a chunk of bytes from the file at the given offset.
    /// Returns the number of bytes read. If 0, EOF has been reached.
    fn read_chunk(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, String>;
}

/// Trait for platform-abstracted, memory-efficient file writing.
pub trait FileWriter {
    /// Writes a chunk of bytes to the file at the given offset.
    fn write_chunk(&mut self, offset: usize, data: &[u8]) -> Result<(), String>;

    /// Flushes any buffered data to the file (optional for buffered writers).
    fn flush(&mut self) -> Result<(), String>;
}

/// Trait for platform-abstracted directory creation.
pub trait DirManager {
    /// Recursively creates a directory and all required parent directories.
    fn create_dir_all(&self, path: &str) -> Result<(), String>;
}

/// Opens a platform-appropriate file reader.
/// 
/// On native platforms, uses std::fs::File.
/// On WASM/browser, uses the JavaScript file system API.
#[cfg(all(not(target_arch = "wasm32"), not(feature = "browser-wasm")))]
pub fn open_file_reader(path: &str) -> Result<Box<dyn FileReader>, String> {
    Ok(Box::new(native::NativeFileReader::open(path)?))
}

/// Opens a platform-appropriate file reader.
/// 
/// On native platforms, uses std::fs::File.
/// On WASM/browser, uses the JavaScript file system API.
#[cfg(all(target_arch = "wasm32", feature = "browser-wasm"))]
pub fn open_file_reader(path: &str) -> Result<Box<dyn FileReader>, String> {
    Ok(Box::new(wasm::BrowserFileReader::new(path)))
}

/// Opens a platform-appropriate file writer.
/// 
/// On native platforms, uses std::fs::File.
/// On WASM/browser, uses the JavaScript file system API.
#[cfg(all(not(target_arch = "wasm32"), not(feature = "browser-wasm")))]
pub fn open_file_writer(path: &str) -> Result<Box<dyn FileWriter>, String> {
    Ok(Box::new(native::NativeFileWriter::create(path)?))
}

/// Opens a platform-appropriate file writer.
/// 
/// On native platforms, uses std::fs::File.
/// On WASM/browser, uses the JavaScript file system API.
#[cfg(all(target_arch = "wasm32", feature = "browser-wasm"))]
pub fn open_file_writer(path: &str) -> Result<Box<dyn FileWriter>, String> {
    Ok(Box::new(wasm::BrowserFileWriter::new(path)))
}

/// Creates a platform-appropriate directory manager.
/// 
/// On native platforms, uses std::fs.
/// On WASM/browser, uses the JavaScript file system API.
#[cfg(all(not(target_arch = "wasm32"), not(feature = "browser-wasm")))]
pub fn get_dir_manager() -> Box<dyn DirManager> {
    Box::new(native::NativeDirManager)
}

/// Creates a platform-appropriate directory manager.
/// 
/// On native platforms, uses std::fs.
/// On WASM/browser, uses the JavaScript file system API.
#[cfg(all(target_arch = "wasm32", feature = "browser-wasm"))]
pub fn get_dir_manager() -> Box<dyn DirManager> {
    Box::new(wasm::BrowserDirManager)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::fs::remove_file;
    use tempfile::NamedTempFile;

    fn write_test_file(contents: &[u8]) -> String {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(contents).unwrap();
        let path = file.path().to_string_lossy().to_string();
        // Keep file alive until end of test
        std::mem::forget(file);
        path
    }

    #[test]
    fn test_file_size() {
        let data = b"hello world!";
        let path = write_test_file(data);
        let reader = open_file_reader(&path).unwrap();
        assert_eq!(reader.file_size().unwrap(), data.len() as u64);
        remove_file(&path).unwrap();
    }

    #[test]
    fn test_chunked_reading() {
        let data = b"abcdefghijklmnopqrstuvwxyz";
        let path = write_test_file(data);
        let mut reader = open_file_reader(&path).unwrap();
        let mut buf = [0u8; 5];
        // Read first chunk
        let n = reader.read_chunk(0, &mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf, b"abcde");
        // Read second chunk
        let n = reader.read_chunk(5, &mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf, b"fghij");
        // Read last chunk (partial)
        let n = reader.read_chunk(25, &mut buf).unwrap();
        assert_eq!(n, 1);
        assert_eq!(&buf[..1], b"z");
        remove_file(&path).unwrap();
    }

    #[test]
    fn test_read_chunk_eof() {
        let data = b"12345";
        let path = write_test_file(data);
        let mut reader = open_file_reader(&path).unwrap();
        let mut buf = [0u8; 10];
        // Read past EOF
        let n = reader.read_chunk(10, &mut buf).unwrap();
        assert_eq!(n, 0);
        remove_file(&path).unwrap();
    }

    #[test]
    fn test_trait_object_usage() {
        let data = b"trait object test";
        let path = write_test_file(data);
        let mut reader: Box<dyn FileReader> = open_file_reader(&path).unwrap();
        let mut buf = [0u8; 5];
        let n = reader.read_chunk(0, &mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf, b"trait");
        assert_eq!(reader.file_size().unwrap(), data.len() as u64);
        remove_file(&path).unwrap();
    }
}
