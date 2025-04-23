// Browser FileSystem API for RaptorQ WASM
// This provides the JavaScript implementation of file system operations
// that are called from the Rust WASM module

/**
 * Get the size of a file
 * @param {string} path - Path to the file
 * @returns {number} - Size of the file in bytes
 */
export function getFileSize(path) {
    console.log(`[JS] Getting size of file: ${path}`);
    // In a real implementation, this would use the File System Access API,
    // IndexedDB, or some other storage mechanism
    
    // For now, we'll use a simple localStorage-based mock
    const metadata = JSON.parse(localStorage.getItem(`file_metadata:${path}`) || '{"size": 0}');
    return metadata.size;
}

// Add synchronous wrapper functions to the global scope for Rust to call

/**
 * Synchronous wrapper for reading a chunk (called from Rust)
 * Uses XMLHttpRequest in synchronous mode as a workaround
 */
window.syncReadChunk = function(path, offset, length) {
    console.log(`[JS] syncReadChunk: ${path}, offset: ${offset}, length: ${length}`);
    
    // In a production app, you'd use more sophisticated synchronization
    // For now, we'll use a simple approach that works for demo purposes
    
    // Get data from localStorage
    const data = localStorage.getItem(`file_data:${path}`) || '';
    
    // Create a Uint8Array from the stored data
    let fullData;
    try {
        fullData = Uint8Array.from(atob(data), c => c.charCodeAt(0));
    } catch (e) {
        fullData = new Uint8Array(0);
    }
    
    // Return the requested chunk
    if (offset >= fullData.length) {
        return new Uint8Array(0);
    }
    
    const availableBytes = Math.min(length, fullData.length - offset);
    return fullData.slice(offset, offset + availableBytes);
};

/**
 * Synchronous wrapper for writing a chunk (called from Rust)
 */
window.syncWriteChunk = function(path, offset, data) {
    console.log(`[JS] syncWriteChunk: ${path}, offset: ${offset}, size: ${data.length} bytes`);
    
    // Get existing file data or create empty data
    let fullData;
    const existingData = localStorage.getItem(`file_data:${path}`);
    
    try {
        fullData = existingData ?
            Uint8Array.from(atob(existingData), c => c.charCodeAt(0)) :
            new Uint8Array(0);
    } catch (e) {
        fullData = new Uint8Array(0);
    }
    
    // If we need to write beyond the current end of the file, resize it
    if (offset + data.length > fullData.length) {
        const newData = new Uint8Array(offset + data.length);
        newData.set(fullData.slice(0, fullData.length));
        fullData = newData;
    }
    
    // Write the data at the specified offset
    fullData.set(new Uint8Array(data), offset);
    
    // Store the modified data
    const base64Data = btoa(String.fromCharCode.apply(null, fullData));
    localStorage.setItem(`file_data:${path}`, base64Data);
    
    // Update metadata
    const metadata = { size: fullData.length };
    localStorage.setItem(`file_metadata:${path}`, JSON.stringify(metadata));
    
    return true;
};

/**
 * Synchronous wrapper for flushing a file (called from Rust)
 */
window.syncFlushFile = function(path) {
    console.log(`[JS] syncFlushFile: ${path}`);
    // No explicit flush needed for localStorage
    return true;
};

/**
 * Synchronous wrapper for creating directories (called from Rust)
 */
window.syncCreateDirAll = function(path) {
    console.log(`[JS] syncCreateDirAll: ${path}`);
    
    const dirs = JSON.parse(localStorage.getItem('directories') || '[]');
    if (!dirs.includes(path)) {
        dirs.push(path);
        localStorage.setItem('directories', JSON.stringify(dirs));
    }
    
    return true;
};

/**
 * Read a chunk of data from a file
 * @param {string} path - Path to the file
 * @param {number} offset - Offset in bytes where to start reading
 * @param {number} length - Number of bytes to read
 * @returns {Promise<Uint8Array>} - Promise resolving to a Uint8Array of bytes read
 */
