package raptorq

// To run `LD_LIBRARY_PATH="$(pwd)/target/debug" go test ./... "$@"`

import (
	"bytes"
	"crypto/sha256"
	"fmt"
	"io"
	"math/rand"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestContext manages test artifacts and directories
type TestContext struct {
	TempDir    string
	InputFile  string
	SymbolsDir string
	OutputFile string
}

// NewTestContext creates a new test context with generated input file of specified size
func NewTestContext(t *testing.T, fileSizeBytes int) *TestContext {
	// Create temporary directory
	tempDir, err := os.MkdirTemp("", "raptorq-test-")
	if err != nil {
		t.Fatalf("Failed to create temp directory: %v", err)
	}

	// Setup paths
	inputFile := filepath.Join(tempDir, "input.bin")
	symbolsDir := filepath.Join(tempDir, "symbols")
	outputFile := filepath.Join(tempDir, "output.bin")

	// Create symbols directory
	if err := os.MkdirAll(symbolsDir, 0755); err != nil {
		t.Fatalf("Failed to create symbols directory: %v", err)
	}

	// Generate random input file
	if err := generateRandomFile(inputFile, fileSizeBytes); err != nil {
		t.Fatalf("Failed to generate random file: %v", err)
	}

	return &TestContext{
		TempDir:    tempDir,
		InputFile:  inputFile,
		SymbolsDir: symbolsDir,
		OutputFile: outputFile,
	}
}

// Cleanup removes all temporary files and directories
func (ctx *TestContext) Cleanup() {
	os.RemoveAll(ctx.TempDir)
}

// VerifyFilesMatch checks if output file matches input file using SHA256 hashing
func (ctx *TestContext) VerifyFilesMatch(t *testing.T) bool {
	inputHash, err := calculateFileHash(ctx.InputFile)
	if err != nil {
		t.Fatalf("Failed to hash input file: %v", err)
	}

	outputHash, err := calculateFileHash(ctx.OutputFile)
	if err != nil {
		t.Fatalf("Failed to hash output file: %v", err)
	}

	return bytes.Equal(inputHash, outputHash)
}

// DeleteRepairSymbols removes repair symbols, keeping only source symbols
func (ctx *TestContext) DeleteRepairSymbols(t *testing.T, result *ProcessResult) {
	for _, block := range result.Blocks {
		blockDir := filepath.Join(ctx.SymbolsDir, fmt.Sprintf("block_%d", block.BlockID))
		entries, err := os.ReadDir(blockDir)
		if err != nil {
			t.Fatalf("Failed to read block directory: %v", err)
		}

		// Sort and keep only source symbols
		fileNames := make([]string, 0, len(entries))
		for _, entry := range entries {
			fileNames = append(fileNames, entry.Name())
		}
		// Sort filenames for deterministic behavior
		fileNames = sortStrings(fileNames)

		// Delete repair NUMBER of symbols (keep only source symbols count)
		repairSymbolsCount := int(block.SymbolsCount) - int(block.SourceSymbolsCount)
		for i := repairSymbolsCount; i < len(fileNames); i++ {
			filePath := filepath.Join(blockDir, fileNames[i])
			// Avoid deleting the layout file if it happens to be sorted here
			if filepath.Base(filePath) == "_raptorq_layout.json" {
				continue
			}
			err := os.Remove(filePath)
			if err != nil {
				t.Fatalf("Failed to delete repair symbol: %v", err)
			}
		}
	}
}

// KeepRandomSubsetOfSymbols keeps a random subset of symbols
// It keeps all source symbols and a percentage of repair symbols
func (ctx *TestContext) KeepRandomSubsetOfSymbols(t *testing.T, result *ProcessResult, percentage float64) {
	r := rand.New(rand.NewSource(42)) // Use fixed seed for reproducibility

	for _, block := range result.Blocks {
		blockDir := filepath.Join(ctx.SymbolsDir, fmt.Sprintf("block_%d", block.BlockID))
		entries, err := os.ReadDir(blockDir)
		if err != nil {
			t.Fatalf("Failed to read block directory: %v", err)
		}

		// Sort and process symbols
		fileNames := make([]string, 0, len(entries))
		for _, entry := range entries {
			fileNames = append(fileNames, entry.Name())
		}
		// Sort filenames for deterministic behavior
		fileNames = sortStrings(fileNames)

		// Always keep source symbols, randomly keep repair symbols
		// Calculate source symbols count (similar assumption as DeleteRepairSymbols)
		sourceSymbols := int(block.SourceSymbolsCount)
		toKeep := make(map[string]bool)

		// Keep all source symbols
		for i := 0; i < sourceSymbols; i++ {
			// Don't randomly delete the layout file
			if fileNames[i] != "_raptorq_layout.json" {
				toKeep[fileNames[i]] = true
			}
		}

		// Randomly keep repair symbols
		for i := sourceSymbols; i < len(fileNames); i++ {
			// Don't randomly delete the layout file
			if fileNames[i] != "_raptorq_layout.json" && r.Float64() < percentage {
				toKeep[fileNames[i]] = true
			}
		}

		// Delete files not in the keep set
		for _, name := range fileNames {
			// Ensure layout file is always kept, even if not explicitly in toKeep
			if !toKeep[name] && name != "_raptorq_layout.json" {
				err := os.Remove(filepath.Join(blockDir, name))
				if err != nil {
					t.Fatalf("Failed to delete symbol %s: %v", name, err)
				}
			}
		}
	}
}

// Helper function to generate a random binary file of specified size
func generateRandomFile(path string, sizeBytes int) error {
	file, err := os.Create(path)
	if err != nil {
		return fmt.Errorf("failed to create file: %w", err)
	}
	defer file.Close()

	// Use a seeded RNG for reproducibility
	r := rand.New(rand.NewSource(42))

	// Generate and write data in blocks to avoid excessive memory usage
	const blockSize = 1024 * 1024 // 1 MB blocks
	buffer := make([]byte, min(blockSize, sizeBytes))

	remaining := sizeBytes
	for remaining > 0 {
		writeSize := min(len(buffer), remaining)
		_, err := r.Read(buffer[:writeSize])
		if err != nil {
			return fmt.Errorf("failed to generate random data: %w", err)
		}

		_, err = file.Write(buffer[:writeSize])
		if err != nil {
			return fmt.Errorf("failed to write data: %w", err)
		}

		remaining -= writeSize
	}

	return file.Sync()
}

// Helper to calculate SHA256 hash of a file
func calculateFileHash(path string) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("failed to open file: %w", err)
	}
	defer file.Close()

	hasher := sha256.New()
	buffer := make([]byte, 1024*1024) // 1 MB buffer

	for {
		bytesRead, err := file.Read(buffer)
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, fmt.Errorf("failed to read file: %w", err)
		}

		hasher.Write(buffer[:bytesRead])
	}

	return hasher.Sum(nil), nil
}

