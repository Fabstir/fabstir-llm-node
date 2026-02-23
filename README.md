<!--
Copyright (c) 2025 Fabstir
SPDX-License-Identifier: BUSL-1.1
-->

# Fabstir LLM Node

**Version**: v8.16.1-auto-image-routing (February 2026)

A peer-to-peer node software for the Fabstir LLM marketplace, enabling GPU owners to provide compute directly to clients without central coordination. Built in Rust using libp2p for networking, integrated with llama.cpp for LLM inference, and supporting multiple blockchain networks for smart contract interactions.

## Features

- **Pure P2P Architecture**: No relay servers or centralized components
- **Multi-Chain Support**: Base Sepolia and opBNB Testnet (more chains coming)
- **Direct Client Connections**: Clients connect directly to nodes via libp2p
- **DHT Discovery**: Nodes announce capabilities using Kademlia DHT
- **LLM Inference**: Integrated with llama-cpp-2 for GPU-accelerated inference
- **Smart Contract Integration**: Multi-chain support for job state and payments
- **Streaming Responses**: Real-time result streaming as generated
- **Chain-Aware Settlement**: Automatic payment settlement on the correct chain
- **WebSocket API**: Production-ready with compression, rate limiting, and authentication
- **End-to-End Encryption**: ECDH + XChaCha20-Poly1305 for secure sessions (v8.0.0+)
- **Zero-Knowledge Proofs**: GPU-accelerated STARK proofs via Risc0 zkVM (v8.1.0+)
- **Host-Side RAG**: Session-scoped vector storage for document retrieval (v8.3.0+)
- **Off-Chain Proof Storage**: S5 decentralized storage for proofs (v8.1.2+)
- **S5 Vector Loading**: Load vector databases from S5 decentralized storage (v8.4.0+)
- **Encrypted Vector Paths**: Support for encrypted vector_database paths in job parameters (v8.4.0+)
- **Chat Templates**: Model-specific formatting (Harmony, Llama, etc.) (v8.3.13+)
- **AUDIT-F4 Compliance**: Model ID in proof signatures prevents cross-model replay attacks (v8.13.0+)
- **Model Validation**: Dynamic model authorization with SHA256 verification (v8.14.0+)
- **Image Generation**: Text-to-image via FLUX.2 Klein 4B diffusion sidecar (v8.16.0+), auto-routing from chat (v8.16.1+)
- **VLM Vision**: GPU-accelerated OCR and image description via Qwen3-VL sidecar (v8.15.3+)

## Prerequisites

- Rust 1.70 or higher
- CUDA toolkit (optional, for GPU acceleration)
- Git

## Installation

1. Clone the repository:
```bash
git clone https://github.com/yourusername/fabstir-llm-node.git
cd fabstir-llm-node
```

2. Download test model (optional):
```bash
./scripts/phase_4_2_2/download_test_model.sh
```

3. Download embedding model (required for RAG):
```bash
./scripts/download_embedding_model.sh
```

4. Build the project:

**🚨 CRITICAL: Production builds MUST use the `--features real-ezkl` flag!**

```bash
# ✅ CORRECT - Production build with REAL Risc0 STARK proofs
cargo build --release --features real-ezkl -j 4

# ❌ WRONG - Creates binary with MOCK proofs (not production-ready!)
# cargo build --release
```

**Why `-j 4`?** Limits parallel jobs to avoid out-of-memory errors during Risc0 compilation.

**How to verify**: After building, check that you have real proofs enabled:
```bash
# Check version
strings target/release/fabstir-llm-node | grep "v8.16.1"

# During inference, logs should show:
# ✅ "🔐 Generating real Risc0 STARK proof" (221KB proofs)
# ❌ NOT "🎭 Generating mock proof" (126 byte mock proofs)
```

## Starting the Node

### Basic Usage

Run the node with default settings:
```bash
cargo run --release
```

### With GPU Acceleration

If you have a CUDA-capable GPU:
```bash
CUDA_VISIBLE_DEVICES=0 cargo run --release
```

### Configuration Options

The node can be configured through environment variables:

