package raptorq

// #cgo LDFLAGS: -L${SRCDIR}/lib -lraptorq_lib -lm
// #include <stdlib.h>
// #include <stdint.h>
// #include "include/raptorq.h"
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
var sessions = make(map[usize]struct{})

// RaptorQProcessor represents a RaptorQ processing session
type RaptorQProcessor struct {
	SessionID usize
}

// ProcessResult holds information about the processing results
type ProcessResult struct {
	EncoderParameters []byte   `json:"encoder_parameters"`
	SourceSymbols     uint32   `json:"source_symbols"`
	RepairSymbols     uint32   `json:"repair_symbols"`
	SymbolsDirectory  string   `json:"symbols_directory"`
	SymbolsCount      uint32   `json:"symbols_count"`
	Chunks            []Chunk  `json:"chunks,omitempty"`
}

// Chunk represents information about a processed chunk
type Chunk struct {
	ChunkID        string `json:"chunk_id"`
	OriginalOffset uint64 `json:"original_offset"`
	Size           uint64 `json:"size"`
	SymbolsCount   uint32 `json:"symbols_count"`
}

// usize is a platform-dependent unsigned integer type to match Rust's usize
type usize C.uintptr_t

// NewRaptorQProcessor creates a new RaptorQ processor with the specified configuration
func NewRaptorQProcessor(symbolSize uint16, redundancyFactor uint8, maxMemoryMB uint32, concurrencyLimit uint32) (*RaptorQProcessor, error) {
	sessionID := C.raptorq_init_session(
		C.uint16_t(symbolSize),
		C.uint8_t(redundancyFactor),
		C.uint32_t(maxMemoryMB),
		C.uint32_t(concurrencyLimit),
	)

	if sessionID == 0 {
		return nil, fmt.Errorf("failed to initialize RaptorQ session")
	}

	// Register session
	sessionMutex.Lock()
	sessions[usize(sessionID)] = struct{}{}
	sessionMutex.Unlock()

	processor := &RaptorQProcessor{
		SessionID: usize(sessionID),
	}

	// Set finalizer to clean up session
	runtime.SetFinalizer(processor, finalizeProcessor)

	return processor, nil
}

// Free manually frees the RaptorQ session
func (p *RaptorQProcessor) Free() {
	if p.SessionID != 0 {
		C.raptorq_free_session(C.uintptr_t(p.SessionID))

		// Unregister session
		sessionMutex.Lock()
		delete(sessions, p.SessionID)
		sessionMutex.Unlock()

		p.SessionID = 0
	}
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
		C.size_t(chunkSize),
		resultBuf,
		C.size_t(resultBufSize),
	)

	if res != 0 {
		errMsg := p.getLastError()
		return nil, fmt.Errorf("encoding failed with code %d: %s", res, errMsg)
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
func (p *RaptorQProcessor) DecodeSymbols(symbolsDir, outputPath string, encoderParams []byte) error {
	if p.SessionID == 0 {
		return fmt.Errorf("RaptorQ session is closed")
	}

	// Validate encoder parameters
	if len(encoderParams) != 12 {
		return fmt.Errorf("encoder parameters must be 12 bytes, got %d", len(encoderParams))
	}

	cSymbolsDir := C.CString(symbolsDir)
	defer C.free(unsafe.Pointer(cSymbolsDir))

	cOutputPath := C.CString(outputPath)
	defer C.free(unsafe.Pointer(cOutputPath))

	// Convert encoder params to C array
	cEncoderParams := (*C.uint8_t)(C.malloc(12))
	defer C.free(unsafe.Pointer(cEncoderParams))

	for i, b := range encoderParams {
		*(*C.uint8_t)(unsafe.Pointer(uintptr(unsafe.Pointer(cEncoderParams)) + uintptr(i))) = C.uint8_t(b)
	}

	res := C.raptorq_decode_symbols(
		C.uintptr_t(p.SessionID),
		cSymbolsDir,
		cOutputPath,
		cEncoderParams,
		12,
	)

	if res != 0 {
		errMsg := p.getLastError()
		return fmt.Errorf("decoding failed with code %d: %s", res, errMsg)
	}

	return nil
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

	C.raptorq_get_last_error(
		C.uintptr_t(p.SessionID),
		errorBuf,
		C.size_t(bufSize),
	)

	return C.GoString(errorBuf)
}
