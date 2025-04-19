package main

import (
	"fmt"
	"os"
	"path/filepath"

	raptorq "github.com/LumeraProtocol/rq-library/bindings/go"
)

func main() {
	// Check command line arguments
	if len(os.Args) < 2 {
		fmt.Println("Usage:")
		fmt.Println("  encode <input_file> [block_size_in_mb]")
		fmt.Println("  decode <symbols_dir> <output_file> <layout_file>")
		fmt.Println("  version")
		os.Exit(1)
	}

	// Get the command
	command := os.Args[1]

	// Create a new RaptorQ processor with default settings
	processor, err := raptorq.NewDefaultRaptorQProcessor()
	if err != nil {
		fmt.Printf("Error creating processor: %v\n", err)
		os.Exit(1)
	}
	defer processor.Free()

	switch command {
	case "version":
		// Get and print the library version
		version := raptorq.GetVersion()
		fmt.Printf("RaptorQ library version: %s\n", version)

	case "encode":
		// Check arguments for encode command
		if len(os.Args) < 3 {
			fmt.Println("Error: Missing input file path")
			os.Exit(1)
		}

		inputFile := os.Args[2]
		blockSize := 0 // Default (auto)

		// Parse optional block size
		if len(os.Args) >= 4 {
			var blockSizeMB int
			_, err := fmt.Sscanf(os.Args[3], "%d", &blockSizeMB)
			if err != nil {
				fmt.Printf("Error parsing block size: %v\n", err)
				os.Exit(1)
			}
			blockSize = blockSizeMB * 1024 * 1024 // Convert MB to bytes
		}

		// Create output directory for symbols
		symbolsDir := inputFile + ".symbols"
		err := os.MkdirAll(symbolsDir, 0755)
		if err != nil {
			fmt.Printf("Error creating symbols directory: %v\n", err)
			os.Exit(1)
		}

		fmt.Printf("Encoding file: %s\n", inputFile)
		fmt.Printf("Output directory: %s\n", symbolsDir)
		if blockSize > 0 {
			fmt.Printf("Block size: %d bytes\n", blockSize)
		} else {
			fmt.Println("Block size: auto")
		}

		// Encode the file
		result, err := processor.EncodeFile(inputFile, symbolsDir, blockSize)
		if err != nil {
			fmt.Printf("Error encoding file: %v\n", err)
			os.Exit(1)
		}

		fmt.Printf("Encoding successful!\n")
		fmt.Printf("Layout file: %s\n", result.LayoutFilePath)
		fmt.Printf("Total symbols generated: %d\n", result.TotalSymbolsCount)
		fmt.Printf("Total repair symbols: %d\n", result.TotalRepairSymbols)

	case "decode":
		// Check arguments for decode command
		if len(os.Args) < 5 {
			fmt.Println("Error: Missing required arguments for decode")
			fmt.Println("Usage: decode <symbols_dir> <output_file> <layout_file>")
			os.Exit(1)
		}

		symbolsDir := os.Args[2]
		outputFile := os.Args[3]
		layoutFile := os.Args[4]

		fmt.Printf("Decoding symbols from: %s\n", symbolsDir)
		fmt.Printf("Output file: %s\n", outputFile)
		fmt.Printf("Layout file: %s\n", layoutFile)

		// Create output directory if it doesn't exist
		outputDir := filepath.Dir(outputFile)
		if outputDir != "." {
			err := os.MkdirAll(outputDir, 0755)
			if err != nil {
				fmt.Printf("Error creating output directory: %v\n", err)
				os.Exit(1)
			}
		}

		// Decode the symbols
		err := processor.DecodeSymbols(symbolsDir, outputFile, layoutFile)
		if err != nil {
			fmt.Printf("Error decoding symbols: %v\n", err)
			os.Exit(1)
		}

		fmt.Println("Decoding successful!")

	default:
		fmt.Printf("Unknown command: %s\n", command)
		os.Exit(1)
	}
}
