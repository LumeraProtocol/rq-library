use criterion::{criterion_group, criterion_main, Criterion, BenchmarkGroup, measurement::WallTime};
use rand::{Rng, rngs::OsRng};
use rq_library::processor::{ProcessorConfig, RaptorQProcessor};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use std::time::Duration;

use allocation_counter;

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
fn encode_file_for_decoding(processor: &RaptorQProcessor, input_path: &Path, output_dir: &Path) -> String {
    let result = processor
        .encode_file_streamed(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            0, // Let the processor determine chunk size
            false,
        )
        .expect("Failed to encode file");

    result.layout_file_path
}

fn bytes_to_mb_or_gb(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    if bytes as f64 >= GB {
        let gigabytes = (bytes as f64 / GB) as u64;
        format!("{}GB", gigabytes)
    } else {
        let megabytes = (bytes as f64 / MB) as u64;
        format!("{}MB", megabytes)
    }
}

// Benchmark encoding a 1MB file
fn bench_encode_1mb(group: &mut BenchmarkGroup<WallTime>) {
    let config = ProcessorConfig::default();
    let processor = RaptorQProcessor::new(config);
        
    let mut max_allocations = 0;
    let mut max_bytes = 0;
    let mut total_allocations = 0;
    let mut total_bytes = 0;
    let mut counter = 0;

    group.bench_function("encode_1mb", |b| {
        // Set up environment fresh for each iteration
        let (temp_dir, input_file, output_dir) = setup_test_env(SIZE_1MB);

        b.iter(|| {
            let info = allocation_counter::measure(|| {
                processor
                    .encode_file_streamed(
                        input_file.to_str().unwrap(),
                        output_dir.to_str().unwrap(),
                        0, // Let the processor determine chunk size
                        false,
                    )
                    .expect("Failed to encode file");
            });
            if max_allocations < info.count_total {
                max_allocations = info.count_total;
            }
            if max_bytes < info.bytes_total {
                max_bytes = info.bytes_total;
            }
            total_allocations += info.count_total;
            total_bytes += info.bytes_total;
            counter += 1;
        });

        // Keep temp_dir in scope until benchmark is done
        drop(temp_dir);
    });

    println!("Max bytes allocated {}; Max number of allocations: {}", bytes_to_mb_or_gb(max_bytes), max_allocations);
    println!("Average bytes allocated: {}; Average number of allocations: {}", bytes_to_mb_or_gb(total_bytes / counter), total_allocations / counter);
}

// Benchmark encoding a 10MB file
fn bench_encode_10mb(group: &mut BenchmarkGroup<WallTime>) {
    let config = ProcessorConfig::default10_mb();
    let processor = RaptorQProcessor::new(config);
    
    let mut max_allocations = 0;
    let mut max_bytes = 0;
    let mut total_allocations = 0;
    let mut total_bytes = 0;
    let mut counter = 0;
    
    group.bench_function("encode_10mb", |b| {
        // Set up environment fresh for each iteration
        let (temp_dir, input_file, output_dir) = setup_test_env(SIZE_10MB);
        
        b.iter(|| {
            let info = allocation_counter::measure(|| {
                processor
                    .encode_file_streamed(
                        input_file.to_str().unwrap(),
                        output_dir.to_str().unwrap(),
                        0, // Let the processor determine chunk size
                        false,
                    )
                    .expect("Failed to encode file");
            });
            if max_allocations < info.count_total {
                max_allocations = info.count_total;
            }
            if max_bytes < info.bytes_total {
                max_bytes = info.bytes_total;
            }
            total_allocations += info.count_total;
            total_bytes += info.bytes_total;
            counter += 1;
        });
        
        // Keep temp_dir in scope until benchmark is done
        drop(temp_dir);
    });

    println!("Max bytes allocated {}; Max number of allocations: {}", bytes_to_mb_or_gb(max_bytes), max_allocations);
    println!("Average bytes allocated: {}; Average number of allocations: {}", bytes_to_mb_or_gb(total_bytes / counter), total_allocations / counter);
}

