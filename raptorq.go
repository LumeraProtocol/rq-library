package raptorq

/*
#cgo LDFLAGS: -L${SRCDIR}/target/release -lrq_library -lm
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>

// Function declarations matching the Rust library's exported functions
extern uintptr_t raptorq_init_session(uint16_t symbol_size, uint8_t redundancy_factor, uint64_t max_memory_mb, uint64_t concurrency_limit);
extern _Bool raptorq_free_session(uintptr_t session_id);
extern int32_t raptorq_encode_file(uintptr_t session_id, const char *input_path, const char *output_dir, uintptr_t chunk_size, char *result_buffer, uintptr_t result_buffer_len);
extern int32_t raptorq_get_last_error(uintptr_t session_id, char *error_buffer, uintptr_t error_buffer_len);
extern int32_t raptorq_decode_symbols(uintptr_t session_id, const char *symbols_dir, const char *output_path, const char *layout_path);
extern uintptr_t raptorq_get_recommended_chunk_size(uintptr_t session_id, uint64_t file_size);
extern int32_t raptorq_version(char *version_buffer, uintptr_t version_buffer_len);
*/
import "C"
import (
	"encoding/json"
	"fmt"
	"runtime"
	"sync"
	"unsafe"
)

// Lock to protect session ID counter
var sessionMutex sync.Mutex
var sessions = make(map[uintptr]struct{})

// RaptorQProcessor represents a RaptorQ processing session
type RaptorQProcessor struct {
	SessionID uintptr
}

// ProcessorConfig holds configuration for the RaptorQ processor
type ProcessorConfig struct {
	SymbolSize       uint16 `json:"symbol_size"`
	RedundancyFactor uint8  `json:"redundancy_factor"`
	MaxMemoryMB      uint64 `json:"max_memory_mb"`
	ConcurrencyLimit uint64 `json:"concurrency_limit"`
}

// ProcessResult holds information about the processing results
type ProcessResult struct {
	SourceSymbols    uint32  `json:"source_symbols"`
	RepairSymbols    uint32  `json:"repair_symbols"`
	SymbolsDirectory string  `json:"symbols_directory"`
	SymbolsCount     uint32  `json:"symbols_count"`
	Chunks           []Chunk `json:"chunks,omitempty"`
}

// Chunk represents information about a processed chunk
type Chunk struct {
	ChunkID        string `json:"chunk_id"`
	OriginalOffset uint64 `json:"original_offset"`
	Size           uint64 `json:"size"`
	SymbolsCount   uint32 `json:"symbols_count"`
}

// NewRaptorQProcessor creates a new RaptorQ processor with the specified configuration
func NewRaptorQProcessor(symbolSize uint16, redundancyFactor uint8, maxMemoryMB uint64, concurrencyLimit uint64) (*RaptorQProcessor, error) {
	sessionID := C.raptorq_init_session(
		C.uint16_t(symbolSize),
		C.uint8_t(redundancyFactor),
		C.uint64_t(maxMemoryMB),
		C.uint64_t(concurrencyLimit),
	)

	if sessionID == 0 {
		return nil, fmt.Errorf("failed to initialize RaptorQ session")
	}

	// Register session
	sessionMutex.Lock()
	sessions[uintptr(sessionID)] = struct{}{}
	sessionMutex.Unlock()

	processor := &RaptorQProcessor{
		SessionID: uintptr(sessionID),
	}

	// Set finalizer to clean up session
	runtime.SetFinalizer(processor, finalizeProcessor)

	return processor, nil
}

// Free manually frees the RaptorQ session
// Returns true if the session was successfully freed, false otherwise
func (p *RaptorQProcessor) Free() bool {
	if p.SessionID != 0 {
		result := C.raptorq_free_session(C.uintptr_t(p.SessionID))
		success := bool(result)

		if success {
			// Unregister session
			sessionMutex.Lock()
			delete(sessions, p.SessionID)
			sessionMutex.Unlock()

			p.SessionID = 0
		}

		return success
	}
	return false
}

// Finalizer for RaptorQProcessor
func finalizeProcessor(p *RaptorQProcessor) {
	p.Free()
}

