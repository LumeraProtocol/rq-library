# RaptorQ Processor Call Tree

## RaptorQProcessor - Main Public Methods

```
RaptorQProcessor
├── new()
├── get_last_error()
├── get_config()
├── get_recommended_block_size()
├── encode_file()
├── decode_symbols()
├── decode_symbols_with_layout()
└── decode_symbols_with_params()
```

## Encoding Flow

```
encode_file()
├── can_start_task()
├── TaskGuard::new()
├── set_last_error() [if error]
├── get_recommended_block_size() [when block_size is 0]
│── estimate_memory_requirements() [when force_single_file]
│
└── encode_file_blocks()
    ├── create_dir_all()
    ├── open_and_validate_file()
    │
    ├── [For each block]
    │   │
    │   ├── create_dir_all() [for each block]
    │   ├── calculate_repair_symbols() [for each block]
    │   │
    │   ├── encode_stream() [for each block]
    │   │   ├── Encoder::new()
    │   │   ├── Encoder::get_encoded_packets()
    │   │   └── calculate_symbol_id() [for each packet]
    │
    ├── serde_json::to_string_pretty() [for layout]
    ├── set_last_error() [if serialization error]
    ├── write_all() [write layout file]
    ├── set_last_error() [if write error]
    └── return ProcessResult
```

## Decoding Flow

```
decode_symbols()
├── set_last_error() [if error]
├── read_to_string() [read layout file]
├── serde_json::from_str() [parse layout]
└── decode_symbols_with_layout()
    ├── can_start_task()
    ├── TaskGuard::new()
    │
    ├── sort blocks
    │
    ├── [For each block]
    │   ├── ObjectTransmissionInformation::deserialize()
    │   ├── Decoder::new()
    │   │
    │   ├── [For each symbol in block layout]
    │   │   ├── read_to_end()
    │   │   ├── EncodingPacket::deserialize()
    │   │   └── safe_decode()
    │   │       └── decoder.decode() [wrapped in catch_unwind]
    │   │
    │   ├── [Or fall back to all files]
    │   │   ├── read_symbol_files()
    │   │   └── decode_symbols_to_memory()
    │   │       ├── EncodingPacket::deserialize() [for each]
    │   │       └── safe_decode() [for each]
    │   │
    │   ├── seek() [to correct position]
    │   └── write_all() [write decoded block data]
    │
    └── return Ok
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