// Benchmark encoding a 100MB file
fn bench_encode_100mb(group: &mut BenchmarkGroup<WallTime>) {
    let config = ProcessorConfig::default100_mb();
    let processor = RaptorQProcessor::new(config);
    
    let mut max_allocations = 0;
    let mut max_bytes = 0;
    let mut total_allocations = 0;
    let mut total_bytes = 0;
    let mut counter = 0;
    
    group.bench_function("encode_100mb", |b| {
        // Set up environment fresh for each iteration
        let (temp_dir, input_file, output_dir) = setup_test_env(SIZE_100MB);
        
        b.iter(|| {
            let info = allocation_counter::measure(|| {
                processor
                    .encode_file_streamed(
                        input_file.to_str().unwrap(),
                        output_dir.to_str().unwrap(),
                        0, // Let the processor determine chunk size
                        false,
                    )
                    .expect("Failed to encode file");
            });
            if max_allocations < info.count_total {
                max_allocations = info.count_total;
            }
            if max_bytes < info.bytes_total {
                max_bytes = info.bytes_total;
            }
            total_allocations += info.count_total;
            total_bytes += info.bytes_total;
            counter += 1;
        });
        
        // Keep temp_dir in scope until benchmark is done
        drop(temp_dir);
    });
    
    println!("Max bytes allocated {}; Max number of allocations: {}", bytes_to_mb_or_gb(max_bytes), max_allocations);
    println!("Average bytes allocated: {}; Average number of allocations: {}", bytes_to_mb_or_gb(total_bytes / counter), total_allocations / counter);
}

// Benchmark encoding a 1GB file
fn bench_encode_1gb(group: &mut BenchmarkGroup<WallTime>) {
    let config = ProcessorConfig::default1_gb();
    let processor = RaptorQProcessor::new(config);
    
    let mut max_allocations = 0;
    let mut max_bytes = 0;
    let mut total_allocations = 0;
    let mut total_bytes = 0;
    let mut counter = 0;
    
    group.bench_function("encode_1gb", |b| {
        // Set up environment fresh for each iteration
        let (temp_dir, input_file, output_dir) = setup_test_env(SIZE_1GB);
        
        b.iter(|| {
            let info = allocation_counter::measure(|| {
                processor
                    .encode_file_streamed(
                        input_file.to_str().unwrap(),
                        output_dir.to_str().unwrap(),
                        0, // Let the processor determine chunk size
                        false,
                    )
                    .expect("Failed to encode file");
            });
            if max_allocations < info.count_total {
                max_allocations = info.count_total;
            }
            if max_bytes < info.bytes_total {
                max_bytes = info.bytes_total;
            }
            total_allocations += info.count_total;
            total_bytes += info.bytes_total;
            counter += 1;
        });
        
        // Keep temp_dir in scope until benchmark is done
        drop(temp_dir);
    });
    
    println!("Max bytes allocated {}; Max number of allocations: {}", bytes_to_mb_or_gb(max_bytes), max_allocations);
    println!("Average bytes allocated: {}; Average number of allocations: {}", bytes_to_mb_or_gb(total_bytes / counter), total_allocations / counter);
}