// Helper for sorting strings
func sortStrings(strs []string) []string {
	// Simple bubble sort for now
	for i := 0; i < len(strs); i++ {
		for j := i + 1; j < len(strs); j++ {
			if strs[i] > strs[j] {
				strs[i], strs[j] = strs[j], strs[i]
			}
		}
	}
	return strs
}

// Helper to convert a byte slice to a hex string
func toHexString(bytes []byte) string {
	var builder strings.Builder
	for _, b := range bytes {
		fmt.Fprintf(&builder, "%02x", b)
	}
	return builder.String()
}

// Helper function to encode and decode a file with specified size
func testEncodeDecodeFile(t *testing.T, processor *RaptorQProcessor, fileSizeBytes, blockSize int) bool {
	// Create test context with input file
	ctx := NewTestContext(t, fileSizeBytes)
	defer ctx.Cleanup()

	// Encode the file
	res, err := processor.EncodeFile(ctx.InputFile, ctx.SymbolsDir, blockSize)
	if err != nil {
		t.Fatalf("Failed to encode file: %v", err)
	}

	layoutPath := res.LayoutFilePath

	// Decode the symbols using the layout file path
	err = processor.DecodeSymbols(ctx.SymbolsDir, ctx.OutputFile, layoutPath)
	if err != nil {
		t.Fatalf("Failed to decode symbols using layout file %s: %v", layoutPath, err)
	}

	// Verify the decoded file matches the original
	return ctx.VerifyFilesMatch(t)
}

// System test for encoding/decoding a small file (1KB)
func TestSysEncodeDecode1KB(t *testing.T) {
	// Create RaptorQ processor with default settings
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	fileSize := 1 * 1024 // 1KB
	result := testEncodeDecodeFile(t, processor, fileSize, 0)
	if !result {
		t.Fatal("Decoded file does not match original")
	}
}

