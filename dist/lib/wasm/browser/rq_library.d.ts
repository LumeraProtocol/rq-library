/* tslint:disable */
/* eslint-disable */

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly raptorq_init_session: (a: number, b: number, c: bigint, d: bigint) => number;
  readonly raptorq_free_session: (a: number) => number;
  readonly raptorq_create_metadata: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => number;
  readonly raptorq_encode_file: (a: number, b: number, c: number, d: number, e: number, f: number) => number;
  readonly raptorq_get_last_error: (a: number, b: number, c: number) => number;
  readonly raptorq_decode_symbols: (a: number, b: number, c: number, d: number) => number;
  readonly raptorq_get_recommended_block_size: (a: number, b: bigint) => number;
  readonly raptorq_version: (a: number, b: number) => number;
  readonly __wbindgen_export_0: WebAssembly.Table;
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
