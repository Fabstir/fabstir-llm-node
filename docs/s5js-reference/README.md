# Enhanced S5.js SDK

An enhanced JavaScript/TypeScript SDK for the S5 decentralized storage network, featuring a simple path-based API for file and directory operations.

## Features

- 🚀 **Path-based API**: Simple `get()`, `put()`, `delete()`, `list()` operations
- 📁 **Directory Support**: Full directory tree management with recursive operations
- 🔄 **Cursor Pagination**: Efficient handling of large directories
- 🔐 **Built-in Encryption**: Automatic encryption for private data
- 📦 **CBOR Serialization**: Deterministic encoding for cross-platform compatibility
- 🌐 **Browser & Node.js**: Works in both environments
- 🗂️ **HAMT Sharding**: Automatic directory sharding for millions of entries
- 🚶 **Directory Walker**: Recursive traversal with filters and resumable cursors
- 📋 **Batch Operations**: High-level copy/delete operations with progress tracking
- 🖼️ **Media Processing**: WASM-based image metadata extraction with Canvas fallback
- 🎨 **Color Analysis**: Dominant color extraction and palette generation
- 📊 **Bundle Optimization**: Code-splitting support (~70KB gzipped total)
- ✅ **Real S5 Portal Integration**: Fully tested with s5.vup.cx portal

## Key Components

### Core API
- **S5**: Main client class for connection and identity management
- **FS5**: File system operations with path-based API
- **S5UserIdentity**: User identity and authentication

### Utility Classes
- **DirectoryWalker**: Recursive directory traversal with cursor support
- **BatchOperations**: High-level copy/delete operations with progress tracking

### Media Processing
- **MediaProcessor**: Unified image metadata extraction with WASM/Canvas
- **BrowserCompat**: Browser capability detection and strategy selection
- **CanvasMetadataExtractor**: Fallback image processing using Canvas API

See the [API Documentation](./docs/API.md) for detailed usage examples.

## Installation

Install the enhanced S5.js SDK with npm:

```bash
npm install @s5-dev/s5js
```

**Prerequisites:**

- **Node.js** v20+ (for Node.js environments)
- Modern browser with ES2022 support (for browser environments)

**For development:**

```bash
# Clone the repository
git clone https://github.com/s5-dev/s5.js
cd s5.js

# Install dependencies
npm install

# Build the project
npm run build

# Run tests
npm test
```

## Quick Start

```typescript
import { S5 } from "@s5-dev/s5js";

// Create S5 instance and connect to real S5 portal
const s5 = await S5.create({
  initialPeers: [
    "wss://z2DWuPbL5pweybXnEB618pMnV58ECj2VPDNfVGm3tFqBvjF@s5.ninja/s5/p2p",
  ],
});

// Generate a new seed phrase (save this securely!)
const seedPhrase = s5.generateSeedPhrase();
console.log("Your seed phrase:", seedPhrase);

// Or recover from existing seed phrase
// const seedPhrase = "your saved twelve word seed phrase here";

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

### Advanced Usage

```typescript
import { DirectoryWalker, BatchOperations, MediaProcessor } from "@s5-dev/s5js";

// Recursive directory traversal
const walker = new DirectoryWalker(s5.fs, '/');
for await (const entry of walker.walk("home", { maxDepth: 3 })) {
  console.log(`${entry.path} (${entry.type})`);
}

// Batch operations with progress
const batch = new BatchOperations(s5.fs);
const result = await batch.copyDirectory("home/source", "home/backup", {
  onProgress: (progress) => {
    console.log(`Copied ${progress.processed} items...`);
  }
});
console.log(`Completed: ${result.success} success, ${result.failed} failed`);