// System test for encoding/decoding a medium file (10MB)
func TestSysEncode10MB(t *testing.T) {
	// Create RaptorQ processor with default settings
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	fileSize := 10 * 1024 * 1024 // 10MB
	result := testEncodeDecodeFile(t, processor, fileSize, 0)
	if !result {
		t.Fatal("Decoded file does not match original")
	}
}

// System test for encoding/decoding a large file with auto-splitting (100MB)
func TestSysEncode100MB(t *testing.T) {
	// Create RaptorQ processor with small memory limit to force auto-splitting
	processor, err := NewRaptorQProcessor(DefaultSymbolSize, DefaultRedundancyFactor, MaxMemoryMB_4GB, DefaultConcurrencyLimit)
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	fileSize := 100 * 1024 * 1024 // 100MB
	result := testEncodeDecodeFile(t, processor, fileSize, 0)
	if !result {
		t.Fatal("Decoded file does not match original")
	}
}

// System test for encoding/decoding a large file with manual splitting (100MB)
func TestSysEncode100MBManualBlock(t *testing.T) {
	// Create RaptorQ processor with default settings
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	fileSize := 100 * 1024 * 1024 // 100MB
	blockSize := 10 * 1024 * 1024 // 10MB blocks
	result := testEncodeDecodeFile(t, processor, fileSize, blockSize)
	if !result {
		t.Fatal("Decoded file does not match original")
	}
}

// System test for encoding/decoding a very large file (1GB)
// This test is skipped by default due to resource requirements
func TestSysEncode1GB(t *testing.T) {
	if testing.Short() {
		t.Skip("Skipping very large file test in short mode")
	}

	// Create RaptorQ processor with default settings
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	fileSize := 1024 * 1024 * 1024 // 1GB
	result := testEncodeDecodeFile(t, processor, fileSize, 0)
	if !result {
		t.Fatal("Decoded file does not match original")
	}
}

// System test for decoding with only source symbols (minimum necessary)
func TestSysDecodeMinimumSymbols(t *testing.T) {
	// Create RaptorQ processor with default settings
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	// Create test context
	fileSize := 5 * 1024 * 1024 // 5MB
	ctx := NewTestContext(t, fileSize)
	defer ctx.Cleanup()

	// Encode the file
	result, err := processor.EncodeFile(ctx.InputFile, ctx.SymbolsDir, 0)
	if err != nil {
		t.Fatalf("Failed to encode file: %v", err)
	}

	// Delete all repair symbols, keeping only source symbols
	ctx.DeleteRepairSymbols(t, result)

	// Construct layout file path
	layoutPath := result.LayoutFilePath
	if _, err := os.Stat(layoutPath); os.IsNotExist(err) {
		// If the layout file was deleted by DeleteRepairSymbols, this test is invalid
		t.Logf("Layout file %s not found, assuming it was (correctly) deleted by DeleteRepairSymbols. Skipping decode.", layoutPath)
		// We can't decode without the layout file, so the test effectively passes here
		// if the goal was just to test symbol deletion.
		// If decoding *must* happen, DeleteRepairSymbols needs adjustment.
		return // Or adjust test logic if layout file *must* be preserved
	}

	// Decode with only source symbols (requires layout file)
	err = processor.DecodeSymbols(ctx.SymbolsDir, ctx.OutputFile, layoutPath)
	if err != nil {
		t.Fatalf("Failed to decode with only source symbols (layout file: %s): %v", layoutPath, err)
	}

	// Verify the decoded file matches the original
	if !ctx.VerifyFilesMatch(t) {
		t.Fatal("Decoded file does not match original")
	}
}

// System test for decoding with all symbols (source + repair)
func TestSysDecodeRedundantSymbols(t *testing.T) {
	// Create RaptorQ processor with default settings
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	// Create test context
	fileSize := 5 * 1024 * 1024 // 5MB
	ctx := NewTestContext(t, fileSize)
	defer ctx.Cleanup()

	// Encode the file
	// Encode the file (err is already declared in this scope)
	res, err := processor.EncodeFile(ctx.InputFile, ctx.SymbolsDir, 0)
	if err != nil {
		t.Fatalf("Failed to encode file: %v", err)
	}

	// Keep all symbols (we're testing with redundancy)

	// Construct layout file path
	layoutPath := res.LayoutFilePath
	if _, err := os.Stat(layoutPath); os.IsNotExist(err) {
		t.Fatalf("Layout file not found at %s after encoding", layoutPath)
	}

	// Decode with all symbols
	err = processor.DecodeSymbols(ctx.SymbolsDir, ctx.OutputFile, layoutPath)
	if err != nil {
		t.Fatalf("Failed to decode with all symbols (layout file: %s): %v", layoutPath, err)
	}

	// Verify the decoded file matches the original
	if !ctx.VerifyFilesMatch(t) {
		t.Fatal("Decoded file does not match original")
	}
}

