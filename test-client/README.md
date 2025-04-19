# RaptorQ Test Client

This is a simple command-line client for testing the RaptorQ erasure coding library. It demonstrates how to use the Go bindings for the RaptorQ library.

## Building

To build the client:

```bash
cd test-client
go build -o rq-client
```

## Usage

The client supports the following commands:

### Get Version

```bash
./rq-client version
```

This will display the version of the RaptorQ library.

### Encode a File

```bash
./rq-client encode <input_file> [block_size_in_mb]
```

This will encode the input file and create symbols in a directory named `<input_file>.symbols`. The optional `block_size_in_mb` parameter specifies the block size in megabytes. If not provided, the library will automatically determine the optimal block size.

### Decode Symbols

```bash
./rq-client decode <symbols_dir> <output_file> <layout_file>
```

This will decode the symbols in the specified directory and create the output file. The layout file is created during the encoding process and is required for decoding.

## Example

```bash
# Create a test file
dd if=/dev/urandom of=testfile.bin bs=1M count=10

# Encode the file
./rq-client encode testfile.bin

# Decode the file
./rq-client decode testfile.bin.symbols testfile_decoded.bin testfile.bin.symbols/layout.json
```

## Moving Outside the Repository

If you want to use this client outside the repository, you need to:

1. Copy the `test-client` directory to your desired location
2. Update the `replace` directive in `go.mod` to point to the correct location of the RaptorQ Go bindings
3. Build the client in the new location

For example, if you've moved the client to `/path/to/new/location` and the RaptorQ repository is at `/path/to/rq-library`, update the `go.mod` file:

```
replace github.com/LumeraProtocol/rq-library/bindings/go => /path/to/rq-library/bindings/go
```

## Creating a New Go Project That Uses the RaptorQ Library

To create a new Go project that uses the RaptorQ library:

1. Create a new directory for your project
2. Initialize a new Go module:
   ```bash
   go mod init github.com/yourusername/yourproject
   ```
3. Add the RaptorQ library as a dependency:
   ```bash
   go get github.com/LumeraProtocol/rq-library/bindings/go
   ```
4. If you're using a local copy of the RaptorQ library, add a replace directive to your go.mod file:
   ```
   replace github.com/LumeraProtocol/rq-library/bindings/go => /path/to/rq-library/bindings/go
   ```
5. Import the library in your Go code:
   ```go
   import "github.com/LumeraProtocol/rq-library/bindings/go"
   ```
6. Use the library as demonstrated in this test client