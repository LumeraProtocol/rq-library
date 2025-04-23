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
//! each symbol must be independently identifiable and retrievable, with a known `(block, symbol)` association.
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
use std::io::{self};
use std::path::Path;
use crate::file_io::{self, FileReader/*, FileWriter, DirManager*/};
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
    /// The layout file content, only populated when return_layout is true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_content: Option<String>,
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
const DEFAULT_SYMBOL_SIZE_B: u16 = 65535;  // 64 KiB, this is MAX possible value for now - symbol size is uint16 in RaptorQ
const DEFAULT_REDUNDANCY_FACTOR: u8 = 4;
// const DEFAULT_STREAM_BUFFER_SIZE_B: usize = 1 * 1024 * 1024; // 1 MiB
const DEFAULT_MAX_MEMORY_MB: u64 = 16 * 1024; // 16 GB
const DEFAULT_CONCURRENCY_LIMIT: u64 = 4;
const MEMORY_SAFETY_MARGIN: f64 = 1.5; // 50% safety margin

/// Estimate the peak memory required to encode or decode a block of the given size (in bytes).
///
/// The estimate is based on the need to hold the entire block in memory, plus
/// additional overhead for RaptorQ's internal allocations (intermediate symbols, etc.).
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
            symbol_size: DEFAULT_SYMBOL_SIZE_B,
            redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
            max_memory_mb: DEFAULT_MAX_MEMORY_MB,
            concurrency_limit: DEFAULT_CONCURRENCY_LIMIT,
        }
    }
}

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("IO error: {0}")]
    IOError(#[from] io::Error),

    #[error("File is not found: {0}")]
    FileNotFound(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Encoding failed: {0}")]
    EncodingFailed(String),

    #[error("Decoding failed: {0}")]
    DecodingFailed(String),

    #[error("Memory limit exceeded. Required: {required}MB, Available: {available}MB")]
    MemoryLimitExceeded {
        required: usize,
        available: usize,
    },

    #[error("Concurrency limit reached")]
    ConcurrencyLimitReached,
}

fn get_hash_as_b58(data: &[u8]) -> String {
    let hash = blake3::hash(data);
    bs58::encode(hash.as_bytes()).into_string()
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

    pub fn get_recommended_block_size(&self, file_size: usize) -> usize {
        let max_memory_bytes = self.config.max_memory_mb * 1024 * 1024;

        // If the file is smaller than max memory divided by MEMORY_SAFETY_MARGIN, don't split it
        let safe_memory = (max_memory_bytes as f64 / MEMORY_SAFETY_MARGIN) as usize;
        if file_size < safe_memory {
            return 0;
        }

        // Otherwise, aim for blocks that would use about 1/4 of available memory
        let target_block_size = safe_memory / 4;

        // Ensure block size is a multiple of symbol size for efficient processing
        let symbol_size = self.config.symbol_size as usize;
        let blocks = (target_block_size / symbol_size).max(1);
        blocks * symbol_size
    }

    /// Create metadata for a file without generating symbols
    ///
    /// This method calculates symbol IDs and creates a layout file without
    /// actually writing the symbol files to disk.
    ///
    /// # Arguments
    ///
    /// * `input_path` - Path to the input file
    /// * `output_path` - Path of the output layout file will be written
    /// * `block_size` - Size of blocks to process at once (0 = auto)
    /// * `return_layout` - Whether to return the layout as an object instead of writing to file
    ///
    /// # Returns
    ///
    /// * `Ok(ProcessResult)` with the layout information
    /// * `Err(ProcessError)` on failure
    pub fn create_metadata(
        &self,
        input_path: &str,
        output_dir: &str,
        block_size: usize,
        return_layout: bool,
    ) -> Result<ProcessResult, ProcessError> {
        // Prepare for processing
        let (file_reader, file_size, actual_block_size) = self.prepare_processing(
            input_path,
            block_size,
            false, // We don't force single file for metadata creation
        )?;
        
        debug!("Creating metadata for file: {:?} ({}B) with block size {}B",
               input_path, file_size, actual_block_size);
                
        // Process file blocks but with the metadata_only flag set to true
        self.process_file_blocks(
            file_reader,
            output_dir,
            actual_block_size,
            file_size,
            true, // metadata_only = true
            return_layout,
        )
    }

    /// Encode a file using RaptorQ
    pub fn encode_file(
        &self,
        input_path: &str,
        output_dir: &str,
        block_size: usize,
        force_single_file: bool,
    ) -> Result<ProcessResult, ProcessError> {
        // Prepare for processing
        let (file_reader, file_size, actual_block_size) = self.prepare_processing(
            input_path,
            block_size,
            force_single_file,
        )?;
        
        debug!("Processing file: {:?} ({}B) with block size {}B",
               input_path, file_size, actual_block_size);
                
        // Process file blocks - create actual symbols
        self.process_file_blocks(
            file_reader,
            output_dir,
            actual_block_size,
            file_size,
            false, // metadata_only = false
            false, // return_layout = false
        )
    }

    /// Prepare the file for processing
    ///
    /// This helper method handles common setup for encode_file and create_metadata
    fn prepare_processing(
        &self,
        input_path: &str,
        block_size: usize,
        force_single_file: bool,
    ) -> Result<(Box<dyn FileReader>, usize, usize), ProcessError> {
        // Check if we can take another task
        if !self.can_start_task() {
            return Err(ProcessError::ConcurrencyLimitReached);
        }
        let _guard = TaskGuard::new(&self.active_tasks);

        let (file_reader, file_size) = match self.open_and_validate_file(input_path) {
            Ok(result) => result,
            Err(e) => {
                self.set_last_error(e.to_string());
                return Err(e);
            }
        };

        // Determine the block size
        let actual_block_size = if force_single_file {
            let memory_required = self.estimate_memory_requirements(file_size);
            if !self.is_memory_available(memory_required) {
                let err = ProcessError::MemoryLimitExceeded {
                    required: memory_required,
                    available: self.config.max_memory_mb as usize,
                };
                self.set_last_error(err.to_string());
                return Err(err);
            }
            debug!("Processing the file forced to skip splitting: {:?} ({}B)", input_path, file_size);
            file_size
        } else if block_size == 0 && self.get_recommended_block_size(file_size) == 0 {
            // Use file size as block size for single file mode
            debug!("Processing the file without splitting: {:?} ({}B)", input_path, file_size);
            file_size
        } else if block_size == 0 {
            // Auto determine block size
            let recommended = self.get_recommended_block_size(file_size);
            debug!("Using the recommended block size: {}B", recommended);
            recommended
        } else {
            // Use provided block size
            debug!("Using the provided block size: {}B", block_size);
            block_size
        };

        Ok((file_reader, file_size, actual_block_size))
    }

    /// Process file blocks for encoding or metadata creation
    ///
    /// This method handles both creating actual symbols or just generating metadata
    fn process_file_blocks(
        &self,
        mut source_reader: Box<dyn FileReader>,
        output_dir: &str,
        block_size: usize,
        total_size: usize,
        metadata_only: bool,
        return_layout: bool,
    ) -> Result<ProcessResult, ProcessError> {
        let dir_manager = file_io::get_dir_manager();

        // Ensure output directory exists
        // if !return_layout {
        //     let exists = dir_manager.dir_exists(output_dir)
        //         .map_err(|e| ProcessError::IOError(io::Error::new(io::ErrorKind::Other, e)))?;
        //     if !exists {
        //         return Err(ProcessError::InvalidPath(format!("Output directory does not exist: {}", output_dir)));
        //     }
        // }

        let base_output_path = Path::new(output_dir);

        // Calculate the number of blocks
        let block_count = if block_size >= total_size {
            1
        } else {
            (total_size as f64 / block_size as f64).ceil() as usize
        };
        
        debug!("File will be split into {} blocks", block_count);

        // Process each block
        let mut blocks = Vec::with_capacity(block_count);
        let mut block_layouts = Vec::with_capacity(block_count);
        let mut total_symbols_count = 0;
        let mut total_repair_symbols = 0;

        for block_index in 0..block_count {
            let block_id = block_index;
            let block_dir = base_output_path.join(format!("block_{}", block_index));
            if !metadata_only {
                let block_dir_path = block_dir.to_string_lossy().to_string();
                dir_manager.create_dir_all(&block_dir_path).map_err(|e| {
                    ProcessError::IOError(io::Error::new(io::ErrorKind::Other, e))
                })?;
            }

            let actual_offset = (block_index * block_size) as u64;
            let remaining = total_size - actual_offset as usize;
            if remaining <= 0 {
                break;
            }
            let actual_block_size = std::cmp::min(block_size, remaining);
            let repair_symbols = self.calculate_repair_symbols(actual_block_size as u64);
            total_repair_symbols += repair_symbols;

            debug!("Processing block {} of {} bytes at offset {}",
                   block_index, actual_block_size, actual_offset);

            // Read this block into memory directly
            let mut block_data = vec![0u8; actual_block_size];
            source_reader.read_chunk(actual_offset, &mut block_data)
                .map_err(|e| ProcessError::IOError(io::Error::new(io::ErrorKind::Other, e)))?;

            // Process this block
            let (params, symbol_ids, hash) = self.encode_block(
                &mut block_data,
                actual_block_size as u64,
                repair_symbols,
                &block_dir,
                metadata_only,
            )?;

            // Add to BlockInfo for ProcessResult
            blocks.push(BlockInfo {
                block_id,
                encoder_parameters: params.clone(),
                original_offset: actual_offset,
                size: actual_block_size as u64,
                symbols_count: symbol_ids.len() as u64,
                source_symbols_count: symbol_ids.len() as u64 - repair_symbols,
                hash: hash.clone(),
            });

            // Add to BlockLayout for the metadata file
            block_layouts.push(BlockLayout {
                block_id,
                encoder_parameters: params.clone(),
                original_offset: actual_offset,
                size: actual_block_size as u64,
                symbols: symbol_ids.clone(), // Clone to avoid ownership issues
                hash,
            });

            total_symbols_count += symbol_ids.len() as u64;
            
            // No need to seek - we'll just read the next block at its offset
        }

        // Create layout information to save
        let layout = RaptorQLayout {
            blocks: block_layouts,
        };

        // Generate the layout JSON
        let layout_json = match serde_json::to_string_pretty(&layout) {
            Ok(json) => json,
            Err(e) => {
                let err = format!("Failed to serialize layout information: {}", e);
                self.set_last_error(err.clone());
                return Err(ProcessError::EncodingFailed(err));
            }
        };

        // If return_layout is true, we don't write to file but include in a result
        let layout_path_str;
        let layout_path;
        
        if !return_layout {
            // Save layout information to the output directory
            layout_path = base_output_path.join(LAYOUT_FILENAME);
            layout_path_str = layout_path.to_string_lossy().to_string();

            let mut writer = file_io::open_file_writer(&layout_path_str)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            writer.write_chunk(0, layout_json.as_bytes())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            writer.flush()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            debug!("Saved the layout file at {:?}", layout_path);
        } else {
            // For browser environments that can't write files, we use a virtual path
            layout_path_str = format!("{}/{}", output_dir, LAYOUT_FILENAME);
            debug!("Layout file (virtual): {}", layout_path_str);
        }

        let mut result = ProcessResult {
            total_symbols_count,
            total_repair_symbols,
            symbols_directory: output_dir.to_string(),
            blocks: Some(blocks),
            layout_file_path: layout_path_str,
            layout_content: None,
        };
        
        // If we're returning the layout directly, include it in the result
        if return_layout {
            result.layout_content = Some(layout_json);
        }
        
        Ok(result)
    }

    fn encode_block(
        &self,
        data: &[u8],
        data_size: u64,
        repair_symbols: u64,
        output_path: &Path,
        metadata_only: bool,
    ) -> Result<(Vec<u8>, Vec<String>, String), ProcessError> {
        //get hash of the data
        let hash_hex = get_hash_as_b58(data);

        // Create object transmission information
        let config = ObjectTransmissionInformation::with_defaults(
            data_size,
            self.config.symbol_size,
        );

        // Encode the data
        debug!("Encoding {} bytes of data with {} repair symbols",
               data.len(), repair_symbols);

        let encoder = Encoder::new(data, config);
        let symbols = encoder.get_encoded_packets(repair_symbols as u32);

        // Generate symbol ids (and write symbols to disk if not metadata_only)
        let mut symbol_ids = Vec::with_capacity(symbols.len());

        for symbol in &symbols {
            let packet = symbol.serialize();
            let symbol_id = self.calculate_symbol_id(&packet);
            
            // Only write the symbols to disk if we're not in metadata_only mode
            if !metadata_only {
                let output_file_path = output_path.join(&symbol_id);
                let path_str = output_file_path.to_string_lossy().to_string();
                let mut writer = file_io::open_file_writer(&path_str)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                writer.write_chunk(0, &packet)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                writer.flush()
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            }

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
        // Check if we can take another task and guard is done in the decode_symbols_with_layout

        let (mut file_reader, file_size) = match self.open_and_validate_file(layout_path) {
            Ok(result) => result,
            Err(e) => {
                self.set_last_error(e.to_string());
                return Err(e);
            }
        };

        let mut layout_content_bytes = vec![0; file_size];
        match file_reader.read_chunk(0, &mut layout_content_bytes) {
            Ok(_) => {},
            Err(e) => {
                let err = format!("Failed to read the layout file: {}", e);
                self.set_last_error(err.clone());
                return Err(ProcessError::DecodingFailed(err));
            }
        }
        
        let layout_content = match String::from_utf8(layout_content_bytes) {
            Ok(content) => content,
            Err(e) => {
                let err = format!("Layout file contains invalid UTF-8: {}", e);
                self.set_last_error(err.clone());
                return Err(ProcessError::DecodingFailed(err.to_string()));
            }
        };

        let layout = match serde_json::from_str::<RaptorQLayout>(&layout_content) {
            Ok(layout) => layout,
            Err(e) => {
                let err = format!("Failed to parse the layout file: {}", e);
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

        if layout.blocks.is_empty() {
            let err = "Layout file has the empty blocks array".to_string();
            self.set_last_error(err.clone());
            return Err(ProcessError::DecodingFailed(err));
        }

        let dir_manager = file_io::get_dir_manager();

        // check if the symbols dir exists
        let exists = dir_manager.dir_exists(symbols_dir)
            .map_err(|e| ProcessError::IOError(io::Error::new(io::ErrorKind::Other, e)))?;
        if !exists {
            return Err(ProcessError::InvalidPath(format!("Symbols directory does not exist: {}",symbols_dir)));
        }

        let mut output_writer = file_io::open_file_writer(output_path)
            .map_err(|e| ProcessError::IOError(io::Error::new(io::ErrorKind::Other, e)))?;

        // Process multiple blocks
        debug!("Decoding the file with {} blocks", layout.blocks.len());
        
        // Sort blocks by their block_id to ensure deterministic processing order
        let mut sorted_blocks = layout.blocks.clone();
        sorted_blocks.sort_by(|a, b| a.block_id.cmp(&b.block_id));

        let symbols_dir_path = Path::new(symbols_dir);

        // Iterate over blocks from the layout file (source of truth)
        for block_layout in &sorted_blocks {
            // Determine the block directory path
            let block_dir_name = format!("{}{}", BLOCK_DIR_PREFIX, block_layout.block_id);
            let block_dir_path = symbols_dir_path.join(block_dir_name);

            // Use the block directory if it exists, otherwise use symbols_dir
            let block_path: std::path::PathBuf;

            // check if the block dir exists
            let block_dir_path_str = block_dir_path.to_string_lossy().to_string();
            let exists = dir_manager.dir_exists(&block_dir_path_str)
                .map_err(|e| ProcessError::IOError(io::Error::new(io::ErrorKind::Other, e)))?;
            if exists {
                debug!("Using block directory: {}", block_dir_path_str);
                block_path = block_dir_path.clone();
            } else {
                debug!("Block directory does not exist, falling back to the symbols directory: {:?}", symbols_dir_path);
                block_path = symbols_dir_path.to_path_buf();
            }

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
            
            // Create the decoder with the parameters specific to this block
            let config = ObjectTransmissionInformation::deserialize(&block_encoder_params);
            let mut decoder = Decoder::new(config);
            
            // Skip blocks that have no symbols in the layout
            if block_layout.symbols.is_empty() {
                debug!("No symbols in the layout for block {}, skipping", block_layout.block_id);
                continue;
            }
            
            // Process symbols from the layout file
            let mut found_any = false;
            for symbol_id in &block_layout.symbols {
                let symbol_path = block_path.join(symbol_id);
                let symbol_path_str = symbol_path.to_string_lossy().to_string();

                let (mut symbol_reader, symbol_size) = match self.open_and_validate_file(&symbol_path_str) {
                    Ok(result) => result,
                    Err(_) => {
                        continue;
                    }
                };

                found_any = true;

                // Read symbol data
                let mut symbol_data = vec![0u8; symbol_size];
                match symbol_reader.read_chunk(0, &mut symbol_data) {
                    Ok(bytes_read) if bytes_read == symbol_size => {
                        // Successfully read the entire symbol
                    },
                    Ok(bytes_read) => {
                        debug!("Partial read of the symbol file {}: {} of {} bytes",
                               symbol_id, bytes_read, symbol_size);
                        continue;
                    },
                    Err(e) => {
                        debug!("Failed to read the symbol file {}: {}", symbol_id, e);
                        continue;
                    }
                };

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

            // Write to the correct position in the output file based on the block's original offset
            output_writer.write_chunk(block_layout.original_offset as usize, &block_data)
                .map_err(|e| ProcessError::IOError(io::Error::new(io::ErrorKind::Other, e)))?;
        }

        Ok(())
    }

    // Helper function to safely attempt the decoding a packet without panicking
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

    /// Read all symbol files from a directory
    #[allow(dead_code)]
    fn read_symbol_files(&self, dir: &Path) -> Result<Vec<Vec<u8>>, ProcessError> {
        let mut symbol_files = Vec::new();
        let _dir_str = dir.to_string_lossy().to_string();

        // Attempt to read symbol files with pattern "symbol_N.bin" which is used in tests
        for i in 0..100 { // Arbitrary limit to avoid infinite loop
            let file_name = format!("symbol_{}.bin", i);
            let path = dir.join(&file_name);
            let path_str = path.to_string_lossy().to_string();

            let (mut symbol_reader, symbol_size) = match self.open_and_validate_file(&path_str) {
                Ok(result) => result,
                Err(_) => {
                    continue;
                }
            };

            let mut symbol_data = vec![0u8; symbol_size];
            match symbol_reader.read_chunk(0, &mut symbol_data) {
                Ok(bytes_read) if bytes_read == symbol_size => {
                    // Successfully read the entire symbol
                    symbol_files.push(symbol_data);
                },
                Ok(_) => {
                    continue;
                },
                Err(_) => {
                    continue;
                }
            };
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
        let _dir_str = dir.to_string_lossy().to_string(); // Keep but prefix with underscore to avoid warning

        for symbol_id in symbol_ids {
            let symbol_path = dir.join(symbol_id);
            let symbol_path_str = symbol_path.to_string_lossy().to_string();

            // Try to open the file using FileReader
            match file_io::open_file_reader(&symbol_path_str) {
                Ok(mut reader) => {
                    // Get file size
                    let size = match reader.file_size() {
                        Ok(size) => size as usize,
                        Err(e) => {
                            debug!("Failed to get file size for symbol {:?}: {}", symbol_path, e);
                            continue;
                        }
                    };

                    // Read the file content
                    let mut symbol_data = vec![0u8; size];
                    match reader.read_chunk(0, &mut symbol_data) {
                        Ok(bytes_read) if bytes_read == size => {
                            // Successfully read the file
                            symbol_files.push(symbol_data);
                        },
                        Ok(bytes_read) => {
                            debug!("Partial read of symbol file {:?}: {} of {} bytes",
                                  symbol_path, bytes_read, size);
                            continue;
                        },
                        Err(e) => {
                            debug!("Failed to read symbol file {:?}: {}", symbol_path, e);
                            continue;
                        }
                    }
                },
                Err(e) => {
                    debug!("Failed to open symbol file {:?}: {}", symbol_path, e);
                    continue;
                }
            }
        }

        if symbol_files.is_empty() {
            let err = format!("No valid symbol files found from the provided IDs in {:?}", dir);
            self.set_last_error(err.clone());
            return Err(ProcessError::DecodingFailed(err));
        }

        Ok(symbol_files)
    }

    // Helper methods

    fn can_start_task(&self) -> bool {
        let current = self.active_tasks.load(Ordering::SeqCst);
        current < self.config.concurrency_limit as usize
    }

    fn open_and_validate_file(&self, path: &str) -> Result<(Box<dyn FileReader>, usize), ProcessError> {
        let file_reader = match file_io::open_file_reader(path) {
            Ok(reader) => reader,
            Err(e) => {
                let err = format!("Failed to open file {:?}: {}", path, e);
                return Err(ProcessError::FileNotFound(err));
            }
        };

        let file_size = match file_reader.file_size() {
            Ok(size) => size,
            Err(e) => {
                let err = format!("Failed to get file size for {:?}: {}", path, e);
                return Err(ProcessError::IOError(io::Error::new(io::ErrorKind::Other, err)));
            }
        };

        if file_size == 0 {
            let err = format!("File is empty: {:?}", path);
            return Err(ProcessError::EncodingFailed(err));
        }

        Ok((file_reader, file_size as usize))
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

    fn estimate_memory_requirements(&self, data_size: usize) -> usize {
        let mb = 1024 * 1024;
        let data_mb = (data_size as f64 / mb as f64).ceil() as usize;
        (data_mb as f64 * RAPTORQ_MEMORY_OVERHEAD_FACTOR).ceil() as usize
    }

    fn is_memory_available(&self, required_mb: usize) -> bool {
        required_mb <= self.config.max_memory_mb as usize
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

    // Helper functions to replace fs:: calls in tests with file_io abstractions
    fn create_dir(path: &Path) -> io::Result<()> {
        let path_str = path.to_string_lossy().to_string();
        file_io::get_dir_manager().create_dir_all(&path_str)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn write_file(path: &Path, contents: &[u8]) -> io::Result<()> {
        let path_str = path.to_string_lossy().to_string();
        let mut writer = file_io::open_file_writer(&path_str)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        writer.write_chunk(0, contents)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        writer.flush()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn read_file(path: &Path) -> io::Result<Vec<u8>> {
        let path_str = path.to_string_lossy().to_string();
        let mut reader = file_io::open_file_reader(&path_str)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let size = reader.file_size()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))? as usize;
        let mut data = vec![0u8; size];
        reader.read_chunk(0, &mut data)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(data)
    }

    fn read_file_to_string(path: &Path) -> io::Result<String> {
        let data = read_file(path)?;
        String::from_utf8(data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    // Limited check if a path exists - used for tests only
    fn path_exists(path: &Path) -> bool {
        let path_str = path.to_string_lossy().to_string();
        file_io::open_file_reader(&path_str).is_ok()
    }

    // Helper function to create an empty file
    fn create_empty_file(path: &Path) -> io::Result<()> {
        write_file(path, &[]).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    // Creates a temporary directory and returns its path
    fn create_temp_dir() -> (TempDir, PathBuf) {
        let dir = tempdir().expect("Failed to create temp directory");
        let path = dir.path().to_path_buf();
        (dir, path)
    }

    // Creates a test file with the specified size at a given path
    fn create_test_file(path: &Path, size: usize) -> io::Result<()> {
        let data = generate_test_data(size);
        write_file(path, &data).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
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
            write_file(&file_path, packet).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            file_paths.push(file_path.to_string_lossy().into_owned());
        }
        
        Ok(file_paths)
    }

    fn create_block_layout(original_data: &Vec<u8>, encoder_params: Vec<u8>, packets: Vec<Vec<u8>>) -> BlockLayout {
        let block_layout = BlockLayout {
            block_id: 0,
            encoder_parameters: encoder_params.to_vec(),
            original_offset: 0,
            size: original_data.len() as u64,
            symbols: packets.iter().enumerate().map(|(i, _)| format!("symbol_{}.bin", i)).collect(),
            hash: get_hash_as_b58(&original_data),
        };
        block_layout
    }

    fn count_files_in_dir(dir: &Path) -> usize {
        let dir_manager = file_io::get_dir_manager();
        let dir = dir.to_string_lossy().to_string();
        dir_manager.count_files(&dir).unwrap()
    }

    // Tests for ProcessorConfig

    #[test]
    fn test_config_default() {
        let config = ProcessorConfig::default();
        
        assert_eq!(config.symbol_size, DEFAULT_SYMBOL_SIZE_B);
        assert_eq!(config.redundancy_factor, DEFAULT_REDUNDANCY_FACTOR);
        assert_eq!(config.max_memory_mb, DEFAULT_MAX_MEMORY_MB);
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
            symbol_size: DEFAULT_SYMBOL_SIZE_B,
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
                symbol_size: DEFAULT_SYMBOL_SIZE_B,
                redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
                max_memory_mb: 8_000, // 8GB
                concurrency_limit: 4,
            },
            ProcessorConfig {
                symbol_size: DEFAULT_SYMBOL_SIZE_B,
                redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
                max_memory_mb: 16_000, // 16GB
                concurrency_limit: 4,
            },
            ProcessorConfig {
                symbol_size: DEFAULT_SYMBOL_SIZE_B,
                redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
                max_memory_mb: 32_000, // 32GB
                concurrency_limit: 4,
            },
            ProcessorConfig {
                symbol_size: DEFAULT_SYMBOL_SIZE_B,
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

                // For small files compared to memory, splitting might not be needed.
                // Convert max_memory_mb to the same type as file_size for comparison
                if file_size < (configs[i].max_memory_mb * 1024 * 1024 / 4) as usize {
                    assert_eq!(block_size, 0,
                               "File size of {} bytes should not need splitting with {} MB memory",
                               file_size, configs[i].max_memory_mb);
                }

                // For large files compared to memory, splitting is required
                if file_size > (configs[i].max_memory_mb * 1024 * 1024) as usize {
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
        create_empty_file(&input_path).expect("Failed to create empty file");
        
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
        assert_eq!(result.total_symbols_count, block.symbols_count);
        assert_eq!(result.total_repair_symbols, block.symbols_count - block.source_symbols_count);

        // Verify output directory exists and contains symbol files
        assert!(path_exists(&output_dir));

        let block_dir = output_dir.join(format!("{}0", BLOCK_DIR_PREFIX));
        let file_count = count_files_in_dir(&block_dir);
        assert!(file_count > 0);
        
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
        assert_eq!(result.total_symbols_count, block.symbols_count);
        assert_eq!(result.total_repair_symbols, block.symbols_count - block.source_symbols_count);

        // Verify output directory exists and contains symbol files
        assert!(path_exists(&output_dir));

        let block_dir = output_dir.join(format!("{}0", BLOCK_DIR_PREFIX));
        let file_count = count_files_in_dir(&block_dir);
        assert!(file_count > 0);
        
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
            symbol_size: DEFAULT_SYMBOL_SIZE_B,
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
        assert!(path_exists(&output_dir));

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
        assert!(!path_exists(&output_dir));
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.encode_file(
            input_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            0,
            false
        );
        
        assert!(result.is_ok());
        // Verify directory was created
        assert!(path_exists(&output_dir));
        
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
            symbol_size: DEFAULT_SYMBOL_SIZE_B,
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
            symbol_size: DEFAULT_SYMBOL_SIZE_B,
            redundancy_factor: DEFAULT_REDUNDANCY_FACTOR,
            max_memory_mb: DEFAULT_MAX_MEMORY_MB,
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
        // Use count_files_in_dir instead of fs::read_dir
        let symbol_count = count_files_in_dir(&block_dir);
        
        assert!(symbol_count > 0);
        assert_eq!(symbol_count, result.total_symbols_count as usize);
        
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
        create_dir(&symbols_dir).expect("Failed to create the symbols directory");

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
        //write the layout file
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        write_file(&layout_path, layout_json.as_bytes()).expect("Failed to write the layout file");

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
        create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create encoded symbols
        let (encoder_params, packets) = encode_test_data(
            &original_data,
            DEFAULT_SYMBOL_SIZE_B,
            10
        );
        
        // Create symbol files
        create_symbol_files(&symbols_dir, &packets).expect("Failed to create symbol files");
        
        // Create layout file
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);
        
        // Create a single BlockLayout for the entire file
        let block_layout = create_block_layout(&original_data, encoder_params, packets);
        
        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        write_file(&layout_path, layout_json.as_bytes()).expect("Failed to write layout file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        assert!(result.is_ok());
        
        // Verify the decoded file matches the original
        let decoded_data = read_file(&output_path).expect("Failed to read decoded file");
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
        create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Split into blocks (3 blocks of 100KB each)
        let block_size = 100 * 1024;
        let mut block_layouts = Vec::with_capacity(3);
        
        for i in 0..3 {
            let block_dir = symbols_dir.join(format!("block_{}", i));
            create_dir(&block_dir).expect("Failed to create block directory");
            
            let start = i * block_size;
            let end = start + block_size;
            let block_data = original_data[start..end].to_vec();

            //Get hash of block
            let block_hash = get_hash_as_b58(&block_data);

            // Encode block
            let (params, packets) = encode_test_data(
                &block_data,
                DEFAULT_SYMBOL_SIZE_B,
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
        write_file(&layout_path, layout_json.as_bytes()).expect("Failed to write layout file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        assert!(result.is_ok());
        
        // Verify the decoded file matches the original
        let decoded_data = read_file(&output_path).expect("Failed to read decoded file");
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
        create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create original data
        let original_data = generate_test_data(100 * 1024);
        
        // Create encoded symbols (only source symbols, no repair symbols)
        // This should not be enough for decoding
        let (encoder_params, mut packets) = encode_test_data(
            &original_data,
            DEFAULT_SYMBOL_SIZE_B,
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
        let block_layout = create_block_layout(&original_data, encoder_params, packets);

        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        write_file(&layout_path, layout_json.as_bytes()).expect("Failed to write layout file");
        
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
        create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create original data
        let original_data = generate_test_data(100 * 1024);
        
        // Create encoded symbols
        let (encoder_params, mut packets) = encode_test_data(
            &original_data,
            DEFAULT_SYMBOL_SIZE_B,
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
        
        // Create and save the layout file
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);

        // Create a single BlockLayout for the entire file
        let block_layout = create_block_layout(&original_data, encoder_params, packets);

        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        write_file(&layout_path, layout_json.as_bytes()).expect("Failed to write layout file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        // It might NOT fail
        assert!(result.is_ok());
        let decoded_data = read_file(&output_path).expect("Failed to read decoded file");
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
        create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create original data
        let original_data = generate_test_data(100 * 1024);
        
        // Create encoded symbols
        let (_, packets) = encode_test_data(
            &original_data,
            DEFAULT_SYMBOL_SIZE_B,
            10
        );
        
        // Create symbol files
        create_symbol_files(&symbols_dir, &packets).expect("Failed to create symbol files");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        // Use invalid encoder params
        let invalid_params = [255; 12]; // Completely invalid params
        
        // Create the layout file with invalid parameters
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);

        // Create a single BlockLayout with invalid parameters
        let block_layout = create_block_layout(&original_data, invalid_params.to_vec(), packets);

        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        write_file(&layout_path, layout_json.as_bytes()).expect("Failed to write the layout file");
        
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
        create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
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
            max_memory_mb: DEFAULT_MAX_MEMORY_MB,
            concurrency_limit: 1,
        };
        
        let processor = Arc::new(RaptorQProcessor::new(config));
        
        // Manually increment active_tasks to simulate a running task
        processor.active_tasks.fetch_add(1, Ordering::SeqCst);

        // Create a single BlockLayout for the entire file
        let block_layout = create_block_layout(&original_data, encoder_params, packets);
        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };

        // Attempt to start another task
        let result = processor.decode_symbols_with_layout(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            &layout
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
        
        // Create the symbols directory
        create_dir(&symbols_dir).expect("Failed to create the symbols directory");
        
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
        
        // Create the layout file
        let layout_path = symbols_dir.join(LAYOUT_FILENAME);
        
        // Create a single BlockLayout for the entire file
        let block_layout = create_block_layout(&original_data, encoder_params, packets);
        
        let layout = RaptorQLayout {
            blocks: vec![block_layout],
        };
        
        let layout_json = serde_json::to_string_pretty(&layout).expect("Failed to serialize layout");
        write_file(&layout_path, layout_json.as_bytes()).expect("Failed to write layout file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        assert!(result.is_ok());
        
        // Verify the output file was created
        assert!(path_exists(&output_path));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    // Tests for internal helper methods

    #[test]
    fn test_open_validate_file_ok() {
        let (temp_dir, dir_path) = create_temp_dir();
        let file_path = dir_path.join("valid.bin");
        let file_path_str = file_path.to_str().unwrap();
        
        // Create a valid file
        create_test_file(&file_path, 10 * 1024).expect("Failed to create the test file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.open_and_validate_file(&file_path_str);
        
        assert!(result.is_ok());
        let (_, size) = result.unwrap();
        assert_eq!(size, 10 * 1024);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_open_validate_file_not_found() {
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.open_and_validate_file("non_existent_file.txt");
        
        assert!(matches!(result, Err(ProcessError::FileNotFound(_))));
    }

    #[test]
    fn test_open_validate_file_empty() {
        let (temp_dir, dir_path) = create_temp_dir();
        let file_path = dir_path.join("empty.txt");
        let file_path_str = file_path.to_str().unwrap();
        
        // Create an empty file
        create_empty_file(&file_path).expect("Failed to create empty file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        let result = processor.open_and_validate_file(&file_path_str);
        
        assert!(matches!(result, Err(ProcessError::EncodingFailed(_))));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_calculate_repair_symbols_logic() {
        let processor = RaptorQProcessor::new(ProcessorConfig {
            symbol_size: 1000,
            redundancy_factor: 10,
            max_memory_mb: DEFAULT_MAX_MEMORY_MB,
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
        assert!(large_repair < large_data_len); // Should be less than data length
        
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
        let data_mb = 10usize; // 10 MB
        let data_size = data_mb * 1024 * 1024;
        let estimate = processor.estimate_memory_requirements(data_size);
        assert!(estimate > data_mb);
    }

    #[test]
    fn test_is_memory_available_logic() {
        let config = ProcessorConfig {
            symbol_size: DEFAULT_SYMBOL_SIZE_B,
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
        create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
        // Create some symbol files
        let symbol_data = vec![
            vec![1, 2, 3, 4],
            vec![5, 6, 7, 8],
            vec![9, 10, 11, 12],
        ];
        
        for (i, data) in symbol_data.iter().enumerate() {
            let file_path = symbols_dir.join(format!("symbol_{}.bin", i));
            write_file(&file_path, data).expect("Failed to write symbol file");
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
        create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
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
        
        assert!(matches!(result, Err(ProcessError::DecodingFailed(_))));
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_decode_multi_block_sorting() {
        let (temp_dir, dir_path) = create_temp_dir();
        let symbols_dir = dir_path.join("symbols");
        let output_path = dir_path.join("output.bin");
        
        // Create main symbols directory
        create_dir(&symbols_dir).expect("Failed to create symbols directory");
        
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
            create_dir(&block_dir).expect("Failed to create block directory");
            
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
                block_id: i,
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
        write_file(&layout_path, layout_json.as_bytes()).expect("Failed to write layout file");
        
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        let result = processor.decode_symbols(
            symbols_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
            layout_path.to_str().unwrap()
        );
        
        assert!(result.is_ok());
        
        // Verify the blocks were processed in the correct order (0, 1, 2)
        // by checking the output file matches the expected content
        let decoded_data = read_file(&output_path).expect("Failed to read decoded file");
        assert_eq!(decoded_data, expected_data);
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }

    #[test]
    fn test_create_metadata_without_saving_symbols() {
        // Create the test environment
        let (temp_dir, temp_path) = create_temp_dir();
        let input_file_path = temp_path.join("input.txt");

        // Create the test input file
        let test_data = generate_test_data(5000);
        write_file(&input_file_path, &test_data).unwrap();

        // Create processor
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        // Create metadata without writing symbols
        // Using the "root" directory for the layout file - 
        // WE ARE NOT CREATING NESTED DIRECTORIES FOR LAYOUT FILE!!!
        let result = processor.create_metadata(
            input_file_path.to_str().unwrap(),
            temp_path.to_str().unwrap(),
            0, // auto block size
            false, // write the layout file
        ).unwrap();
        
        // Verify the layout file exists
        let layout_file_path = Path::new(&result.layout_file_path);
        assert!(layout_file_path.exists());
        
        // Read and parse the layout file
        let layout_content = read_file_to_string(layout_file_path).unwrap();
        let layout: RaptorQLayout = serde_json::from_str(&layout_content).unwrap();
        
        // Verify layout contains expected data
        assert!(!layout.blocks.is_empty());
        assert!(!layout.blocks[0].symbols.is_empty());
        
        // Verify symbols were NOT created
        let symbol_id = &layout.blocks[0].symbols[0];
        let block_dir = temp_path.join(format!("block_{}", layout.blocks[0].block_id));
        let symbol_path = block_dir.join(symbol_id);
        assert!(!path_exists(&symbol_path), "Symbol file should not exist");
        
        // Ensure temp_dir isn't dropped early
        drop(temp_dir);
    }
    
    #[test]
    fn test_create_metadata_return_layout() {
        // Create the test environment
        let (_temp_dir, temp_path) = create_temp_dir();
        let input_file_path = temp_path.join("input.txt");
        let output_dir_path = temp_path.join("output");
        
        // Create the test input file
        let test_data = generate_test_data(5000);
        write_file(&input_file_path, &test_data).unwrap();
        
        // Create processor
        let processor = RaptorQProcessor::new(ProcessorConfig::default());
        
        // Create metadata and return layout content
        let result = processor.create_metadata(
            input_file_path.to_str().unwrap(),
            output_dir_path.to_str().unwrap(),
            0, // auto block size
            true, // return layout content
        ).unwrap();
        
        // Verify layout content is returned
        assert!(result.layout_content.is_some());
        
        // Parse layout content
        let layout: RaptorQLayout = serde_json::from_str(&result.layout_content.unwrap()).unwrap();
        
        // Verify layout contains expected data
        assert!(!layout.blocks.is_empty());
        assert!(!layout.blocks[0].symbols.is_empty());
        
        // Verify symbols were NOT created
        let symbol_id = &layout.blocks[0].symbols[0];
        let block_dir = output_dir_path.join(format!("block_{}", layout.blocks[0].block_id));
        let symbol_path = block_dir.join(symbol_id);
        assert!(!symbol_path.exists(), "Symbol file should not exist");
        
        // Verify layout file was not written to disk
        let layout_file_path = Path::new(&result.layout_file_path);
        assert!(!path_exists(layout_file_path), "Layout file should not exist on disk when return_layout is true");
    }
}
