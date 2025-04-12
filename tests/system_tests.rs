//! System tests for RaptorQ library
//!
//! These tests verify the end-to-end functionality of encoding and decoding
//! files of various sizes, including chunking scenarios, using the library
//! directly from Rust.

use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use rq_library::processor::{RaptorQProcessor, ProcessorConfig, ProcessResult};
use sha3::{Digest, Sha3_256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tempfile::{tempdir, TempDir};

/// Helper function to generate a random binary file of specified size
fn generate_random_file(path: &Path, size_bytes: usize) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    
    // Use a seeded RNG for reproducibility
    let seed = [42u8; 32];
    let mut rng = StdRng::from_seed(seed);
    
    // Generate and write data in chunks to avoid excessive memory usage
    const CHUNK_SIZE: usize = 1024 * 1024; // 1 MB chunks
    let mut buffer = vec![0u8; std::cmp::min(CHUNK_SIZE, size_bytes)];
    
    let mut remaining = size_bytes;
    while remaining > 0 {
        let write_size = std::cmp::min(buffer.len(), remaining);
        rng.fill(&mut buffer[0..write_size]);
        file.write_all(&buffer[0..write_size])?;
        remaining -= write_size;
    }
    
    let _ = file.flush();
    Ok(())
}

/// Calculate SHA3-256 hash of a file to verify integrity
fn calculate_file_hash(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut hasher = Sha3_256::new();
    let mut buffer = [0u8; 1024 * 1024]; // 1 MB buffer
    
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    
    Ok(hasher.finalize().to_vec())
}

/// Simple helper to convert a byte slice to a hex string
#[allow(dead_code)]
fn to_hex_string(bytes: &[u8]) -> String {
    bytes.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<String>>()
        .join("")
}

/// Helper struct to manage test artifacts
struct TestContext {
    temp_dir: TempDir,
    input_file: PathBuf,
    symbols_dir: PathBuf,
    output_file: PathBuf,
}

impl TestContext {
    /// Create a new test context with generated input file of specified size
    fn new(file_size_bytes: usize) -> std::io::Result<Self> {
        let temp_dir = tempdir()?;
        let input_file = temp_dir.path().join("input.bin");
        let symbols_dir = temp_dir.path().join("symbols");
        let output_file = temp_dir.path().join("output.bin");
        
        // Create symbols directory
        fs::create_dir_all(&symbols_dir)?;
        
        // Generate random input file
        generate_random_file(&input_file, file_size_bytes)?;
        
        Ok(Self {
            temp_dir,
            input_file,
            symbols_dir,
            output_file,
        })
    }
    
    /// Get input file path as string
    fn input_path(&self) -> String {
        self.input_file.to_string_lossy().into_owned()
    }
    
    /// Get symbols directory path as string
    fn symbols_path(&self) -> String {
        self.symbols_dir.to_string_lossy().into_owned()
    }
    
    /// Get output file path as string
    fn output_path(&self) -> String {
        self.output_file.to_string_lossy().into_owned()
    }
    
    /// Verify if output file matches input file
    fn verify_files_match(&self) -> std::io::Result<bool> {
        let input_hash = calculate_file_hash(&self.input_file)?;
        let output_hash = calculate_file_hash(&self.output_file)?;
        
        Ok(input_hash == output_hash)
    }
    
    /// Delete repair symbols from the symbols directory
    /// (leaving only source symbols)
    fn delete_repair_symbols(&self, result: &ProcessResult) -> std::io::Result<()> {
        if let Some(chunks) = &result.chunks {
            // For chunked encoding
            for chunk in chunks {
                let chunk_dir = self.symbols_dir.join(&chunk.chunk_id);
                let entries = fs::read_dir(&chunk_dir)?;
                
                // Keep only source symbols (based on count)
                let source_symbols = chunk.symbols_count - result.repair_symbols;
                let mut files: Vec<_> = entries.collect::<Result<Vec<_>, _>>()?;
                
                // Sort files to ensure deterministic behavior
                files.sort_by_key(|entry| entry.file_name());
                
                // Delete repair symbols (keep only source_symbols count)
                for entry in files.iter().skip(source_symbols as usize) {
                    fs::remove_file(entry.path())?;
                }
            }
        } else {
            // For non-chunked encoding
            let entries = fs::read_dir(&self.symbols_dir)?;
            let mut files: Vec<_> = entries.collect::<Result<Vec<_>, _>>()?;
            
            // Sort files to ensure deterministic behavior
            files.sort_by_key(|entry| entry.file_name());
            
            // Delete repair symbols (keep only source_symbols count)
            for entry in files.iter().skip(result.source_symbols as usize) {
                fs::remove_file(entry.path())?;
            }
        }
        
        Ok(())
    }
    
