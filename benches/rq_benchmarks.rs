use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use rand::{Rng, rngs::OsRng};
use rq_library::processor::{ProcessorConfig, RaptorQProcessor};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// Constants for file sizes
const SIZE_1MB: usize = 1 * 1024 * 1024;
const SIZE_10MB: usize = 10 * 1024 * 1024;
const SIZE_100MB: usize = 100 * 1024 * 1024;
const SIZE_1GB: usize = 1024 * 1024 * 1024;

// Helper function to generate random data of specified size
fn generate_test_data(size: usize) -> Vec<u8> {
    let mut data = vec![0u8; size];
    OsRng.fill(&mut data[..]);
    data
}

// Helper function to create a test file with random data
fn create_test_file(path: &Path, size: usize) -> io::Result<()> {
    let data = generate_test_data(size);
    let mut file = File::create(path)?;
    file.write_all(&data)?;
    Ok(())
}

// Helper function to set up a test environment with a file of specified size
fn setup_test_env(size: usize) -> (TempDir, PathBuf, PathBuf) {
    // Create temporary directory
    let temp_dir = TempDir::new().expect("Failed to create temporary directory");
    
    // Create input file path
    let input_file = temp_dir.path().join("test_file.dat");
    
    // Create output directory for symbols
    let output_dir = temp_dir.path().join("symbols");
    fs::create_dir_all(&output_dir).expect("Failed to create output directory");
    
    // Create test file with random data
    create_test_file(&input_file, size).expect("Failed to create test file");
    
    (temp_dir, input_file, output_dir)
}

// Helper function to encode a file and return encoder parameters
fn encode_file_for_decoding(processor: &RaptorQProcessor, input_path: &Path, output_dir: &Path) -> Vec<u8> {
    let result = processor
        .encode_file_streamed(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            0, // Let the processor determine chunk size
        )
        .expect("Failed to encode file");
    
    result.encoder_parameters
}

// Benchmark encoding a 1MB file
fn bench_encode_1mb(c: &mut Criterion) {
    let config = ProcessorConfig::default();
    let processor = RaptorQProcessor::new(config);
    
    c.bench_function("encode_1mb", |b| {
        // Set up environment fresh for each iteration
        let (temp_dir, input_file, output_dir) = setup_test_env(SIZE_1MB);
        
        b.iter(|| {
            processor
                .encode_file_streamed(
                    input_file.to_str().unwrap(),
                    output_dir.to_str().unwrap(),
                    0, // Let the processor determine chunk size
                )
                .expect("Failed to encode file");
        });
        
        // Keep temp_dir in scope until benchmark is done
        drop(temp_dir);
    });
}

// Benchmark encoding a 10MB file
fn bench_encode_10mb(c: &mut Criterion) {
    let config = ProcessorConfig::default();
    let processor = RaptorQProcessor::new(config);
    
    c.bench_function("encode_10mb", |b| {
        // Set up environment fresh for each iteration
        let (temp_dir, input_file, output_dir) = setup_test_env(SIZE_10MB);
        
        b.iter(|| {
            processor
                .encode_file_streamed(
                    input_file.to_str().unwrap(),
                    output_dir.to_str().unwrap(),
                    0, // Let the processor determine chunk size
                )
                .expect("Failed to encode file");
        });
        
        // Keep temp_dir in scope until benchmark is done
        drop(temp_dir);
    });
}

// Benchmark encoding a 100MB file
fn bench_encode_100mb(c: &mut Criterion) {
    let config = ProcessorConfig::default();
    let processor = RaptorQProcessor::new(config);
    
    c.bench_function("encode_100mb", |b| {
        // Set up environment fresh for each iteration
        let (temp_dir, input_file, output_dir) = setup_test_env(SIZE_100MB);
        
        b.iter(|| {
            processor
                .encode_file_streamed(
                    input_file.to_str().unwrap(),
                    output_dir.to_str().unwrap(),
                    0, // Let the processor determine chunk size
                )
                .expect("Failed to encode file");
        });
        
        // Keep temp_dir in scope until benchmark is done
        drop(temp_dir);
    });
}

// Benchmark encoding a 1GB file
fn bench_encode_1gb(c: &mut Criterion) {
    let config = ProcessorConfig {
        max_memory_mb: 2048, // Increase memory limit for 1GB file
        ..ProcessorConfig::default()
    };
    let processor = RaptorQProcessor::new(config);
    
    c.bench_function("encode_1gb", |b| {
        // Set up environment fresh for each iteration
        let (temp_dir, input_file, output_dir) = setup_test_env(SIZE_1GB);
        
        b.iter(|| {
            processor
                .encode_file_streamed(
                    input_file.to_str().unwrap(),
                    output_dir.to_str().unwrap(),
                    0, // Let the processor determine chunk size
                )
                .expect("Failed to encode file");
        });
        
        // Keep temp_dir in scope until benchmark is done
        drop(temp_dir);
    });
}

