# RaptorQ Processor Call Tree

## RaptorQProcessor - Main Public Methods

```
RaptorQProcessor
├── new()
├── get_last_error()
├── get_config()
├── get_recommended_block_size()
├── encode_file_streamed()
├── decode_symbols()
├── decode_symbols_with_layout()
└── decode_symbols_with_params()
```

## Encoding Flow

```
encode_file_streamed()
├── can_start_task()
├── TaskGuard::new()
├── set_last_error() [if error]
├── get_recommended_block_size() [when block_size is 0]
│
├── encode_single_file() [if no chunking]
│   ├── create_dir_all()
│   ├── open_and_validate_file()
│   ├── estimate_memory_requirements()
│   ├── is_memory_available()
│   ├── set_last_error() [if memory error]
│   │
│   ├── encode_stream()
│   │   ├── calculate_repair_symbols()
│   │   ├── Encoder::new()
│   │   ├── Encoder::get_encoded_packets()
│   │   └── calculate_symbol_id() [for each packet]
│   │
│   ├── serde_json::to_string_pretty() [for layout]
│   ├── set_last_error() [if serialization error]
│   ├── write_all() [write layout file]
│   ├── set_last_error() [if write error]
│   ├── calculate_repair_symbols()
│   └── return ProcessResult
│
└── encode_file_in_blocks() [if chunking]
    ├── create_dir_all()
    ├── open_and_validate_file()
    ├── create_dir_all() [for each chunk]
    │
    ├── encode_stream() [for each chunk]
    │   ├── calculate_repair_symbols()
    │   ├── Encoder::new()
    │   ├── Encoder::get_encoded_packets()
    │   └── calculate_symbol_id() [for each packet]
    │
    ├── serde_json::to_string_pretty() [for layout]
    ├── set_last_error() [if serialization error]
    ├── write_all() [write layout file]
    ├── set_last_error() [if write error]
    ├── calculate_repair_symbols()
    └── return ProcessResult
```

## Decoding Flow

```
decode_symbols()
├── can_start_task()
├── TaskGuard::new()
├── set_last_error() [if error]
├── read_to_string() [read layout file]
├── serde_json::from_str() [parse layout]
└── decode_symbols_with_layout()
    │
    ├── decode_single_file_internal() [if not chunked]
    │   ├── ObjectTransmissionInformation::deserialize()
    │   ├── Decoder::new()
    │   │
    │   ├── read_specific_symbol_files() [if symbol_ids provided]
    │   │   └── read_to_end() [for each file]
    │   │
    │   ├── read_symbol_files() [otherwise]
    │   │   └── read_to_end() [for each file]
    │   │
    │   └── decode_symbols_to_file()
    │       ├── EncodingPacket::deserialize() [for each symbol]
    │       ├── safe_decode() [for each packet]
    │       │   └── decoder.decode() [wrapped in catch_unwind]
    │       └── write_all() [write decoded data]
    │
    ├── decode_chunked_file_with_layout() [if chunked with layout]
    │   ├── sort blocks
    │   │
    │   ├── [For each chunk]
    │   │   ├── ObjectTransmissionInformation::deserialize()
    │   │   ├── Decoder::new()
    │   │   │
    │   │   ├── [For each symbol in chunk layout]
    │   │   │   ├── read_to_end()
    │   │   │   ├── EncodingPacket::deserialize()
    │   │   │   └── safe_decode()
    │   │   │       └── decoder.decode() [wrapped in catch_unwind]
    │   │   │
    │   │   ├── [Or fall back to all files]
    │   │   │   ├── read_symbol_files()
    │   │   │   └── decode_symbols_to_memory()
    │   │   │       ├── EncodingPacket::deserialize() [for each]
    │   │   │       └── safe_decode() [for each]
    │   │   │
    │   │   ├── seek() [to correct position]
    │   │   └── write_all() [write decoded chunk data]
    │
    └── decode_chunked_file_internal() [if chunked without detailed layout]
        ├── sort blocks
        │
        ├── [For each chunk]
        │   ├── ObjectTransmissionInformation::deserialize()
        │   ├── Decoder::new()
        │   ├── read_symbol_files()
        │   └── decode_symbols_to_memory()
        │       ├── EncodingPacket::deserialize() [for each]
        │       ├── safe_decode() [for each]
        │       │   └── decoder.decode() [wrapped in catch_unwind]
        │       └── write_all() [write chunk data]
```

## Helper Functions

```
├── set_last_error()
├── can_start_task()
│   └── active_tasks.load()
│
├── open_and_validate_file()
│   ├── File::open()
│   ├── set_last_error() [if error]
│   ├── file.metadata()
│   └── set_last_error() [if empty file]
│
├── calculate_repair_symbols()
│
├── calculate_symbol_id()
│   ├── Sha3_256::new()
│   ├── hasher.update()
│   └── bs58::encode()
│
├── estimate_memory_requirements()
│
├── is_memory_available()
│
├── read_symbol_files()
│   ├── fs::read_dir()
│   ├── File::open() [for each file]
│   └── read_to_end() [for each file]
│
├── read_specific_symbol_files()
│   ├── File::open() [for each specified ID]
│   └── read_to_end() [for each file]
│
├── safe_decode()
│   └── std::panic::catch_unwind(decoder.decode())
│
├── decode_symbols_to_file()
│   ├── EncodingPacket::deserialize() [for each]
│   ├── safe_decode() [for each]
│   └── write_all() [write decoded data]
│
└── decode_symbols_to_memory()
    ├── EncodingPacket::deserialize() [for each]
    ├── safe_decode() [for each]
    └── extend_from_slice() [append decoded data]
```

## Utility Classes

```
TaskGuard (RAII guard for task counting)
├── new() 
│   └── counter.fetch_add()
└── drop() [automatic]
    └── counter.fetch_sub()
```