export async function readFileChunk(path, offset, length) {
    console.log(`[JS] Reading chunk from file: ${path}, offset: ${offset}, length: ${length}`);
    
    // In a real implementation, this would use File System Access API or similar
    // For now, we'll use a simple localStorage-based mock
    const data = localStorage.getItem(`file_data:${path}`) || '';
    
    // Create a Uint8Array from the stored data, or an empty one if no data exists
    let fullData;
    try {
        // Try to parse as base64
        fullData = Uint8Array.from(atob(data), c => c.charCodeAt(0));
    } catch (e) {
        // If not base64, create an empty array
        fullData = new Uint8Array(0);
    }
    
    // Return the requested chunk, or an empty array if offset is beyond the file size
    if (offset >= fullData.length) {
        return new Uint8Array(0);
    }
    
    // Calculate how many bytes we can actually read
    const availableBytes = Math.min(length, fullData.length - offset);
    return fullData.slice(offset, offset + availableBytes);
}

/**
 * Write a chunk of data to a file
 * @param {string} path - Path to the file
 * @param {number} offset - Offset in bytes where to start writing
 * @param {Uint8Array} data - Data to write
 * @returns {Promise<void>} - Promise resolving when the write is complete
 */
export async function writeFileChunk(path, offset, data) {
    console.log(`[JS] Writing chunk to file: ${path}, offset: ${offset}, size: ${data.length} bytes`);
    
    // In a real implementation, this would use File System Access API or similar
    // For now, we'll use a simple localStorage-based mock
    
    // Get existing file data or create empty data
    let fullData;
    const existingData = localStorage.getItem(`file_data:${path}`);
    
    try {
        // Try to parse existing data as base64
        fullData = existingData ? 
            Uint8Array.from(atob(existingData), c => c.charCodeAt(0)) : 
            new Uint8Array(0);
    } catch (e) {
        // If parsing fails, start with an empty array
        fullData = new Uint8Array(0);
    }
    
    // If we need to write beyond the current end of the file, resize it
    if (offset + data.length > fullData.length) {
        const newData = new Uint8Array(offset + data.length);
        newData.set(fullData.slice(0, fullData.length));
        fullData = newData;
    }
    
    // Write the data at the specified offset
    fullData.set(data, offset);
    
    // Store the modified data
    const base64Data = btoa(String.fromCharCode.apply(null, fullData));
    localStorage.setItem(`file_data:${path}`, base64Data);
    
    // Update metadata
    const metadata = { size: fullData.length };
    localStorage.setItem(`file_metadata:${path}`, JSON.stringify(metadata));
}

/**
 * Flush any cached file data to storage
 * @param {string} path - Path to the file to flush
 * @returns {Promise<void>} - Promise resolving when the flush is complete
 */
export async function flushFile(path) {
    console.log(`[JS] Flushing file: ${path}`);
    // No explicit flush needed for localStorage, but we could implement
    // additional sync logic here if needed for other storage backends
    return Promise.resolve();
}

/**
 * Recursively create a directory path
 * @param {string} path - Directory path to create
 * @returns {Promise<void>} - Promise resolving when the directories are created
 */
export async function createDirAll(path) {
    console.log(`[JS] Creating directory path: ${path}`);
    
    // In a real implementation, this would use File System Access API
    // For now, just track the directory in localStorage
    const dirs = JSON.parse(localStorage.getItem('directories') || '[]');
    if (!dirs.includes(path)) {
        dirs.push(path);
        localStorage.setItem('directories', JSON.stringify(dirs));
    }
}

/**
 * Synchronous wrapper for checking if a directory exists (called from Rust)
 * @param {string} path - Directory path
 * @returns {boolean} - True if directory exists, false otherwise
 */
window.syncDirExists = function(path) {
    const dirs = JSON.parse(localStorage.getItem('directories') || '[]');
    return dirs.includes(path);
};

/**
 * Check if a directory exists (async version)
 * @param {string} path - Directory path to check
 * @returns {Promise<boolean>}
 */
export async function dirExists(path) {
    const dirs = JSON.parse(localStorage.getItem('directories') || '[]');
    return dirs.includes(path);
}
