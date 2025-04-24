## 1. Current Architecture Summary

**File Upload and VFS Flow in test.html:**
- User uploads a file via `<input type="file">`.
- The file is read into a Uint8Array, then base64-encoded in JS (in chunks for memory safety).
- The base64 string is stored in localStorage under `file_data:/filename`.
- Metadata (file size) is stored in localStorage under `file_metadata:/filename`.
- The Rust WASM code, via JS glue, reads file chunks by decoding the base64 string from localStorage and slicing the appropriate bytes.

**Relevant Code Paths:**
- `test.html` handles file upload, base64 encoding, and storage.
- `js/browser_fs.js` (not shown, but referenced) provides JS functions for chunked file access.
- `src/file_io/wasm.rs` and `src/file_io.rs` define the Rust traits and WASM-side implementations for chunked file reading.

**Diagram:**

```mermaid
flowchart TD
    A[User uploads file] --> B[JS reads file as Uint8Array]
    B --> C[JS encodes to base64 in chunks]
    C --> D[JS stores base64 in localStorage (file_data:/filename)]
    B --> E[JS stores metadata in localStorage (file_metadata:/filename)]
    F[Rust WASM code] --calls--> G[JS glue: readFileChunk]
    G --reads--> D
    G --decodes base64, slices bytes--> F
```

---

## 2. LocalStorage Limitations

### a. Size Limits

- **localStorage** is limited per origin (domain+protocol+port).
- Most browsers (Chrome, Firefox, Edge, Safari) limit localStorage to **5MB** per origin.
- This is a hard limit; attempts to store more will throw a `QuotaExceededError`.
- **Base64 encoding increases file size by ~33%** (every 3 bytes become 4 base64 chars).

**Example:**
- 1MB file → ~1.33MB base64 → fits in localStorage.
- 5MB file → ~6.65MB base64 → **exceeds localStorage limit**.

### b. Performance

- **Base64 encoding/decoding is CPU-intensive** for large files.
- Storing/reading multi-megabyte strings in localStorage is slow and can block the main thread.
- localStorage is synchronous; large operations can freeze the UI.
- Reading a large file from localStorage requires decoding the entire base64 string, even for chunked access (unless you store raw binary, which localStorage does not support).

### c. Loading Time

- For a 100MB file:
  - Base64 size: ~133MB.
  - Encoding in JS (chunked) will take several seconds to minutes, depending on device.
  - Writing to localStorage will likely fail due to quota.
- For a 1GB file:
  - Base64 size: ~1.33GB.
  - **Impossible** to store in localStorage; will fail immediately.

---

## 3. Alternatives to localStorage for Browser VFS

### a. **In-Memory File Objects**
- Keep the uploaded File/Blob in memory (window-scoped variable or Map).
- Pros: No size limit except RAM; no base64 overhead; fast.
- Cons: Data lost on reload; not persistent.

### b. **IndexedDB**
- Browser's built-in database for storing large binary data (Blobs, ArrayBuffers).
- Quota: 50MB–2GB+ depending on browser, user, and device.
- Asynchronous API; can store files as Blobs or ArrayBuffers.
- Pros: Designed for large data; no base64 overhead; persistent.
- Cons: More complex API; async (but can be wrapped for chunked sync-like access).

### c. **File System Access API** (a.k.a. Native File System API)
- Allows direct read/write access to user files (with permission).
- Supported in Chromium browsers (Chrome, Edge, Opera).
- Pros: No artificial size limit; true streaming; no copy to browser storage.
- Cons: Not supported in Firefox/Safari; requires user permission.

### d. **Service Worker Caches**
- Can store Blobs, but not designed for large files or random access.

---

## 4. Recommendations

- **Do NOT use localStorage.**
- Use **in-memory** storage for performance as persistent storage of is not required.

---

## 5. Summary Table

| Storage      | Max Size (per origin) | Binary Support | Performance | Persistence | Browser Support     | Async |
|--------------|-----------------------|----------------|-------------|-------------|---------------------|-------|
| localStorage | ~5MB                  | No (base64)    | Poor        | Yes         | All                 | No    |
| IndexedDB    | 50MB–2GB+             | Yes (Blob)     | Good        | Yes         | All modern browsers | Yes   |
| In-memory    | RAM-limited           | Yes (Blob)     | Best        | No          | All                 | No    |
| FS Access    | OS file size          | Yes            | Best        | Yes         | Chromium only       | No    |

---
