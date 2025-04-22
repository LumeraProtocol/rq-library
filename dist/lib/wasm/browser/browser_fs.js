// browser_fs.js
// JS glue for WASM browser file access

// Internal file registry: id -> { file: File, buffer: ArrayBuffer }
const fileRegistry = {};

/**
 * Registers a File object for WASM access.
 * @param {File} file - The browser File object.
 * @param {string} id - Unique identifier for the file.
 * Loads the file into memory for efficient chunked access.
 */
export function registerFile(file, id) {
    const reader = new FileReader();
    reader.onload = function(e) {
        fileRegistry[id] = {
            file: file,
            buffer: e.target.result
        };
    };
    reader.readAsArrayBuffer(file);
}

/**
 * Returns the size of the registered file in bytes.
 * @param {string} id
 * @returns {number}
 */
export function getFileSize(id) {
    const entry = fileRegistry[id];
    if (!entry) throw new Error("File not registered: " + id);
    return entry.file.size;
}

/**
 * Reads a chunk of the file as a Uint8Array.
 * @param {string} id
 * @param {number} offset
 * @param {number} length
 * @returns {Uint8Array}
 */
export function readFileChunk(id, offset, length) {
    const entry = fileRegistry[id];
    if (!entry) throw new Error("File not registered: " + id);
    const buffer = entry.buffer;
    if (!buffer) throw new Error("File not loaded yet: " + id);
    const end = Math.min(offset + length, buffer.byteLength);
    return new Uint8Array(buffer.slice(offset, end));
}

/**
 * Expose a FileSystem object for WASM interop.
 */
export const FileSystem = {
    registerFile,
    getFileSize,
    readFileChunk
};