// EncodeFile encodes a file using RaptorQ
func (p *RaptorQProcessor) EncodeFile(inputPath, outputDir string, chunkSize int) (*ProcessResult, error) {
	if p.SessionID == 0 {
		return nil, fmt.Errorf("RaptorQ session is closed")
	}

	cInputPath := C.CString(inputPath)
	defer C.free(unsafe.Pointer(cInputPath))

	cOutputDir := C.CString(outputDir)
	defer C.free(unsafe.Pointer(cOutputDir))

	// Buffer for result (4KB should be enough for metadata)
	resultBufSize := 4096
	resultBuf := (*C.char)(C.malloc(C.size_t(resultBufSize)))
	defer C.free(unsafe.Pointer(resultBuf))

	res := C.raptorq_encode_file(
		C.uintptr_t(p.SessionID),
		cInputPath,
		cOutputDir,
		C.uintptr_t(chunkSize),
		resultBuf,
		C.uintptr_t(resultBufSize),
	)

	switch res {
	case 0:
		// Success
	case -1:
		return nil, fmt.Errorf("generic error: %s", p.getLastError())
	case -2:
		return nil, fmt.Errorf("file not found: %s", p.getLastError())
	case -3:
		return nil, fmt.Errorf("encoding failed: %s", p.getLastError())
	case -4:
		return nil, fmt.Errorf("invalid session")
	case -5:
		return nil, fmt.Errorf("memory allocation error")
	default:
		return nil, fmt.Errorf("unknown error code %d: %s", res, p.getLastError())
	}

	// Parse the JSON result
	resultJSON := C.GoString(resultBuf)
	var result ProcessResult
	if err := json.Unmarshal([]byte(resultJSON), &result); err != nil {
		return nil, fmt.Errorf("failed to parse result: %w", err)
	}

	return &result, nil
}

// DecodeSymbols decodes RaptorQ symbols back to the original file
func (p *RaptorQProcessor) DecodeSymbols(symbolsDir, outputPath, layoutPath string) error {
	if p.SessionID == 0 {
		return fmt.Errorf("RaptorQ session is closed")
	}

	// Input validation
	if symbolsDir == "" || outputPath == "" || layoutPath == "" {
		return fmt.Errorf("symbolsDir, outputPath, and layoutPath cannot be empty")
	}

	cSymbolsDir := C.CString(symbolsDir)
	defer C.free(unsafe.Pointer(cSymbolsDir))

	cOutputPath := C.CString(outputPath)
	defer C.free(unsafe.Pointer(cOutputPath))

	cLayoutPath := C.CString(layoutPath)
	defer C.free(unsafe.Pointer(cLayoutPath))

	res := C.raptorq_decode_symbols(
		C.uintptr_t(p.SessionID),
		cSymbolsDir,
		cOutputPath,
		cLayoutPath,
	)

	switch res {
	case 0:
		return nil
	case -1:
		return fmt.Errorf("generic error: %s", p.getLastError())
	case -2:
		return fmt.Errorf("file not found: %s", p.getLastError())
	case -3:
		return fmt.Errorf("decoding failed: %s", p.getLastError())
	case -4:
		return fmt.Errorf("invalid session")
	default:
		return fmt.Errorf("unknown error code %d: %s", res, p.getLastError())
	}
}

// GetRecommendedChunkSize returns a recommended chunk size for a file
func (p *RaptorQProcessor) GetRecommendedChunkSize(fileSize uint64) int {
	if p.SessionID == 0 {
		return 0
	}

	return int(C.raptorq_get_recommended_chunk_size(
		C.uintptr_t(p.SessionID),
		C.uint64_t(fileSize),
	))
}

// GetVersion returns the library version
func GetVersion() string {
	bufSize := 128
	versionBuf := (*C.char)(C.malloc(C.size_t(bufSize)))
	defer C.free(unsafe.Pointer(versionBuf))

	res := C.raptorq_version(versionBuf, C.size_t(bufSize))

	if res != 0 {
		return "Unknown version"
	}

	return C.GoString(versionBuf)
}

// Internal function to get the last error message
func (p *RaptorQProcessor) getLastError() string {
	if p.SessionID == 0 {
		return "Session closed"
	}

	bufSize := 1024
	errorBuf := (*C.char)(C.malloc(C.size_t(bufSize)))
	defer C.free(unsafe.Pointer(errorBuf))

	result := C.raptorq_get_last_error(
		C.uintptr_t(p.SessionID),
		errorBuf,
		C.size_t(bufSize),
	)

	if result != 0 {
		return "Error retrieving error message"
	}

	return C.GoString(errorBuf)
}