    /// Keep only a random subset of symbols (but at least source_symbols count)
    fn keep_random_subset_of_symbols(
        &self, 
        result: &ProcessResult,
        percentage: f64
    ) -> std::io::Result<()> {
        let mut rng = rand::thread_rng();
        
        if let Some(chunks) = &result.chunks {
            // For chunked encoding
            for chunk in chunks {
                let chunk_dir = self.symbols_dir.join(&chunk.chunk_id);
                let entries = fs::read_dir(&chunk_dir)?;
                // Collect file entries and their paths
                let files: Vec<_> = entries.collect::<Result<Vec<_>, _>>()?;
                let source_symbols = (chunk.symbols_count - result.repair_symbols) as usize;
                
                // Keep paths instead of DirEntry objects
                let mut to_keep_paths = Vec::new();
                
                // Always keep source symbols
                for i in 0..std::cmp::min(source_symbols, files.len()) {
                    to_keep_paths.push(files[i].path());
                }
                
                // Add random repair symbols
                for i in source_symbols..files.len() {
                    if rng.gen_bool(percentage) {
                        to_keep_paths.push(files[i].path());
                    }
                }
                // Create a set of paths to keep (using HashSet for O(1) lookups)
                let keep_paths: HashSet<_> = to_keep_paths.into_iter().collect();
                
                // Delete files not in the keep set
                for entry in &files {
                    if !keep_paths.contains(&entry.path()) {
                        fs::remove_file(entry.path())?;
                    }
                }
            }
        } else {
            // For non-chunked encoding
            let entries = fs::read_dir(&self.symbols_dir)?;
            let files: Vec<_> = entries.collect::<Result<Vec<_>, _>>()?;
            
            let source_symbols = result.source_symbols as usize;
            
            // Create a set of paths to keep
            let mut keep_paths = HashSet::new();
            
            // Always keep source symbols
            for i in 0..std::cmp::min(source_symbols, files.len()) {
                keep_paths.insert(files[i].path());
            }
            
            // Add random repair symbols
            for i in source_symbols..files.len() {
                if rng.gen_bool(percentage) {
                    keep_paths.insert(files[i].path());
                }
            }
            
            // Delete files not in the keep set
            for entry in &files {
                if !keep_paths.contains(&entry.path()) {
                    fs::remove_file(entry.path())?;
                }
            }
        }
        
        Ok(())
    }
}

/// Basic encode-decode test with specified file size
fn test_encode_decode(
    file_size_bytes: usize, 
    processor: &RaptorQProcessor, 
    chunk_size: usize
) -> std::io::Result<bool> {
    // Create test context with input file
    let ctx = TestContext::new(file_size_bytes)?;
    
    // Encode the file
    let _ = processor.encode_file_streamed(
        &ctx.input_path(),
        &ctx.symbols_path(),
        chunk_size
    ).expect("Failed to encode file");
    
    // Use the layout file that was generated during encoding
    let layout_path = Path::new(&ctx.symbols_path()).join("_raptorq_layout.json");
    
    // Decode the symbols using the layout file
    processor.decode_symbols(
        &ctx.symbols_path(),
        &ctx.output_path(),
        &layout_path.to_string_lossy()
    ).expect("Failed to decode symbols");
    
    // Verify the decoded file matches the original
    ctx.verify_files_match()
}