// Media processing - extract image metadata
await MediaProcessor.initialize();
const imageBlob = await fetch('/path/to/image.jpg').then(r => r.blob());
const metadata = await MediaProcessor.extractMetadata(imageBlob);
console.log(`Image: ${metadata.width}x${metadata.height} ${metadata.format}`);
console.log(`Dominant colors:`, metadata.dominantColors);
```

## Testing with Real S5 Portal

The enhanced S5.js has been successfully integrated with real S5 portal infrastructure. To test:

### 1. Fresh Identity Test (Recommended)

This test creates a new identity and verifies all functionality:

```bash
node test/integration/test-fresh-s5.js
```

Expected output: 100% success rate (9/9 tests passing)

### 2. Full Integration Test

Comprehensive test of all features:

```bash
node test/integration/test-s5-full-integration.js
```

### 3. Direct Portal API Test

Tests direct portal communication:

```bash
node test/integration/test-portal-direct.js
```

### 4. Batch Operations Test

Tests BatchOperations (copy/delete) with real S5 portal:

```bash
node test/integration/test-batch-real.js
```

This test validates:
- Copy directory with progress tracking
- Delete directory with progress tracking
- Error handling modes
- Metadata preservation

### 5. Media Extensions Test (Phase 6.3)

Tests FS5 media integration (putImage, getThumbnail, getImageMetadata, createImageGallery) with real S5 instance:

```bash
node test/integration/test-media-real.js
```

This test validates:
- Image upload with automatic thumbnail generation
- Metadata extraction (format, dimensions)
- Thumbnail retrieval (pre-generated and on-demand)
- Gallery creation with manifest.json
- Directory integration with media operations
- Path-based API (no CID exposure)

Expected output: 10/10 tests passing

### Important Notes

- **Use Fresh Identities**: The new deterministic key derivation system requires fresh identities. Old accounts created with the previous system won't work.
- **Portal URL**: Use `https://s5.vup.cx` which has the updated API. Other portals may not have the required updates.
- **Path Requirements**: All paths must start with either `home/` or `archive/`

## Performance Benchmarks

The enhanced S5.js includes comprehensive performance benchmarks to verify HAMT efficiency and scaling behaviour.

### Running Benchmarks

#### Local Mock Benchmarks (Fast)

Test HAMT performance with mock S5 API:

```bash
# Basic HAMT verification
node test/mocked/integration/test-hamt-local-simple.js

# Comprehensive scaling test (up to 100K entries)
node test/mocked/integration/test-hamt-mock-comprehensive.js
```

#### Real Portal Benchmarks (Network)

Test with actual S5 portal (requires internet connection):

```bash
# Minimal real portal test
node test/integration/test-hamt-real-minimal.js

# HAMT activation threshold test
node test/integration/test-hamt-activation-real.js

# Full portal performance analysis
node test/integration/test-hamt-real-portal.js
```

### Benchmark Results

See [BENCHMARKS.md](./docs/BENCHMARKS.md) for detailed performance analysis showing:
- HAMT activation at exactly 1000 entries
- O(log n) scaling verified up to 100K+ entries
- ~800ms per operation on real S5 network
- Memory usage of ~650 bytes per entry

For production deployments, these benchmarks confirm the implementation is ready for large-scale directory operations.

## Bundle Size & Code Splitting

The library supports multiple import strategies to optimize bundle size:

```javascript
// Full bundle (~60KB compressed with brotli)
import { S5, MediaProcessor } from "s5";

// Core only - no media features (~60KB compressed)
import { S5, FS5 } from "s5/core";

// Media only - for lazy loading (~10KB compressed)
import { MediaProcessor } from "s5/media";

// Advanced CID API - for power users (~60KB compressed)
import { FS5Advanced, formatCID, parseCID } from "s5/advanced";

// Dynamic import for code-splitting
const { MediaProcessor } = await import("s5/media");
```

Monitor bundle sizes with:
```bash
npm run analyze-bundle
```

## Advanced CID API

For power users who need direct access to Content Identifiers (CIDs), the Advanced API provides content-addressed storage capabilities without affecting the simplicity of the path-based API.

### When to Use

**Use the Advanced API if you:**
- Need to reference content by its cryptographic hash
- Are building content-addressed storage applications
- Require deduplication or content verification
- Work with distributed systems that use CIDs

