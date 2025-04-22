use std::path::Path;
use std::{fs, io};
use tempfile::TempDir;
use rq_library::file_io::open_file_reader;

// Test helper to create a test file of a given size
fn create_test_file(dir: &Path, name: &str, size_bytes: usize) -> io::Result<String> {
    let path = dir.join(name);
    let mut file = fs::File::create(&path)?;
    
    // Generate deterministic pattern for verification
    let chunk_size = 1024;
    let mut buffer = vec![0u8; chunk_size];
    
    let mut bytes_written = 0;
    while bytes_written < size_bytes {
        // Fill buffer with a simple pattern
        for i in 0..chunk_size {
            buffer[i] = ((bytes_written + i) % 256) as u8;
        }
        
        let to_write = std::cmp::min(chunk_size, size_bytes - bytes_written);
        io::copy(&mut &buffer[0..to_write], &mut file)?;
        bytes_written += to_write;
    }
    
    Ok(path.to_string_lossy().to_string())
}

// Verify a file has expected content
#[allow(dead_code)]
fn verify_file_content(path: &str, expected_size: usize) -> bool {
    let content = match fs::read(path) {
        Ok(data) => data,
        Err(_) => return false,
    };
    
    if content.len() != expected_size {
        return false;
    }
    
    // Verify the pattern
    for i in 0..expected_size {
        if content[i] != ((i) % 256) as u8 {
            return false;
        }
    }
    
    true
}

// Test different chunk sizes for reading through FileReader trait
#[test]
fn test_reader_various_chunk_sizes() {
    let temp_dir = TempDir::new().unwrap();
    let file_size = 1024 * 1024; // 1MB
    let file_path = create_test_file(temp_dir.path(), "chunked_read_test.bin", file_size).unwrap();
    
    // Test different chunk sizes
    for chunk_size in [1024, 4096, 8192, 16384, 65536].iter() {
        let mut reader = open_file_reader(&file_path).unwrap();
        let mut output = Vec::with_capacity(file_size);
        let mut buf = vec![0u8; *chunk_size];
        
        let mut offset = 0;
        loop {
            let bytes_read = reader.read_chunk(offset, &mut buf).unwrap();
            if bytes_read == 0 {
                break;
            }
            output.extend_from_slice(&buf[0..bytes_read]);
            offset += bytes_read as u64;
        }
        
        assert_eq!(output.len(), file_size);
        
        // Verify the pattern
        for i in 0..file_size {
            assert_eq!(output[i], ((i) % 256) as u8, "Data mismatch at index {} with chunk size {}", i, chunk_size);
        }
    }
}

// Test edge cases with FileReader
#[test]
fn test_reader_edge_cases() {
    let temp_dir = TempDir::new().unwrap();
    
    // Empty file
    let empty_file_path = create_test_file(temp_dir.path(), "empty.bin", 0).unwrap();
    let mut reader = open_file_reader(&empty_file_path).unwrap();
    assert_eq!(reader.file_size().unwrap(), 0);
    let mut buf = [0u8; 1024];
    assert_eq!(reader.read_chunk(0, &mut buf).unwrap(), 0);

    // Small file, reading with larger buffer
    let small_size = 100;
    let small_file_path = create_test_file(temp_dir.path(), "small.bin", small_size).unwrap(); 
    let mut reader = open_file_reader(&small_file_path).unwrap();
    assert_eq!(reader.file_size().unwrap(), small_size as u64);
    let mut buf = vec![0u8; 1024];
    let bytes_read = reader.read_chunk(0, &mut buf).unwrap();
    assert_eq!(bytes_read, small_size);
    
    // Random access reads
    let file_size = 10_000;
    let random_access_path = create_test_file(temp_dir.path(), "random_access.bin", file_size).unwrap();
    let mut reader = open_file_reader(&random_access_path).unwrap();
    
    // Read from middle
    let mut middle_buf = [0u8; 100];
    let middle_offset = 5000;
    let bytes_read = reader.read_chunk(middle_offset, &mut middle_buf).unwrap();
    assert_eq!(bytes_read, 100);
    for i in 0..100 {
        assert_eq!(middle_buf[i], (((middle_offset as usize) + i) % 256) as u8);
    }
    
    // Read from end (partial)
    let mut end_buf = [0u8; 100];
    let end_offset = 9950;
    let bytes_read = reader.read_chunk(end_offset, &mut end_buf).unwrap();
    assert_eq!(bytes_read, 50); // Only 50 bytes should be read
    for i in 0..50 {
        assert_eq!(end_buf[i], (((end_offset as usize) + i) % 256) as u8);
    }
}

// Testing trait-based polymorphism with the FileReader trait
#[test]
fn test_trait_polymorphism() {
    let temp_dir = TempDir::new().unwrap();
    let file_size = 10_000;
    let file_path = create_test_file(temp_dir.path(), "trait_test.bin", file_size).unwrap();
    
    // Create a Box<dyn FileReader>
    // Create a Box<dyn FileReader> using the platform-abstracted function
    let mut reader = open_file_reader(&file_path).unwrap();
    
    // Test basic operations
    assert_eq!(reader.file_size().unwrap(), file_size as u64);
    
    // Read in chunks of 1000 bytes
    let chunk_size = 1000;
    let mut buf = vec![0u8; chunk_size];
    let mut total_read = 0;
    
    for i in 0..10 {
        let offset = i * chunk_size;
        let bytes_read = reader.read_chunk(offset as u64, &mut buf).unwrap();
        assert_eq!(bytes_read, chunk_size);
        
        // Verify content
        for j in 0..chunk_size {
            assert_eq!(buf[j], ((offset + j) % 256) as u8);
        }
        
        total_read += bytes_read;
    }
    
    assert_eq!(total_read, file_size);
}

// Note: WASM/browser tests would be structured similarly but are implemented 
// separately as they require the browser environment and wasm-bindgen-test.
// See test.html for manual browser tests.