```bash
# Network Configuration
P2P_PORT=9001                    # P2P listening port (default: 9000)
API_PORT=8081                    # API server port (default: 8080)

# Multi-Chain Configuration
CHAIN_ID=84532                   # Active chain ID (84532=Base Sepolia, 5611=opBNB Testnet)
BASE_SEPOLIA_RPC=https://...    # Base Sepolia RPC endpoint
OPBNB_TESTNET_RPC=https://...   # opBNB Testnet RPC endpoint

# Model Configuration
MODEL_PATH=./models/model.gguf   # Path to GGUF model file
CUDA_VISIBLE_DEVICES=0           # GPU device selection

# Storage Configuration
ENHANCED_S5_URL=http://localhost:5522  # Enhanced S5.js endpoint
VECTOR_DB_URL=http://localhost:8081    # Vector DB endpoint

# Encryption & RAG (v8.0.0+)
HOST_PRIVATE_KEY=0x...           # Required for encryption and settlements
SESSION_KEY_TTL_SECONDS=3600     # Session key expiration (default: 1 hour)

# Model Validation (v8.14.0+)
REQUIRE_MODEL_VALIDATION=false   # Enable model authorization enforcement
                                 # When true: validates MODEL_PATH, SHA256, host auth

# Image Generation (v8.16.0+)
AUTO_IMAGE_ROUTING=false         # Auto-detect image intent from chat and route to
                                 # diffusion sidecar (v8.16.1+, opt-in, default off)

# Logging
RUST_LOG=debug                   # Log level (trace, debug, info, warn, error)
```

### Running in Production

For production deployment:
```bash
# Build optimized binary with REAL proofs (CRITICAL!)
cargo build --release --features real-ezkl -j 4

# Verify version
./target/release/fabstir-llm-node --version

# Run the binary directly
./target/release/fabstir-llm-node
```

**Important**: Building requires CUDA libraries. For deployment to environments without build tools, use pre-built tarballs:
```bash
# Extract pre-built binary
tar -xzf fabstir-llm-node-v8.13.0-audit-remediation.tar.gz
cd fabstir-llm-node-v8.13.0
./fabstir-llm-node --version
```

## Smart Contract Configuration

**Single Source of Truth**: All contract addresses are defined in `.env.contracts`

Key contracts (Base Sepolia, v8.13.0+ AUDIT-F4 Remediated):
- **CONTRACT_NODE_REGISTRY**: `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22` (unchanged)
- **CONTRACT_JOB_MARKETPLACE**: `0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4` (AUDIT-F4 compliant)
- **CONTRACT_HOST_EARNINGS**: `0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0` (unchanged)
- **CONTRACT_PROOF_SYSTEM**: `0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31` (AUDIT-F4 compliant)
- **CONTRACT_MODEL_REGISTRY**: `0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2` (unchanged)

**Deprecated (pre-AUDIT-F4)**:
- Old JobMarketplace: `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` (deprecated Jan 31, 2026)
- Old ProofSystem: `0x5afB91977e69Cc5003288849059bc62d47E7deeb` (deprecated Jan 31, 2026)

## Project Structure

```
fabstir-llm-node/
├── src/
│   ├── p2p/          # P2P networking layer
│   ├── inference/    # LLM inference engine
│   ├── contracts/    # Smart contract integration
│   ├── blockchain/   # Multi-chain configuration
│   ├── settlement/   # Payment distribution
│   ├── crypto/       # End-to-end encryption
│   ├── rag/          # Session-scoped vector storage
│   ├── storage/      # S5 storage clients
│   └── api/          # Client API layer
├── tests/            # Comprehensive test suite
├── models/           # Model files directory
├── contracts/        # Contract ABIs
└── docs/             # Documentation
```

## Development

### Running Tests

```bash
# Critical CI/CD pipeline tests (must pass for deployment)
cargo test --lib                              # Unit tests
cargo test --test integration_tests           # Integration tests
cargo test --test test_host_management        # Host management
cargo test --test test_job_assignment         # Job assignment
cargo test --test contracts_tests             # Contract tests
cargo test --test api_tests                   # API tests
cargo test --test websocket_tests             # WebSocket tests

# Module-specific test suites
cargo test --test crypto_tests                # Encryption tests (111 tests)
cargo test --test inference_tests             # Inference engine tests
cargo test --test vector_tests                # Vector/RAG tests
cargo test --test ezkl_tests                  # Proof generation tests
cargo test --test settlement_tests            # Payment settlement tests

# Run specific test function
cargo test test_function_name -- --exact

# Run with output visible
cargo test -- --nocapture

# Timeout tests to avoid CPU overload
timeout 60 cargo test --test integration_tests
```

**Known Testing Issues**:
- CPU Overload: Some tests can consume 100% CPU. Use `timeout` command or run tests individually
- sccache Issues: If compilation hangs, run `pkill sccache` and `unset RUSTC_WRAPPER`
- Memory Issues: Contract tests may fail with linker errors due to memory constraints

### Code Formatting

```bash
# Format code
cargo fmt

# Run linter
cargo clippy --all-targets --all-features
```

### Building Documentation

```bash
cargo doc --open
```

## Model Support

The node supports GGUF format models. Place your models in the `models/` directory:

- Test model: `models/tiny-vicuna-1b.q4_k_m.gguf`
- Supports various quantization formats (Q4_K_M, Q5_K_M, Q8_0, etc.)

## API Endpoints

Once the node is running, it exposes the following endpoints:

