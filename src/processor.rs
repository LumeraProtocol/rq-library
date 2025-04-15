//! RaptorQ Processing Logic
//!
//! # Memory Model and Blocks
//!
//! This module implements the core logic for encoding and decoding files using the RaptorQ algorithm.
//! Due to the design of the underlying `raptorq` crate (v2.0.0), both encoding and decoding require
//! the entire data segment (file or block) to be loaded into memory. There is no support for true
//! streaming input or output at the encoder/decoder level.
//!
//! ## Block Strategy
//!
//! To handle large files without exceeding memory limits, this module splits input files into fixed-size
//! blocks. Each block is processed independently using a dedicated RaptorQ encoder instance. The block size
//! is dynamically determined based on the configured `max_memory_mb`, ensuring bounded peak memory usage.
//! This approach makes encoding and decoding predictable and scalable for arbitrarily large input files.
//!
//! ## Why Manual Splitting Instead of RaptorQ Source Blocks
//!
//! Although RFC 6330 supports partitioning data into multiple "source blocks" for independent encoding,
//! this functionality is intentionally not used. The main reason is that the `raptorq` crate does not expose
//! source block metadata (such as block number or symbol mapping) in a way that is externally addressable.
//! Since this system stores encoded symbols in a Kademlia-based DHT under content-derived keys,
//! each symbol must be independently identifiable and retrievable, with known `(block, symbol)` association.
//!
//! Manual splitting provides precise control over this mapping by treating each block as an independent unit,
//! each with its own object transmission information (OTI) and set of symbol hashes. This model avoids the need
//! to reconstruct internal state from opaque symbol data and aligns better with the distributed storage layer.
//!
//! ## Limitations
//!
//! - Memory usage scales with the block size (or total file size if splitting is disabled).
//! - Decoding loads each block entirely into memory before reconstructing and writing to disk.
//! - For more architectural details, see ARCHITECTURE_REVIEW.md.

use raptorq::{Decoder, Encoder, EncodingPacket, ObjectTransmissionInformation};
use sha3::{Digest, Sha3_256};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write, BufWriter, Seek, SeekFrom};
use std::path::{Path};
use std::sync::atomic::{AtomicUsize, Ordering};
use parking_lot::Mutex;
use thiserror::Error;
use serde::{Serialize, Deserialize};
use log::{error, debug};

const LAYOUT_FILENAME: &str = "_raptorq_layout.json";
const BLOCK_DIR_PREFIX: &str = "block_";

/// Layout information structure saved to disk during encoding
/// and read during decoding to facilitate proper file reassembly.
#[derive(Debug, Serialize, Deserialize)]
pub struct RaptorQLayout {
    /// Detailed layout for each block. Will always contain at least one block,
    /// even if the file was processed as a single block.
    pub blocks: Vec<BlockLayout>,
}