// Benchmark decoding a 1MB file
fn bench_decode_1mb(group: &mut BenchmarkGroup<WallTime>) {
    let config = ProcessorConfig::default();
    let processor = RaptorQProcessor::new(config);
    
    let mut max_allocations = 0;
    let mut max_bytes = 0;
    let mut total_allocations = 0;
    let mut total_bytes = 0;
    let mut counter = 0;
    
    // Setup (outside of the benchmark iteration)
    let (temp_dir, input_file, symbols_dir) = setup_test_env(SIZE_1MB);
    let output_file = temp_dir.path().join("decoded_file.dat");
    
    // Encode the file first to generate symbols
    let layout_file_path = encode_file_for_decoding(&processor, &input_file, &symbols_dir);
    
    group.bench_function("decode_1mb", |b| {
        b.iter(|| {
            let info = allocation_counter::measure(|| {
                processor
                    .decode_symbols(
                        symbols_dir.to_str().unwrap(),
                        output_file.to_str().unwrap(),
                        layout_file_path.as_str(),
                    )
                    .expect("Failed to decode symbols");
            });
            if max_allocations < info.count_total {
                max_allocations = info.count_total;
            }
            if max_bytes < info.bytes_total {
                max_bytes = info.bytes_total;
            }
            total_allocations += info.count_total;
            total_bytes += info.bytes_total;
            counter += 1;
        });
    });
    println!("Max bytes allocated {}; Max number of allocations: {}", bytes_to_mb_or_gb(max_bytes), max_allocations);
    println!("Average bytes allocated: {}; Average number of allocations: {}", bytes_to_mb_or_gb(total_bytes / counter), total_allocations / counter);
    
    // Cleanup happens automatically when temp_dir is dropped
}

// Benchmark decoding a 10MB file
fn bench_decode_10mb(group: &mut BenchmarkGroup<WallTime>) {
    let config = ProcessorConfig::default10_mb();
    let processor = RaptorQProcessor::new(config);
    
    let mut max_allocations = 0;
    let mut max_bytes = 0;
    let mut total_allocations = 0;
    let mut total_bytes = 0;
    let mut counter = 0;
    
    // Setup (outside of the benchmark iteration)
    let (temp_dir, input_file, symbols_dir) = setup_test_env(SIZE_10MB);
    let output_file = temp_dir.path().join("decoded_file.dat");
    
    // Encode the file first to generate symbols
    let layout_file_path = encode_file_for_decoding(&processor, &input_file, &symbols_dir);
    
    group.bench_function("decode_10mb", |b| {
        b.iter(|| {
            let info = allocation_counter::measure(|| {
                processor
                    .decode_symbols(
                        symbols_dir.to_str().unwrap(),
                        output_file.to_str().unwrap(),
                        layout_file_path.as_str(),
                    )
                    .expect("Failed to decode symbols");
            });
            if max_allocations < info.count_total {
                max_allocations = info.count_total;
            }
            if max_bytes < info.bytes_total {
                max_bytes = info.bytes_total;
            }
            total_allocations += info.count_total;
            total_bytes += info.bytes_total;
            counter += 1;
        });
    });
    
    println!("Max bytes allocated {}; Max number of allocations: {}", bytes_to_mb_or_gb(max_bytes), max_allocations);
    println!("Average bytes allocated: {}; Average number of allocations: {}", bytes_to_mb_or_gb(total_bytes / counter), total_allocations / counter);
    
    // Cleanup happens automatically when temp_dir is dropped
}

// Benchmark decoding a 100MB file
fn bench_decode_100mb(group: &mut BenchmarkGroup<WallTime>) {
    let config = ProcessorConfig::default100_mb();
    let processor = RaptorQProcessor::new(config);
    
    let mut max_allocations = 0;
    let mut max_bytes = 0;
    let mut total_allocations = 0;
    let mut total_bytes = 0;
    let mut counter = 0;
    
    // Setup (outside of the benchmark iteration)
    let (temp_dir, input_file, symbols_dir) = setup_test_env(SIZE_100MB);
    let output_file = temp_dir.path().join("decoded_file.dat");
    
    // Encode the file first to generate symbols
    let layout_file_path = encode_file_for_decoding(&processor, &input_file, &symbols_dir);
    
    group.bench_function("decode_100mb", |b| {
        b.iter(|| {
            let info = allocation_counter::measure(|| {
                processor
                    .decode_symbols(
                        symbols_dir.to_str().unwrap(),
                        output_file.to_str().unwrap(),
                        layout_file_path.as_str(),
                    )
                    .expect("Failed to decode symbols");
            });
            if max_allocations < info.count_total {
                max_allocations = info.count_total;
            }
            if max_bytes < info.bytes_total {
                max_bytes = info.bytes_total;
            }
            total_allocations += info.count_total;
            total_bytes += info.bytes_total;
            counter += 1;
        });
    });
    
    println!("Max bytes allocated {}; Max number of allocations: {}", bytes_to_mb_or_gb(max_bytes), max_allocations);
    println!("Average bytes allocated: {}; Average number of allocations: {}", bytes_to_mb_or_gb(total_bytes / counter), total_allocations / counter);
    
    // Cleanup happens automatically when temp_dir is dropped
}

