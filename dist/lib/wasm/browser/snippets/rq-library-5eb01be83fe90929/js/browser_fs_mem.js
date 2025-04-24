// In-memory VFS drop-in replacement for browser_fs.js
// All file and directory operations are handled in RAM (not persistent).

// In-memory stores
const memFiles = new Map(); // path -> Uint8Array
const memMetadata = new Map(); // path -> { size: number }
const memDirs = new Set(['/']); // Always include root

// Expose for debugging in DevTools
window.memFiles = memFiles;
window.memMetadata = memMetadata;
window.memDirs = memDirs;

// --- File API ---

/**
 * Get the size of a file
 * @param {string} path
 * @returns {number}
 */
export function getFileSize(path) {
    const meta = memMetadata.get(path);
    return meta ? meta.size : 0;
}

/**
 * Synchronous check if a directory exists
 * @param {string} path
 * @returns {boolean}
 */
export function syncDirExists(path) {
    return memDirs.has(path);
}

// --- Global sync wrappers for Rust/WASM ---

/**
 * Synchronous read chunk (called from Rust)
 */
window.syncReadChunk = function(path, offset, length) {
    const data = memFiles.get(path) || new Uint8Array(0);
    if (offset >= data.length) return new Uint8Array(0);
    const available = Math.min(length, data.length - offset);
    return data.slice(offset, offset + available);
};

/**
 * Synchronous write chunk (called from Rust)
 */
window.syncWriteChunk = function(path, offset, data) {
    let file = memFiles.get(path) || new Uint8Array(0);
    if (offset + data.length > file.length) {
        const newFile = new Uint8Array(offset + data.length);
        newFile.set(file, 0);
        file = newFile;
    }
    file.set(new Uint8Array(data), offset);
    memFiles.set(path, file);
    memMetadata.set(path, { size: file.length });
    return true;
};

/**
 * Synchronous flush (no-op for memory)
 */
window.syncFlushFile = function(path) {
    return true;
};

/**
 * Synchronous create directory (called from Rust)
 */
window.syncCreateDirAll = function(path) {
    memDirs.add(path);
    return true;
};

// --- Async API ---

/**
 * Read a chunk of data from a file
 * @param {string} path
 * @param {number} offset
 * @param {number} length
 * @returns {Promise<Uint8Array>}
 */
export async function readFileChunk(path, offset, length) {
    const data = memFiles.get(path) || new Uint8Array(0);
    if (offset >= data.length) return new Uint8Array(0);
    const available = Math.min(length, data.length - offset);
    return data.slice(offset, offset + available);
}

/**
 * Write a chunk of data to a file
 * @param {string} path
 * @param {number} offset
 * @param {Uint8Array} data
 * @returns {Promise<void>}
 */
export async function writeFileChunk(path, offset, data) {
    let file = memFiles.get(path) || new Uint8Array(0);
    if (offset + data.length > file.length) {
        const newFile = new Uint8Array(offset + data.length);
        newFile.set(file, 0);
        file = newFile;
    }
    file.set(data, offset);
    memFiles.set(path, file);
    memMetadata.set(path, { size: file.length });
}

/**
 * Flush file (no-op for memory)
 * @param {string} path
 * @returns {Promise<void>}
 */
export async function flushFile(path) {
    // No-op
}

/**
 * Recursively create a directory path
 * @param {string} path
 * @returns {Promise<void>}
 */
export async function createDirAll(path) {
    memDirs.add(path);
}

/**
 * Synchronous wrapper for checking if a directory exists (called from Rust)
 * @param {string} path
 * @returns {boolean}
 */
window.syncDirExists = function(path) {
    return memDirs.has(path);
};

/**
 * Async check if a directory exists
 * @param {string} path
 * @returns {Promise<boolean>}
 */
export async function dirExists(path) {
    return memDirs.has(path);
}