/// Information about a single block
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlockLayout {
    /// Identifier for the block (0, 1, 2, etc.)
    pub block_id: usize,

    /// The 12-byte encoder parameters needed to initialize the RaptorQ decoder.
    pub encoder_parameters: Vec<u8>,

    /// The starting byte offset of this block in the original file.
    pub original_offset: u64,

    /// The exact size (in bytes) of the original data contained in this block.
    pub size: u64,

    /// List of symbol identifiers (hashes) generated specifically for this block.
    pub symbols: Vec<String>,

    /// Hash of the block data for integrity verification.
    pub hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessResult {
    pub total_symbols_count: u64,
    pub total_repair_symbols: u64,
    pub symbols_directory: String,
    pub blocks: Option<Vec<BlockInfo>>,
    pub layout_file_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockInfo {
    pub block_id: usize,
    pub encoder_parameters: Vec<u8>,
    pub original_offset: u64,
    pub size: u64,
    pub symbols_count: u64,
    pub source_symbols_count: u64,
    pub hash: String,
}
const DEFAULT_SYMBOL_SIZE: u16 = 65535;  // 64 KiB, this is MAX possible value for now - symbol size is uint16 in RaptorQ
const DEFAULT_REDUNDANCY_FACTOR: u8 = 4;
const DEFAULT_STREAM_BUFFER_SIZE: usize = 1 * 1024 * 1024; // 1 MiB
const DEFAULT_MAX_MEMORY: u64 = 16*1024; // 16 GB
const DEFAULT_CONCURRENCY_LIMIT: u64 = 4;
const MEMORY_SAFETY_MARGIN: f64 = 1.5; // 50% safety margin

/// Estimate the peak memory required to encode or decode a block of the given size (in bytes).
///
/// The estimate is based on the need to hold the entire block in memory, plus
/// additional overhead for RaptorQ's internal allocations (intermediate symbols, etc).
/// The multiplier is conservative and based on empirical observation.
/// See ARCHITECTURE_REVIEW.md for details.
const RAPTORQ_MEMORY_OVERHEAD_FACTOR: f64 = 2.5;


#[derive(Debug, Clone)]
pub struct ProcessorConfig {
    pub symbol_size: u16,
    pub redundancy_factor: u8,
    pub max_memory_mb: u64,
    pub concurrency_limit: u64,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            symbol_size: DEFAULT_SYMBOL_SIZE,
            redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
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
        required: u64,
        available: u64,
    },

    #[error("Concurrency limit reached")]
    ConcurrencyLimitReached,
}

fn get_hash_as_b58(data: &[u8]) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(data);
    bs58::encode(hasher.finalize()).into_string()
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

    #[allow(dead_code)] // Only used in tests
    pub fn get_config(&self) -> &ProcessorConfig {
        &self.config
    }

    pub fn get_recommended_block_size(&self, file_size: u64) -> usize {
        let max_memory_bytes = (self.config.max_memory_mb as u64) * 1024 * 1024;

        // If file is smaller than max memory divided by MEMORY_SAFETY_MARGIN, don't split it
        let safe_memory = (max_memory_bytes as f64 / MEMORY_SAFETY_MARGIN) as u64;
        if file_size < safe_memory {
            return 0;
        }

        // Otherwise, aim for blocks that would use about 1/4 of available memory
        let target_block_size = safe_memory / 4;

        // Ensure block size is a multiple of symbol size for efficient processing
        let symbol_size = self.config.symbol_size as u64;
        let blocks = (target_block_size / symbol_size).max(1);
        (blocks * symbol_size) as usize
    }

    pub fn encode_file(
        &self,
        input_path: &str,
        output_dir: &str,
        block_size: usize,
        force_single_file: bool,
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

        // Determine the block size
        let actual_block_size = if force_single_file {
            let memory_required = self.estimate_memory_requirements(file_size);
            if !self.is_memory_available(memory_required) {
                let err = ProcessError::MemoryLimitExceeded {
                    required: memory_required,
                    available: self.config.max_memory_mb,
                };
                self.set_last_error(err.to_string());
                return Err(err);
            }
            debug!("Processing file forced to skip splitting: {:?} ({}B)", input_path, file_size);
            file_size as usize
        } else if block_size == 0 && self.get_recommended_block_size(file_size) == 0 {
            // Use file size as block size for single file mode
            debug!("Processing file without splitting: {:?} ({}B)", input_path, file_size);
            file_size as usize
        } else if block_size == 0 {
            // Auto determine block size
            let recommended = self.get_recommended_block_size(file_size);
            debug!("Using recommended block size: {}B", recommended);
            recommended
        } else {
            // Use provided block size
            debug!("Using provided block size: {}B", block_size);
            block_size
        };

        debug!("Processing file: {:?} ({}B) with block size {}B",
               input_path, file_size, actual_block_size);

        self.encode_file_blocks(input_path, output_dir, actual_block_size)
    }

    fn encode_file_blocks(
        &self,
        input_path: &Path,
        output_dir: &str,
        block_size: usize,
    ) -> Result<ProcessResult, ProcessError> {
        // Ensure output directory exists
        let base_output_path = Path::new(output_dir);
        fs::create_dir_all(base_output_path)?;

        let (source_file, total_size) = self.open_and_validate_file(input_path)?;
        
        // Calculate number of blocks
        let block_count = if block_size >= total_size as usize {
            1
        } else {
            (total_size as f64 / block_size as f64).ceil() as usize
        };
        
        let mut reader = BufReader::new(source_file);
        
        debug!("File will be split into {} blocks", block_count);

        // Process each block
        let mut blocks = Vec::with_capacity(block_count);
        let mut block_layouts = Vec::with_capacity(block_count);
        let mut total_symbols_count = 0;
        let mut total_repair_symbols = 0;

        for block_index in 0..block_count {
            let block_id = block_index; // Now using usize instead of String
            let block_dir = base_output_path.join(format!("block_{}", block_index));
            fs::create_dir_all(&block_dir)?;

            let actual_offset = block_index as u64 * block_size as u64;
            let remaining = total_size - actual_offset;
            let actual_block_size = std::cmp::min(block_size as u64, remaining);
            let repair_symbols = self.calculate_repair_symbols(actual_block_size);
            total_repair_symbols += repair_symbols;

            debug!("Processing block {} of {} bytes at offset {}",
                   block_index, actual_block_size, actual_offset);

            // Create a limited reader for this block
            let mut block_reader = reader.by_ref().take(actual_block_size);

            // Process this block
            let (params, symbol_ids, hash) = self.encode_stream(
                &mut block_reader,
                actual_block_size,
                repair_symbols,
                &block_dir,
            )?;

            // Add to BlockInfo for ProcessResult
            blocks.push(BlockInfo {
                block_id,
                encoder_parameters: params.clone(),
                original_offset: actual_offset,
                size: actual_block_size,
                symbols_count: symbol_ids.len() as u64,
                source_symbols_count: symbol_ids.len() as u64 - repair_symbols,
                hash: hash.clone(),
            });

            // Add to BlockLayout for metadata file
            block_layouts.push(BlockLayout {
                block_id,
                encoder_parameters: params.clone(),
                original_offset: actual_offset,
                size: actual_block_size,
                symbols: symbol_ids.clone(), // Clone to avoid ownership issues
                hash,
            });

            total_symbols_count += symbol_ids.len() as u64;

            // Seek to the beginning of the next block
            reader.seek(SeekFrom::Start(actual_offset + actual_block_size))?;
        }

        // Create layout information to save
        let layout = RaptorQLayout {
            blocks: block_layouts,
        };

        // Save layout information to output directory
        let layout_path = base_output_path.join(LAYOUT_FILENAME);
        let layout_json = match serde_json::to_string_pretty(&layout) {
            Ok(json) => json,
            Err(e) => {
                let err = format!("Failed to serialize layout information: {}", e);
                self.set_last_error(err.clone());
                return Err(ProcessError::EncodingFailed(err));
            }
        };

        let mut layout_file = match File::create(&layout_path) {
            Ok(file) => file,
            Err(e) => {
                let err = format!("Failed to create layout file: {}", e);
                self.set_last_error(err.clone());
                return Err(ProcessError::IOError(e));
            }
        };

        if let Err(e) = layout_file.write_all(layout_json.as_bytes()) {
            let err = format!("Failed to write layout file: {}", e);
            self.set_last_error(err.clone());
            return Err(ProcessError::IOError(e));
        }

        debug!("Saved layout file for at {:?}", layout_path);

        Ok(ProcessResult {
            total_symbols_count,
            total_repair_symbols,
            symbols_directory: output_dir.to_string(),
            blocks: Some(blocks),
            layout_file_path: layout_path.to_string_lossy().to_string(),
        })
    }

    fn encode_stream<R: Read>(
        &self,
        reader: &mut R,
        data_size: u64,
        repair_symbols: u64,
        output_path: &Path,
    ) -> Result<(Vec<u8>, Vec<String>, String), ProcessError> {
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

        //get hash of the data
        let hash_hex = get_hash_as_b58(&data);

        // Encode the data
        debug!("Encoding {} bytes of data with {} repair symbols",
               data.len(), repair_symbols);

        let encoder = Encoder::new(&data, config);
        let symbols = encoder.get_encoded_packets(repair_symbols as u32);

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

        Ok((encoder.get_config().serialize().to_vec(), symbol_ids, hash_hex))
    }

    /// Decode RaptorQ symbols to recreate the original file, using a layout file path
    ///
    /// This function reads the RaptorQ layout information from the specified file path,
    /// which contains encoding parameters and blocks metadata.
    ///
    /// # Arguments
    ///
    /// * `symbols_dir` - Path to the directory containing the symbol files
    /// * `output_path` - Path where the decoded file will be written
    /// * `layout_path` - Path to the layout JSON file that contains encoding parameters and blocks information
    ///
    /// # Returns
    ///
    /// * `Ok(())` on successful decoding
    /// * `Err(ProcessError)` on error (e.g., file not found, decoding failed)
    pub fn decode_symbols(
        &self,
        symbols_dir: &str,
        output_path: &str,
        layout_path: &str,
    ) -> Result<(), ProcessError> {
        // Check if we can take another task and guard as done in the decode_symbols_with_layout

        // Load and parse layout file
        let layout_path = Path::new(layout_path);
        if !layout_path.exists() {
            let err = format!("Layout file not found: {:?}", layout_path);
            self.set_last_error(err.clone());
            return Err(ProcessError::FileNotFound(err));
        }

        let layout_content = match fs::read_to_string(layout_path) {
            Ok(content) => content,
            Err(e) => {
                let err = format!("Failed to read layout file: {}", e);
                self.set_last_error(err.clone());
                return Err(ProcessError::DecodingFailed(err));
            }
        };

        let layout = match serde_json::from_str::<RaptorQLayout>(&layout_content) {
            Ok(layout) => layout,
            Err(e) => {
                let err = format!("Failed to parse layout file: {}", e);
                self.set_last_error(err.clone());
                return Err(ProcessError::DecodingFailed(err));
            }
        };

        // Now that we have the layout, delegate to decode_symbols_with_layout
        self.decode_symbols_with_layout(symbols_dir, output_path, &layout)
    }

    /// Decode RaptorQ symbols to recreate the original file, using a RaptorQLayout object
    ///
    /// This function uses the provided RaptorQLayout structure which contains
    /// encoding parameters and block metadata
    ///
    /// # Arguments
    ///
    /// * `symbols_dir` - Path to the directory containing the symbol files
    /// * `output_path` - Path where the decoded file will be written
    /// * `layout` - The RaptorQLayout object containing encoding parameters and block information
    ///
    /// # Returns
    ///
    /// * `Ok(())` on successful decoding
    /// * `Err(ProcessError)` on error (e.g., file not found, decoding failed)
    pub fn decode_symbols_with_layout(
        &self,
        symbols_dir: &str,
        output_path: &str,
        layout: &RaptorQLayout,
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

        if layout.blocks.is_empty() {
            let err = "Layout file has empty blocks array".to_string();
            self.set_last_error(err.clone());
            return Err(ProcessError::DecodingFailed(err));
        }

        // Single unified decoding approach
        let mut output_file = BufWriter::new(File::create(output_path)?);

        // Process multiple blocks
        debug!("Decoding file with {} blocks", layout.blocks.len());
        
        // Sort blocks by their block_id to ensure deterministic processing order
        let mut sorted_blocks = layout.blocks.clone();
        sorted_blocks.sort_by(|a, b| a.block_id.cmp(&b.block_id));
        
        // Iterate over blocks from the layout file (source of truth)
        for block_layout in &sorted_blocks {
            // Determine the block directory path
            let block_dir_name = format!("{}{}", BLOCK_DIR_PREFIX, block_layout.block_id);
            let block_dir_path = symbols_dir.join(block_dir_name);
            
            // Use block directory if it exists, otherwise use symbols_dir
            let block_path = if block_dir_path.exists() && block_dir_path.is_dir() {
                block_dir_path
            } else {
                // Fall back to using the main symbols directory
                symbols_dir.to_path_buf()
            };

            // Extract encoder parameters for this specific block
            if block_layout.encoder_parameters.len() < 12 {
                let err = format!("Invalid encoder parameters in block {}", block_layout.block_id);
                self.set_last_error(err.clone());
                return Err(ProcessError::DecodingFailed(err));
            }
            
            let mut block_encoder_params = [0u8; 12];
            block_encoder_params.copy_from_slice(&block_layout.encoder_parameters[0..12]);
            
            // Decode block data
            let mut block_data = Vec::with_capacity(block_layout.size as usize);
            
            // Create decoder with the parameters specific to this block
            let config = ObjectTransmissionInformation::deserialize(&block_encoder_params);
            let mut decoder = Decoder::new(config);
            
            // Skip blocks that have no symbols in the layout
            if block_layout.symbols.is_empty() {
                debug!("No symbols in layout for block {}, skipping", block_layout.block_id);
                continue;
            }
            
            // Process symbols from the layout file
            let mut found_any = false;
            for symbol_id in &block_layout.symbols {
                let symbol_path = block_path.join(symbol_id);
                if !symbol_path.exists() {
                    debug!("Symbol {} not found in {:?}, skipping", symbol_id, block_path);
                    continue;
                }

                found_any = true;
                let mut symbol_data = Vec::new();
                let mut file = match File::open(&symbol_path) {
                    Ok(f) => f,
                    Err(e) => {
                        debug!("Failed to open symbol file {}: {}", symbol_id, e);
                        continue;
                    }
                };

                if let Err(e) = file.read_to_end(&mut symbol_data) {
                    debug!("Failed to read symbol file {}: {}", symbol_id, e);
                    continue;
                }

                let packet = EncodingPacket::deserialize(&symbol_data);
                if let Some(result) = self.safe_decode(&mut decoder, packet) {
                    block_data.extend_from_slice(&result);
                    break; // Successfully decoded
                }
            }
            
            // If we couldn't find any of the specified symbols
            if !found_any {
                let err = format!("None of the symbols for block {} could be found", block_layout.block_id);
                self.set_last_error(err.clone());
                return Err(ProcessError::DecodingFailed(err));
            }

            // Validate hash if available
            if !block_layout.hash.is_empty() {
                let computed_hash = get_hash_as_b58(&block_data);
                if computed_hash != block_layout.hash {
                    let err = format!("Hash mismatch for block {}: expected {}, got {}",
                                     block_layout.block_id, block_layout.hash, computed_hash);
                    self.set_last_error(err.clone());
                    return Err(ProcessError::DecodingFailed(err));
                }
            }

            // Seek to the correct position in the output file based on block's original offset
            output_file.seek(SeekFrom::Start(block_layout.original_offset))?;
            output_file.write_all(&block_data)?;
        }

        Ok(())
    }

    /// Read all symbol files from a directory
    #[allow(dead_code)]
    fn read_symbol_files(&self, dir: &Path) -> Result<Vec<Vec<u8>>, ProcessError> {
        let mut symbol_files = Vec::new();

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.file_name().unwrap_or_default() != LAYOUT_FILENAME {
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

    /// Read specific symbol files by ID from a directory
    #[allow(dead_code)]
    fn read_specific_symbol_files(&self, dir: &Path, symbol_ids: &[String]) -> Result<Vec<Vec<u8>>, ProcessError> {
        let mut symbol_files = Vec::new();

        for symbol_id in symbol_ids {
            let symbol_path = dir.join(symbol_id);
            if !symbol_path.exists() {
                debug!("Symbol file not found: {:?}, skipping", symbol_path);
                continue;
            }

            let mut symbol_data = Vec::new();
            match File::open(&symbol_path) {
                Ok(mut file) => {
                    if let Err(e) = file.read_to_end(&mut symbol_data) {
                        debug!("Failed to read symbol file {:?}: {}", symbol_path, e);
                        continue;
                    }
                },
                Err(e) => {
                    debug!("Failed to open symbol file {:?}: {}", symbol_path, e);
                    continue;
                }
            }

            symbol_files.push(symbol_data);
        }

        if symbol_files.is_empty() {
            let err = format!("No valid symbol files found from the provided IDs in {:?}", dir);
            self.set_last_error(err.clone());
            return Err(ProcessError::DecodingFailed(err));
        }

        Ok(symbol_files)
    }

    // Helper function to safely attempt decoding a packet without panicking
    fn safe_decode(&self, decoder: &mut Decoder, packet: EncodingPacket) -> Option<Vec<u8>> {
        // Use catch_unwind to prevent panics from propagating
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decoder.decode(packet)
        })).unwrap_or_else(|_| {
            // Log corrupted symbol and return None
            debug!("Skipping corrupted symbol: panic during decoding");
            None
        })
    }

    #[allow(dead_code)]
    fn decode_symbols_to_file<W: Write>(
        &self,
        mut decoder: Decoder,
        symbol_files: Vec<Vec<u8>>,
        mut output_file: W,
    ) -> Result<(), ProcessError> {
        let mut decoded = false;

        for symbol_data in symbol_files {
            let packet = EncodingPacket::deserialize(&symbol_data);
            
            // Use our safe decode helper that won't panic
            if let Some(result) = self.safe_decode(&mut decoder, packet) {
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

    #[allow(dead_code)]
    fn decode_symbols_to_memory(
        &self,
        mut decoder: Decoder,
        symbol_files: Vec<Vec<u8>>,
        output: &mut Vec<u8>,
    ) -> Result<(), ProcessError> {
        let mut decoded = false;

        for symbol_data in symbol_files {
            let packet = EncodingPacket::deserialize(&symbol_data);
            
            // Use our safe decode helper that won't panic
            if let Some(result) = self.safe_decode(&mut decoder, packet) {
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

    fn calculate_repair_symbols(&self, data_len: u64) -> u64 {
        let redundancy_factor = self.config.redundancy_factor as f64;
        let symbol_size = self.config.symbol_size as f64;

        if data_len <= self.config.symbol_size as u64 {
            self.config.redundancy_factor as u64
        } else {
            (data_len as f64 * (redundancy_factor - 1.0) / symbol_size).ceil() as u64
        }
    }

    fn calculate_symbol_id(&self, symbol: &[u8]) -> String {
        get_hash_as_b58(symbol)
    }

    fn estimate_memory_requirements(&self, data_size: u64) -> u64 {
        let mb = 1024 * 1024;
        let data_mb = (data_size as f64 / mb as f64).ceil() as u64;
        (data_mb as f64 * RAPTORQ_MEMORY_OVERHEAD_FACTOR).ceil() as u64
    }

    fn is_memory_available(&self, required_mb: u64) -> bool {
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::{tempdir, TempDir};
    use rand::{seq::SliceRandom, thread_rng};

    // Helper functions for test setup

    // Creates a temporary directory and returns its path
    fn create_temp_dir() -> (TempDir, PathBuf) {
        let dir = tempdir().expect("Failed to create temp directory");
        let path = dir.path().to_path_buf();
        (dir, path)
    }

    // Creates a test file with specified size at a given path
    fn create_test_file(path: &Path, size: usize) -> io::Result<()> {
        let mut file = File::create(path)?;
        let data = generate_test_data(size);
        file.write_all(&data)?;
        Ok(())
    }

    // Generates test data of specified size
    fn generate_test_data(size: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(size);
        for i in 0..size {
            data.push((i % 256) as u8);
        }
        data
    }

    // Creates encoded symbols for a given data vector
    fn encode_test_data(data: &[u8], symbol_size: u16, repair_symbols: u64) -> (Vec<u8>, Vec<Vec<u8>>) {
        let config = ObjectTransmissionInformation::with_defaults(
            data.len() as u64,
            symbol_size,
        );
        
        let encoder = Encoder::new(data, config);
        let packets = encoder.get_encoded_packets(repair_symbols as u32);
        
        let mut serialized_packets = Vec::new();
        for packet in &packets {
            serialized_packets.push(packet.serialize());
        }
        
        (encoder.get_config().serialize().to_vec(), serialized_packets)
    }

    // Creates symbol files in a directory from serialized packets
    fn create_symbol_files(dir: &Path, packets: &[Vec<u8>]) -> io::Result<Vec<String>> {
        let mut file_paths = Vec::new();
        
        for (i, packet) in packets.iter().enumerate() {
            let file_path = dir.join(format!("symbol_{}.bin", i));
            let mut file = File::create(&file_path)?;
            file.write_all(packet)?;
            file_paths.push(file_path.to_string_lossy().into_owned());
        }
        
        Ok(file_paths)
    }

    // Tests for ProcessorConfig

    #[test]
    fn test_config_default() {
        let config = ProcessorConfig::default();
        
        assert_eq!(config.symbol_size, DEFAULT_SYMBOL_SIZE);
        assert_eq!(config.redundancy_factor, DEFAULT_REDUNDANCY_FACTOR);
        assert_eq!(config.max_memory_mb, DEFAULT_MAX_MEMORY);
        assert_eq!(config.concurrency_limit, DEFAULT_CONCURRENCY_LIMIT);
    }

    #[test]
    fn test_config_custom() {
        let config = ProcessorConfig {
            symbol_size: 1000,
            redundancy_factor: 20,
            max_memory_mb: 512,
            concurrency_limit: 8,
        };
        
        assert_eq!(config.symbol_size, 1000);
        assert_eq!(config.redundancy_factor, 20);
        assert_eq!(config.max_memory_mb, 512);
        assert_eq!(config.concurrency_limit, 8);
    }

    // Tests for RaptorQProcessor::new

    #[test]
    fn test_new_processor() {
        let config = ProcessorConfig::default();
        let processor = RaptorQProcessor::new(config.clone());
        
        // Verify the config is stored correctly
        assert_eq!(processor.config.symbol_size, config.symbol_size);
        assert_eq!(processor.config.redundancy_factor, config.redundancy_factor);
        assert_eq!(processor.config.max_memory_mb, config.max_memory_mb);
        assert_eq!(processor.config.concurrency_limit, config.concurrency_limit);
    }

    #[test]
    fn test_new_processor_initial_state() {
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        // Verify initial state
        assert_eq!(processor.active_tasks.load(Ordering::SeqCst), 0);
        assert_eq!(processor.get_last_error(), "");
    }

    // Tests for RaptorQProcessor::get_last_error

    #[test]
    fn test_get_last_error_empty() {
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        assert_eq!(processor.get_last_error(), "");
    }

    #[test]
    fn test_get_last_error_after_error() {
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let error_message = "Test error message";
        
        // Set the error using the internal method
        processor.set_last_error(error_message.to_string());
        
        // Verify the error message is returned correctly
        assert_eq!(processor.get_last_error(), error_message);
    }

    // Tests for RaptorQProcessor::get_recommended_block_size

    #[test]
    fn test_block_size_small_file() {
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let file_size = 10 * 1024 * 1024; // 10 MB file
        
        // With default config (max_memory_mb = 1024), a 10 MB file should not need splitting
        let block_size = processor.get_recommended_block_size(file_size);
        
        assert_eq!(block_size, 0); // 0 means no splitting needed
    }

    #[test]
    fn test_block_size_large_file() {
        let config = ProcessorConfig {
            symbol_size: DEFAULT_SYMBOL_SIZE,
            redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
            max_memory_mb: 100, // Deliberately small to force splitting
            concurrency_limit: 4,
        };
        let processor = RaptorQProcessor::new(config);
        let file_size = 1024 * 1024 * 1024; // 1 GB file
        
        let block_size = processor.get_recommended_block_size(file_size);
        
        // Block size should be non-zero
        assert!(block_size > 0);
        // Block size should be a multiple of symbol size
        assert_eq!(block_size % processor.config.symbol_size as usize, 0);
    }

    #[test]
    fn test_block_size_edge_memory() {
        // Create an array of processor configs with different memory limits
        let configs = [
            ProcessorConfig {
                symbol_size: DEFAULT_SYMBOL_SIZE,
                redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
                max_memory_mb: 8_000, // 8GB
                concurrency_limit: 4,
            },
            ProcessorConfig {
                symbol_size: DEFAULT_SYMBOL_SIZE,
                redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
                max_memory_mb: 16_000, // 16GB
                concurrency_limit: 4,
            },
            ProcessorConfig {
                symbol_size: DEFAULT_SYMBOL_SIZE,
                redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
                max_memory_mb: 32_000, // 32GB
                concurrency_limit: 4,
            },
            ProcessorConfig {
                symbol_size: DEFAULT_SYMBOL_SIZE,
                redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
                max_memory_mb: 64_000, // 64GB
                concurrency_limit: 4,
            },
        ];

        // Create processors from configs
        let processors: Vec<RaptorQProcessor> = configs.iter()
            .map(|config| RaptorQProcessor::new(config.clone()))
            .collect();

        // Define file sizes to test
        let file_sizes = [
            10 * 1024 * 1024,          // 10 MB
            100 * 1024 * 1024,         // 100 MB
            1024 * 1024 * 1024,        // 1 GB
            10 * 1024 * 1024 * 1024,   // 10 GB
            100 * 1024 * 1024 * 1024,  // 100 GB
        ];

        // Test each file size with each processor
        for &file_size in &file_sizes {
            println!("Testing file size: {} bytes", file_size);

            // Get block sizes for all processors
            // Changed type from Vec<u64> to Vec<usize> to match the return type
            let block_sizes: Vec<usize> = processors.iter()
                .map(|processor| processor.get_recommended_block_size(file_size))
                .collect();

            for (i, &block_size) in block_sizes.iter().enumerate() {
                println!("  Processor with {} MB memory: block size = {} bytes",
                         configs[i].max_memory_mb, block_size);

                // For small files compared to memory, splitting might not be needed
                // Convert max_memory_mb to same type as file_size for comparison
                if file_size < (configs[i].max_memory_mb as u64) * 1024 * 1024 / 4 {
                    assert_eq!(block_size, 0,
                               "File size of {} bytes should not need splitting with {} MB memory",
                               file_size, configs[i].max_memory_mb);
                }

                // For large files compared to memory, splitting is required
                if file_size > (configs[i].max_memory_mb as u64) * 1024 * 1024 {
                    assert!(block_size > 0,
                            "File size of {} bytes should need splitting with {} MB memory",
                            file_size, configs[i].max_memory_mb);
                }

                // Compare with next processor (with more memory)
                if i < block_sizes.len() - 1 {
                    let next_block_size = block_sizes[i + 1];

                    // If both need splitting, more memory should allow larger blocks
                    if block_size > 0 && next_block_size > 0 {
                        assert!(next_block_size >= block_size,
                                "Processor with more memory should allow larger blocks");
                    }

                    // If current doesn't need splitting, next shouldn't either
                    if block_size == 0 {
                        assert_eq!(next_block_size, 0,
                                   "If a processor with less memory doesn't need splitting, a processor with more memory shouldn't either");
                    }
                }
            }
        }
    }

    #[test]
    fn test_blcok_size_zero_file() {
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let file_size = 0;
        
        let block_size = processor.get_recommended_block_size(file_size);
        
        assert_eq!(block_size, 0); // 0 means no splitting needed
    }

    // Tests for RaptorQProcessor::encode_file

    #[test]
    fn test_encode_file_not_found() {
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.encode_file("non_existent_file.txt", "output_dir", 0, false);
        
        assert!(matches!(result, Err(ProcessError::FileNotFound(_))));
    }

    #[test]
    fn test_encode_empty_file() {
        let (temp_dir, dir_path) = create_temp_dir();
        let input_path = dir_path.join("empty.txt");
        let output_dir = dir_path.join("output");
        
        // Create an empty file
        File::create(&input_path).expect("Failed to create empty file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.encode_file(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            0,
            false
        );
        
        assert!(matches!(result, Err(ProcessError::EncodingFailed(_))));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_encode_success_no_splitting() {
        let (temp_dir, dir_path) = create_temp_dir();
        let input_path = dir_path.join("small_file.bin");
        let output_dir = dir_path.join("output");
        
        // Create a small test file (100 KB)
        create_test_file(&input_path, 100 * 1024).expect("Failed to create test file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.encode_file(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            0, // 0 means auto-determine if splitting is needed
            false
        );
        
        assert!(result.is_ok());
        let result = result.unwrap();
        let blocks = result.blocks.unwrap();
        assert_eq!(blocks.len(), 1);
        let block = &blocks[0];
        assert_eq!(result.total_symbols_count, block.symbols_count as u64);
        assert_eq!(result.total_repair_symbols, (block.symbols_count - block.source_symbols_count) as u64);
        
        // Verify output directory exists and contains symbol files
        assert!(output_dir.exists());
        assert!(fs::read_dir(&output_dir).unwrap().count() > 0);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_encode_success_manual_no_splitting() {
        let (temp_dir, dir_path) = create_temp_dir();
        let input_path = dir_path.join("small_file.bin");
        let output_dir = dir_path.join("output");
        
        // Create a small test file (100 KB)
        create_test_file(&input_path, 100 * 1024).expect("Failed to create test file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.encode_file(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            500 * 1024, // Larger than file size
            false
        );

        assert!(result.is_ok());
        let result = result.unwrap();
        let blocks = result.blocks.unwrap();
        assert_eq!(blocks.len(), 1);
        let block = &blocks[0];
        assert_eq!(result.total_symbols_count, block.symbols_count as u64);
        assert_eq!(result.total_repair_symbols, (block.symbols_count - block.source_symbols_count) as u64);

        // Verify output directory exists and contains symbol files
        assert!(output_dir.exists());
        assert!(fs::read_dir(&output_dir).unwrap().count() > 0);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_encode_success_auto_splitting() {
        let (temp_dir, dir_path) = create_temp_dir();
        let input_path = dir_path.join("large_file.bin");
        let output_dir = dir_path.join("output");
        
        // Create a "large" test file (3 MB for test purposes)
        create_test_file(&input_path, 3 * 1024 * 1024).expect("Failed to create test file");
        
        let config = ProcessorConfig {
            symbol_size: DEFAULT_SYMBOL_SIZE,
            redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
            max_memory_mb: 1, // Very small memory limit to force splitting
            concurrency_limit: 4,
        };
        
        let processor = RaptorQProcessor::new(config);
        let result = processor.encode_file(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            0, // Auto splitting
            false
        );
        
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.blocks.is_some());
        assert!(!result.blocks.unwrap().is_empty());
        
        // Verify block directories exist
        assert!(output_dir.exists());
        assert!(fs::read_dir(&output_dir).unwrap()
            .any(|entry| entry.unwrap().path().file_name().unwrap()
                .to_string_lossy().starts_with(BLOCK_DIR_PREFIX)));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_encode_success_manual_splitting() {
        let (temp_dir, dir_path) = create_temp_dir();
        let input_path = dir_path.join("large_file.bin");
        let output_dir = dir_path.join("output");
        
        // Create a "large" test file (3 MB for test purposes)
        create_test_file(&input_path, 3 * 1024 * 1024).expect("Failed to create test file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let block_size = 1 * 1024 * 1024; // 1 MB blocks
        
        let result = processor.encode_file(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            block_size,
            false
        );
        
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.blocks.is_some());
        let blocks = result.blocks.unwrap();
        assert!(!blocks.is_empty());
        assert_eq!(blocks.len(), 3); // 3 MB should create 3 blocks of 1 MB each
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_encode_output_dir_creation() {
        let (temp_dir, dir_path) = create_temp_dir();
        let input_path = dir_path.join("file.bin");
        let output_dir = dir_path.join("nested/output/dir");
        
        // Create a test file
        create_test_file(&input_path, 100 * 1024).expect("Failed to create test file");
        
        // Verify directory doesn't exist yet
        assert!(!output_dir.exists());
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.encode_file(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            0,
            false
        );
        
        assert!(result.is_ok());
        // Verify directory was created
        assert!(output_dir.exists());
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_encode_memory_limit_exceeded() {
        let (temp_dir, dir_path) = create_temp_dir();
        let input_path = dir_path.join("file.bin");
        let output_dir = dir_path.join("output");
        
        // Create a test file (5 MB)
        create_test_file(&input_path, 5 * 1024 * 1024).expect("Failed to create test file");
        
        // Create processor with tiny memory limit
        let config = ProcessorConfig {
            symbol_size: DEFAULT_SYMBOL_SIZE,
            redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
            max_memory_mb: 1, // 1 MB max memory
            concurrency_limit: 4,
        };
        
        let processor = RaptorQProcessor::new(config);
        let result = processor.encode_file(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            0, // block_size = 0
            true // force_single_file = true, bypassing block size determination
        );
        
        assert!(matches!(result, Err(ProcessError::MemoryLimitExceeded { .. })));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_encode_concurrency_limit() {
        let (temp_dir, dir_path) = create_temp_dir();
        let input_path = dir_path.join("file.bin");
        let output_dir = dir_path.join("output");
        
        // Create a test file
        create_test_file(&input_path, 100 * 1024).expect("Failed to create test file");
        
        // Create processor with concurrency limit of 1
        let config = ProcessorConfig {
            symbol_size: DEFAULT_SYMBOL_SIZE,
            redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
            max_memory_mb: DEFAULT_MAX_MEMORY,
            concurrency_limit: 1,
        };
        
        let processor = Arc::new(RaptorQProcessor::new(config));
        
        // Start a task that will keep the processor busy
        // let processor_clone = Arc::clone(&processor);
        let input_path_str = input_path.to_str().unwrap().to_string();
        let output_dir_str = output_dir.to_str().unwrap().to_string();
        
        // Manually increment active_tasks to simulate a running task
        processor.active_tasks.fetch_add(1, Ordering::SeqCst);
        
        // Attempt to start another task
        let result = processor.encode_file(
            &input_path_str,
            &output_dir_str,
            0,
            false
        );
        
        // This should fail with ConcurrencyLimitReached
        assert!(matches!(result, Err(ProcessError::ConcurrencyLimitReached)));
        
        // Clean up
        processor.active_tasks.fetch_sub(1, Ordering::SeqCst);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_encode_verify_result() {
        let (temp_dir, dir_path) = create_temp_dir();
        let input_path = dir_path.join("file.bin");
        let output_dir = dir_path.join("output");
        
        // Create a test file (200 KB)
        let file_size = 200 * 1024;
        create_test_file(&input_path, file_size).expect("Failed to create test file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.encode_file(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            0,
            false
        );
        
        assert!(result.is_ok());
        let result = result.unwrap();
        
        // Verify result fields
        assert!(result.total_symbols_count > 0);
        assert!(result.total_repair_symbols > 0);
        assert_eq!(result.symbols_directory, output_dir.to_str().unwrap());
        assert!(result.total_symbols_count > 0);

        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_encode_verify_symbols() {
        let (temp_dir, dir_path) = create_temp_dir();
        let input_path = dir_path.join("file.bin");
        let output_dir = dir_path.join("output");
        let block_dir = output_dir.join("block_0");
        
        // Create a test file (200 KB)
        create_test_file(&input_path, 200 * 1024).expect("Failed to create test file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.encode_file(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            0,
            false
        );
        
        assert!(result.is_ok());
        let result = result.unwrap();
        
        // Check that symbol files are created in the output directory
        let symbol_files: Vec<_> = fs::read_dir(&block_dir)
            .expect("Failed to read output dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != LAYOUT_FILENAME) // Exclude layout file from count
            .collect();
        
        assert!(!symbol_files.is_empty());
        assert_eq!(symbol_files.len(), result.total_symbols_count as usize);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    // Tests for RaptorQProcessor::decode_symbols

    #[test]
    fn test_decode_symbols_dir_not_found() {
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.decode_symbols(
            "non_existent_dir",
            "output.bin",
            "layout.json"
        );
        
        assert!(matches!(result, Err(ProcessError::FileNotFound(_))));
    }

    #[test]
    fn test_decode_no_symbols_in_dir() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        let output_path = dir_path.join("output.bin");
        let layout_path = dir_path.join("layout.json");
        
        // Create an empty directory
        fs::create_dir(&symbols_dir).expect("Failed to create symbols directory");

        // create fake layout file
        let config = ObjectTransmissionInformation::with_defaults(1024, 128);
        let encoder_params = config.serialize().to_vec();
        let packets = vec![vec![0; 128]; 10];
        
        // Create a single BlockLayout for the entire file
        let block_layout = BlockLayout {
            block_id: 0,
            encoder_parameters: encoder_params,
            original_offset: 0,
            size: 1024,
            symbols: packets.iter().enumerate().map(|(i, _)| format!("symbol_{}.bin", i)).collect(),
            hash: "dummy_hash".to_string(),
        };
        
        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        //write layout file
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        fs::write(&layout_path, layout_json).expect("Failed to write layout file");

        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        assert!(matches!(result, Err(ProcessError::DecodingFailed(_))));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_decode_success_single_file() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        let output_path = dir_path.join("output.bin");
        
        // Create original data
        let original_data = generate_test_data(100 * 1024);
        
        // Create symbols directory
        fs::create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create encoded symbols
        let (encoder_params, packets) = encode_test_data(
            &original_data,
            DEFAULT_SYMBOL_SIZE,
            10
        );
        
        // Create symbol files
        create_symbol_files(&symbols_dir, &packets).expect("Failed to create symbol files");
        
        // Create layout file
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);
        
        // Create a single BlockLayout for the entire file
        let block_layout = BlockLayout {
            block_id: 0,
            encoder_parameters: encoder_params.to_vec(),
            original_offset: 0,
            size: original_data.len() as u64,
            symbols: packets.iter().enumerate().map(|(i, _)| format!("symbol_{}.bin", i)).collect(),
            hash: get_hash_as_b58(&original_data),
        };
        
        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        fs::write(&layout_path, layout_json).expect("Failed to write layout file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        assert!(result.is_ok());
        
        // Verify the decoded file matches the original
        let decoded_data = fs::read(output_path).expect("Failed to read decoded file");
        assert_eq!(decoded_data, original_data);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_decode_success_multi_block_file() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        let output_path = dir_path.join("output.bin");
        
        // Create original data
        let original_data = generate_test_data(300 * 1024);
        
        // Create main symbols directory
        fs::create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Split into blocks (3 blocks of 100KB each)
        let block_size = 100 * 1024;
        let mut block_layouts = Vec::with_capacity(3);
        
        for i in 0..3 {
            let block_dir = symbols_dir.join(format!("block_{}", i));
            fs::create_dir(&block_dir).expect("Failed to create block directory");
            
            let start = i * block_size;
            let end = start + block_size;
            let block_data = original_data[start..end].to_vec();

            //Get hash of block
            let block_hash = get_hash_as_b58(&block_data);

            // Encode block
            let (params, packets) = encode_test_data(
                &block_data,
                DEFAULT_SYMBOL_SIZE,
                5
            );

            // Create symbol files for this block
            create_symbol_files(&block_dir, &packets).expect("Failed to create symbol files");

            // Create layout file with block information
            let block_layout = BlockLayout {
                block_id: i,
                encoder_parameters: params,
                original_offset: (i * block_size) as u64,
                size: block_size as u64,
                symbols: (0..packets.len()).map(|j| format!("symbol_{}.bin", j)).collect(),
                hash: block_hash,
            };
            block_layouts.push(block_layout);
        }
        let layout = RaptorQLayout {
            blocks: block_layouts,
        };
        
        // Save layout file
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        fs::write(&layout_path, layout_json).expect("Failed to write layout file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        assert!(result.is_ok());
        
        // Verify the decoded file matches the original
        let decoded_data = fs::read(output_path).expect("Failed to read decoded file");
        assert_eq!(decoded_data, original_data);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_decode_insufficient_symbols() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        let output_path = dir_path.join("output.bin");
        
        // Create symbols directory
        fs::create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create original data
        let original_data = generate_test_data(100 * 1024);
        
        // Create encoded symbols (only source symbols, no repair symbols)
        // This should not be enough for decoding
        let (encoder_params, mut packets) = encode_test_data(
            &original_data,
            DEFAULT_SYMBOL_SIZE,
            10
        );
        
        // Remove all but one symbol to ensure decoding fails
        if packets.len() > 1 {
            packets = vec![packets[0].clone()];
        }
        
        // Create symbol files
        create_symbol_files(&symbols_dir, &packets).expect("Failed to create symbol files");
        
        // Create and save layout file
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);
        
        // Create a single BlockLayout for the entire file
        let block_layout = BlockLayout {
            block_id: 0,
            encoder_parameters: encoder_params.to_vec(),
            original_offset: 0,
            size: original_data.len() as u64,
            symbols: packets.iter().enumerate().map(|(i, _)| format!("symbol_{}.bin", i)).collect(),
            hash: get_hash_as_b58(&original_data),
        };
        
        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        fs::write(&layout_path, layout_json).expect("Failed to write layout file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        assert!(matches!(result, Err(ProcessError::DecodingFailed(_))));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_decode_corrupted_symbol() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        let output_path = dir_path.join("output.bin");
        
        // Create symbols directory
        fs::create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create original data
        let original_data = generate_test_data(100 * 1024);
        
        // Create encoded symbols
        let (encoder_params, mut packets) = encode_test_data(
            &original_data,
            DEFAULT_SYMBOL_SIZE,
            10
        );
        
        // Corrupt several random symbols
        if !packets.is_empty() {
            let num_to_corrupt = std::cmp::min(5, packets.len());

            // Get random indices to corrupt
            let mut rng = thread_rng();
            let mut indices: Vec<usize> = (0..packets.len()).collect();
            indices.shuffle(&mut rng);
            let indices_to_corrupt = &indices[0..num_to_corrupt];
            
            // Corrupt the selected packets
            for &idx in indices_to_corrupt {
                // Replace each selected packet with random data
                packets[idx] = vec![42; packets[idx].len()];
            }            
            println!("Corrupted {} out of {} packets", num_to_corrupt, packets.len());            
        }
        
        // Create symbol files
        create_symbol_files(&symbols_dir, &packets).expect("Failed to create symbol files");
        
        // Create and save layout file
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);
        // Create a single BlockLayout for the entire file
        let block_layout = BlockLayout {
            block_id: 0,
            encoder_parameters: encoder_params.to_vec(),
            original_offset: 0,
            size: original_data.len() as u64,
            symbols: packets.iter().enumerate().map(|(i, _)| format!("symbol_{}.bin", i)).collect(),
            hash: get_hash_as_b58(&original_data),
        };
        
        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        fs::write(&layout_path, layout_json).expect("Failed to write layout file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        // It might NOT fail
        assert!(result.is_ok());
        let decoded_data = fs::read(output_path).expect("Failed to read decoded file");
        assert_eq!(decoded_data, original_data);

        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_decode_invalid_encoder_params() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        let output_path = dir_path.join("output.bin");
        
        // Create symbols directory
        fs::create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create original data
        let original_data = generate_test_data(100 * 1024);
        
        // Create encoded symbols
        let (_, packets) = encode_test_data(
            &original_data,
            DEFAULT_SYMBOL_SIZE,
            10
        );
        
        // Create symbol files
        create_symbol_files(&symbols_dir, &packets).expect("Failed to create symbol files");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        // Use invalid encoder params
        let invalid_params = [255; 12]; // Completely invalid params
        
        // Create layout file with invalid parameters
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);
        
        // Create a single BlockLayout with invalid parameters
        let block_layout = BlockLayout {
            block_id: 0,
            encoder_parameters: invalid_params.to_vec(),
            original_offset: 0,
            size: original_data.len() as u64,
            symbols: packets.iter().enumerate().map(|(i, _)| format!("symbol_{}.bin", i)).collect(),
            hash: get_hash_as_b58(&original_data),
        };
        
        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        fs::write(&layout_path, layout_json).expect("Failed to write layout file");
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        assert!(matches!(result, Err(ProcessError::DecodingFailed(_))));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_decode_concurrency_limit() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        let output_path = dir_path.join("output.bin");
        
        // Create symbols directory
        fs::create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create some symbol files
        let original_data = generate_test_data(10 * 1024);
        let (encoder_params, packets) = encode_test_data(
            &original_data,
            1024,
            5
        );
        create_symbol_files(&symbols_dir, &packets).expect("Failed to create symbol files");
        
        // Create processor with concurrency limit of 1
        let config = ProcessorConfig {
            symbol_size: 1024,
            redundancy_factor: 10,
            max_memory_mb: DEFAULT_MAX_MEMORY,
            concurrency_limit: 1,
        };
        
        let processor = Arc::new(RaptorQProcessor::new(config));
        
        // Manually increment active_tasks to simulate a running task
        processor.active_tasks.fetch_add(1, Ordering::SeqCst);
        
        // Create layout file
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);
        
        // Create a single BlockLayout for the entire file
        let block_layout = BlockLayout {
            block_id: 0,
            encoder_parameters: encoder_params.to_vec(),
            original_offset: 0,
            size: original_data.len() as u64,
            symbols: packets.iter().enumerate().map(|(i, _)| format!("symbol_{}.bin", i)).collect(),
            hash: get_hash_as_b58(&original_data),
        };
        
        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        fs::write(&layout_path, layout_json).expect("Failed to write layout file");
        
        // Attempt to start another task
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        // This should fail with ConcurrencyLimitReached
        assert!(matches!(result, Err(ProcessError::ConcurrencyLimitReached)));
        
        // Clean up
        processor.active_tasks.fetch_sub(1, Ordering::SeqCst);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_decode_output_file_creation() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        let output_path = dir_path.join("output.bin");
        
        // Create symbols directory
        fs::create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create original data
        let original_data = generate_test_data(10 * 1024);
        
        // Create encoded symbols
        let (encoder_params, packets) = encode_test_data(
            &original_data,
            1024,
            5
        );
        
        // Create symbol files
        create_symbol_files(&symbols_dir, &packets).expect("Failed to create symbol files");
        
        // Create layout file
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);
        
        // Create a single BlockLayout for the entire file
        let block_layout = BlockLayout {
            block_id: 0,
            encoder_parameters: encoder_params.to_vec(),
            original_offset: 0,
            size: original_data.len() as u64,
            symbols: packets.iter().enumerate().map(|(i, _)| format!("symbol_{}.bin", i)).collect(),
            hash: get_hash_as_b58(&original_data),
        };
        
        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        fs::write(&layout_path, layout_json).expect("Failed to write layout file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        assert!(result.is_ok());
        
        // Verify output file was created
        assert!(output_path.exists());
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    // Tests for internal helper methods

    #[test]
    fn test_open_validate_file_ok() {
        let (temp_dir, dir_path) = create_temp_dir();
        let file_path = dir_path.join("valid.bin");
        
        // Create a valid file
        create_test_file(&file_path, 10 * 1024).expect("Failed to create test file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.open_and_validate_file(&file_path);
        
        assert!(result.is_ok());
        let (_, size) = result.unwrap();
        assert_eq!(size, 10 * 1024);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_open_validate_file_not_found() {
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.open_and_validate_file(Path::new("non_existent_file.txt"));
        
        assert!(matches!(result, Err(ProcessError::IOError(_))));
    }

    #[test]
    fn test_open_validate_file_empty() {
        let (temp_dir, dir_path) = create_temp_dir();
        let file_path = dir_path.join("empty.txt");
        
        // Create an empty file
        File::create(&file_path).expect("Failed to create empty file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.open_and_validate_file(&file_path);
        
        assert!(matches!(result, Err(ProcessError::EncodingFailed(_))));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_calculate_repair_symbols_logic() {
        let processor = RaptorQProcessor::new(ProcessorConfig {
            symbol_size: 1000,
            redundancy_factor: 10,
            max_memory_mb: DEFAULT_MAX_MEMORY,
            concurrency_limit: 4,
        });
        
        // Test with data smaller than symbol size
        let small_data_len = 500;
        let small_repair = processor.calculate_repair_symbols(small_data_len);
        assert_eq!(small_repair, 10); // Should be equal to redundancy_factor
        
        // Test with data larger than symbol size
        let large_data_len = 10000;
        let large_repair = processor.calculate_repair_symbols(large_data_len);
        assert!(large_repair > 0);
        assert!(large_repair < large_data_len as u64); // Should be less than data length
        
        // Test with exactly symbol size
        let exact_size_data_len = 1000;
        let exact_repair = processor.calculate_repair_symbols(exact_size_data_len);
        assert!(exact_repair > 0);
    }

    #[test]
    fn test_estimate_memory_logic() {
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        // Test with increasing data sizes
        let sizes = [
            1024 * 1024,         // 1 MB
            10 * 1024 * 1024,    // 10 MB
            100 * 1024 * 1024,   // 100 MB
        ];
        
        let mut last_estimate = 0;
        for size in sizes {
            let estimate = processor.estimate_memory_requirements(size);
            assert!(estimate > 0);
            assert!(estimate > last_estimate);
            last_estimate = estimate;
        }
        
        // Verify estimate is larger than actual data size (due to overhead)
        let data_mb = 10 as u64; // 10 MB
        let data_size = data_mb * 1024 * 1024;
        let estimate = processor.estimate_memory_requirements(data_size);
        assert!(estimate > data_mb);
    }

    #[test]
    fn test_is_memory_available_logic() {
        let config = ProcessorConfig {
            symbol_size: DEFAULT_SYMBOL_SIZE,
            redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
            max_memory_mb: 100,
            concurrency_limit: 4,
        };
        
        let processor = RaptorQProcessor::new(config);
        
        // Test with available memory
        assert!(processor.is_memory_available(50));
        assert!(processor.is_memory_available(100));
        
        // Test with exceeding memory
        assert!(!processor.is_memory_available(101));
        assert!(!processor.is_memory_available(200));
    }

    #[test]
    fn test_read_symbol_files_ok() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        
        // Create symbols directory
        fs::create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create some symbol files
        let symbol_data = vec![
            vec![1, 2, 3, 4],
            vec![5, 6, 7, 8],
            vec![9, 10, 11, 12],
        ];
        
        for (i, data) in symbol_data.iter().enumerate() {
            let file_path = symbols_dir.join(format!("symbol_{}.bin", i));
            let mut file = File::create(&file_path).expect("Failed to create symbol file");
            file.write_all(data).expect("Failed to write symbol data");
        }
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.read_symbol_files(&symbols_dir);
        
        assert!(result.is_ok());
        let files = result.unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0], symbol_data[0]);
        assert_eq!(files[1], symbol_data[1]);
        assert_eq!(files[2], symbol_data[2]);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_read_symbol_files_empty() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        
        // Create an empty symbols directory
        fs::create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.read_symbol_files(&symbols_dir);
        
        assert!(matches!(result, Err(ProcessError::DecodingFailed(_))));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_read_symbol_files_io_error() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        
        // Don't create the directory to simulate an IO error
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.read_symbol_files(&symbols_dir);
        
        assert!(matches!(result, Err(ProcessError::IOError(_))));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_decode_multi_block_sorting() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        let output_path = dir_path.join("output.bin");
        
        // Create main symbols directory
        fs::create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create blocks in reverse order to test sorting
        let block_order = vec![2, 0, 1]; // Deliberately out of order

        // Store the original data we expect in the correct order
        let mut expected_data = Vec::new();
        
        for &block_idx in &[0, 1, 2] { // Correct order for verification
            let block_data = generate_test_data(1000 + block_idx * 100); // Different sizes
            expected_data.extend_from_slice(&block_data);
        }

        let mut block_layouts_map: HashMap<i32, BlockLayout> = HashMap::new();

        // Create the blocks in mixed order
        for &i in &block_order {
            let block_dir = symbols_dir.join(format!("block_{}", i));
            fs::create_dir(&block_dir).expect("Failed to create block directory");
            
            let block_data = generate_test_data(1000 + i * 100);
            let block_hash = get_hash_as_b58(&block_data);

            // Encode block
            let (params, packets) = encode_test_data(
                &block_data,
                1000,
                5
            );

            // Create symbol files for this block
            create_symbol_files(&block_dir, &packets).expect("Failed to create symbol files");

            let mut offset = 0;
            if i == 1 {
                offset = 1000;
            } else if i == 2 {
                offset = 2100;
            }
            
            let block_layout = BlockLayout {
                block_id: i as usize,
                encoder_parameters: params,
                original_offset: offset,
                size: (1000 + i * 100) as u64,
                symbols: (0..packets.len()).map(|j| format!("symbol_{}.bin", j)).collect(),
                hash: block_hash,
            };

            block_layouts_map.insert(i as i32, block_layout);
        }
        
        // Create layout file
        let mut block_layouts = Vec::with_capacity(3);
        for &i in &[0, 1, 2] {  // In correct order for the layout
            block_layouts.push(block_layouts_map[&i].clone());
        }

        let layout = RaptorQLayout {
            blocks: block_layouts,
        };
        
        // Save layout file
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        fs::write(&layout_path, layout_json).expect("Failed to write layout file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        assert!(result.is_ok());
        
        // Verify the blocks were processed in the correct order (0, 1, 2)
        // by checking the output file matches the expected content
        let decoded_data = fs::read(output_path).expect("Failed to read decoded file");
        assert_eq!(decoded_data, expected_data);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }
}
