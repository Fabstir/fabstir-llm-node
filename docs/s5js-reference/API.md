# Enhanced S5.js Path-Based API Documentation

## Table of Contents

- [Enhanced S5.js Path-Based API Documentation](#enhanced-s5js-path-based-api-documentation)
  - [Table of Contents](#table-of-contents)
  - [Overview](#overview)
  - [Installation](#installation)
  - [Quick Start](#quick-start)
  - [Core API Methods](#core-api-methods)
    - [get(path, options?)](#getpath-options)
      - [Parameters](#parameters)
      - [Returns](#returns)
      - [Data Decoding](#data-decoding)
      - [Example](#example)
    - [put(path, data, options?)](#putpath-data-options)
      - [Parameters](#parameters-1)
      - [Automatic Encoding](#automatic-encoding)
      - [Example](#example-1)
    - [getMetadata(path)](#getmetadatapath)
      - [Parameters](#parameters-2)
      - [Returns](#returns-1)
      - [File Metadata](#file-metadata)
      - [Directory Metadata](#directory-metadata)
      - [Example](#example-2)
    - [delete(path)](#deletepath)
      - [Parameters](#parameters-3)
      - [Returns](#returns-2)
      - [Notes](#notes)
      - [Example](#example-3)
    - [list(path, options?)](#listpath-options)
      - [Parameters](#parameters-4)
      - [Yields](#yields)
      - [Example](#example-4)
  - [Encryption](#encryption)
    - [Overview](#overview-1)
    - [Basic Usage](#basic-usage)
    - [User-Provided Encryption Keys](#user-provided-encryption-keys)
    - [Encryption Examples](#encryption-examples)
    - [How Encryption Works](#how-encryption-works)
    - [Security Considerations](#security-considerations)
    - [Encryption Metadata](#encryption-metadata)
    - [Performance Impact](#performance-impact)
    - [Limitations](#limitations-1)
  - [Types and Interfaces](#types-and-interfaces)
    - [PutOptions](#putoptions)
    - [GetOptions](#getoptions)
    - [ListOptions](#listoptions)
    - [ListResult](#listresult)
  - [Path Resolution](#path-resolution)
  - [Cursor-Based Pagination](#cursor-based-pagination)
    - [How Cursors Work](#how-cursors-work)
    - [Pagination Example](#pagination-example)
    - [Cursor Stability](#cursor-stability)
  - [Error Handling](#error-handling)
    - [Common Errors](#common-errors)
    - [Invalid Cursor Errors](#invalid-cursor-errors)
  - [Examples](#examples)
    - [File Management](#file-management)
    - [Batch Operations with Progress](#batch-operations-with-progress)
    - [Clean-up Operations](#clean-up-operations)
  - [Integration with FS5 Class Methods](#integration-with-fs5-class-methods)
  - [Best Practices](#best-practices)
  - [Limitations](#limitations)
  - [HAMT (Hash Array Mapped Trie) Support](#hamt-hash-array-mapped-trie-support)
    - [How HAMT Works](#how-hamt-works)
    - [HAMT Behavior](#hamt-behavior)
    - [Working with Large Directories](#working-with-large-directories)
    - [HAMT Implementation Details](#hamt-implementation-details)
  - [Directory Utilities (Phase 4)](#directory-utilities-phase-4)
    - [DirectoryWalker](#directorywalker)
    - [BatchOperations](#batchoperations)
    - [Directory Utility Examples](#directory-utility-examples)
  - [Media Processing (Phase 5)](#media-processing-phase-5)
    - [MediaProcessor](#mediaprocessor)
    - [Image Metadata Extraction](#image-metadata-extraction)
    - [Browser Compatibility Detection](#browser-compatibility-detection)
    - [Processing Strategies](#processing-strategies)
    - [Lazy Loading and Code Splitting](#lazy-loading-and-code-splitting)
    - [Media Processing Examples](#media-processing-examples)
  - [Performance Considerations](#performance-considerations)
  - [Performance Testing](#performance-testing)
  - [Bundle Size Optimization](#bundle-size-optimization)
  - [Next Steps](#next-steps)

## Overview

The Enhanced S5.js Path-Based API provides developer-friendly methods for file and directory operations on the S5 decentralised storage network. This implementation uses a **new data format**:

- **CBOR serialization** instead of MessagePack
- **DirV1 specification** with deterministic encoding
- **No backward compatibility** with old S5 data formats

The API offers an intuitive interface using familiar path syntax while implementing this clean, new format.

## Installation

The enhanced path-based API features are currently in development as part of a Sia Foundation grant project.

**For production use:**

```bash
npm install @s5-dev/s5js
```

**To try the enhanced features:**

- Clone from: https://github.com/julesl23/s5.js
- See the [Development Setup](#development-setup) section for build instructions

**Status**: These features are pending review and have not been merged into the main S5.js repository.

## Quick Start

```typescript
import { S5 } from "@s5-dev/s5js";

// Create S5 instance and connect to peers
const s5 = await S5.create({
  initialPeers: [
    "wss://z2DWuPbL5pweybXnEB618pMnV58ECj2VPDNfVGm3tFqBvjF@s5.ninja/s5/p2p"
  ]
});

// Generate a new seed phrase
const seedPhrase = s5.generateSeedPhrase();

// Or recover from existing seed phrase
await s5.recoverIdentityFromSeedPhrase(seedPhrase);

// Register on S5 portal (s5.vup.cx supports the new API)
await s5.registerOnNewPortal("https://s5.vup.cx");

// Initialize filesystem (creates home and archive directories)
await s5.fs.ensureIdentityInitialized();

// Store data
await s5.fs.put("home/documents/hello.txt", "Hello, S5!");

// Retrieve data
const content = await s5.fs.get("home/documents/hello.txt");
console.log(content); // "Hello, S5!"

// List directory contents
for await (const item of s5.fs.list("home/documents")) {
  console.log(`${item.type}: ${item.name}`);
}
```

## Core API Methods

### get(path, options?)

Retrieve data from a file at the specified path.

```typescript
async get(path: string, options?: GetOptions): Promise<any | undefined>
```

#### Parameters

- **path** (string): The file path (e.g., "home/documents/file.txt")
- **options** (GetOptions, optional): Configuration options
  - `defaultMediaType` (string): Default media type for content interpretation

#### Returns

- The decoded file data (string, object, or Uint8Array)
- `undefined` if the file doesn't exist

#### Data Decoding

The method automatically detects and decodes data:

1. Attempts CBOR decoding first (for objects)
2. Falls back to JSON parsing
3. Then attempts UTF-8 text decoding
4. Returns raw Uint8Array if all decoding fails

#### Example

```typescript
// Get text file
const content = await s5.fs.get("home/readme.txt");
console.log(content); // "Hello, world!"

// Get JSON/CBOR data
const data = await s5.fs.get("home/config.json");
console.log(data); // { version: "1.0", settings: {...} }

// Get binary data
const image = await s5.fs.get("home/photo.jpg");
console.log(image); // Uint8Array[...]
```

### put(path, data, options?)

Store data at the specified path, creating intermediate directories as needed.

```typescript
async put(path: string, data: any, options?: PutOptions): Promise<void>
```

#### Parameters

- **path** (string): The file path where data will be stored
- **data** (any): The data to store (string, object, or Uint8Array)
- **options** (PutOptions, optional): Configuration options
  - `mediaType` (string): MIME type for the file
  - `timestamp` (number): Custom timestamp (milliseconds since epoch)

#### Automatic Encoding

- Objects are encoded as CBOR
- Strings are encoded as UTF-8
- Uint8Array stored as-is
- Media type auto-detected from file extension if not provided

#### Example

```typescript
// Store text
await s5.fs.put("home/notes.txt", "My notes here");

// Store JSON data (encoded as CBOR)
await s5.fs.put("home/data.json", {
  name: "Test",
  values: [1, 2, 3],
});

// Store with custom media type
await s5.fs.put("home/styles.css", cssContent, {
  mediaType: "text/css",
});

// Store with custom timestamp
await s5.fs.put("home/backup.txt", "content", {
  timestamp: Date.now() - 86400000, // 1 day ago
});
```

### getMetadata(path)

Retrieve metadata about a file or directory without downloading the content.

```typescript
async getMetadata(path: string): Promise<Record<string, any> | undefined>
```

#### Parameters

- **path** (string): The file or directory path

#### Returns

- Metadata object for the file/directory
- `undefined` if the path doesn't exist

#### File Metadata

```typescript
{
  type: "file",
  name: "example.txt",
  size: 1234,              // Size in bytes
  mediaType: "text/plain",
  timestamp: 1705432100000 // Milliseconds since epoch
  // Note: Content hashes (CIDs) are not exposed in the path-based API
  // Files are identified by their paths, abstracting away content addressing
}
```

#### Directory Metadata

```typescript
{
  type: "directory",
  name: "documents",
  fileCount: 10,       // Number of files
  directoryCount: 3    // Number of subdirectories
}
```

#### Example

```typescript
const fileMeta = await s5.fs.getMetadata("home/document.pdf");
if (fileMeta) {
  console.log(`Size: ${fileMeta.size} bytes`);
  console.log(`Type: ${fileMeta.mediaType}`);
}

const dirMeta = await s5.fs.getMetadata("home/photos");
if (dirMeta) {
  console.log(`Contains ${dirMeta.fileCount} files`);
}
```

### delete(path)

Delete a file or empty directory.

```typescript
async delete(path: string): Promise<boolean>
```

#### Parameters

- **path** (string): The file or directory path to delete

#### Returns

- `true` if successfully deleted
- `false` if the path doesn't exist

#### Notes

- Only empty directories can be deleted
- Root directories ("home", "archive") cannot be deleted
- Parent directory must exist

#### Example

```typescript
// Delete a file
const deleted = await s5.fs.delete("home/temp.txt");
console.log(deleted ? "Deleted" : "Not found");

// Delete an empty directory
await s5.fs.delete("home/old-folder");

// Returns false for non-existent paths
const result = await s5.fs.delete("home/ghost.txt"); // false
```

### list(path, options?)

List contents of a directory with optional cursor-based pagination.

```typescript
async *list(path: string, options?: ListOptions): AsyncIterableIterator<ListResult>
```

#### Parameters

- **path** (string): The directory path
- **options** (ListOptions, optional): Configuration options
  - `limit` (number): Maximum items to return
  - `cursor` (string): Resume from a previous position

#### Yields

```typescript
interface ListResult {
  name: string;
  type: "file" | "directory";
  size?: number;         // File size in bytes (for files)
  mediaType?: string;    // MIME type (for files)
  timestamp?: number;    // Milliseconds since epoch
  cursor?: string;       // Pagination cursor
}
```

#### Example

```typescript
// List all items
for await (const item of s5.fs.list("home")) {
  console.log(`${item.type}: ${item.name}`);
}

// List with limit
for await (const item of s5.fs.list("home", { limit: 10 })) {
  console.log(item.name);
}

// Pagination example
const firstPage = [];
let lastCursor;

for await (const item of s5.fs.list("home/docs", { limit: 20 })) {
  firstPage.push(item);
  lastCursor = item.cursor;
}

// Get next page
for await (const item of s5.fs.list("home/docs", {
  cursor: lastCursor,
  limit: 20,
})) {
  console.log(item.name);
}
```

## Encryption

Enhanced S5.js provides built-in encryption support using **XChaCha20-Poly1305**, an authenticated encryption algorithm that ensures both confidentiality and integrity of your data.

### Overview

- **Algorithm**: XChaCha20-Poly1305 (AEAD cipher)
- **Key Size**: 256-bit (32 bytes)
- **Chunk Size**: 256 KiB chunks for large files
- **Automatic**: Encryption/decryption is transparent once configured
- **Secure**: Each chunk gets a unique nonce for maximum security

### Basic Usage

Encrypt data by adding the `encryption` option to `put()`:

```typescript
// Auto-generate encryption key
await s5.fs.put("home/secrets/credentials.json", sensitiveData, {
  encryption: {
    algorithm: "xchacha20-poly1305",
  },
});

// Retrieve and decrypt automatically
const data = await s5.fs.get("home/secrets/credentials.json");
console.log(data); // Original decrypted data
```

### User-Provided Encryption Keys

For advanced use cases, you can provide your own encryption key:

```typescript
// Generate or derive a 32-byte encryption key
const encryptionKey = s5.api.crypto.generateSecureRandomBytes(32);

// Store with custom key
await s5.fs.put("home/vault/secret.txt", "Top secret message", {
  encryption: {
    algorithm: "xchacha20-poly1305",
    key: encryptionKey,
  },
});

// Retrieve - decryption is automatic if you have access
const secret = await s5.fs.get("home/vault/secret.txt");
```

### Encryption Examples

#### Encrypting Sensitive Configuration

```typescript
const apiConfig = {
  apiKey: "sk_live_abc123xyz789",
  secretKey: "whsec_def456uvw012",
  databaseUrl: "postgresql://user:pass@host/db",
};

// Store encrypted configuration
await s5.fs.put("home/config/api-keys.json", apiConfig, {
  encryption: {
    algorithm: "xchacha20-poly1305",
  },
});

// Later: retrieve and use
const config = await s5.fs.get("home/config/api-keys.json");
console.log(config.apiKey); // Decrypted value
```

#### Encrypting Personal Documents

```typescript
const documents = [
  { path: "home/personal/passport.pdf", data: passportScan },
  { path: "home/personal/ssn.txt", data: "123-45-6789" },
  { path: "home/personal/bank-info.json", data: bankDetails },
];

// Encrypt all personal documents
for (const doc of documents) {
  await s5.fs.put(doc.path, doc.data, {
    encryption: {
      algorithm: "xchacha20-poly1305",
    },
  });
}

// List directory - filenames visible, contents encrypted
for await (const item of s5.fs.list("home/personal")) {
  console.log(item.name); // File names are visible
  const content = await s5.fs.get(`home/personal/${item.name}`);
  // Content is automatically decrypted
}
```

#### Key Management with Derived Keys

```typescript
// Derive encryption key from user password (in production, use proper KDF)
import { hashBlake3 } from "@s5-dev/s5js";

async function deriveKeyFromPassword(password: string): Promise<Uint8Array> {
  const encoder = new TextEncoder();
  return s5.api.crypto.hashBlake3(encoder.encode(password));
}

// Encrypt with password-derived key
const userPassword = "correct-horse-battery-staple";
const derivedKey = await deriveKeyFromPassword(userPassword);

await s5.fs.put("home/diary/2024-01-15.txt", "Dear diary...", {
  encryption: {
    algorithm: "xchacha20-poly1305",
    key: derivedKey,
  },
});

// Decrypt with same password
const sameKey = await deriveKeyFromPassword(userPassword);
// Note: The key must match for decryption to work
const entry = await s5.fs.get("home/diary/2024-01-15.txt");
```

#### Encrypting Binary Data

```typescript
// Encrypt image files
const imageData = await fetch("/path/to/photo.jpg").then((r) =>
  r.arrayBuffer()
);

await s5.fs.put("home/photos/private/vacation.jpg", new Uint8Array(imageData), {
  mediaType: "image/jpeg",
  encryption: {
    algorithm: "xchacha20-poly1305",
  },
});

// Retrieve encrypted image
const decryptedImage = await s5.fs.get("home/photos/private/vacation.jpg");
// decryptedImage is a Uint8Array of the original image
```

#### Large File Encryption

```typescript
// Large files are automatically chunked during encryption
const largeFile = new Uint8Array(10 * 1024 * 1024); // 10 MB
// ... fill with data ...

await s5.fs.put("home/backups/database.sql", largeFile, {
  mediaType: "application/sql",
  encryption: {
    algorithm: "xchacha20-poly1305",
  },
});

// Retrieval automatically handles chunked decryption
const restored = await s5.fs.get("home/backups/database.sql");
console.log(`Restored ${restored.length} bytes`);
```

### How Encryption Works

1. **Encryption Process** (`put()` with encryption):

   - Data is split into 256 KiB chunks
   - Each chunk is encrypted with XChaCha20-Poly1305
   - Each chunk gets a unique nonce (sequential: 0, 1, 2...)
   - Encrypted blob is uploaded to S5 network
   - Encryption metadata (key, algorithm) stored in directory entry

2. **Decryption Process** (`get()` on encrypted file):
   - Encryption metadata retrieved from directory entry
   - Encrypted blob downloaded from S5 network
   - Each chunk is decrypted with the stored key
   - Chunks are reassembled into original data
   - Data is returned to caller

### Security Considerations

#### Encryption Key Storage

**Important**: The encryption key is stored in the directory metadata. This means:

- ✅ **Convenience**: No separate key management needed
- ✅ **Automatic**: Decryption works transparently with directory access
- ⚠️ **Access Control**: Anyone with directory read access can decrypt files
- ⚠️ **Key Exposure**: Keys are visible to anyone who can read the directory

**For maximum security**, consider:

1. **User-Provided Keys**: Supply your own keys and manage them separately

   ```typescript
   const userKey = deriveFromPassword(password); // Keep key separate
   await s5.fs.put(path, data, { encryption: { key: userKey } });
   ```

2. **Directory-Level Encryption**: Encrypt the entire directory with a separate key
3. **Key Derivation**: Derive keys from user credentials that aren't stored

#### Best Practices

1. **Use Auto-Generated Keys** for convenience when directory access control is sufficient
2. **Use Custom Keys** when you need encryption independent of directory access
3. **Never commit encryption keys** to source control
4. **Rotate keys periodically** for sensitive data
5. **Use strong key derivation** (e.g., PBKDF2, Argon2) if deriving from passwords
6. **Consider the threat model**: Encryption protects against network observers and storage providers, but not against directory access

#### What Encryption Protects

- ✅ **Content confidentiality**: File contents cannot be read without the key
- ✅ **Content integrity**: Modifications are detected (authenticated encryption)
- ✅ **Network privacy**: Data is encrypted in transit and at rest
- ❌ **File names**: Directory entry names are NOT encrypted
- ❌ **Metadata**: File sizes, timestamps, counts remain visible
- ❌ **Access patterns**: Who accesses which files can still be observed

### Encryption Metadata

Encrypted files store metadata in the FileRef's `extra` field:

```typescript
// Example FileRef for encrypted file
{
  hash: Uint8Array,        // Encrypted blob hash
  size: 12345,             // Original plaintext size
  media_type: "text/plain",
  timestamp: 1705432100,
  extra: Map([
    ['encryption', {
      algorithm: 'xchacha20-poly1305',
      key: [123, 45, 67, ...],  // 32-byte encryption key
      plaintextHash: [...]       // Original plaintext hash
    }]
  ])
}
```

You can check if a file is encrypted via metadata:

```typescript
const metadata = await s5.fs.getMetadata("home/secrets/file.txt");
if (metadata.custom?.encryption) {
  console.log("File is encrypted");
  console.log("Algorithm:", metadata.custom.encryption.algorithm);
}
```

### Performance Impact

Encryption has minimal performance impact:

- **Encryption overhead**: ~1-2% for XChaCha20-Poly1305 (very fast)
- **Chunk processing**: Parallel chunk encryption for large files
- **Memory usage**: Chunks processed incrementally (constant memory)
- **Network**: Same upload/download sizes (minimal encryption expansion)

**Benchmarks** (approximate):

- Small files (<1 MB): Negligible overhead (~5-10ms)
- Large files (>10 MB): ~1-2% slower than unencrypted
- Very large files (>100 MB): Chunked processing maintains performance

### Limitations

- **Algorithm**: Currently only XChaCha20-Poly1305 is supported
- **Key Storage**: Keys are stored in directory metadata (see Security Considerations)
- **Migration**: Cannot change encryption key for existing files (must re-upload)
- **Partial Decryption**: Must decrypt entire file (no partial chunk reads)
- **Compression**: No automatic compression before encryption (plan ahead)

## Types and Interfaces

### PutOptions

```typescript
interface PutOptions {
  mediaType?: string; // MIME type (e.g., "text/plain", "image/jpeg")
  timestamp?: number; // Custom timestamp (milliseconds since epoch)
  encryption?: {
    // Encryption configuration
    algorithm: "xchacha20-poly1305"; // Currently only supported algorithm
    key?: Uint8Array; // Optional 32-byte encryption key (auto-generated if omitted)
  };
}
```

### GetOptions

```typescript
interface GetOptions {
  defaultMediaType?: string; // Default media type for content interpretation
}
```

### ListOptions

```typescript
interface ListOptions {
  limit?: number; // Maximum items to return
  cursor?: string; // Pagination cursor from previous result
}
```

### ListResult

```typescript
interface ListResult {
  name: string;
  type: "file" | "directory";
  size?: number;         // File size in bytes (for files)
  mediaType?: string;    // MIME type (for files)
  timestamp?: number;    // Milliseconds since epoch
  cursor?: string;       // Opaque cursor for pagination
}
```

## Path Resolution

- Paths use forward slashes (`/`) as separators
- Leading slash is optional: `"home/file.txt"` equals `"/home/file.txt"`
- Empty path (`""`) refers to the root directory
- Paths are case-sensitive
- UTF-8 characters are supported in file and directory names
- Avoid trailing slashes except for clarity

## Cursor-Based Pagination

The `list()` method supports efficient pagination through large directories using cursors.

### How Cursors Work

- Each item in a listing includes a `cursor` field
- The cursor encodes the position of that item deterministically
- To get the next page, pass the last item's cursor to the next `list()` call
- Cursors are stable - the same position produces the same cursor
- Cursors are opaque base64url-encoded strings - don't parse or modify them
- Invalid cursors will throw an "Invalid cursor" error

### Pagination Example

```typescript
async function listAllItems(path: string, pageSize: number = 100) {
  const allItems = [];
  let cursor: string | undefined;

  while (true) {
    let hasItems = false;

    for await (const item of s5.fs.list(path, { cursor, limit: pageSize })) {
      allItems.push(item);
      cursor = item.cursor;
      hasItems = true;
    }

    if (!hasItems) break;
  }

  return allItems;
}
```

### Cursor Stability

- Cursors remain valid as long as the directory structure is stable
- Adding items after the cursor position doesn't invalidate it
- Deleting items before the cursor may cause skipped entries
- Cursors encode position, type, and name for stability

## Error Handling

All methods handle errors gracefully:

### Common Errors

```typescript
try {
  await s5.fs.put("invalid/path", "content");
} catch (error) {
  if (error.message.includes("does not exist")) {
    // Parent directory doesn't exist
  }
}

try {
  await s5.fs.delete("home"); // Cannot delete root
} catch (error) {
  console.error("Cannot delete root directory");
}
```

### Invalid Cursor Errors

```typescript
try {
  for await (const item of s5.fs.list("home", { cursor: "invalid!" })) {
    // ...
  }
} catch (error) {
  if (error.message.includes("Invalid cursor")) {
    // Handle invalid cursor - start from beginning
    for await (const item of s5.fs.list("home")) {
      // ...
    }
  }
}
```

## Examples

### File Management

```typescript
// Create a project structure
const files = {
  "home/project/README.md": "# My Project\n\nDescription here",
  "home/project/src/index.js": "console.log('Hello');",
  "home/project/package.json": {
    name: "my-project",
    version: "1.0.0",
    main: "src/index.js",
  },
};

// Upload all files
for (const [path, content] of Object.entries(files)) {
  await s5.fs.put(path, content);
}

// Verify structure
async function printTree(path: string, indent = "") {
  for await (const item of s5.fs.list(path)) {
    console.log(
      `${indent}${item.type === "directory" ? "📁" : "📄"} ${item.name}`
    );
    if (item.type === "directory") {
      await printTree(`${path}/${item.name}`, indent + "  ");
    }
  }
}

await printTree("home/project");
```

### Batch Operations with Progress

```typescript
async function uploadDirectory(localPath: string, s5Path: string) {
  const files = await getLocalFiles(localPath); // Your implementation
  let uploaded = 0;

  for (const file of files) {
    const content = await readFile(file.path);
    await s5.fs.put(`${s5Path}/${file.relativePath}`, content, {
      mediaType: file.mimeType,
    });

    uploaded++;
    console.log(`Progress: ${uploaded}/${files.length}`);
  }
}
```

### Clean-up Operations

```typescript
async function cleanupTempFiles(basePath: string) {
  let cleaned = 0;

  for await (const item of s5.fs.list(basePath)) {
    if (item.type === "file" && item.name.endsWith(".tmp")) {
      const deleted = await s5.fs.delete(`${basePath}/${item.name}`);
      if (deleted) cleaned++;
    } else if (item.type === "directory") {
      // Recursively clean subdirectories
      await cleanupTempFiles(`${basePath}/${item.name}`);
    }
  }

  console.log(`Cleaned ${cleaned} temporary files`);
}
```

## Integration with FS5 Class Methods

The path-based API methods work alongside the existing FS5 class methods. Both use the same underlying DirV1 format:

```typescript
// Use existing FS5 methods (now using DirV1 format)
const fileVersion = await s5.fs.uploadBlobWithoutEncryption(blob);
await s5.fs.createFile("home", "newfile.txt", fileVersion, "text/plain");

// Access the same file via path API
const content = await s5.fs.get("home/newfile.txt");

// Mix approaches as needed - all using DirV1 format
await s5.fs.createDirectory("home", "newfolder");
await s5.fs.put("home/newfolder/data.json", { created: Date.now() });
```

**Note**: All methods now use the new CBOR/DirV1 format. There is no compatibility with old S5 data.

## Best Practices

1. **Path Format**: Use forward slashes (`/`) without leading slashes
2. **Error Handling**: Always wrap API calls in try-catch blocks
3. **Pagination**: Use cursors for directories with many items (>100)
4. **Media Types**: Explicitly specify media types for better content handling
5. **Batch Operations**: Group related operations when possible
6. **Directory Creation**: Intermediate directories are created automatically with `put()`
7. **Binary Data**: Use Uint8Array for binary content
8. **Timestamps**: Use milliseconds since epoch for consistency

## Limitations

- Cannot delete non-empty directories
- Cannot store data directly at the root path
- Cursor pagination is forward-only (no backwards navigation)
- Maximum file size depends on S5 network limits
- Path segments cannot contain forward slashes
- Root directories ("home", "archive") are immutable

## HAMT (Hash Array Mapped Trie) Support

The Enhanced S5.js implementation includes automatic HAMT sharding for efficient handling of large directories. This feature activates transparently when directories exceed 1000 entries.

### How HAMT Works

- **Automatic Activation**: Directories automatically convert to HAMT structure at 1000+ entries
- **Transparent Operation**: All existing API methods work seamlessly with sharded directories
- **Performance**: O(log n) access time for directories with millions of entries
- **Lazy Loading**: HAMT nodes are loaded on-demand for memory efficiency
- **Deterministic**: Uses xxhash64 for consistent sharding across implementations

### HAMT Behavior

When a directory reaches the sharding threshold:

1. The directory structure automatically converts to HAMT format
2. Entries are distributed across multiple nodes based on hash values
3. All operations continue to work without code changes
4. Performance remains consistent even with millions of entries

### Working with Large Directories

```typescript
// Adding many files - HAMT activates automatically
for (let i = 0; i < 10000; i++) {
  await s5.fs.put(`home/large-dir/file${i}.txt`, `Content ${i}`);
}

// Listing still works normally with cursor pagination
for await (const item of s5.fs.list("home/large-dir", { limit: 100 })) {
  console.log(item.name); // Efficiently iterates through sharded structure
}

// Direct access remains fast even with millions of entries
const file = await s5.fs.get("home/large-dir/file9999.txt");
console.log(file); // O(log n) lookup time
```

### HAMT Implementation Details

- **Branching Factor**: 32-way branching using 5-bit chunks
- **Hash Function**: xxhash64 for key distribution
- **Node Types**: Internal nodes (pointers) and leaf nodes (entries)
- **Serialization**: CBOR format matching Rust S5 implementation
- **Memory Efficient**: Nodes loaded only when accessed

## Directory Utilities (Phase 4)

Phase 4 adds powerful utility classes for recursive directory operations and batch processing.

### DirectoryWalker

The `DirectoryWalker` class provides efficient recursive directory traversal with cursor support for resumable operations.

#### Constructor

```typescript
import { DirectoryWalker } from "@s5-dev/s5js";

const walker = new DirectoryWalker(s5.fs, '/home/projects');
```

#### walk(options?)

Recursively traverse a directory tree, yielding entries as they are discovered.

```typescript
interface WalkOptions {
  recursive?: boolean;      // Whether to recurse into subdirectories (default: true)
  maxDepth?: number;        // Maximum depth to traverse
  includeFiles?: boolean;   // Whether to include files in results (default: true)
  includeDirectories?: boolean; // Whether to include directories in results (default: true)
  filter?: (name: string, type: 'file' | 'directory') => boolean;  // Filter entries
  cursor?: string;          // Resume from cursor position
}

interface WalkResult {
  path: string;             // Full path to the entry
  name: string;             // Entry name
  type: 'file' | 'directory'; // Type of entry
  size?: number;            // Size in bytes (for files)
  depth: number;            // Depth from starting directory
  cursor?: string;          // Cursor for resuming
}

// Basic usage
const walker = new DirectoryWalker(s5.fs, "home/projects");
for await (const result of walker.walk()) {
  console.log(`${result.path} (depth: ${result.depth})`);
}

// With options
const walker2 = new DirectoryWalker(s5.fs, "home");
for await (const result of walker2.walk({
  maxDepth: 2,
  filter: (name, type) => !name.startsWith(".")  // Skip hidden files
})) {
  if (result.type === 'file') {
    console.log(`File: ${result.path} (${result.size} bytes)`);
  } else {
    console.log(`Dir: ${result.path}`);
  }
}

// Resumable walk with cursor
const walker3 = new DirectoryWalker(s5.fs, "home/large-dir");
let lastCursor: string | undefined;
try {
  for await (const result of walker3.walk({ cursor: savedCursor })) {
    lastCursor = result.cursor;
    // Process entry...
  }
} catch (error) {
  // Can resume from lastCursor
  await saveResumePoint(lastCursor);
}
```

#### count(options?)

Count entries in a directory tree without loading all data.

```typescript
interface WalkStats {
  files: number;
  directories: number;
  totalSize: number;
}

const walker = new DirectoryWalker(s5.fs, "home/projects");
const stats = await walker.count({ recursive: true });
console.log(`Files: ${stats.files}, Dirs: ${stats.directories}, Size: ${stats.totalSize}`);
```

### BatchOperations

The `BatchOperations` class provides high-level operations for copying and deleting entire directory trees with progress tracking and error handling.

#### Constructor

```typescript
import { BatchOperations } from "@s5-dev/s5js";

const batch = new BatchOperations(s5.fs);
```

#### copyDirectory(sourcePath, destPath, options?)

Copy an entire directory tree to a new location.

```typescript
interface BatchOptions {
  recursive?: boolean;       // Copy subdirectories (default: true)
  onProgress?: (progress: BatchProgress) => void;  // Progress callback
  onError?: "stop" | "continue" | ((error: Error, path: string) => "stop" | "continue");
  cursor?: string;           // Resume from cursor
  preserveMetadata?: boolean; // Preserve file metadata (default: true)
}

interface BatchProgress {
  operation: "copy" | "delete";
  total?: number;
  processed: number;
  currentPath: string;
  cursor?: string;
}

interface BatchResult {
  success: number;
  failed: number;
  errors: Array<{ path: string; error: Error }>;
  cursor?: string;          // For resuming if interrupted
}

// Basic copy
const result = await batch.copyDirectory("home/source", "home/backup");
console.log(`Copied ${result.success} items`);

// With progress tracking
const result = await batch.copyDirectory("home/photos", "archive/photos-2024", {
  onProgress: (progress) => {
    console.log(`Copying ${progress.currentPath} (${progress.processed} done)`);
  },
  onError: "continue"  // Continue on errors
});

if (result.failed > 0) {
  console.log(`Failed to copy ${result.failed} items:`);
  result.errors.forEach(e => console.log(`  ${e.path}: ${e.error.message}`));
}

// Resumable copy
let resumeCursor = savedCursor; // From previous interrupted operation
const result = await batch.copyDirectory("home/large-project", "backup/project", {
  cursor: resumeCursor,
  onProgress: (progress) => {
    // Save cursor periodically for resume capability
    if (progress.processed % 100 === 0) {
      saveCursor(progress.cursor);
    }
  }
});
```

#### deleteDirectory(path, options?)

Delete a directory and optionally all its contents.

```typescript
// Delete empty directory only
await batch.deleteDirectory("home/temp", { recursive: false });

// Delete directory tree
const result = await batch.deleteDirectory("home/old-project", {
  recursive: true,
  onProgress: (progress) => {
    console.log(`Deleting ${progress.currentPath} (${progress.processed}/${progress.total})`);
  }
});

// With error handling
const result = await batch.deleteDirectory("home/cache", {
  recursive: true,
  onError: (error, path) => {
    if (error.message.includes("permission")) {
      console.log(`Skipping protected file: ${path}`);
      return "continue";
    }
    return "stop";
  }
});
```

### Directory Utility Examples

#### Backup with Progress

```typescript
async function backupDirectory(source: string, dest: string) {
  const batch = new BatchOperations(s5.fs);
  const startTime = Date.now();

  console.log(`Starting backup of ${source}...`);

  const result = await batch.copyDirectory(source, dest, {
    onProgress: (progress) => {
      process.stdout.write(`\rProcessed: ${progress.processed} items`);
    },
    onError: "continue"
  });

  const duration = (Date.now() - startTime) / 1000;
  console.log(`\nBackup complete in ${duration}s`);
  console.log(`Success: ${result.success}, Failed: ${result.failed}`);

  if (result.failed > 0) {
    const logPath = `${dest}-errors.log`;
    const errorLog = result.errors.map(e =>
      `${e.path}: ${e.error.message}`
    ).join('\n');
    await s5.fs.put(logPath, errorLog);
    console.log(`Error log saved to ${logPath}`);
  }
}
```

## Media Processing (Phase 5)

Phase 5 introduces a comprehensive media processing foundation with WASM-based image metadata extraction, Canvas fallback, and intelligent browser capability detection.

### MediaProcessor

The `MediaProcessor` class provides unified image metadata extraction with automatic fallback between WASM and Canvas implementations based on browser capabilities.

#### Basic Usage

```typescript
import { MediaProcessor } from "@s5-dev/s5js";
// Or for code-splitting:
import { MediaProcessor } from "s5/media";

// Initialize the processor (auto-detects best strategy)
await MediaProcessor.initialize();

// Extract metadata from an image
const imageBlob = await fetch('/path/to/image.jpg').then(r => r.blob());
const metadata = await MediaProcessor.extractMetadata(imageBlob);

console.log(metadata);
// {
//   width: 1920,
//   height: 1080,
//   format: 'jpeg',
//   size: 245678,
//   hasAlpha: false,
//   dominantColors: [...],
//   aspectRatio: 'landscape',
//   ...
// }
```

#### Initialization Options

```typescript
interface InitializeOptions {
  wasmUrl?: string;          // Custom WASM binary URL
  onProgress?: (percent: number) => void;  // Loading progress callback
  preferredStrategy?: ProcessingStrategy;  // Force specific strategy
}

// With progress tracking
await MediaProcessor.initialize({
  onProgress: (percent) => {
    console.log(`Loading: ${percent}%`);
  }
});

// Force Canvas-only mode (no WASM)
const metadata = await MediaProcessor.extractMetadata(blob, {
  useWASM: false
});

// With timeout
const metadata = await MediaProcessor.extractMetadata(blob, {
  timeout: 5000  // 5 second timeout
});
```

### Image Metadata Extraction

The media processor can extract comprehensive metadata from images:

#### ImageMetadata Interface

```typescript
interface ImageMetadata {
  // Basic properties
  width: number;
  height: number;
  format: 'jpeg' | 'png' | 'webp' | 'gif' | 'bmp' | 'unknown';
  size: number;              // File size in bytes
  hasAlpha: boolean;         // Transparency support

  // Color analysis
  dominantColors?: DominantColor[];
  isMonochrome?: boolean;
  colorSpace?: 'srgb' | 'display-p3' | 'rec2020' | 'unknown';

  // Image characteristics
  aspectRatio?: 'landscape' | 'portrait' | 'square';
  aspectRatioValue?: number; // Numerical ratio (width/height)
  commonAspectRatio?: string; // e.g., "16:9", "4:3", "1:1"

  // Technical details
  bitDepth?: number;         // Bits per channel (8, 16, etc.)
  isProgressive?: boolean;   // Progressive JPEG
  isInterlaced?: boolean;    // Interlaced PNG/GIF
  isAnimated?: boolean;      // Animated GIF/WebP
  frameCount?: number;       // Number of animation frames

  // EXIF data (if available)
  exifData?: {
    make?: string;           // Camera manufacturer
    model?: string;          // Camera model
    dateTime?: string;       // Creation date
    orientation?: number;    // EXIF orientation (1-8)
    gpsLocation?: {
      latitude: number;
      longitude: number;
    };
  };

  // Quality metrics
  estimatedQuality?: number; // JPEG quality estimate (0-100)
  histogram?: HistogramData; // Color distribution
  exposureWarning?: 'overexposed' | 'underexposed' | 'normal';

  // Processing metadata
  source: 'wasm' | 'canvas'; // Which engine processed it
  processingTime?: number;   // Milliseconds
  processingSpeed?: 'fast' | 'normal' | 'slow';

  // Validation
  isValidImage: boolean;
  validationErrors?: string[];
}

interface DominantColor {
  hex: string;              // "#FF5733"
  rgb: { r: number; g: number; b: number };
  percentage: number;       // Percentage of image
}
```

### Browser Compatibility Detection

The `BrowserCompat` class automatically detects browser capabilities and selects the optimal processing strategy:

```typescript
import { BrowserCompat } from "@s5-dev/s5js";

// Check browser capabilities
const capabilities = await BrowserCompat.checkCapabilities();
console.log(capabilities);
// {
//   webAssembly: true,
//   webAssemblyStreaming: true,
//   sharedArrayBuffer: false,
//   webWorkers: true,
//   offscreenCanvas: true,
//   webP: true,
//   avif: false,
//   createImageBitmap: true,
//   webGL: true,
//   webGL2: true,
//   memoryLimit: 2048,
//   performanceAPI: true,
//   memoryInfo: true
// }

// Get recommended processing strategy
const strategy = BrowserCompat.selectProcessingStrategy(capabilities);
console.log(strategy); // 'wasm-worker' | 'wasm-main' | 'canvas-worker' | 'canvas-main'

// Get optimization recommendations
const recommendations = BrowserCompat.getOptimizationRecommendations(capabilities);
recommendations.forEach(rec => console.log(rec));
// ["Consider enabling SharedArrayBuffer for better WASM performance"]
// ["WebP support available - use for better compression"]
```

### Processing Strategies

The media processor automatically selects the best strategy based on browser capabilities:

1. **`wasm-worker`** - WASM in Web Worker (best performance)
2. **`wasm-main`** - WASM in main thread (good performance)
3. **`canvas-worker`** - Canvas in Web Worker (moderate performance)
4. **`canvas-main`** - Canvas in main thread (baseline)

```typescript
// Check current strategy
const strategy = MediaProcessor.getProcessingStrategy();
console.log(`Using ${strategy} for image processing`);

// Force specific strategy
await MediaProcessor.initialize({
  preferredStrategy: 'canvas-main'  // Force Canvas-only
});
```

### Lazy Loading and Code Splitting

The media processing module supports code-splitting for optimal bundle sizes:

```typescript
// Option 1: Direct import (includes in main bundle)
import { MediaProcessor } from "@s5-dev/s5js";

// Option 2: Separate media bundle (recommended)
import { MediaProcessor } from "s5/media";

// Option 3: Dynamic import (lazy loading)
const { MediaProcessor } = await import("s5/media");
await MediaProcessor.initialize();

// Option 4: Core-only import (no media features)
import { S5, FS5 } from "s5/core";  // Lighter bundle without media
```

### Media Processing Examples

#### Extract and Display Image Metadata

```typescript
async function analyzeImage(imagePath: string) {
  const blob = await s5.fs.get(imagePath);
  const metadata = await MediaProcessor.extractMetadata(
    new Blob([blob], { type: 'image/jpeg' })
  );

  console.log(`Image: ${imagePath}`);
  console.log(`Dimensions: ${metadata.width}x${metadata.height}`);
  console.log(`Format: ${metadata.format.toUpperCase()}`);
  console.log(`Size: ${(metadata.size / 1024).toFixed(2)} KB`);
  console.log(`Aspect Ratio: ${metadata.commonAspectRatio || metadata.aspectRatio}`);

  if (metadata.dominantColors) {
    console.log('Dominant Colors:');
    metadata.dominantColors.forEach(color => {
      console.log(`  ${color.hex} (${color.percentage.toFixed(1)}%)`);
    });
  }

  if (metadata.exifData) {
    console.log('EXIF Data:', metadata.exifData);
  }

  if (metadata.exposureWarning !== 'normal') {
    console.log(`⚠️ Image is ${metadata.exposureWarning}`);
  }
}
```

#### Batch Process Images with Progress

```typescript
async function processImageDirectory(dirPath: string) {
  const walker = new DirectoryWalker(s5.fs, dirPath);
  const imageExtensions = ['.jpg', '.jpeg', '.png', '.webp', '.gif', '.bmp'];

  let processed = 0;
  let totalSize = 0;
  const formats = new Map<string, number>();

  for await (const entry of walker.walk()) {
    if (entry.type !== 'file') continue;

    const ext = entry.name.substring(entry.name.lastIndexOf('.')).toLowerCase();
    if (!imageExtensions.includes(ext)) continue;

    const blob = await s5.fs.get(entry.path);
    const metadata = await MediaProcessor.extractMetadata(
      new Blob([blob], { type: `image/${ext.substring(1)}` })
    );

    processed++;
    totalSize += metadata.size;
    formats.set(metadata.format, (formats.get(metadata.format) || 0) + 1);

    // Store metadata alongside image
    await s5.fs.put(`${entry.path}.meta.json`, metadata);

    console.log(`Processed ${entry.name}: ${metadata.width}x${metadata.height}`);
  }

  console.log('\nSummary:');
  console.log(`Total images: ${processed}`);
  console.log(`Total size: ${(totalSize / 1024 / 1024).toFixed(2)} MB`);
  console.log('Formats:', Object.fromEntries(formats));
}
```

#### Image Validation and Quality Check

```typescript
async function validateImages(dirPath: string) {
  const issues: Array<{ path: string; issues: string[] }> = [];
  const walker = new DirectoryWalker(s5.fs, dirPath);

  for await (const entry of walker.walk({
    filter: (name) => /\.(jpg|jpeg|png|webp|gif|bmp)$/i.test(name)
  })) {
    if (entry.type !== 'file') continue;

    const blob = await s5.fs.get(entry.path);
    const metadata = await MediaProcessor.extractMetadata(
      new Blob([blob])
    );

    const fileIssues: string[] = [];

    // Check for issues
    if (!metadata.isValidImage) {
      fileIssues.push('Invalid image format');
      if (metadata.validationErrors) {
        fileIssues.push(...metadata.validationErrors);
      }
    }

    if (metadata.width > 4096 || metadata.height > 4096) {
      fileIssues.push(`Very large dimensions: ${metadata.width}x${metadata.height}`);
    }

    if (metadata.estimatedQuality && metadata.estimatedQuality < 60) {
      fileIssues.push(`Low quality: ${metadata.estimatedQuality}/100`);
    }

    if (metadata.exposureWarning && metadata.exposureWarning !== 'normal') {
      fileIssues.push(`Exposure issue: ${metadata.exposureWarning}`);
    }

    if (fileIssues.length > 0) {
      issues.push({ path: entry.path, issues: fileIssues });
    }
  }

  if (issues.length > 0) {
    console.log('Image Quality Issues Found:');
    issues.forEach(({ path, issues }) => {
      console.log(`\n${path}:`);
      issues.forEach(issue => console.log(`  - ${issue}`));
    });
  } else {
    console.log('All images passed validation ✅');
  }
}
```

#### Color Palette Extraction

```typescript
async function extractColorPalette(imagePath: string) {
  const blob = await s5.fs.get(imagePath);
  const metadata = await MediaProcessor.extractMetadata(
    new Blob([blob])
  );

  if (!metadata.dominantColors || metadata.dominantColors.length === 0) {
    console.log('No colors extracted');
    return;
  }

  // Create HTML color palette
  const paletteHtml = `
    <!DOCTYPE html>
    <html>
    <head>
      <title>Color Palette - ${imagePath}</title>
      <style>
        body { font-family: system-ui; padding: 20px; }
        .palette { display: flex; gap: 10px; margin: 20px 0; }
        .color {
          width: 100px;
          height: 100px;
          border-radius: 8px;
          box-shadow: 0 2px 8px rgba(0,0,0,0.1);
          display: flex;
          align-items: flex-end;
          justify-content: center;
          color: white;
          text-shadow: 1px 1px 2px rgba(0,0,0,0.5);
          padding: 8px;
          font-size: 12px;
        }
        .stats { margin-top: 20px; }
      </style>
    </head>
    <body>
      <h1>Color Palette: ${imagePath}</h1>
      <div class="palette">
        ${metadata.dominantColors.map(color => `
          <div class="color" style="background: ${color.hex}">
            ${color.percentage.toFixed(1)}%
          </div>
        `).join('')}
      </div>
      <div class="stats">
        <p>Image: ${metadata.width}x${metadata.height} ${metadata.format}</p>
        <p>Monochrome: ${metadata.isMonochrome ? 'Yes' : 'No'}</p>
        <p>Processing: ${metadata.processingTime}ms via ${metadata.source}</p>
      </div>
      <h2>Color Details</h2>
      <ul>
        ${metadata.dominantColors.map(color => `
          <li>
            <strong>${color.hex}</strong> -
            RGB(${color.rgb.r}, ${color.rgb.g}, ${color.rgb.b}) -
            ${color.percentage.toFixed(2)}%
          </li>
        `).join('')}
      </ul>
    </body>
    </html>
  `;

  await s5.fs.put(`${imagePath}.palette.html`, paletteHtml, {
    mediaType: 'text/html'
  });

  console.log(`Color palette saved to ${imagePath}.palette.html`);
}
```


## FS5 Media Extensions (Phase 6.3)

The FS5 class provides integrated media operations that combine file system functionality with image processing capabilities. These methods use path-based identifiers consistent with FS5's design philosophy.

### putImage()

Upload an image with automatic metadata extraction and thumbnail generation.

```typescript
async putImage(
  path: string,
  blob: Blob,
  options?: PutImageOptions
): Promise<ImageReference>
```

#### Parameters

- **path** (string): File system path where the image will be stored
- **blob** (Blob): Image data to upload
- **options** (PutImageOptions): Optional configuration

#### PutImageOptions

```typescript
interface PutImageOptions {
  generateThumbnail?: boolean;      // Default: true
  thumbnailOptions?: ThumbnailOptions;
  extractMetadata?: boolean;        // Default: true
  progressive?: boolean;            // Default: false
  progressiveOptions?: ProgressiveLoadingOptions;
  // Plus all standard PutOptions (encryption, etc.)
}
```

#### Returns

```typescript
interface ImageReference {
  path: string;              // Path to uploaded image
  thumbnailPath?: string;    // Path to generated thumbnail
  metadata?: ImageMetadata;  // Extracted image metadata
}
```

**Note**: Content identifiers (CIDs) are not exposed. The path-based API abstracts away content addressing - files are identified by paths.

#### Example

```typescript
// Basic usage
const imageFile = await fetch('/photo.jpg').then(r => r.blob());
const result = await s5.fs.putImage('home/photos/vacation.jpg', imageFile);

console.log(`Uploaded to: ${result.path}`);
console.log(`Thumbnail at: ${result.thumbnailPath}`);
console.log(`Dimensions: ${result.metadata.width}x${result.metadata.height}`);

// With custom options
const result = await s5.fs.putImage('home/photos/portrait.jpg', imageFile, {
  generateThumbnail: true,
  thumbnailOptions: {
    maxWidth: 256,
    maxHeight: 256,
    quality: 85,
    format: 'webp'
  },
  extractMetadata: true
});

// Skip thumbnail generation
const result = await s5.fs.putImage('home/photos/raw.jpg', imageFile, {
  generateThumbnail: false
});
```

### getThumbnail()

Retrieve or generate a thumbnail for an image.

```typescript
async getThumbnail(
  path: string,
  options?: GetThumbnailOptions
): Promise<Blob>
```

#### Parameters

- **path** (string): Path to the image file
- **options** (GetThumbnailOptions): Optional configuration

#### GetThumbnailOptions

```typescript
interface GetThumbnailOptions {
  thumbnailOptions?: ThumbnailOptions;  // Used if generating on-demand
  cache?: boolean;                       // Cache generated thumbnail (default: true)
}
```

#### Example

```typescript
// Get pre-generated thumbnail
const thumbnail = await s5.fs.getThumbnail('home/photos/vacation.jpg');
const url = URL.createObjectURL(thumbnail);
document.getElementById('img').src = url;

// Generate on-demand with custom size
const thumbnail = await s5.fs.getThumbnail('home/photos/large.jpg', {
  thumbnailOptions: {
    maxWidth: 128,
    maxHeight: 128
  },
  cache: true  // Save generated thumbnail for future use
});
```

### getImageMetadata()

Extract metadata from a stored image.

```typescript
async getImageMetadata(path: string): Promise<ImageMetadata>
```

#### Example

```typescript
const metadata = await s5.fs.getImageMetadata('home/photos/vacation.jpg');

console.log(`Format: ${metadata.format}`);
console.log(`Size: ${metadata.width}x${metadata.height}`);
console.log(`Aspect: ${metadata.aspectRatio}`);
if (metadata.exif) {
  console.log(`Camera: ${metadata.exif.make} ${metadata.exif.model}`);
}
```

### createImageGallery()

Batch upload multiple images with thumbnails and manifest generation.

```typescript
async createImageGallery(
  galleryPath: string,
  images: ImageUpload[],
  options?: CreateImageGalleryOptions
): Promise<ImageReference[]>
```

#### Parameters

- **galleryPath** (string): Directory path for the gallery
- **images** (ImageUpload[]): Array of images to upload
- **options** (CreateImageGalleryOptions): Optional configuration

#### CreateImageGalleryOptions

```typescript
interface CreateImageGalleryOptions {
  concurrency?: number;              // Parallel uploads (default: 4)
  generateThumbnails?: boolean;      // Generate thumbnails (default: true)
  thumbnailOptions?: ThumbnailOptions;
  onProgress?: (completed: number, total: number) => void;
  createManifest?: boolean;          // Create manifest.json (default: true)
}
```

#### Example

```typescript
// Prepare images
const images = [
  { name: 'photo1.jpg', blob: await fetch('/img1.jpg').then(r => r.blob()) },
  { name: 'photo2.jpg', blob: await fetch('/img2.jpg').then(r => r.blob()) },
  { name: 'photo3.jpg', blob: await fetch('/img3.jpg').then(r => r.blob()) }
];

// Upload gallery with progress tracking
const results = await s5.fs.createImageGallery('home/galleries/vacation', images, {
  concurrency: 2,
  generateThumbnails: true,
  thumbnailOptions: {
    maxWidth: 256,
    maxHeight: 256,
    quality: 85
  },
  onProgress: (completed, total) => {
    console.log(`Uploaded ${completed}/${total} images`);
  },
  createManifest: true
});

// Access the manifest
const manifestData = await s5.fs.get('home/galleries/vacation/manifest.json');
const manifest = JSON.parse(manifestData);
console.log(`Gallery contains ${manifest.count} images`);
```

#### Gallery Manifest Structure

```typescript
interface GalleryManifest {
  created: string;      // ISO 8601 timestamp
  count: number;        // Number of images
  images: Array<{
    name: string;       // Image filename
    path: string;       // Full path to image
    thumbnailPath?: string;    // Path to thumbnail
    metadata?: ImageMetadata;  // Image metadata
  }>;
}
```

### Path-Based Design Philosophy

FS5 media extensions follow the path-based API design:

- **Paths are identifiers**: Files are accessed by filesystem paths, not content hashes
- **Content addressing abstracted**: The underlying S5 content-addressed storage is an implementation detail
- **Simple, familiar interface**: Works like traditional file systems
- **No CID exposure**: Content identifiers (CIDs) are not exposed in the public API

This design makes the API:
- Easier to use for web developers
- Consistent with file system semantics
- Independent of underlying storage implementation

For advanced use cases requiring content addressing, access the internal `FileRef` structures through the S5Node API.

## Performance Considerations

- **Directory Caching**: Directory metadata is cached during path traversal
- **Efficient Pagination**: Use cursors to avoid loading entire large directories
- **Batch Registry Updates**: Multiple operations in succession are optimised
- **Network Latency**: Operations require network round-trips to S5 portals
- **CBOR Efficiency**: Object data is stored efficiently using CBOR encoding
- **HAMT Performance**: Automatic sharding maintains O(log n) performance for large directories
- **Walker Efficiency**: DirectoryWalker uses depth-first traversal with lazy loading
- **Batch Operations**: Progress callbacks allow for UI updates without blocking
- **Resumable Operations**: Cursor support enables efficient resume after interruption
- **WASM Loading**: WebAssembly module is loaded once and cached for reuse
- **Image Processing**: Large images (>50MB) are automatically sampled for performance
- **Memory Management**: WASM module includes automatic memory cleanup
- **Code Splitting**: Media features can be loaded separately from core functionality

## Performance Testing

To run performance benchmarks and verify HAMT efficiency:

### Local Mock Benchmarks (Fast)

```bash
# Basic HAMT verification
node test/integration/test-hamt-local-simple.js

# Comprehensive scaling test (up to 100K entries)
node test/integration/test-hamt-mock-comprehensive.js
```

### Real Portal Benchmarks (Network)

```bash
# Minimal real portal test
node test/integration/test-hamt-real-minimal.js

# HAMT activation threshold test
node test/integration/test-hamt-activation-real.js

# Full portal performance analysis
node test/integration/test-hamt-real-portal.js
```

See [BENCHMARKS.md](./BENCHMARKS.md) for detailed performance results.

## Bundle Size Optimization

The Enhanced S5.js library implements several strategies to minimize bundle size:

### Export Paths

Different export paths allow you to include only what you need:

```javascript
// Full bundle (273KB uncompressed, 70KB gzipped)
import { S5, MediaProcessor } from "@s5-dev/s5js";

// Core only - no media features (195KB uncompressed, 51KB gzipped)
import { S5, FS5 } from "s5/core";

// Media only - for lazy loading (79KB uncompressed, 19KB gzipped)
import { MediaProcessor } from "s5/media";
```

### Tree Shaking

The library is configured with `sideEffects: false` for optimal tree shaking:

```json
{
  "sideEffects": false,
  "exports": {
    ".": "./dist/src/index.js",
    "./core": "./dist/src/exports/core.js",
    "./media": "./dist/src/exports/media.js"
  }
}
```

### Bundle Analysis

Run the bundle analyzer to monitor sizes:

```bash
node scripts/analyze-bundle.js
```

Output shows module breakdown:
- Core functionality: ~195KB (51KB gzipped)
- Media processing: ~79KB (19KB gzipped)
- File system: ~109KB (24KB gzipped)
- Total bundle: ~273KB (70KB gzipped)

## Advanced CID API

### Overview

The Advanced CID API provides direct access to Content Identifiers (CIDs) for power users who need content-addressed storage capabilities. This API is available as a separate export (`s5/advanced`) and does not affect the simplicity of the standard path-based API.

**When to use the Advanced API:**
- You need to reference content by its cryptographic hash
- Building content-addressed storage applications
- Implementing deduplication or content verification
- Working with distributed systems that use CIDs
- Need to track content independently of file paths

**When to use the Path-based API:**
- Simple file storage and retrieval (most use cases)
- Traditional file system operations
- When paths are more meaningful than hashes
- Building user-facing applications

### Installation

```typescript
import { S5 } from 's5';
import { FS5Advanced, formatCID, parseCID, verifyCID } from 's5/advanced';
```

### FS5Advanced Class

The `FS5Advanced` class wraps an `FS5` instance to provide CID-aware operations.

#### Constructor

```typescript
const advanced = new FS5Advanced(s5.fs);
```

**Parameters:**
- `fs5: FS5` - The FS5 instance to wrap

**Throws:**
- `Error` if fs5 is null or undefined

#### pathToCID(path)

Extract the CID (Content Identifier) from a file or directory path.

```typescript
async pathToCID(path: string): Promise<Uint8Array>
```

**Parameters:**
- `path: string` - The file or directory path

**Returns:**
- `Promise<Uint8Array>` - The CID as a 32-byte Uint8Array

**Throws:**
- `Error` if path does not exist

**Example:**

```typescript
const s5 = await S5.create();
await s5.recoverIdentityFromSeedPhrase(seedPhrase);

const advanced = new FS5Advanced(s5.fs);

// Store a file
await s5.fs.put('home/data.txt', 'Hello, World!');

// Extract its CID
const cid = await advanced.pathToCID('home/data.txt');
console.log(cid); // Uint8Array(32) [...]

// Format for display
const formatted = formatCID(cid, 'base32');
console.log(formatted); // "bafybeig..."
```

#### cidToPath(cid)

Find the path for a given CID. If multiple paths have the same CID, returns the first user path found (excludes temporary `.cid/` paths).

```typescript
async cidToPath(cid: Uint8Array): Promise<string | null>
```

**Parameters:**
- `cid: Uint8Array` - The CID to search for (must be 32 bytes)

**Returns:**
- `Promise<string | null>` - The path if found, null if not found

**Throws:**
- `Error` if CID size is invalid

**Example:**

```typescript
const cid = await advanced.pathToCID('home/data.txt');

// Find path from CID
const path = await advanced.cidToPath(cid);
console.log(path); // "home/data.txt"

// Unknown CID returns null
const unknownCID = new Uint8Array(32);
const result = await advanced.cidToPath(unknownCID);
console.log(result); // null
```

#### getByCID(cid)

Retrieve data directly by its CID, without knowing the path.

```typescript
async getByCID(cid: Uint8Array): Promise<any>
```

**Parameters:**
- `cid: Uint8Array` - The CID to retrieve (must be 32 bytes)

**Returns:**
- `Promise<any>` - The data associated with the CID

**Throws:**
- `Error` if CID is not found or invalid size

**Example:**

```typescript
// Store data
await s5.fs.put('home/document.txt', 'Important data');

// Get CID
const cid = await advanced.pathToCID('home/document.txt');

// Later, retrieve by CID alone
const data = await advanced.getByCID(cid);
console.log(data); // "Important data"

// Works with any data type
await s5.fs.put('home/config.json', { setting: 'value' });
const configCID = await advanced.pathToCID('home/config.json');
const config = await advanced.getByCID(configCID);
console.log(config); // { setting: 'value' }
```

#### putByCID(data)

Store data in content-addressed storage and return its CID. The data is stored but not assigned a user-visible path.

```typescript
async putByCID(data: any): Promise<Uint8Array>
```

**Parameters:**
- `data: any` - The data to store

**Returns:**
- `Promise<Uint8Array>` - The CID of the stored data

**Example:**

```typescript
// Store data and get its CID
const cid = await advanced.putByCID('Temporary content');
console.log(formatCID(cid)); // "bafybeih..."

// Retrieve it later by CID
const data = await advanced.getByCID(cid);
console.log(data); // "Temporary content"

// Works with binary data
const binaryData = new Uint8Array([1, 2, 3, 4, 5]);
const binaryCID = await advanced.putByCID(binaryData);
```

### Composition Patterns

The FS5Advanced API is intentionally minimal with just 4 core methods. For common workflows, compose these with regular FS5 methods:

#### Store with Path and Get CID

```typescript
// Instead of putWithCID(path, data) - use composition:
await s5.fs.put('home/file.txt', 'Content');
const cid = await advanced.pathToCID('home/file.txt');

console.log(`Stored at: home/file.txt`);
console.log(`CID: ${formatCID(cid)}`); // "bafybeif..."

// With encryption
await s5.fs.put('home/secret.txt', 'Secret data', {
  encryption: { algorithm: 'xchacha20-poly1305' }
});
const secretCid = await advanced.pathToCID('home/secret.txt');

// Can retrieve by either path or CID
const byPath = await s5.fs.get('home/secret.txt');
const byCID = await advanced.getByCID(secretCid);
console.log(byPath === byCID); // true
```

#### Get Metadata with CID

```typescript
// Instead of getMetadataWithCID(path) - use composition:
await s5.fs.put('home/data.txt', 'Content');

const metadata = await s5.fs.getMetadata('home/data.txt');
const cid = await advanced.pathToCID('home/data.txt');

console.log(metadata);
// {
//   type: 'file',
//   size: 7,
//   created: 1234567890,
//   modified: 1234567890
// }

console.log(formatCID(cid)); // "bafybeih..."
```

**Why Composition?**
- Keeps API minimal and easy to learn (4 methods vs 6)
- Makes intent explicit (store *then* extract CID)
- Reduces maintenance burden
- Still provides all functionality

### CID Utility Functions

#### formatCID(cid, encoding?)

Format a CID as a multibase-encoded string for display or transmission.

```typescript
function formatCID(
  cid: Uint8Array,
  encoding?: 'base32' | 'base58btc' | 'base64'
): string
```

**Parameters:**
- `cid: Uint8Array` - The CID to format (must be 32 bytes)
- `encoding?: string` - The encoding to use (default: 'base32')
  - `'base32'` - Base32 encoding (prefix: 'b')
  - `'base58btc'` - Base58 Bitcoin encoding (prefix: 'z')
  - `'base64'` - Base64 encoding (prefix: 'm')

**Returns:**
- `string` - The formatted CID string with multibase prefix

**Throws:**
- `Error` if CID is invalid size or encoding is unsupported

**Example:**

```typescript
const cid = await advanced.pathToCID('home/file.txt');

// Default base32
const base32 = formatCID(cid);
console.log(base32); // "bafybeig..."

// Base58btc (shorter, more compact)
const base58 = formatCID(cid, 'base58btc');
console.log(base58); // "zb2rh..."

// Base64 (URL-safe)
const base64 = formatCID(cid, 'base64');
console.log(base64); // "mAXASI..."
```

#### parseCID(cidString)

Parse a CID string back into a Uint8Array. Automatically detects the encoding format.

```typescript
function parseCID(cidString: string): Uint8Array
```

**Parameters:**
- `cidString: string` - The CID string to parse (with or without multibase prefix)

**Returns:**
- `Uint8Array` - The parsed CID (32 bytes)

**Throws:**
- `Error` if CID string is invalid or has wrong size after parsing

**Supported formats:**
- Base32 with prefix: `"bafybei..."`
- Base32 without prefix: `"afybei..."`
- Base58btc with prefix: `"zb2rh..."`
- Base58btc without prefix: `"Qm..."`
- Base64 with prefix: `"mAXASI..."`
- Base64 without prefix: `"AXASI..."`

**Example:**

```typescript
// Parse base32
const cid1 = parseCID('bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi');

// Parse base58btc
const cid2 = parseCID('zb2rhk6GMPQF8p1NMJEqvJ3XFfNBqJNfiXzJaJkPiA9kMvNaJ');

// Parse without prefix (auto-detect)
const cid3 = parseCID('afybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi');

// All return Uint8Array(32)
console.log(cid1); // Uint8Array(32) [...]
```

#### verifyCID(cid, data, crypto)

Verify that a CID matches the given data by recomputing the hash.

```typescript
async function verifyCID(
  cid: Uint8Array,
  data: Uint8Array,
  crypto: CryptoImplementation
): Promise<boolean>
```

**Parameters:**
- `cid: Uint8Array` - The CID to verify (must be 32 bytes)
- `data: Uint8Array` - The data to check
- `crypto: CryptoImplementation` - The crypto implementation to use

**Returns:**
- `Promise<boolean>` - True if CID matches data, false otherwise

**Throws:**
- `Error` if CID size is invalid

**Example:**

```typescript
import { JSCryptoImplementation } from 's5/core';

const crypto = new JSCryptoImplementation();
const data = new TextEncoder().encode('Hello, World!');

// Store data and get CID
await s5.fs.put('home/data.txt', 'Hello, World!');
const cid = await advanced.pathToCID('home/data.txt');

// Verify CID matches
const isValid = await verifyCID(cid, data, crypto);
console.log(isValid); // true

// Tampered data fails verification
const tamperedData = new TextEncoder().encode('Goodbye, World!');
const isInvalid = await verifyCID(cid, tamperedData, crypto);
console.log(isInvalid); // false
```

#### cidToString(cid)

Convert a CID to a hexadecimal string for debugging or display.

```typescript
function cidToString(cid: Uint8Array): string
```

**Parameters:**
- `cid: Uint8Array` - The CID to convert (must be 32 bytes)

**Returns:**
- `string` - Hexadecimal representation of the CID

**Throws:**
- `Error` if CID is invalid size

**Example:**

```typescript
const cid = await advanced.pathToCID('home/file.txt');

const hexString = cidToString(cid);
console.log(hexString);
// "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b"

// Useful for logging and debugging
console.log(`File CID: ${hexString}`);
```

### Complete Example

Here's a comprehensive example showing the Advanced CID API workflow:

```typescript
import { S5 } from 's5';
import { FS5Advanced, formatCID, parseCID, verifyCID } from 's5/advanced';
import { JSCryptoImplementation } from 's5/core';

// Initialize S5
const s5 = await S5.create();
const seedPhrase = s5.generateSeedPhrase();
await s5.recoverIdentityFromSeedPhrase(seedPhrase);

// Create Advanced API
const advanced = new FS5Advanced(s5.fs);
const crypto = new JSCryptoImplementation();

// 1. Store data and get CID (composition pattern)
await s5.fs.put('home/document.txt', 'Important data');
const cid = await advanced.pathToCID('home/document.txt');
console.log(`Stored at: home/document.txt`);
console.log(`CID: ${formatCID(cid, 'base32')}`);

// 2. Verify the CID
const data = new TextEncoder().encode('Important data');
const isValid = await verifyCID(cid, data, crypto);
console.log(`CID valid: ${isValid}`); // true

// 3. Share the CID (as string)
const cidString = formatCID(cid, 'base58btc');
console.log(`Share this CID: ${cidString}`);

// 4. Recipient: parse CID and retrieve data
const receivedCID = parseCID(cidString);
const retrievedData = await advanced.getByCID(receivedCID);
console.log(`Retrieved: ${retrievedData}`); // "Important data"

// 5. Find path from CID
const foundPath = await advanced.cidToPath(receivedCID);
console.log(`Path: ${foundPath}`); // "home/document.txt"

// 6. Get metadata and CID (composition pattern)
const metadata = await s5.fs.getMetadata(foundPath);
const metaCid = await advanced.pathToCID(foundPath);
console.log(metadata);
// { type: 'file', size: 14, ... }
console.log(`CID: ${formatCID(metaCid)}`)

// 7. CID-only storage (no path)
const tempCID = await advanced.putByCID('Temporary content');
console.log(`Temp CID: ${cidToString(tempCID)}`);

// Retrieve later without knowing path
const tempData = await advanced.getByCID(tempCID);
console.log(tempData); // "Temporary content"
```

### Bundle Size

The Advanced API export is optimized for tree-shaking:

- **Advanced bundle**: 59.53 KB compressed (brotli)
- **Includes**: Core functionality + CID utilities
- **Tree-shakeable**: Only imported functions are included

```json
{
  "exports": {
    "./advanced": "./dist/src/exports/advanced.js"
  }
}
```

### Type Definitions

The Advanced API exports additional types for power users:

```typescript
import type {
  DirV1,
  FileRef,
  DirRef,
  DirLink,
  BlobLocation,
  HAMTShardingConfig,
  PutOptions,
  ListOptions,
  GetOptions,
  ListResult,
  PutWithCIDResult,
  MetadataWithCIDResult
} from 's5/advanced';
```

## Next Steps

- Review the [test suite](https://github.com/julesl23/s5.js/tree/main/test/fs) for comprehensive usage examples
- Check [TypeScript definitions](https://github.com/julesl23/s5.js/blob/main/src/fs/dirv1/types.ts) for complete type information
- Explore [S5 network documentation](https://docs.sfive.net/) for deeper understanding
- See the [grant proposal](https://github.com/julesl23/s5.js/blob/main/docs/MILESTONES.md) for upcoming features

---

_This documentation covers Phases 2-6 of the Enhanced S5.js grant project. Phase 3 added automatic HAMT sharding for efficient handling of large directories. Phase 4 added the DirectoryWalker and BatchOperations utilities for recursive directory operations. Phase 5 added the media processing foundation with WASM-based image metadata extraction, Canvas fallback, browser compatibility detection, and bundle size optimization. Phase 6 added advanced media processing with thumbnail generation, progressive loading, FS5 integration, and the Advanced CID API for power users._