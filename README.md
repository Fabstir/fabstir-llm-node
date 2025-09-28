# Fabstir LLM Node

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
- `GET /status` - Node status and capabilities
- `GET /chains` - List supported chains
- `GET /chain/{chain_id}` - Get specific chain configuration
- `POST /inference` - Submit inference request (includes chain_id)

### WebSocket Endpoints
- `WS /ws` - WebSocket connection for streaming inference
  - Session management with chain tracking
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

## Contributing

Please read [CONTRIBUTING.md](docs/CONTRIBUTING.md) for details on our code of conduct and the process for submitting pull requests.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

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
- [Implementation Roadmap](docs/IMPLEMENTATION.md) - Development progress
- [Multi-Chain Implementation](docs/IMPLEMENTATION-MULTI.md) - Multi-chain feature details