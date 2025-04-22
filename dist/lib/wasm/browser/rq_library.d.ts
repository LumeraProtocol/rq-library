/* tslint:disable */
/* eslint-disable */
export function start(): void;
export class RaptorQSession {
  free(): void;
  constructor(symbol_size: number, redundancy_factor: number, max_memory_mb: bigint, concurrency_limit: bigint);
  create_metadata(input_path: string, output_dir: string, block_size: number, return_layout: boolean): Promise<any>;
  encode_file(input_path: string, output_dir: string, block_size: number): Promise<any>;
  decode_symbols(symbols_dir: string, output_path: string, layout_path: string): Promise<any>;
  get_recommended_block_size(file_size: number): number;
  static version(): string;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_raptorqsession_free: (a: number, b: number) => void;
  readonly raptorqsession_new: (a: number, b: number, c: bigint, d: bigint) => number;
  readonly raptorqsession_create_metadata: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => any;
  readonly raptorqsession_encode_file: (a: number, b: number, c: number, d: number, e: number, f: number) => any;
  readonly raptorqsession_decode_symbols: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => any;
  readonly raptorqsession_get_recommended_block_size: (a: number, b: number) => number;
  readonly raptorqsession_version: () => [number, number];
  readonly start: () => void;
  readonly raptorq_init_session: (a: number, b: number, c: bigint, d: bigint) => number;
  readonly raptorq_free_session: (a: number) => number;
  readonly raptorq_create_metadata: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => number;
  readonly raptorq_encode_file: (a: number, b: number, c: number, d: number, e: number, f: number) => number;
  readonly raptorq_get_last_error: (a: number, b: number, c: number) => number;
  readonly raptorq_decode_symbols: (a: number, b: number, c: number, d: number) => number;
  readonly raptorq_get_recommended_block_size: (a: number, b: bigint) => number;
  readonly raptorq_version: (a: number, b: number) => number;
  readonly __wbindgen_exn_store: (a: number) => void;
  readonly __externref_table_alloc: () => number;
  readonly __wbindgen_export_2: WebAssembly.Table;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_export_6: WebAssembly.Table;
  readonly closure822_externref_shim: (a: number, b: number, c: any) => void;
  readonly closure839_externref_shim: (a: number, b: number, c: any, d: any) => void;
  readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;
/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
