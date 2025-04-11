//! RaptorQ Processing Logic
//!
//! # Memory Model and Chunking
//!
//! This module implements the core logic for encoding and decoding files using the RaptorQ algorithm.
//! Due to the design of the underlying `raptorq` crate (v2.0.0), both encoding and decoding require
//! the entire data segment (file or chunk) to be loaded into memory. There is no support for true
//! streaming input or output at the encoder/decoder level.
//!
//! ## Chunking Strategy
//!
//! To handle large files without exceeding memory limits, this module splits input files into chunks.
//! Each chunk is processed independently, and the peak memory usage is determined by the chunk size
//! plus RaptorQ's internal overhead. The `max_memory_mb` configuration parameter controls the maximum
//! allowed memory usage per operation, and the chunk size is chosen accordingly.
//!
//! ## Limitations
//!
//! - The memory usage will always scale with the chunk size (or file size if not chunked).
//! - Decoding also loads each chunk fully into memory before writing to disk.
//! - For more details, see ARCHITECTURE_REVIEW.md.

use raptorq::{Decoder, Encoder, EncodingPacket, ObjectTransmissionInformation};
use sha3::{Digest, Sha3_256};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write, BufWriter, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use parking_lot::Mutex;
use thiserror::Error;
use serde::{Serialize, Deserialize};
use log::{error, debug};

const DEFAULT_STREAM_BUFFER_SIZE: usize = 1 * 1024 * 1024; // 1 MiB
const DEFAULT_MAX_MEMORY: u32 = 1024; // 1 GB
const DEFAULT_CONCURRENCY_LIMIT: u32 = 4;
const MEMORY_SAFETY_MARGIN: f64 = 1.5; // 50% safety margin