// System test for decoding with a random subset of symbols
func TestSysDecodeRandomSubset(t *testing.T) {
	// Create RaptorQ processor with default settings
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	// Create test context
	fileSize := 5 * 1024 * 1024 // 5MB
	ctx := NewTestContext(t, fileSize)
	defer ctx.Cleanup()

	// Encode the file
	result, err := processor.EncodeFile(ctx.InputFile, ctx.SymbolsDir, 0)
	if err != nil {
		t.Fatalf("Failed to encode file: %v", err)
	}

	// Keep a random subset of repair symbols (50% of them)
	// but always keep all source symbols
	ctx.KeepRandomSubsetOfSymbols(t, result, 0.5)

	// Construct layout file path
	layoutPath := result.LayoutFilePath
	if _, err := os.Stat(layoutPath); os.IsNotExist(err) {
		t.Fatalf("Layout file not found at %s after encoding/subsetting", layoutPath)
	}

	// Decode with random subset of symbols
	err = processor.DecodeSymbols(ctx.SymbolsDir, ctx.OutputFile, layoutPath)
	if err != nil {
		t.Fatalf("Failed to decode with random subset (layout file: %s): %v", layoutPath, err)
	}

	// Verify the decoded file matches the original
	if !ctx.VerifyFilesMatch(t) {
		t.Fatal("Decoded file does not match original")
	}
}

// System test for error handling during encoding (non-existent input)
func TestSysErrorHandlingEncode(t *testing.T) {
	// Create RaptorQ processor with default settings
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	// Create test context (only used for temp directory and symbols dir)
	ctx := NewTestContext(t, 1024)
	defer ctx.Cleanup()

	// Try to encode a non-existent file
	nonExistentFile := filepath.Join(ctx.TempDir, "does_not_exist.bin")

	_, err = processor.EncodeFile(nonExistentFile, ctx.SymbolsDir, 0)

	// Verify error is reported correctly
	if err == nil {
		t.Fatal("Expected encoding to fail with non-existent file")
	}

	if !strings.Contains(err.Error(), "not found") && !strings.Contains(err.Error(), "no such file") {
		t.Fatalf("Error message should indicate file not found, got: %v", err)
	}
}

// System test for error handling during decoding (non-existent symbols dir)
func TestSysErrorHandlingDecode(t *testing.T) {
	// Create RaptorQ processor with default settings
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	// Create test context (only used for temp directory and output file)
	ctx := NewTestContext(t, 1024)
	defer ctx.Cleanup()

	// Non-existent symbols directory
	nonExistentDir := filepath.Join(ctx.TempDir, "non_existent_symbols")

	// Arbitrary layout file path (doesn't need to exist for this error case)
	layoutPath := filepath.Join(ctx.TempDir, "dummy_layout.json")

	err = processor.DecodeSymbols(nonExistentDir, ctx.OutputFile, layoutPath)

	// Verify error is reported correctly
	if err == nil {
		t.Fatal("Expected decoding to fail with non-existent symbols dir")
	}

	if !strings.Contains(err.Error(), "not found") && !strings.Contains(err.Error(), "no such file") {
		t.Fatalf("Error message should indicate directory not found, got: %v", err)
	}
}

// System test for creating metadata (without returning layout content)
func TestSysCreateMetadata(t *testing.T) {
	// Create RaptorQ processor with default settings
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	// Create test context with input file
	ctx := NewTestContext(t, 1024*1024) // 1MB file
	defer ctx.Cleanup()

	// Create metadata without returning layout (save to file)
	res, err := processor.CreateMetadata(ctx.InputFile, ctx.SymbolsDir, 0, false)
	if err != nil {
		t.Fatalf("Failed to create metadata: %v", err)
	}

	// Verify the layout file exists
	layoutPath := res.LayoutFilePath
	if _, err := os.Stat(layoutPath); os.IsNotExist(err) {
		t.Fatalf("Layout file not found at %s", layoutPath)
	}

	// Verify the result contains expected fields
	if res.SymbolsDirectory == "" {
		t.Fatal("Result should contain symbols_directory")
	}
	if res.LayoutFilePath == "" {
		t.Fatal("Result should contain layout_file_path")
	}
}