/// System test for encoding/decoding a small file (1KB)
#[test]
fn test_sys_encode_decode_small_file() {
    let processor = RaptorQProcessor::new(ProcessorConfig::default());
    let file_size = 1 * 1024; // 1KB
    
    let result = test_encode_decode(file_size, &processor, 0)
        .expect("Test failed with IO error");
    
    assert!(result, "Decoded file does not match original");
}

/// System test for encoding/decoding a medium file (10MB)
#[test]
fn test_sys_encode_decode_medium_file() {
    let processor = RaptorQProcessor::new(ProcessorConfig::default());
    let file_size = 10 * 1024 * 1024; // 10MB
    
    let result = test_encode_decode(file_size, &processor, 0)
        .expect("Test failed with IO error");
    
    assert!(result, "Decoded file does not match original");
}

/// System test for encoding/decoding a large file with auto-chunking (100MB)
#[test]
fn test_sys_encode_decode_large_file_auto_chunk() {
    // Use smaller memory limit to force auto-chunking
    let config = ProcessorConfig {
        symbol_size: 50_000,
        redundancy_factor: 12,
        max_memory_mb: 10, // Small memory limit to force chunking
        concurrency_limit: 4,
    };
    
    let processor = RaptorQProcessor::new(config);
    let file_size = 100 * 1024 * 1024; // 100MB
    
    let result = test_encode_decode(file_size, &processor, 0)
        .expect("Test failed with IO error");
    
    assert!(result, "Decoded file does not match original");
}

/// System test for encoding/decoding a large file with manual chunking (100MB)
#[test]
fn test_sys_encode_decode_large_file_manual_chunk() {
    let processor = RaptorQProcessor::new(ProcessorConfig::default());
    let file_size = 100 * 1024 * 1024; // 100MB
    let chunk_size = 10 * 1024 * 1024; // 10MB chunks
    
    let result = test_encode_decode(file_size, &processor, chunk_size)
        .expect("Test failed with IO error");
    
    assert!(result, "Decoded file does not match original");
}

/// System test for encoding/decoding a very large file (1GB)
#[test]
#[ignore] // Ignored by default since it's resource-intensive
fn test_sys_encode_decode_very_large_file() {
    let processor = RaptorQProcessor::new(ProcessorConfig::default());
    let file_size = 1024 * 1024 * 1024; // 1GB
    let chunk_size = 50 * 1024 * 1024; // 50MB chunks
    
    let result = test_encode_decode(file_size, &processor, chunk_size)
        .expect("Test failed with IO error");
    
    assert!(result, "Decoded file does not match original");
}

/// System test for decoding with only source symbols (minimum necessary)
#[test]
fn test_sys_decode_minimum_symbols() {
    let processor = RaptorQProcessor::new(ProcessorConfig::default());
    
    // Create test context
    let file_size = 5 * 1024 * 1024; // 5MB
    let ctx = TestContext::new(file_size).expect("Failed to create test context");
    
    // Encode the file
    let result = processor.encode_file_streamed(
        &ctx.input_path(),
        &ctx.symbols_path(),
        0
    ).expect("Failed to encode file");
    
    // Delete all repair symbols, keeping only source symbols
    ctx.delete_repair_symbols(&result).expect("Failed to delete repair symbols");
    
    // Use the layout file that was generated during encoding
    let layout_path = Path::new(&ctx.symbols_path()).join("_raptorq_layout.json");
    
    // Decode with only source symbols
    processor.decode_symbols(
        &ctx.symbols_path(),
        &ctx.output_path(),
        &layout_path.to_string_lossy()
    ).expect("Failed to decode with only source symbols");
    
    // Verify the decoded file matches the original
    let files_match = ctx.verify_files_match().expect("Failed to verify files");
    assert!(files_match, "Decoded file does not match original");
}