#[derive(Debug, Clone)]
pub struct ProcessorConfig {
    pub symbol_size: u16,
    pub redundancy_factor: u8,
    pub max_memory_mb: u32,
    pub concurrency_limit: u32,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            symbol_size: 50_000,
            redundancy_factor: 12,
            max_memory_mb: DEFAULT_MAX_MEMORY,
            concurrency_limit: DEFAULT_CONCURRENCY_LIMIT,
        }
    }
}

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("IO error: {0}")]
    IOError(#[from] io::Error),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Encoding failed: {0}")]
    EncodingFailed(String),

    #[error("Decoding failed: {0}")]
    DecodingFailed(String),

    #[error("Memory limit exceeded. Required: {required}MB, Available: {available}MB")]
    MemoryLimitExceeded {
        required: u32,
        available: u32,
    },

    #[error("Concurrency limit reached")]
    ConcurrencyLimitReached,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessResult {
    pub encoder_parameters: Vec<u8>,
    pub source_symbols: u32,
    pub repair_symbols: u32,
    pub symbols_directory: String,
    pub symbols_count: u32,
    pub chunks: Option<Vec<ChunkInfo>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChunkInfo {
    pub chunk_id: String,
    pub original_offset: u64,
    pub size: u64,
    pub symbols_count: u32,
}

pub struct RaptorQProcessor {
    config: ProcessorConfig,
    active_tasks: AtomicUsize,
    last_error: Mutex<String>,
}

impl RaptorQProcessor {
    pub fn new(config: ProcessorConfig) -> Self {
        Self {
            config,
            active_tasks: AtomicUsize::new(0),
            last_error: Mutex::new(String::new()),
        }
    }

    pub fn get_last_error(&self) -> String {
        self.last_error.lock().clone()
    }

    fn set_last_error(&self, error: String) {
        *self.last_error.lock() = error;
    }

    pub fn get_recommended_chunk_size(&self, file_size: u64) -> usize {
        let max_memory_bytes = (self.config.max_memory_mb as u64) * 1024 * 1024;

        // If file is smaller than max memory divided by MEMORY_SAFETY_MARGIN,
        // don't chunk it
        let safe_memory = (max_memory_bytes as f64 / MEMORY_SAFETY_MARGIN) as u64;
        if file_size < safe_memory {
            return 0;
        }

        // Otherwise, aim for chunks that would use about 1/4 of available memory
        let target_chunk_size = safe_memory / 4;

        // Ensure chunk size is a multiple of symbol size for efficient processing
        let symbol_size = self.config.symbol_size as u64;
        let chunks = (target_chunk_size / symbol_size).max(1);
        (chunks * symbol_size) as usize
    }

    pub fn encode_file_streamed(
        &self,
        input_path: &str,
        output_dir: &str,
        chunk_size: usize,
    ) -> Result<ProcessResult, ProcessError> {
        // Check if we can take another task
        if !self.can_start_task() {
            return Err(ProcessError::ConcurrencyLimitReached);
        }

        let _guard = TaskGuard::new(&self.active_tasks);

        let input_path = Path::new(input_path);
        if !input_path.exists() {
            let err = format!("Input file not found: {:?}", input_path);
            self.set_last_error(err.clone());
            return Err(ProcessError::FileNotFound(err));
        }

        let file_size = input_path.metadata()?.len();

        // Determine if we need to chunk the file
        let actual_chunk_size = if chunk_size == 0 {
            self.get_recommended_chunk_size(file_size)
        } else {
            chunk_size
        };

        // If we don't need to chunk, process the whole file
        if actual_chunk_size == 0 {
            debug!("Processing file without chunking: {:?} ({}B)", input_path, file_size);
            return self.encode_single_file(input_path, output_dir);
        }

        // Otherwise, process in chunks
        debug!("Processing file in chunks: {:?} ({}B) with chunk size {}B",
               input_path, file_size, actual_chunk_size);

        self.encode_file_in_chunks(input_path, output_dir, actual_chunk_size)
    }

    fn encode_single_file(
        &self,
        input_path: &Path,
        output_dir: &str,
    ) -> Result<ProcessResult, ProcessError> {
        // Ensure output directory exists
        let output_path = Path::new(output_dir);
        fs::create_dir_all(output_path)?;

        let (source_file, total_size) = self.open_and_validate_file(input_path)?;
        let mut reader = BufReader::new(source_file);

        // Estimate memory requirements - if too high, recommend chunking
        let memory_required = self.estimate_memory_requirements(total_size);
        if !self.is_memory_available(memory_required) {
            let err = ProcessError::MemoryLimitExceeded {
                required: memory_required,
                available: self.config.max_memory_mb,
            };
            self.set_last_error(err.to_string());
            return Err(err);
        }

        // Process the file in a streaming manner
        let (encoder_params, symbols) = self.encode_stream(
            &mut reader,
            total_size,
            output_path,
        )?;

        let source_symbols = symbols.len() as u32 - self.calculate_repair_symbols(total_size);
        let repair_symbols = self.calculate_repair_symbols(total_size);

        Ok(ProcessResult {
            encoder_parameters: encoder_params,
            source_symbols,
            repair_symbols,
            symbols_directory: output_dir.to_string(),
            symbols_count: symbols.len() as u32,
            chunks: None,
        })
    }

    fn encode_file_in_chunks(
        &self,
        input_path: &Path,
        output_dir: &str,
        chunk_size: usize,
    ) -> Result<ProcessResult, ProcessError> {
        // Ensure output directory exists
        let base_output_path = Path::new(output_dir);
        fs::create_dir_all(base_output_path)?;

        let (source_file, total_size) = self.open_and_validate_file(input_path)?;
        let mut reader = BufReader::new(source_file);

        // Calculate number of chunks
        let chunk_count = (total_size as f64 / chunk_size as f64).ceil() as usize;
        debug!("File will be split into {} chunks", chunk_count);

        // Process each chunk
        let mut chunks = Vec::with_capacity(chunk_count);
        let mut total_symbols_count = 0;

        // All chunks share the same encoder parameters, so we'll use the first chunk's
        let mut encoder_parameters = Vec::new();

        for chunk_index in 0..chunk_count {
            let chunk_id = format!("chunk_{}", chunk_index);
            let chunk_dir = base_output_path.join(&chunk_id);
            fs::create_dir_all(&chunk_dir)?;

            let chunk_offset = chunk_index as u64 * chunk_size as u64;
            let remaining = total_size - chunk_offset;
            let actual_chunk_size = std::cmp::min(chunk_size as u64, remaining);

            debug!("Processing chunk {} of {} bytes at offset {}",
                   chunk_index, actual_chunk_size, chunk_offset);

            // Create a limited reader for this chunk
            let mut chunk_reader = reader.by_ref().take(actual_chunk_size);

            // Process this chunk
            let (params, symbols) = self.encode_stream(
                &mut chunk_reader,
                actual_chunk_size,
                &chunk_dir,
            )?;

            // Store encoder parameters from the first chunk
            if encoder_parameters.is_empty() {
                encoder_parameters = params.clone();
            }

            let _source_symbols = symbols.len() as u32 - self.calculate_repair_symbols(actual_chunk_size);

            chunks.push(ChunkInfo {
                chunk_id,
                original_offset: chunk_offset,
                size: actual_chunk_size,
                symbols_count: symbols.len() as u32,
            });

            total_symbols_count += symbols.len() as u32;

            // Seek to the beginning of the next chunk
            reader.seek(SeekFrom::Start(chunk_offset + actual_chunk_size))?;
        }

        // Calculate overall symbols
        let source_symbols = total_symbols_count;
            chunks.iter().map(|c| self.calculate_repair_symbols(c.size)).sum::<u32>();
        let repair_symbols = total_symbols_count - source_symbols;

        Ok(ProcessResult {
            encoder_parameters,
            source_symbols,
            repair_symbols,
            symbols_directory: output_dir.to_string(),
            symbols_count: total_symbols_count,
            chunks: Some(chunks),
        })
    }

    fn encode_stream<R: Read>(
        &self,
        reader: &mut R,
        data_size: u64,
        output_path: &Path,
    ) -> Result<(Vec<u8>, Vec<String>), ProcessError> {
        // Calculate buffer size - aim for processing in 16 pieces or DEFAULT_STREAM_BUFFER_SIZE,
        // whichever is larger
        let buffer_size = std::cmp::max(
            (data_size / 16) as usize,
            std::cmp::min(DEFAULT_STREAM_BUFFER_SIZE, data_size as usize)
        );

        debug!("Using buffer size of {}B for {}B of data", buffer_size, data_size);

        // Create object transmission information
        let config = ObjectTransmissionInformation::with_defaults(
            data_size,
            self.config.symbol_size,
        );

        // Calculate repair symbols
        let repair_symbols = self.calculate_repair_symbols(data_size);

        // We'll accumulate the data and then encode
        let mut data = Vec::with_capacity(data_size as usize);
        let mut buffer = vec![0u8; buffer_size];

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // End of file
                Ok(n) => {
                    data.extend_from_slice(&buffer[..n]);
                }
                Err(e) => {
                    let err = format!("Failed to read data: {}", e);
                    self.set_last_error(err.clone());
                    return Err(ProcessError::IOError(e));
                }
            }
        }

        // Encode the data
        debug!("Encoding {} bytes of data with {} repair symbols",
               data.len(), repair_symbols);

        let encoder = Encoder::new(&data, config);
        let symbols = encoder.get_encoded_packets(repair_symbols);

        // Write symbols to disk
        let mut symbol_ids = Vec::with_capacity(symbols.len());

        for symbol in &symbols {
            let packet = symbol.serialize();
            let symbol_id = self.calculate_symbol_id(&packet);
            let output_file_path = output_path.join(&symbol_id);

            let mut file = BufWriter::new(File::create(&output_file_path)?);
            file.write_all(&packet)?;

            symbol_ids.push(symbol_id);
        }

        Ok((encoder.get_config().serialize().to_vec(), symbol_ids))
    }

    pub fn decode_symbols(
        &self,
        symbols_dir: &str,
        output_path: &str,
        encoder_params: &[u8; 12],
    ) -> Result<(), ProcessError> {
        // Check if we can take another task
        if !self.can_start_task() {
            return Err(ProcessError::ConcurrencyLimitReached);
        }

        let _guard = TaskGuard::new(&self.active_tasks);

        let symbols_dir = Path::new(symbols_dir);
        if !symbols_dir.exists() {
            let err = format!("Symbols directory not found: {:?}", symbols_dir);
            self.set_last_error(err.clone());
            return Err(ProcessError::FileNotFound(err));
        }

        // Check if this is a chunked file
        let mut chunks = Vec::new();
        for entry in fs::read_dir(symbols_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && path.file_name().unwrap().to_string_lossy().starts_with("chunk_") {
                chunks.push(path);
            }
        }

        if chunks.is_empty() {
            // No chunks, decode as a single file
            self.decode_single_file(symbols_dir, output_path, encoder_params)
        } else {
            // Decode each chunk and combine
            self.decode_chunked_file(&chunks, output_path, encoder_params)
        }
    }

    fn decode_single_file(
        &self,
        symbols_dir: &Path,
        output_path: &str,
        encoder_params: &[u8; 12],
    ) -> Result<(), ProcessError> {
        debug!("Decoding single file from {:?} to {:?}", symbols_dir, output_path);

        // Create decoder with the provided parameters
        let config = ObjectTransmissionInformation::deserialize(encoder_params);
        let decoder = Decoder::new(config);

        // Read symbol files
        let symbol_files = self.read_symbol_files(symbols_dir)?;

        let output_file = BufWriter::new(File::create(output_path)?);

        // Decode symbols
        self.decode_symbols_to_file(decoder, symbol_files, output_file)
    }

    fn decode_chunked_file(
        &self,
        chunks: &[PathBuf],
        output_path: &str,
        encoder_params: &[u8; 12],
    ) -> Result<(), ProcessError> {
        debug!("Decoding chunked file with {} chunks to {:?}", chunks.len(), output_path);

        let mut output_file = BufWriter::new(File::create(output_path)?);

        // Sort chunks by their index
        let mut sorted_chunks = chunks.to_vec();
        sorted_chunks.sort_by(|a, b| {
            let a_index = a.file_name().unwrap().to_string_lossy()
                .strip_prefix("chunk_").unwrap().parse::<usize>().unwrap();
            let b_index = b.file_name().unwrap().to_string_lossy()
                .strip_prefix("chunk_").unwrap().parse::<usize>().unwrap();
            a_index.cmp(&b_index)
        });

        // Process each chunk
        for chunk_path in sorted_chunks {
            debug!("Decoding chunk {:?}", chunk_path);

            // Create decoder with the provided parameters
            let config = ObjectTransmissionInformation::deserialize(encoder_params);
            let decoder = Decoder::new(config);

            // Read symbol files for this chunk
            let symbol_files = self.read_symbol_files(&chunk_path)?;

            // Decode this chunk directly to the output file
            let mut chunk_data = Vec::new();
            self.decode_symbols_to_memory(decoder, symbol_files, &mut chunk_data)?;

            // Write chunk data to the output file
            output_file.write_all(&chunk_data)?;
        }

        Ok(())
    }

    fn read_symbol_files(&self, dir: &Path) -> Result<Vec<Vec<u8>>, ProcessError> {
        let mut symbol_files = Vec::new();

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                // Read symbol file
                let mut symbol_data = Vec::new();
                let mut file = File::open(&path)?;
                file.read_to_end(&mut symbol_data)?;

                symbol_files.push(symbol_data);
            }
        }

        if symbol_files.is_empty() {
            let err = format!("No symbol files found in {:?}", dir);
            self.set_last_error(err.clone());
            return Err(ProcessError::DecodingFailed(err));
        }

        Ok(symbol_files)
    }

    fn decode_symbols_to_file<W: Write>(
        &self,
        mut decoder: Decoder,
        symbol_files: Vec<Vec<u8>>,
        mut output_file: W,
    ) -> Result<(), ProcessError> {
        let mut decoded = false;

        for symbol_data in symbol_files {
            let packet = EncodingPacket::deserialize(&symbol_data);

            if let Some(result) = decoder.decode(packet) {
                output_file.write_all(&result)?;
                decoded = true;
                break;
            }
        }

        if !decoded {
            let err = "Failed to decode symbols - not enough symbols available".to_string();
            self.set_last_error(err.clone());
            return Err(ProcessError::DecodingFailed(err));
        }

        Ok(())
    }

    fn decode_symbols_to_memory(
        &self,
        mut decoder: Decoder,
        symbol_files: Vec<Vec<u8>>,
        output: &mut Vec<u8>,
    ) -> Result<(), ProcessError> {
        let mut decoded = false;

        for symbol_data in symbol_files {
            let packet = EncodingPacket::deserialize(&symbol_data);

            if let Some(result) = decoder.decode(packet) {
                output.extend_from_slice(&result);
                decoded = true;
                break;
            }
        }

        if !decoded {
            let err = "Failed to decode symbols - not enough symbols available".to_string();
            self.set_last_error(err.clone());
            return Err(ProcessError::DecodingFailed(err));
        }

        Ok(())
    }

    // Helper methods

    fn can_start_task(&self) -> bool {
        let current = self.active_tasks.load(Ordering::SeqCst);
        current < self.config.concurrency_limit as usize
    }

    fn open_and_validate_file(&self, path: &Path) -> Result<(File, u64), ProcessError> {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                let err = format!("Failed to open file {:?}: {}", path, e);
                self.set_last_error(err.clone());
                return Err(ProcessError::IOError(e));
            }
        };

        let metadata = file.metadata()?;
        let file_size = metadata.len();

        if file_size == 0 {
            let err = format!("File is empty: {:?}", path);
            self.set_last_error(err.clone());
            return Err(ProcessError::EncodingFailed(err));
        }

        Ok((file, file_size))
    }

    fn calculate_repair_symbols(&self, data_len: u64) -> u32 {
        let redundancy_factor = self.config.redundancy_factor as f64;
        let symbol_size = self.config.symbol_size as f64;

        if data_len <= self.config.symbol_size as u64 {
            self.config.redundancy_factor as u32
        } else {
            (data_len as f64 * (redundancy_factor - 1.0) / symbol_size).ceil() as u32
        }
    }

    fn calculate_symbol_id(&self, symbol: &[u8]) -> String {
        let mut hasher = Sha3_256::new();
        hasher.update(symbol);
        bs58::encode(hasher.finalize()).into_string()
    }

    /// Estimate the peak memory required to encode or decode a chunk of the given size (in bytes).
    ///
    /// The estimate is based on the need to hold the entire chunk in memory, plus
    /// additional overhead for RaptorQ's internal allocations (intermediate symbols, etc).
    /// The multiplier is conservative and based on empirical observation.
    /// See ARCHITECTURE_REVIEW.md for details.
    const RAPTORQ_MEMORY_OVERHEAD_FACTOR: f64 = 2.5;

    fn estimate_memory_requirements(&self, data_size: u64) -> u32 {
        let mb = 1024 * 1024;
        let data_mb = (data_size as f64 / mb as f64).ceil() as u32;
        (data_mb as f64 * RAPTORQ_MEMORY_OVERHEAD_FACTOR).ceil() as u32
    }

    fn is_memory_available(&self, required_mb: u32) -> bool {
        required_mb <= self.config.max_memory_mb
    }
}

// RAII guard for task counting
struct TaskGuard<'a> {
    counter: &'a AtomicUsize,
}

impl<'a> TaskGuard<'a> {
    fn new(counter: &'a AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self { counter }
    }
}

impl<'a> Drop for TaskGuard<'a> {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}