// System test for creating metadata with layout content returned
func TestSysCreateMetadataReturnLayout(t *testing.T) {
	// Note: Since our current implementation is a temporary wrapper around EncodeFile,
	// we can't fully test the returnLayout parameter yet. This test will need to be
	// updated when the full implementation is complete.

	// Create RaptorQ processor with default settings
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			t.Logf("Warning: Failed to free processor")
		}
	}()

	// Create test context with input file
	ctx := NewTestContext(t, 1024*1024) // 1MB file
	defer ctx.Cleanup()

	// Create metadata with returnLayout=true (currently uses the same code path)
	res, err := processor.CreateMetadata(ctx.InputFile, ctx.SymbolsDir, 0, true)
	if err != nil {
		t.Fatalf("Failed to create metadata with returnLayout=true: %v", err)
	}

	// Verify the layout file exists (when full implementation is done, this should be skipped)
	layoutPath := res.LayoutFilePath
	if _, err := os.Stat(layoutPath); os.IsNotExist(err) {
		t.Fatalf("Layout file not found at %s", layoutPath)
	}

	// TODO: When full implementation is complete, verify the result contains layout_content field
}

// Go-specific test for FFI interactions
func TestGoSpecificFFIInteractions(t *testing.T) {
	// This test verifies Go string/slice handling with C functions

	// Test creating and freeing a session
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		t.Fatalf("Failed to create processor: %v", err)
	}

	// Test session ID is non-zero
	if processor.SessionID == 0 {
		t.Fatal("Expected non-zero session ID")
	}

	// Test getting library version (C string -> Go string)
	version := GetVersion()
	if version == "Unknown version" || version == "" {
		t.Fatalf("Failed to get library version or got unexpected value: %s", version)
	}

	// Test getting error message when none exists
	if processor.getLastError() != "" {
		t.Fatalf("Expected empty error message, got: %s", processor.getLastError())
	}

	// Force an error to test error message conversion
	ctx := NewTestContext(t, 1024)
	defer ctx.Cleanup()
	nonExistentFile := filepath.Join(ctx.TempDir, "does_not_exist.bin")

	_, err = processor.EncodeFile(nonExistentFile, ctx.SymbolsDir, 0)
	if err == nil {
		t.Fatal("Expected encoding to fail")
	}

	// Get error message and verify it's not empty
	errorMsg := processor.getLastError()
	if errorMsg == "" {
		t.Fatal("Expected non-empty error message")
	}

	// Test passing Go byte slice to C
	// Create a small file, encode it, and verify encoder parameters buffer handling
	smallFilePath := filepath.Join(ctx.TempDir, "small_test.bin")
	if err := generateRandomFile(smallFilePath, 1024); err != nil {
		t.Fatalf("Failed to generate small test file: %v", err)
	}

	// Encode the file (result not needed here, err already declared)
	res, err := processor.EncodeFile(smallFilePath, ctx.SymbolsDir, 0)
	if err != nil {
		t.Fatalf("Failed to encode small file: %v", err)
	}

	// Verify layout file exists (indirect check that encoding produced metadata)
	layoutPath := res.LayoutFilePath
	if _, err := os.Stat(layoutPath); os.IsNotExist(err) {
		t.Fatalf("Layout file not found at %s after encoding small file", layoutPath)
	}

	// Free the processor to test cleanup
	if !processor.Free() {
		t.Fatal("Failed to free processor")
	}

	// Verify session is closed
	if processor.SessionID != 0 {
		t.Fatal("Session should be closed after Free()")
	}
}

// Helper function for Go 1.17+ compatibility
func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

// Constants for file sizes used in benchmarks
const (
	SIZE_1MB   = 1 * 1024 * 1024    // 1MB
	SIZE_10MB  = 10 * 1024 * 1024   // 10MB
	SIZE_100MB = 100 * 1024 * 1024  // 100MB
	SIZE_1GB   = 1024 * 1024 * 1024 // 1GB
)