**Use the Path-based API if you:**
- Need simple file storage (most use cases)
- Prefer traditional file system operations
- Want paths to be more meaningful than hashes

### Quick Example

```typescript
import { S5 } from "s5";
import { FS5Advanced, formatCID, parseCID } from "s5/advanced";

// Setup
const s5 = await S5.create();
await s5.recoverIdentityFromSeedPhrase(seedPhrase);
const advanced = new FS5Advanced(s5.fs);

// Store data and get CID
await s5.fs.put('home/document.txt', 'Important data');
const cid = await advanced.pathToCID('home/document.txt');
console.log(`CID: ${formatCID(cid, 'base32')}`);

// Share the CID string
const cidString = formatCID(cid, 'base58btc');

// Recipient: retrieve by CID alone
const receivedCID = parseCID(cidString);
const data = await advanced.getByCID(receivedCID);
console.log(data); // "Important data"

// Find path from CID
const path = await advanced.cidToPath(receivedCID);
console.log(path); // "home/document.txt"
```

### Available Methods

**FS5Advanced Class (4 essential methods):**
- `pathToCID(path)` - Extract CID from file/directory path
- `cidToPath(cid)` - Find path for a given CID
- `getByCID(cid)` - Retrieve data by CID directly
- `putByCID(data)` - Store content-only and return CID

**Composition Pattern:**
- For path + CID: Use `fs.put(path, data)` then `advanced.pathToCID(path)`
- For metadata + CID: Use `fs.getMetadata(path)` then `advanced.pathToCID(path)`

**CID Utilities:**
- `formatCID(cid, encoding?)` - Format CID as multibase string
- `parseCID(cidString)` - Parse CID from string
- `verifyCID(cid, data, crypto)` - Verify CID matches data
- `cidToString(cid)` - Convert to hex string

