<!--
Copyright (c) 2025 Fabstir
SPDX-License-Identifier: BUSL-1.1
-->

# Fabstir LLM Node

**Version**: v8.3.6 (November 2025)

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
./download_test_model.sh
```

3. Build the project:
```bash
cargo build --release
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

# Logging
RUST_LOG=debug                   # Log level (trace, debug, info, warn, error)
```

### Running in Production

For production deployment:
```bash
# Build optimized binary
cargo build --release

# Run the binary directly
./target/release/fabstir-llm-node
```

## Project Structure

```
fabstir-llm-node/
├── src/
│   ├── p2p/          # P2P networking layer
│   ├── inference/    # LLM inference engine
│   ├── contracts/    # Smart contract integration
│   └── api/          # Client API layer
├── tests/            # Comprehensive test suite
├── models/           # Model files directory
└── docs/             # Documentation
```

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test module
cargo test p2p::

# Run with output
cargo test -- --nocapture
```

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
  - Automatic settlement on disconnect
  - Message compression support

## Troubleshooting

### Port Already in Use

If you get a "port already in use" error:
```bash
# Use different ports
P2P_PORT=9001 API_PORT=8081 cargo run --release
```

### CUDA Not Found

If CUDA is not detected but you have a GPU:
```bash
# Verify CUDA installation
nvidia-smi

# Set CUDA path explicitly
export CUDA_PATH=/usr/local/cuda
cargo run --release
```

### Model Loading Issues

Ensure models are in GGUF format and placed in the correct directory:
```bash
# Check model directory
ls -la models/

# Verify model format
file models/your-model.gguf
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

- [Multi-Chain Configuration Guide](docs/MULTI_CHAIN_CONFIG.md) - Configure multi-chain support
- [Deployment Guide](docs/DEPLOYMENT.md) - Deploy nodes in production
- [Troubleshooting Guide](docs/TROUBLESHOOTING.md) - Common issues and solutions
- [API Documentation](docs/API.md) - Complete API reference
- [RAG SDK Integration Guide](docs/RAG_SDK_INTEGRATION.md) - RAG implementation for SDK developers
- [Encryption Security Guide](docs/ENCRYPTION_SECURITY.md) - End-to-end encryption details
- [Implementation Roadmap](docs/IMPLEMENTATION.md) - Development progress
- [Multi-Chain Implementation](docs/IMPLEMENTATION-MULTI.md) - Multi-chain feature details