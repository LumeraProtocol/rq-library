//! Native (std) implementations of FileReader, FileWriter, and DirManager traits.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::{FileReader, FileWriter, DirManager};

/// Native implementation of FileReader using std::fs::File.
pub struct NativeFileReader {
    file: File,
    size: u64,
}

impl NativeFileReader {
    pub fn open(path: &str) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        Ok(Self { file, size })
    }
}

impl FileReader for NativeFileReader {
    fn file_size(&self) -> Result<u64, String> {
        Ok(self.size)
    }

    fn read_chunk(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, String> {
        self.file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        let bytes_read = self.file.read(buf).map_err(|e| e.to_string())?;
        Ok(bytes_read)
    }
}

/// Native implementation of FileWriter using std::fs::File.
pub struct NativeFileWriter {
    file: File,
}

impl NativeFileWriter {
    pub fn create(path: &str) -> Result<Self, String> {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        Ok(Self { file })
    }
}

impl FileWriter for NativeFileWriter {
    fn write_chunk(&mut self, offset: usize, data: &[u8]) -> Result<(), String> {
        self.file.seek(SeekFrom::Start(offset as u64)).map_err(|e| e.to_string())?;
        self.file.write_all(data).map_err(|e| e.to_string())
    }

    fn flush(&mut self) -> Result<(), String> {
        self.file.flush().map_err(|e| e.to_string())
    }
}

/// Native implementation of DirManager using std::fs::create_dir_all.
pub struct NativeDirManager;

impl DirManager for NativeDirManager {
    fn create_dir_all(&self, path: &str) -> Result<(), String> {
        std::fs::create_dir_all(Path::new(path)).map_err(|e| e.to_string())
    }

    fn dir_exists(&self, path: &str) -> Result<bool, String> {
        Ok(std::fs::metadata(path)
            .map(|m| m.is_dir())
            .unwrap_or(false))
    }

    fn count_files(&self, path: &str) -> Result<usize, String> {
        let dir = Path::new(path);
        let mut count = 0;
        let entries = std::fs::read_dir(dir)
            .map_err(|e| format!("Failed to read directory: {}", e))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("Failed to access directory entry: {}", e))?;
            if entry.path().is_file() {
                count += 1;
            }
        }
        Ok(count)
    }
}