// Benchmark decoding a 1GB file
fn bench_decode_1gb(group: &mut BenchmarkGroup<WallTime>) {
    let config = ProcessorConfig::default1_gb();
    let processor = RaptorQProcessor::new(config);
    
    let mut max_allocations = 0;
    let mut max_bytes = 0;
    let mut total_allocations = 0;
    let mut total_bytes = 0;
    let mut counter = 0;
    
    // Setup (outside of the benchmark iteration)
    let (temp_dir, input_file, symbols_dir) = setup_test_env(SIZE_1GB);
    let output_file = temp_dir.path().join("decoded_file.dat");
    
    // Encode the file first to generate symbols
    let layout_file_path = encode_file_for_decoding(&processor, &input_file, &symbols_dir);
    
    group.bench_function("decode_1gb", |b| {
        b.iter(|| {
            let info = allocation_counter::measure(|| {
                processor
                    .decode_symbols(
                        symbols_dir.to_str().unwrap(),
                        output_file.to_str().unwrap(),
                        layout_file_path.as_str(),
                    )
                    .expect("Failed to decode symbols");
            });
            if max_allocations < info.count_total {
                max_allocations = info.count_total;
            }
            if max_bytes < info.bytes_total {
                max_bytes = info.bytes_total;
            }
            total_allocations += info.count_total;
            total_bytes += info.bytes_total;
            counter += 1;
        });
    });
    
    println!("Max bytes allocated {}; Max number of allocations: {}", bytes_to_mb_or_gb(max_bytes), max_allocations);
    println!("Average bytes allocated: {}; Average number of allocations: {}", bytes_to_mb_or_gb(total_bytes / counter), total_allocations / counter);
    
    // Cleanup happens automatically when temp_dir is dropped
}

// Group encoding benchmarks
fn encoding_benchmarks(c: &mut Criterion) {
    // Create a benchmark group with specific configuration for encoding
    let mut group = c.benchmark_group("Encoding");

    // group.measurement_time(Duration::from_secs(5));  <-- this is default
    // group.sample_size(100);                          <-- this is default
    bench_encode_1mb(&mut group);
    
    group.measurement_time(Duration::from_secs(40));
    group.sample_size(100);
    bench_encode_10mb(&mut group);
    println!();

    group.measurement_time(Duration::from_secs(300));
    group.sample_size(50);
    bench_encode_100mb(&mut group);
    println!();

    group.measurement_time(Duration::from_secs(1000));
    group.sample_size(10);
    bench_encode_1gb(&mut group);
    println!();
    
    group.finish();
}

// Group decoding benchmarks
fn decoding_benchmarks(c: &mut Criterion) {
    // Create a benchmark group with specific configuration for decoding
    let mut group = c.benchmark_group("Decoding");

    // group.measurement_time(Duration::from_secs(5));  <-- this is default
    // group.sample_size(100);                          <-- this is default
    bench_decode_1mb(&mut group);

    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);
    bench_decode_10mb(&mut group);
    println!();

    group.measurement_time(Duration::from_secs(60));
    group.sample_size(50);
    bench_decode_100mb(&mut group);
    println!();

    group.measurement_time(Duration::from_secs(180));
    group.sample_size(10);
    bench_decode_1gb(&mut group);
    
    group.finish();
}

criterion_group!(benches, encoding_benchmarks, decoding_benchmarks);
criterion_main!(benches);