### HTTP Endpoints
- `GET /health` - Health check
- `GET /v1/version` - Version information and features
- `GET /status` - Node status and capabilities
- `GET /chains` - List supported chains
- `GET /chain/{chain_id}` - Get specific chain configuration
- `POST /inference` - Submit inference request (includes chain_id)
- `POST /v1/embed` - Generate 384D embeddings (for RAG)

### WebSocket Endpoints
- `WS /v1/ws` - WebSocket connection for streaming inference
  - Session management with chain tracking
  - End-to-end encryption support (v8.0.0+)
  - RAG vector upload/search (v8.3.0+)
  - S5 vector database loading (v8.4.0+)
  - Encrypted vector_database path support (v8.4.0+)
  - Image generation via diffusion sidecar (v8.16.0+)
  - Auto-route image intent from chat (v8.16.1+)
  - Automatic settlement on disconnect
  - Message compression support

## Troubleshooting

### Build Issues

#### Mock Proofs Instead of Real Proofs
If you see `🎭 Generating mock proof` in logs instead of `🔐 Generating real Risc0 STARK proof`:
```bash
# Rebuild with correct flags
cargo clean
cargo build --release --features real-ezkl -j 4

# Verify version
strings target/release/fabstir-llm-node | grep "v8.16.1"
```

#### Out of Memory During Build
If Risc0 compilation fails with OOM errors:
```bash
# Use -j 4 to limit parallel jobs
cargo build --release --features real-ezkl -j 4
```

#### sccache Hanging
If compilation hangs:
```bash
pkill sccache
unset RUSTC_WRAPPER
cargo build --release --features real-ezkl -j 4
```

### Runtime Issues

#### Port Already in Use
If you get a "port already in use" error:
```bash
# Use different ports
P2P_PORT=9001 API_PORT=8081 cargo run --release
```

#### CUDA Not Found
If CUDA is not detected but you have a GPU:
```bash
# Verify CUDA installation
nvidia-smi

# Set CUDA path explicitly
export CUDA_PATH=/usr/local/cuda
cargo run --release --features real-ezkl

# Check CUDA libraries in binary
ldd target/release/fabstir-llm-node | grep cuda
```

### Model Issues

#### Model Loading Failures
Ensure models are in GGUF format and placed in the correct directory:
```bash
# Check model directory
ls -la models/

# Verify model format
file models/your-model.gguf
```

#### Embedding Model Missing
For RAG support, the embedding model must be downloaded:
```bash
./scripts/download_embedding_model.sh

# Verify installation
ls -la models/all-MiniLM-L6-v2-onnx/
```

## License & Usage

This project is source-available under the **Business Source License 1.1** (BUSL-1.1).

### You MAY:
- ✅ View, audit, and review the code (trustless verification)
- ✅ Use in production on the Official Platformless AI Network with FAB token
- ✅ Run nodes on the Official Platformless AI Network
- ✅ Fork for development, testing, research, and security audits

### You MAY NOT (before 2029-01-01):
- ❌ Launch competing networks with different staking tokens
- ❌ Operate nodes on competing networks
- ❌ Offer as commercial hosting service (SaaS/PaaS)

**After 2029-01-01**: Automatically converts to AGPL-3.0-or-later.

See [LICENSE](LICENSE) for full terms.

### Interested in Contributing?

We welcome contributions! If you're interested in contributing, please reach out via:
- 💬 [Discord Community](https://discord.gg/fabstir)
- 📧 Email: support@fabstir.com

For code contributions, please ensure you've read and understood the license terms above.

## Support

For issues and questions:
- Open an issue on GitHub
- Join our Discord community
- Check the [documentation](docs/) for detailed guides

## Documentation

### Node Reference
- [API Documentation](docs/API.md) - Complete API reference including encryption protocol
- [Deployment Guide](docs/DEPLOYMENT.md) - Deploy nodes in production
- [Troubleshooting Guide](docs/TROUBLESHOOTING.md) - Common issues and solutions
- [Encryption Security Guide](docs/ENCRYPTION_SECURITY.md) - End-to-end encryption details (v8.0.0+)
- [Multi-Chain Configuration Guide](docs/MULTI_CHAIN_CONFIG.md) - Configure multi-chain support

### SDK Developer Guides
- [WebSocket API Integration](docs/sdk-reference/WEBSOCKET_API_SDK_GUIDE.md) - WebSocket protocol for SDK developers
- [S5 Vector Loading](docs/sdk-reference/S5_VECTOR_LOADING.md) - Load vector databases from S5 storage (v8.4.0+)
- [SDK Encryption Integration](docs/sdk-reference/SDK_ENCRYPTION_INTEGRATION.md) - Client-side encryption integration
- [Model Validation Compatibility](docs/sdk-reference/MODEL-VALIDATION-SDK-COMPATIBILITY.md) - Model authorization guide (v8.14.0+)