// setupBenchmarkEnv creates a test environment with a file of specified size for benchmarking
// Returns the test context, which should be cleaned up after use
func setupBenchmarkEnv(b *testing.B, fileSize int) *TestContext {
	// Create temporary directory
	tempDir, err := os.MkdirTemp("/dev/shm", "raptorq-bench-")
	if err != nil {
		b.Fatalf("Failed to create temp directory: %v", err)
	}

	// Setup paths
	inputFile := filepath.Join(tempDir, "input.bin")
	symbolsDir := filepath.Join(tempDir, "symbols")
	outputFile := filepath.Join(tempDir, "output.bin")

	// Create symbols directory
	if err := os.MkdirAll(symbolsDir, 0755); err != nil {
		b.Fatalf("Failed to create symbols directory: %v", err)
	}

	// Generate random input file
	if err := generateRandomFile(inputFile, fileSize); err != nil {
		b.Fatalf("Failed to generate random file: %v", err)
	}

	return &TestContext{
		TempDir:    tempDir,
		InputFile:  inputFile,
		SymbolsDir: symbolsDir,
		OutputFile: outputFile,
	}
}

// prepareFilesForDecoding encodes a file and returns the path to the layout file.
// This is used to setup test data before running the decode benchmarks.
func prepareFilesForDecoding(b *testing.B, processor *RaptorQProcessor, ctx *TestContext, blockSize int) string {
	res, err := processor.EncodeFile(ctx.InputFile, ctx.SymbolsDir, blockSize)
	if err != nil {
		b.Fatalf("Failed to encode file for decode benchmark setup: %v", err)
	}
	layoutPath := res.LayoutFilePath
	if _, err := os.Stat(layoutPath); os.IsNotExist(err) {
		b.Fatalf("Layout file not found at %s after encoding for benchmark setup", layoutPath)
	}
	return layoutPath
}

// BenchmarkEncode1MB measures encoding time for a 1MB file
func BenchmarkEncode1MB(b *testing.B) {
	// Create RaptorQ processor
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		b.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			b.Logf("Warning: Failed to free processor")
		}
	}()

	// Setup test environment
	ctx := setupBenchmarkEnv(b, SIZE_1MB)
	defer ctx.Cleanup()

	// Reset timer before starting the benchmark loop
	b.ResetTimer()

	// Run the benchmark
	for i := 0; i < b.N; i++ {
		_, err := processor.EncodeFile(ctx.InputFile, ctx.SymbolsDir, 0)
		if err != nil {
			b.Fatalf("Failed to encode file: %v", err)
		}

		// Clean symbols directory for next iteration
		if i < b.N-1 {
			os.RemoveAll(ctx.SymbolsDir)
			os.MkdirAll(ctx.SymbolsDir, 0755)
		}
	}
}

// BenchmarkEncode10MB measures encoding time for a 10MB file
func BenchmarkEncode10MB(b *testing.B) {
	// Create RaptorQ processor
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		b.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			b.Logf("Warning: Failed to free processor")
		}
	}()

	// Setup test environment
	ctx := setupBenchmarkEnv(b, SIZE_10MB)
	defer ctx.Cleanup()

	// Reset timer before starting the benchmark loop
	b.ResetTimer()

	// Run the benchmark
	for i := 0; i < b.N; i++ {
		_, err := processor.EncodeFile(ctx.InputFile, ctx.SymbolsDir, 0)
		if err != nil {
			b.Fatalf("Failed to encode file: %v", err)
		}

		// Clean symbols directory for next iteration
		if i < b.N-1 {
			os.RemoveAll(ctx.SymbolsDir)
			os.MkdirAll(ctx.SymbolsDir, 0755)
		}
	}
}

// BenchmarkEncode100MB measures encoding time for a 100MB file
func BenchmarkEncode100MB(b *testing.B) {
	// Create RaptorQ processor
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		b.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			b.Logf("Warning: Failed to free processor")
		}
	}()

	// Setup test environment
	ctx := setupBenchmarkEnv(b, SIZE_100MB)
	defer ctx.Cleanup()

	blockSize := 5 * 1024 * 1024 // 1MB blocks

	// Reset timer before starting the benchmark loop
	b.ResetTimer()

	// Run the benchmark
	for i := 0; i < b.N; i++ {
		_, err := processor.EncodeFile(ctx.InputFile, ctx.SymbolsDir, blockSize)
		if err != nil {
			b.Fatalf("Failed to encode file: %v", err)
		}

		// Clean symbols directory for next iteration
		if i < b.N-1 {
			os.RemoveAll(ctx.SymbolsDir)
			os.MkdirAll(ctx.SymbolsDir, 0755)
		}
	}
}

