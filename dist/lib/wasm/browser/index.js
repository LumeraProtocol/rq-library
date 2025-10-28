// dist/lib/wasm/browser/index.js

// Import and re-export from the core WASM module
import initWasm, { RaptorQSession } from './rq_library.js';
export { RaptorQSession };
export default initWasm;

// Import and re-export from the in-memory file system utilities
export {
  writeFileChunk,
  readFileChunk,
  getFileSize,
  createDirAll,
  dirExists,
  syncDirExists,
  flushFile
} from './snippets/rq-library-5eb01be83fe90929/js/browser_fs_mem.js';