See the [Advanced API Documentation](./docs/API.md#advanced-cid-api) for complete details.

## Encryption

Enhanced S5.js includes **built-in encryption** using XChaCha20-Poly1305, providing both confidentiality and integrity for sensitive data.

### Basic Encryption

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

```typescript
// Use your own 32-byte encryption key
const myKey = new Uint8Array(32); // Your secure key
crypto.getRandomValues(myKey);

await s5.fs.put("home/private/document.txt", "Secret content", {
  encryption: {
    algorithm: "xchacha20-poly1305",
    key: myKey, // Use specific key
  },
});

// Decryption uses key from metadata automatically
const content = await s5.fs.get("home/private/document.txt");
```

### Features

- **Algorithm**: XChaCha20-Poly1305 (AEAD cipher)
- **Key Size**: 256-bit (32 bytes)
- **Chunk-based**: Large files encrypted in 256 KiB chunks
- **Transparent**: Automatic encryption/decryption
- **Secure**: Each chunk uses unique nonce

### Security Considerations

⚠️ **Important**: Encryption keys are stored in directory metadata. Anyone with directory read access can decrypt files. This design provides:

- ✅ Convenience: No separate key management needed
- ✅ Automatic decryption with directory access
- ⚠️ Access control: Secure your directory access credentials

For complete encryption documentation, examples, and security best practices, see the [Encryption section in API.md](./docs/API.md#encryption).

## Documentation

- [API Documentation](./docs/API.md) - Complete API reference with examples
- [Implementation Status](./docs/IMPLEMENTATION.md) - Development progress tracking
- [Milestones](./docs/MILESTONES.md) - Grant milestone tracking
- [Benchmarks](./docs/BENCHMARKS.md) - Performance analysis and results

## Development

This is an enhanced version of s5.js being developed under an 8-month grant from the Sia Foundation. The project implements a new format using:

- **New Format**: CBOR serialization with DirV1 specification (replaces MessagePack)
- **Path-based API**: Simple file operations with familiar syntax
- **HAMT sharding**: Automatic directory sharding for efficient large directory support
- **Directory utilities**: Recursive operations with progress tracking and error handling
- **Deterministic Key Derivation**: Subdirectory keys derived from parent keys
- **Real Portal Integration**: Successfully tested with s5.vup.cx

**Note**: This is a clean implementation that does NOT maintain backward compatibility with old S5 data formats.

### Building

```bash
npm run build     # Compile TypeScript
npm run dev       # Watch mode
npm run test      # Run tests
```

### Development Commands

```bash
npm run build       # Compile TypeScript to JavaScript
npm run dev         # Watch mode for development
npm run type-check  # Run TypeScript type checking
```

### Testing

```bash
npm run test        # Run real implementation tests only
npm run test:run    # Run tests once
npm run test:mocked # Run mock-based tests
npm run test:all    # Run all tests (real + mocked)
npm run test:ui     # Run tests with UI
npm run test:coverage # Generate coverage report

# Run specific test suites
npm run test:run test/fs/cid-utils.test.ts test/fs/fs5-advanced.test.ts  # Advanced CID API unit tests (74 tests)
```

### Test Organization

- **`test/`** - Real implementation tests using actual S5.js functionality
  - Run with `npm test` (30+ test files, 284+ tests)
  - Tests core functionality without mocks

- **`test/mocked/`** - Mock-based unit and performance tests
  - Run with `npm run test:mocked` (15 test files)
  - Includes HAMT performance benchmarks and isolated component tests
  - `test/mocked/integration/` - Mock-based integration and performance tests

- **`test/integration/`** - Real S5 integration tests with actual network connections
  - Tests that connect to real S5 portals (e.g., s5.vup.cx)
  - Use real seed phrases and portal registration

### Running Real S5 Portal Integration Tests

For comprehensive testing with real S5 infrastructure, use the standalone integration test scripts:

```bash
# Build the project first
npm run build

# Run Advanced CID API integration tests with real S5 portal
node test/integration/test-advanced-cid-real.js
```

**Note:** These tests:
- Connect to real S5 portals (default: https://s5.vup.cx)
- Use actual registry operations with 5+ second propagation delays
- Run sequentially to avoid registry conflicts
- Generate temporary test files (auto-cleaned)
- Take ~2 minutes to complete (18 tests)

## Media Processing Tests & Demos

### Phase 5 Media Processing Foundation

The media processing implementation includes comprehensive demos and tests. All Phase 5 deliverables are complete with 100% test coverage.

#### Quick Start - Run All Demos

```bash
# Build the project first
npm run build

# Run all Node.js demos
node demos/media/benchmark-media.js         # Performance benchmarking
node demos/media/demo-pipeline.js           # Pipeline initialization
node demos/media/demo-metadata.js           # Metadata extraction
node demos/media/test-media-integration.js  # Integration tests (Node.js)

# Run browser tests (all 20 tests pass in browser)
./demos/media/run-browser-tests.sh          # Linux/Mac
# Windows: npx http-server -p 8080, then open http://localhost:8080/demos/media/browser-tests.html

# View code-splitting demo (requires HTTP server)
# Linux/Mac: ./demos/media/run-browser-tests.sh (uses port 8081)
# Windows: npx http-server -p 8081, then open http://localhost:8081/demos/media/demo-splitting-simple.html
```

#### ⚙️ Platform-Specific Notes

**Node.js Test Expectations:**

When running `node demos/media/test-media-integration.js`:
- ✅ **Expected: 17/20 tests pass (85%)**
- ❌ 3 tests fail due to Node.js platform limitations (NOT bugs):
  1. "WASM Module Loading" - Canvas is 42x faster in Node.js, WASM not loaded (correct)
  2. "Process Real JPEG Image - Width" - Node.js lacks full Canvas API for dimensions (works in browser)
  3. "Dominant Color Extraction" - Node.js can't access pixel data (works in browser)

**Browser Test Expectations:**
- ✅ **All 20/20 tests pass (100%)**

**Windows Users:**

The bash script `./demos/media/run-browser-tests.sh` won't work in Windows CMD. Use one of these alternatives:

```cmd
# Option 1: Using npx (recommended - no Python needed)
npx http-server -p 8080

# Option 2: Using Python (if installed)
python -m http.server 8080

# Then open in browser:
http://localhost:8080/demos/media/browser-tests.html
```

**Linux/Mac Users:**

```bash
# Use the provided script
./demos/media/run-browser-tests.sh

# Automatically opens: http://localhost:8081/demos/media/browser-tests.html
```

#### 🧪 Browser Tests - All 20 Tests Passing

**Expected Results:**
- ✅ 20/20 tests pass in browser (100%)
- ✅ Full WASM functionality
- ✅ Real dimensions, color extraction, all features working

**Tests Include**:
1. MediaProcessor initialization
2. Browser capability detection
3. Strategy selection (wasm-worker, canvas-main, etc.)
4. PNG/JPEG/GIF/BMP/WebP metadata extraction
5. Dominant color extraction
6. Transparency detection
7. Aspect ratio calculation
8. Processing time tracking
9. Speed classification (fast/normal/slow)
10. WASM to Canvas fallback
11. Invalid image handling
12. Timeout support
13. Orientation detection
14. Concurrent extractions
15. WASM module validation
16. Multiple format support

**Evidence Column**: Each test shows verification data proving it passes

#### 📊 Performance Benchmarking

**Run**: `node demos/media/benchmark-media.js`

**Output**:
- Processes test images with WASM and Canvas strategies
- Generates performance comparison table
- Saves baseline metrics to `baseline-performance.json`
- Shows processing times, memory usage, success rates

**Expected Results**:
- Canvas faster in Node.js (175x faster due to no Web Workers)
- WASM initialization: ~83ms first image, <1ms subsequent
- Canvas: consistent 0.03-0.31ms
- Strategy adapts to environment (canvas-main for Node.js)

#### 🔧 Pipeline Setup Demo

**Run**: `node demos/media/demo-pipeline.js`

**Demonstrates**:
- Environment capability detection
- Smart strategy selection based on capabilities
- WASM module initialization with progress tracking
- Memory management and cleanup
- Fallback handling scenarios

**Key Features**:
- Shows decision tree for strategy selection
- ASCII pipeline flow diagram
- Real-time progress tracking
- Memory delta measurements

#### 🎨 Metadata Extraction

**Run**: `node demos/media/demo-metadata.js`

**Processes**:
- All image formats (PNG, JPEG, GIF, BMP, WebP)
- Magic byte format detection
- Processing speed classification
- Generates HTML report at `metadata-report.html`

**Note**: In Node.js, dimensions show 0x0 (expected limitation). Works fully in browser.

#### 📦 Code-Splitting Demo

**Prerequisites**: Requires HTTP server

**Windows:**
```cmd
npx http-server -p 8081
# Then open: http://localhost:8081/demos/media/demo-splitting-simple.html
```

**Linux/Mac:**
```bash
./demos/media/run-browser-tests.sh
# Then open: http://localhost:8081/demos/media/demo-splitting-simple.html
```

**Shows**:
- Core bundle: 195 KB (-27% from full)
- Media bundle: 79 KB (loaded on-demand)
- Real image processing with loaded modules
- Bundle size comparison table
- Live implementation examples

#### Expected Test Results

**Browser Environment (Full Support)**:
- ✅ 20/20 tests passing
- ✅ Real image dimensions extracted
- ✅ Dominant colors working
- ✅ WASM module loads
- ✅ Web Workers available
- ✅ Strategy: wasm-worker

**Node.js Environment (Limited Canvas)**:
- ✅ 16-19/20 tests passing (expected)
- ⚠️ Dimensions show 0x0 for some formats (no full Canvas API)
- ⚠️ No color extraction (needs pixel access)
- ✅ Format detection works
- ✅ Falls back to canvas-main strategy
- ✅ All operations < 50ms (fast)

### Why These Results Are Expected

1. **Node.js Limitations**: No Web Workers, limited Canvas API, so it uses fallbacks
2. **Browser Full Support**: All features work with real Canvas and WASM
3. **Adaptive Strategy**: System detects capabilities and chooses optimal path
4. **Performance**: Canvas faster in Node.js, WASM better for larger images in browser

### Media Processing API Usage

```javascript
import { MediaProcessor } from 's5/media';

// Initialize (automatic in browser)
await MediaProcessor.initialize();

// Extract metadata
const blob = new Blob([imageData], { type: 'image/png' });
const metadata = await MediaProcessor.extractMetadata(blob);

console.log(`Image: ${metadata.width}x${metadata.height}`);
console.log(`Format: ${metadata.format}`);
console.log(`Processing: ${metadata.processingTime}ms`);
```

### Test Server

For integration testing with mock S5 services:

```bash
node test/mocked/integration/test-server.js  # Start mock server on port 3000
```

See [test-server-README.md](./test-server-README.md) for details.

## Project Architecture

### Technology Stack

- **Language**: TypeScript (ES2022 target, ESNext modules)
- **Runtime**: Dual-targeted for Browser and Node.js
- **Test Framework**: Vitest with global test functions
- **Crypto**: @noble libraries for cryptographic operations
- **Storage**: IndexedDB (browser) and memory-level (Node.js)
- **Serialization**: CBOR via cbor-x
- **Networking**: WebSocket-based P2P connections

### Module Structure

- `src/api/` - Core S5 API interfaces and crypto implementations
- `src/fs/` - File system operations (FS5 implementation)
  - `dirv1/` - CBOR-based directory format implementation
  - `hamt/` - Hash Array Mapped Trie for large directories
  - `utils/` - Directory walker and batch operations
- `src/media/` - Media processing and metadata extraction
  - `wasm/` - WebAssembly module wrapper for image processing
  - `fallback/` - Canvas-based fallback implementation
  - `compat/` - Browser compatibility detection
- `src/identity/` - User identity and authentication
- `src/node/` - P2P networking and registry operations
- `src/kv/` - Key-value storage abstractions
- `src/encryption/` - Encryption utilities
- `src/identifier/` - Content identifiers and multibase encoding
- `src/util/` - Utility functions
- `src/exports/` - Modular export paths for code-splitting

## Project Status

- ✅ Month 1: Project Setup - Complete
- ✅ Month 2: Path Helpers v0.1 - Complete
- ✅ Month 3: Path-cascade Optimization & HAMT - Complete
- ✅ Month 4: Directory Utilities - Complete
- ✅ Month 5: Media Processing Foundation - Complete
- ✅ Month 6: Advanced Media Processing - Complete
- ✅ **S5 Portal Integration** - Complete (100% test success rate)
- ✅ **Phase 6.5**: Advanced CID API - Complete (74 tests passing)
- ✅ Month 7: Testing & Performance - Substantially Complete (~85%)
- 🚧 Month 8: Documentation & Upstream Integration - In Progress (~40%)

See [MILESTONES.md](./docs/MILESTONES.md) for detailed progress.

## Grant Milestone 5 Deliverables

**Milestone 5** (Advanced Media Processing) has been completed and validated. All grant requirements have been met and exceeded:

### Requirements Met ✅

1. **Thumbnail Generation** ✅
   - JPEG, PNG, and WebP format support
   - Smart cropping with face/object detection
   - Size constraints: All thumbnails ≤64 KB (average: 29.5 KB)
   - 21 dedicated tests passing

2. **Progressive Rendering** ✅
   - Three strategies implemented: Blur, Scan Lines, Interlaced
   - Browser compatibility with graceful fallbacks
   - Visual demo validated in Chrome, Edge, and Firefox
   - 27 dedicated tests passing

3. **Browser Compatibility Matrix** ✅
   - Tested: Chrome 90+, Firefox 88+, Edge 90+, Node.js 20+
   - 10 capability detection features (Canvas, WebP, WASM, etc.)
   - Graceful fallback system implemented
   - 31 browser compatibility tests passing

4. **Bundle Size Optimization** ✅
   - **Requirement**: ≤700 KB (compressed)
   - **Achieved**: 60.09 KB (brotli) - **10x under budget**
   - Modular exports for code-splitting: `s5`, `s5/core`, `s5/media`, `s5/advanced`

### Documentation & Validation

For complete evidence and testing instructions, see:

- **[MILESTONE5_EVIDENCE.md](./docs/MILESTONE5_EVIDENCE.md)** - Comprehensive evidence document with:
  - Detailed proof of all requirements met
  - Test results (437 tests passing, 225+ media-specific)
  - Browser compatibility matrix
  - Performance metrics and bundle analysis
  - Integration test results on real S5 network

- **[MILESTONE5_TESTING_GUIDE.md](./docs/MILESTONE5_TESTING_GUIDE.md)** - Step-by-step validation guide with:
  - How to run unit tests (`npm run test:run`)
  - How to run integration test (`node test/integration/test-media-real.js`)
  - How to launch browser demo (`./test/browser/run-demo.sh`)
  - Bundle size verification steps
  - Troubleshooting guide

### Quick Validation

```bash
# 1. Run unit tests (437 tests)
npm run test:run

# 2. Run integration test with real S5 network
npm run build
node test/integration/test-media-real.js

# 3. Launch progressive rendering browser demo
./test/browser/run-demo.sh

# 4. Verify bundle size
npm run build
brotli -f -k dist/src/index.js
du -h dist/src/index.js.br  # Should show ~60 KB
```

**Status**: All Milestone 5 deliverables complete and ready for review.

### Completed Phases ✅

- **Phase 1**: Core Infrastructure (CBOR, DirV1 types)
- **Phase 2**: Path-Based API (get, put, delete, list, getMetadata)
- **Phase 3**: HAMT Integration (auto-sharding at 1000+ entries)
- **Phase 4**: Directory Utilities (walker, batch operations)
- **Phase 5**: Media Processing Foundation (WASM + Canvas with browser detection)
- **Phase 6**: Advanced Media Processing (thumbnail generation, progressive loading, FS5 integration, bundle optimization)
- **Phase 6.5**: Advanced CID API (74 tests passing, `s5/advanced` export)
- **Phase 7**: Testing & Performance (280+ tests, benchmarks complete)

### Remaining Work ⏳

- **Phase 8**: Documentation & Upstream Integration
  - Community outreach (blog post, forum announcements)
  - Upstream PR to s5-dev/s5.js
  - Optional: Firefox/Safari browser testing

## Performance

The implementation has been benchmarked to ensure efficient operation:

- **HAMT activation**: Automatic at 1000+ entries
- **Scaling**: O(log n) performance verified up to 100K+ entries
- **Memory usage**: ~650 bytes per directory entry
- **Network latency**: ~800ms per operation on real S5 network

See [BENCHMARKS.md](./docs/BENCHMARKS.md) for detailed results.

## Testing & Integration

- For S5 portal testing, see the test files mentioned above
- For integration testing with external services, see [test-server-README.md](./test/integration/test-server-README.md)

## Troubleshooting

### "Invalid base length" errors

- Solution: Use a fresh seed phrase. Old accounts have incompatible key structures.

### Directory not found errors

- Solution: Ensure you call `ensureIdentityInitialized()` after portal registration
- All paths must start with `home/` or `archive/`

### Portal connection issues

- Use `https://s5.vup.cx` which has the updated API
- Ensure you have Node.js v20+ for proper crypto support

## Important Notes

- **Format**: Uses new CBOR/DirV1 format - NOT compatible with old S5 data
- **Paths**: Must start with `home/` or `archive/`
- **Portal**: Use `https://s5.vup.cx` for testing (has updated API)
- **Identity**: Requires fresh seed phrases (old accounts incompatible)

## Contributing

This project is being developed under a Sia Foundation grant. For contributions or issues, please refer to the [grant proposal](./docs/grant/Sia-Standard-Grant-Enhanced-s5js.md).

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

---

*This is an enhanced version of s5.js being developed under an 8-month grant from the Sia Foundation. The project implements a new format using CBOR serialization with the DirV1 specification.*