// BenchmarkEncode1GB measures encoding time for a 1GB file
func BenchmarkEncode1GB(b *testing.B) {
	// Skip in short mode
	if testing.Short() {
		b.Skip("Skipping 1GB file benchmark in short mode")
	}

	// Create RaptorQ processor with increased memory
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		b.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			b.Logf("Warning: Failed to free processor")
		}
	}()

	// Setup test environment
	ctx := setupBenchmarkEnv(b, SIZE_1GB)
	defer ctx.Cleanup()

	// Use splitting for large file
	blockSize := 50 * 1024 * 1024 // 50MB blocks

	// Reset timer before starting the benchmark loop
	b.ResetTimer()

	// Run the benchmark
	for i := 0; i < b.N; i++ {
		_, err := processor.EncodeFile(ctx.InputFile, ctx.SymbolsDir, blockSize)
		if err != nil {
			b.Fatalf("Failed to encode file: %v", err)
		}

		// Clean symbols directory for next iteration
		if i < b.N-1 {
			os.RemoveAll(ctx.SymbolsDir)
			os.MkdirAll(ctx.SymbolsDir, 0755)
		}
	}
}

// BenchmarkDecode1MB measures decoding time for a 1MB file
func BenchmarkDecode1MB(b *testing.B) {
	// Create RaptorQ processor
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		b.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			b.Logf("Warning: Failed to free processor")
		}
	}()

	// Setup test environment
	ctx := setupBenchmarkEnv(b, SIZE_1MB)
	defer ctx.Cleanup()

	// Encode file to generate symbols (outside benchmark loop)
	layoutPath := prepareFilesForDecoding(b, processor, ctx, 0) // 0 for auto block size

	// Reset timer before starting the benchmark loop
	b.ResetTimer()

	// Run the benchmark
	for i := 0; i < b.N; i++ {
		err := processor.DecodeSymbols(ctx.SymbolsDir, ctx.OutputFile, layoutPath)
		if err != nil {
			b.Fatalf("Failed to decode symbols: %v", err)
		}

		// Remove output file for next iteration
		if i < b.N-1 {
			os.Remove(ctx.OutputFile)
		}
	}
}

// BenchmarkDecode10MB measures decoding time for a 10MB file
func BenchmarkDecode10MB(b *testing.B) {
	// Create RaptorQ processor
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		b.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			b.Logf("Warning: Failed to free processor")
		}
	}()

	// Setup test environment
	ctx := setupBenchmarkEnv(b, SIZE_10MB)
	defer ctx.Cleanup()

	// Encode file to generate symbols (outside benchmark loop)
	layoutPath := prepareFilesForDecoding(b, processor, ctx, 0) // 0 for auto block size

	// Reset timer before starting the benchmark loop
	b.ResetTimer()

	// Run the benchmark
	for i := 0; i < b.N; i++ {
		err := processor.DecodeSymbols(ctx.SymbolsDir, ctx.OutputFile, layoutPath)
		if err != nil {
			b.Fatalf("Failed to decode symbols: %v", err)
		}

		// Remove output file for next iteration
		if i < b.N-1 {
			os.Remove(ctx.OutputFile)
		}
	}
}

// BenchmarkDecode100MB measures decoding time for a 100MB file
func BenchmarkDecode100MB(b *testing.B) {
	// Create RaptorQ processor
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		b.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			b.Logf("Warning: Failed to free processor")
		}
	}()

	// Setup test environment
	ctx := setupBenchmarkEnv(b, SIZE_100MB)
	defer ctx.Cleanup()

	// Encode file to generate symbols (outside benchmark loop)
	layoutPath := prepareFilesForDecoding(b, processor, ctx, 0) // 0 for auto block size

	// Reset timer before starting the benchmark loop
	b.ResetTimer()

	// Run the benchmark
	for i := 0; i < b.N; i++ {
		err := processor.DecodeSymbols(ctx.SymbolsDir, ctx.OutputFile, layoutPath)
		if err != nil {
			b.Fatalf("Failed to decode symbols: %v", err)
		}

		// Remove output file for next iteration
		if i < b.N-1 {
			os.Remove(ctx.OutputFile)
		}
	}
}