/// System test for decoding with all symbols (source + repair)
#[test]
fn test_sys_decode_redundant_symbols() {
    let processor = RaptorQProcessor::new(ProcessorConfig::default());
    
    // Create test context
    let file_size = 5 * 1024 * 1024; // 5MB
    let ctx = TestContext::new(file_size).expect("Failed to create test context");
    
    // Encode the file
    let _ = processor.encode_file_streamed(
        &ctx.input_path(),
        &ctx.symbols_path(),
        0
    ).expect("Failed to encode file");
    
    // Keep all symbols (we're testing with redundancy)
    
    // Use the layout file that was generated during encoding
    let layout_path = Path::new(&ctx.symbols_path()).join("_raptorq_layout.json");
    
    // Decode with all symbols
    processor.decode_symbols(
        &ctx.symbols_path(),
        &ctx.output_path(),
        &layout_path.to_string_lossy()
    ).expect("Failed to decode with all symbols");
    
    // Verify the decoded file matches the original
    let files_match = ctx.verify_files_match().expect("Failed to verify files");
    assert!(files_match, "Decoded file does not match original");
}

/// System test for decoding with a random subset of symbols
#[test]
fn test_sys_decode_random_subset() {
    let processor = RaptorQProcessor::new(ProcessorConfig::default());
    
    // Create test context
    let file_size = 5 * 1024 * 1024; // 5MB
    let ctx = TestContext::new(file_size).expect("Failed to create test context");
    
    // Encode the file
    let result = processor.encode_file_streamed(
        &ctx.input_path(),
        &ctx.symbols_path(),
        0
    ).expect("Failed to encode file");
    
    // Keep a random subset of repair symbols (50% of them)
    // but always keep all source symbols
    ctx.keep_random_subset_of_symbols(&result, 0.5).expect("Failed to select random subset");
    
    // Use the layout file that was generated during encoding
    let layout_path = Path::new(&ctx.symbols_path()).join("_raptorq_layout.json");
    
    // Decode with random subset of symbols
    processor.decode_symbols(
        &ctx.symbols_path(),
        &ctx.output_path(),
        &layout_path.to_string_lossy()
    ).expect("Failed to decode with random subset of symbols");
    
    // Verify the decoded file matches the original
    let files_match = ctx.verify_files_match().expect("Failed to verify files");
    assert!(files_match, "Decoded file does not match original");
}

/// System test for error handling during encoding (non-existent input)
#[test]
fn test_sys_error_handling_encode() {
    let processor = RaptorQProcessor::new(ProcessorConfig::default());
    
    // Create test context (only used for the symbols directory)
    let ctx = TestContext::new(1024).expect("Failed to create test context");
    
    // Try to encode a non-existent file
    let non_existent_file = ctx.temp_dir.path().join("does_not_exist.bin");
    
    let result = processor.encode_file_streamed(
        &non_existent_file.to_string_lossy(),
        &ctx.symbols_path(),
        0
    );
    
    // Verify error is reported correctly
    assert!(result.is_err(), "Expected encoding to fail with non-existent file");
    match result {
        Err(err) => {
            assert!(format!("{}", err).contains("not found"), 
                    "Error message should indicate file not found");
        },
        _ => panic!("Expected FileNotFound error"),
    }
}

/// System test for error handling during decoding (non-existent symbols dir)
#[test]
fn test_sys_error_handling_decode() {
    let processor = RaptorQProcessor::new(ProcessorConfig::default());
    
    // Create test context (only used for the output file path)
    let ctx = TestContext::new(1024).expect("Failed to create test context");
    
    // Non-existent symbols directory
    let non_existent_dir = ctx.temp_dir.path().join("non_existent_symbols");
    
    // Non-existent layout file
    let non_existent_layout = ctx.temp_dir.path().join("non_existent_layout.json");
    
    let result = processor.decode_symbols(
        &non_existent_dir.to_string_lossy(),
        &ctx.output_path(),
        &non_existent_layout.to_string_lossy()
    );
    
    // Verify error is reported correctly
    assert!(result.is_err(), "Expected decoding to fail with non-existent symbols dir");
    match result {
        Err(err) => {
            assert!(format!("{}", err).contains("not found"), 
                    "Error message should indicate directory not found");
        },
        _ => panic!("Expected FileNotFound error"),
    }
}