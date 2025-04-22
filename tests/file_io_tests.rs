//! Tests for the unified FileReader and FileWriter traits (native implementation).

use std::fs::remove_file;
use std::io::Write;
use tempfile::NamedTempFile;

use rq_library::file_io::{NativeFileReader, NativeFileWriter, NativeDirManager, FileReader, FileWriter, DirManager};

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
    let reader = NativeFileReader::open(&path).unwrap();
    assert_eq!(reader.file_size().unwrap(), data.len() as u64);
    remove_file(&path).unwrap();
}

#[test]
fn test_chunked_reading() {
    let data = b"abcdefghijklmnopqrstuvwxyz";
    let path = write_test_file(data);
    let mut reader = NativeFileReader::open(&path).unwrap();
    
    // Read first chunk
    let mut buf1 = [0u8; 5];
    let bytes_read1 = reader.read_chunk(0, &mut buf1).unwrap();
    assert_eq!(bytes_read1, 5);
    assert_eq!(&buf1, b"abcde");
    
    // Read second chunk
    let mut buf2 = [0u8; 5];
    let bytes_read2 = reader.read_chunk(5, &mut buf2).unwrap();
    assert_eq!(bytes_read2, 5);
    assert_eq!(&buf2, b"fghij");
    
    // Read last chunk (partial)
    let mut buf3 = [0u8; 1];
    let bytes_read3 = reader.read_chunk(25, &mut buf3).unwrap();
    assert_eq!(bytes_read3, 1);
    assert_eq!(&buf3, b"z");
    
    remove_file(&path).unwrap();
}

#[test]
fn test_read_chunk_eof() {
    let data = b"12345";
    let path = write_test_file(data);
    let mut reader = NativeFileReader::open(&path).unwrap();
    
    // Read past EOF
    let mut buf = [0u8; 10];
    let bytes_read = reader.read_chunk(10, &mut buf).unwrap();
    assert_eq!(bytes_read, 0);
    
    remove_file(&path).unwrap();
}

#[test]
fn test_file_writer_and_flush() {
    let data = b"write test";
    let path = write_test_file(b"");
    {
        let mut writer = NativeFileWriter::create(&path).unwrap();
        writer.write_chunk(0, data).unwrap();
        writer.flush().unwrap();
    }
    
    let mut reader = NativeFileReader::open(&path).unwrap();
    let mut buf = vec![0u8; data.len()];
    let bytes_read = reader.read_chunk(0, &mut buf).unwrap();
    
    assert_eq!(bytes_read, data.len());
    assert_eq!(&buf, data);
    
    remove_file(&path).unwrap();
}

#[test]
fn test_dir_manager_create() {
    let dir_manager = NativeDirManager;
    let tmp_dir = tempfile::tempdir().unwrap();
    let nested = tmp_dir.path().join("a/b/c");
    let nested_str = nested.to_string_lossy();
    dir_manager.create_dir_all(&nested_str).unwrap();
    assert!(nested.exists());
}