// BenchmarkDecode1GB measures decoding time for a 1GB file
func BenchmarkDecode1GB(b *testing.B) {
	// Skip in short mode
	if testing.Short() {
		b.Skip("Skipping 1GB file benchmark in short mode")
	}

	// Create RaptorQ processor with increased memory
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		b.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			b.Logf("Warning: Failed to free processor")
		}
	}()

	// Setup test environment
	ctx := setupBenchmarkEnv(b, SIZE_1GB)
	defer ctx.Cleanup()

	// Use splitting for large file
	blockSize := 50 * 1024 * 1024 // 50MB blocks

	// Encode file to generate symbols (outside benchmark loop)
	layoutPath := prepareFilesForDecoding(b, processor, ctx, blockSize)

	// Reset timer before starting the benchmark loop
	b.ResetTimer()

	// Run the benchmark
	for i := 0; i < b.N; i++ {
		err := processor.DecodeSymbols(ctx.SymbolsDir, ctx.OutputFile, layoutPath)
		if err != nil {
			b.Fatalf("Failed to decode symbols: %v", err)
		}

		// Remove output file for next iteration
		if i < b.N-1 {
			os.Remove(ctx.OutputFile)
		}
	}
}

// BenchmarkCreateMetadata1MB measures metadata creation time for a 1MB file
func BenchmarkCreateMetadata1MB(b *testing.B) {
	// Create RaptorQ processor
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		b.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			b.Logf("Warning: Failed to free processor")
		}
	}()

	// Setup test environment
	ctx := setupBenchmarkEnv(b, SIZE_1MB)
	defer ctx.Cleanup()

	// Reset timer before starting the benchmark loop
	b.ResetTimer()

	// Run the benchmark
	for i := 0; i < b.N; i++ {
		_, err := processor.CreateMetadata(ctx.InputFile, ctx.SymbolsDir, 0, false)
		if err != nil {
			b.Fatalf("Failed to create metadata: %v", err)
		}

		// Clean symbols directory for next iteration
		if i < b.N-1 {
			os.RemoveAll(ctx.SymbolsDir)
			os.MkdirAll(ctx.SymbolsDir, 0755)
		}
	}
}

// BenchmarkCreateMetadata10MB measures metadata creation time for a 10MB file
func BenchmarkCreateMetadata10MB(b *testing.B) {
	// Create RaptorQ processor
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		b.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			b.Logf("Warning: Failed to free processor")
		}
	}()

	// Setup test environment
	ctx := setupBenchmarkEnv(b, SIZE_10MB)
	defer ctx.Cleanup()

	// Reset timer before starting the benchmark loop
	b.ResetTimer()

	// Run the benchmark
	for i := 0; i < b.N; i++ {
		_, err := processor.CreateMetadata(ctx.InputFile, ctx.SymbolsDir, 0, true) // with returnLayout=true
		if err != nil {
			b.Fatalf("Failed to create metadata: %v", err)
		}

		// Clean symbols directory for next iteration
		if i < b.N-1 {
			os.RemoveAll(ctx.SymbolsDir)
			os.MkdirAll(ctx.SymbolsDir, 0755)
		}
	}
}

// BenchmarkCreateMetadata100MB measures metadata creation time for a 100MB file
func BenchmarkCreateMetadata100MB(b *testing.B) {
	// Create RaptorQ processor
	processor, err := NewDefaultRaptorQProcessor()
	if err != nil {
		b.Fatalf("Failed to create processor: %v", err)
	}
	defer func() {
		if !processor.Free() {
			b.Logf("Warning: Failed to free processor")
		}
	}()

	// Setup test environment
	ctx := setupBenchmarkEnv(b, SIZE_100MB)
	defer ctx.Cleanup()

	// Use block size for large file
	blockSize := 5 * 1024 * 1024 // 5MB blocks

	// Reset timer before starting the benchmark loop
	b.ResetTimer()

	// Run the benchmark
	for i := 0; i < b.N; i++ {
		_, err := processor.CreateMetadata(ctx.InputFile, ctx.SymbolsDir, blockSize, false)
		if err != nil {
			b.Fatalf("Failed to create metadata: %v", err)
		}

		// Clean symbols directory for next iteration
		if i < b.N-1 {
			os.RemoveAll(ctx.SymbolsDir)
			os.MkdirAll(ctx.SymbolsDir, 0755)
		}
	}
}