// Benchmark decoding a 1MB file
fn bench_decode_1mb(c: &mut Criterion) {
    let config = ProcessorConfig::default();
    let processor = RaptorQProcessor::new(config);
    
    // Setup (outside of the benchmark iteration)
    let (temp_dir, input_file, symbols_dir) = setup_test_env(SIZE_1MB);
    let output_file = temp_dir.path().join("decoded_file.dat");
    
    // Encode the file first to generate symbols
    let encoder_params = encode_file_for_decoding(&processor, &input_file, &symbols_dir);
    
    // Convert encoder parameters to the expected format
    let mut params_array = [0u8; 12];
    params_array.copy_from_slice(&encoder_params);
    
    c.bench_function("decode_1mb", |b| {
        b.iter(|| {
            processor
                .decode_symbols(
                    symbols_dir.to_str().unwrap(),
                    output_file.to_str().unwrap(),
                    &params_array,
                )
                .expect("Failed to decode symbols");
        });
    });
    
    // Cleanup happens automatically when temp_dir is dropped
}

// Benchmark decoding a 10MB file
fn bench_decode_10mb(c: &mut Criterion) {
    let config = ProcessorConfig::default();
    let processor = RaptorQProcessor::new(config);
    
    // Setup (outside of the benchmark iteration)
    let (temp_dir, input_file, symbols_dir) = setup_test_env(SIZE_10MB);
    let output_file = temp_dir.path().join("decoded_file.dat");
    
    // Encode the file first to generate symbols
    let encoder_params = encode_file_for_decoding(&processor, &input_file, &symbols_dir);
    
    // Convert encoder parameters to the expected format
    let mut params_array = [0u8; 12];
    params_array.copy_from_slice(&encoder_params);
    
    c.bench_function("decode_10mb", |b| {
        b.iter(|| {
            processor
                .decode_symbols(
                    symbols_dir.to_str().unwrap(),
                    output_file.to_str().unwrap(),
                    &params_array,
                )
                .expect("Failed to decode symbols");
        });
    });
    
    // Cleanup happens automatically when temp_dir is dropped
}

// Benchmark decoding a 100MB file
fn bench_decode_100mb(c: &mut Criterion) {
    let config = ProcessorConfig::default();
    let processor = RaptorQProcessor::new(config);
    
    // Setup (outside of the benchmark iteration)
    let (temp_dir, input_file, symbols_dir) = setup_test_env(SIZE_100MB);
    let output_file = temp_dir.path().join("decoded_file.dat");
    
    // Encode the file first to generate symbols
    let encoder_params = encode_file_for_decoding(&processor, &input_file, &symbols_dir);
    
    // Convert encoder parameters to the expected format
    let mut params_array = [0u8; 12];
    params_array.copy_from_slice(&encoder_params);
    
    c.bench_function("decode_100mb", |b| {
        b.iter(|| {
            processor
                .decode_symbols(
                    symbols_dir.to_str().unwrap(),
                    output_file.to_str().unwrap(),
                    &params_array,
                )
                .expect("Failed to decode symbols");
        });
    });
    
    // Cleanup happens automatically when temp_dir is dropped
}

// Benchmark decoding a 1GB file
fn bench_decode_1gb(c: &mut Criterion) {
    let config = ProcessorConfig {
        max_memory_mb: 2048, // Increase memory limit for 1GB file
        ..ProcessorConfig::default()
    };
    let processor = RaptorQProcessor::new(config);
    
    // Setup (outside of the benchmark iteration)
    let (temp_dir, input_file, symbols_dir) = setup_test_env(SIZE_1GB);
    let output_file = temp_dir.path().join("decoded_file.dat");
    
    // Encode the file first to generate symbols
    let encoder_params = encode_file_for_decoding(&processor, &input_file, &symbols_dir);
    
    // Convert encoder parameters to the expected format
    let mut params_array = [0u8; 12];
    params_array.copy_from_slice(&encoder_params);
    
    c.bench_function("decode_1gb", |b| {
        b.iter(|| {
            processor
                .decode_symbols(
                    symbols_dir.to_str().unwrap(),
                    output_file.to_str().unwrap(),
                    &params_array,
                )
                .expect("Failed to decode symbols");
        });
    });
    
    // Cleanup happens automatically when temp_dir is dropped
}

// Group encoding benchmarks
fn encoding_benchmarks(c: &mut Criterion) {
    bench_encode_1mb(c);
    bench_encode_10mb(c);
    bench_encode_100mb(c);
    bench_encode_1gb(c);
}

// Group decoding benchmarks
fn decoding_benchmarks(c: &mut Criterion) {
    bench_decode_1mb(c);
    bench_decode_10mb(c);
    bench_decode_100mb(c);
    bench_decode_1gb(c);
}

criterion_group!(benches, encoding_benchmarks, decoding_benchmarks);
criterion_main!(benches);