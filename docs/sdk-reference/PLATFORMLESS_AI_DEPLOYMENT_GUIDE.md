// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

# Platformless AI (Fabstir LLM Marketplace) - Deployment Guide

**Audience**: DevOps Engineers
**Purpose**: Understand the architecture and deploy Platformless AI to cloud providers

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Project Components](#project-components)
3. [Deployment Architecture](#deployment-architecture)
4. [Cloud Deployment Guide](#cloud-deployment-guide)
5. [Monitoring & Operations](#monitoring--operations)

---

## Architecture Overview

**Platformless AI** is a **fully decentralized** P2P marketplace for LLM inference where:
- **GPU providers** run independent nodes that execute AI workloads
- **Clients** discover nodes via blockchain (no central discovery service)
- **Smart contracts** handle payments, reputation, and proof verification
- **No central server** - all coordination happens via blockchain + direct P2P connections

**Key Principle**: Each node is operated by a different entity. There is **no centralized infrastructure** to deploy.

```
┌──────────────────────────────────────────────────────────────────────┐
│                      PLATFORMLESS AI - DECENTRALIZED                  │
├──────────────────────────────────────────────────────────────────────┤
│                                                                       │
│  ┌────────────────────┐                                             │
│  │ Client (Browser)   │                                             │
│  │ fabstir-llm-ui3    │  ◄── Frontend dapp (Next.js)               │
│  └──────────┬─────────┘                                             │
│             │ Uses SDK                                               │
│             ▼                                                        │
│  ┌────────────────────┐                                             │
│  │ fabstir-llm-sdk    │  ◄── Client library                        │
│  └──────────┬─────────┘                                             │
│             │                                                        │
│             │ 1. Query blockchain for registered nodes              │
│             │ 2. Connect directly to chosen node via WebSocket      │
│             │                                                        │
│             ▼                                                        │
│  ┌──────────────────────────┐                                       │
│  │ Base Blockchain          │                                       │
│  │ (Smart Contracts)        │                                       │
│  ├──────────────────────────┤                                       │
│  │ • JobMarketplace         │  ◄── Handles payments, sessions      │
│  │ • NodeRegistry           │  ◄── Stores node metadata/URLs       │
│  │ • ProofSystem            │  ◄── Verifies computation            │
│  └───────────┬──────────────┘                                       │
│              │ Nodes register themselves                            │
│              │                                                       │
│       ┌──────┴───────┬─────────────────────┐                        │
│       │              │                     │                        │
│       ▼              ▼                     ▼                        │
│  ┌─────────┐   ┌─────────┐          ┌─────────┐                    │
│  │ Node 1  │   │ Node 2  │   ...    │ Node N  │                    │
│  │ (Alice) │   │ (Bob)   │          │ (Eve)   │                    │
│  ├─────────┤   ├─────────┤          ├─────────┤                    │
│  │ GPU     │   │ GPU     │          │ GPU     │                    │
│  │ A16 2GB │   │ A16 2GB │          │ A16 2GB │                    │
│  │         │   │         │          │         │                    │
│  │ Vultr   │   │ Vultr   │          │ Vultr   │                    │
│  │ US-East │   │ EU-West │          │ Asia    │                    │
│  └─────────┘   └─────────┘          └─────────┘                    │
│       ▲              ▲                     ▲                        │
│       │              │                     │                        │
│       └──────────────┴─────────────────────┘                        │
│              Direct WebSocket connections                           │
│              (SDK auto-selects host based on model chosen)          │
│              (Advanced: manual host selection available)            │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

**Important**: Nodes do NOT communicate with each other. Clients discover nodes via blockchain and connect directly via WebSocket.

---

## Project Components

### 1. fabstir-compute-contracts

**What it is**: Solidity smart contracts deployed on Base blockchain

**Purpose**:
- Handles all on-chain logic for the marketplace
- Manages payments, escrow, and settlements
- Tracks node registration and reputation
- Verifies computation proofs (STARK proofs)
- Enforces pricing and token economics

**Key Contracts**:
- `JobMarketplace.sol` - Main marketplace logic (jobs, payments, sessions)
- `NodeRegistry.sol` - GPU node registration and discovery
- `ProofSystem.sol` - Computation proof verification
- `HostEarnings.sol` - Earnings tracking and withdrawal
- `ModelRegistry.sol` - Approved AI models governance

**Deployment**:
- Deployed to Base Sepolia (testnet) and Base Mainnet
- Contract addresses stored in `.env.test`
- Deployed using Hardhat/Foundry
- Upgradeable via proxy pattern (if applicable)

**Who deploys**: Project owner only (centralized governance during BUSL period)

**DevOps responsibility**: None (blockchain handles hosting)

---

### 2. fabstir-llm-auth

**What it is**: Wallet-based authentication (decentralized)

**Purpose**:
- Validates Ethereum wallet signatures
- No centralized auth server - authentication is wallet-based
- Each node verifies signatures independently
- Prevents replay attacks and unauthorized access

**How it works**:
1. Client signs a challenge with their Ethereum wallet
2. Node verifies the signature against the wallet address
3. No JWT tokens needed - wallet signatures prove identity

**Technology Stack**:
- Ethereum signature verification (ethers.js)
- Wallet signature challenges (EIP-712)
- No separate auth service to deploy

**DevOps responsibility**:
- **None** - authentication is built into the node software
- No separate auth service to deploy or manage

---

### 3. fabstir-llm-ui3

**What it is**: Frontend dapp for end users (browser-based UI)

**Purpose**:
- User-friendly interface for Platformless AI marketplace
- Wallet connection (MetaMask, WalletConnect, Coinbase Wallet)
- Deposit USDC/ETH for inference credits
- Discover available GPU nodes
- Chat interface for LLM conversations
- View transaction history and earnings

**Technology Stack**:
- Next.js 14 (React framework)
- TypeScript
- Tailwind CSS for styling
- ethers.js v6 for wallet connections
- @fabstir/sdk-core integration

**Deployment Requirements**:
- **Static hosting** (Vercel, Netlify, Cloudflare Pages)
- **OR Docker container** for self-hosting
- **Environment Variables**:
  - `NEXT_PUBLIC_RPC_URL` - Base blockchain RPC endpoint
  - `NEXT_PUBLIC_CHAIN_ID` - Chain ID (84532 for Base Sepolia)
  - `NEXT_PUBLIC_CONTRACT_JOB_MARKETPLACE` - Contract address
  - `NEXT_PUBLIC_CONTRACT_NODE_REGISTRY` - Contract address
  - `NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID` - WalletConnect Project ID

**DevOps responsibility**:
- Deploy to Vercel/Netlify (easiest) OR self-host
- Configure custom domain and SSL
- Set up CDN for static assets
- Monitor uptime and performance

---

### 4. fabstir-llm-node

**What it is**: GPU-accelerated inference node that executes LLM workloads

**Purpose**:
- Runs AI models (Llama, GPT, etc.)
- Accepts WebSocket connections from clients
- Streams inference results in real-time
- Generates and submits STARK proofs to blockchain
- Earns FAB/USDC tokens for computation

**Technology Stack**:
- **Rust** (compiled native binary, not Node.js)
- WebSocket server: **Axum** web framework with async support
- LLM inference: **llama-cpp-2** Rust bindings (GGUF format models)
- P2P networking: **libp2p** for DHT-based discovery
- Blockchain integration: **ethers-rs** for smart contract interactions
- Proof generation: **Risc0 zkVM v3.0** for GPU-accelerated STARK proofs
- Encryption: **XChaCha20-Poly1305 AEAD** with ECDH key exchange
- Storage: **s5-rs** for decentralized proof storage

**System Requirements**:
- **CPU**: 4+ vCPUs
- **RAM**: 8GB system memory
- **GPU**: NVIDIA A16 with CUDA 12.2+
  - Available on Vultr Cloud GPU at $0.059/GPU/hour
  - **Note**: Consumer GPUs (RTX series) are NOT allowed on most cloud services
  - Suitable for small models (1-7B parameters with GGUF Q4_K_M quantization)
- **Storage**: 50GB+ SSD for binary, model weights, and logs
- **Network**:
  - **Inbound**: Port 9000 (P2P/libp2p), Port 8080 (WebSocket API)
  - **Outbound**: Access to blockchain RPC endpoint and S5 network
  - **Bandwidth**: 100+ Mbps (1+ Gbps recommended for high throughput)

**Environment Variables**:
- `HOST_PRIVATE_KEY` - Ethereum private key (required for encryption, checkpoints, settlements)
- `RPC_URL` - Blockchain RPC endpoint (e.g., https://sepolia.base.org)
- `CHAIN_ID` - Blockchain chain ID (e.g., 84532 for Base Sepolia)
- `MODEL_PATH` - Path to GGUF model file (e.g., /app/models/tinyllama-1b.Q4_K_M.gguf)
- `P2P_PORT` - libp2p networking port (default: 9000)
- `API_PORT` - WebSocket API port (default: 8080)
- `PUBLIC_URL` - Node's public URL for registration (e.g., https://node1.example.com)
- `GPU_LAYERS` - Number of model layers to offload to GPU (default: 35)
- `NODE_REGISTRY_FAB_ADDRESS` - NodeRegistry contract address
- `JOB_MARKETPLACE_FAB_WITH_S5_ADDRESS` - JobMarketplace contract address
- `RUST_LOG` - Logging level (info, debug, warn, error)
- `SESSION_KEY_TTL_SECONDS` - Encryption session key expiration (default: 3600)

**Note**: `--features real-ezkl` is a **build-time** flag, not a runtime environment variable. STARK proof generation is controlled at compile time.

**DevOps responsibility**:
- Provision GPU instances on **Vultr Cloud GPU**
- Configure NVIDIA drivers and CUDA 12.2+
- **Build** node from source with `--features real-ezkl` flag (CRITICAL!)
- Deploy node software (Docker + nvidia-docker2)
- Register node on blockchain using built-in CLI
- Monitor GPU utilization, temperature, and node health
- Set up auto-restart on crashes
- Configure firewall (allow ports 9000 and 8080 inbound)
- Set up local log management (Docker log rotation)

---

### 5. Built-in CLI Commands (fabstir-llm-node)

**What it is**: Command-line interface built directly into the fabstir-llm-node binary

**Purpose**:
- Register nodes on blockchain (NodeRegistry contract)
- Check registration status
- Update node configuration (API URL, models)
- Manage node lifecycle without separate tools

**Technology Stack**:
- Rust with **clap** CLI framework
- Built into main binary - no separate installation needed
- Uses `HOST_PRIVATE_KEY` environment variable for authentication

**Key Commands**:
```bash
# Register node on blockchain
./fabstir-llm-node register-node \
  --chain 84532 \
  --name "My GPU Node" \
  --api-url "https://node1.example.com:8080" \
  --models "0xmodel-hash-1,0xmodel-hash-2" \
  --performance-tier standard

# Register on all supported chains
./fabstir-llm-node register-node \
  --all-chains \
  --name "Multi-Chain Node" \
  --api-url "https://node.example.com:8080" \
  --models "0xmodel-hash-1"

# Check registration status
./fabstir-llm-node registration-status --chain 84532

# Check status on all chains
./fabstir-llm-node registration-status --all-chains

# Update registration
./fabstir-llm-node update-registration \
  --chain 84532 \
  --api-url "https://new-url.example.com:8080" \
  --models "0xnew-model-hash"

# Show version
./fabstir-llm-node --version

# Start node (normal operation)
./fabstir-llm-node
```

**Configuration**:
- Uses environment variables (no config files)
- Requires `HOST_PRIVATE_KEY` to be set
- Reads from `.env` file if present (via `dotenv`)

**DevOps responsibility**:
- Use built-in commands to register after deployment
- Monitor registration status before going live
- Update registration when changing infrastructure

**Important Notes**:
- ⚠️ There is NO separate `@fabstir/host-cli` NPM package
- All CLI functionality is built into the main binary
- Use `docker exec` to run commands inside containers

---

### 6. fabstir-llm-sdk

**What it is**: TypeScript SDK for client applications to interact with Platformless AI

**Purpose**:
- Discover available GPU nodes via blockchain
- Authenticate with wallet (MetaMask, WalletConnect)
- Deposit USDC/ETH for inference credits
- Submit inference requests to nodes
- Stream LLM responses via WebSocket
- Handle payments and proof verification

**Technology Stack**:
- TypeScript/JavaScript (browser + Node.js compatible)
- ethers.js v6 for blockchain interactions
- WebSocket client for streaming
- S5 client for decentralized storage

**Package Structure**:
- `@fabstir/sdk-core` - Browser-compatible core SDK (main package)
- Host CLI is built into fabstir-llm-node binary, not a separate NPM package

**Integration Points**:
- **Frontend**: React, Next.js, Vue.js apps (e.g., fabstir-llm-ui3)
- **Backend**: Node.js services, serverless functions
- **Mobile**: React Native (via shims)

**Deployment**:
- **Tarballs** (`.tgz` files) or direct project directory access
- No NPM registry - installed as local dependency
- Bundled into client applications

**DevOps responsibility**:
- None (library consumed by developers as dependency)
- fabstir-llm-ui3 includes it as a dependency automatically

---

## Deployment Architecture

### Decentralized Setup (Production)

**Key Concept**: There is **NO centralized infrastructure**. Each deployment scenario below represents **independent host providers** joining the network.

```
┌──────────────────────────────────────────────────────────────────┐
│                  DECENTRALIZED DEPLOYMENT EXAMPLE                 │
├──────────────────────────────────────────────────────────────────┤
│                                                                   │
│  HOST PROVIDER A (e.g., You - for testing/demo)                 │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │                                                             │ │
│  │  Vultr Instance 1 (vgp-a16-8gb-50gb)                        │ │
│  │  ┌──────────────────────────────────────────────────────┐  │ │
│  │  │ GPU Node 1                                           │  │ │
│  │  │ - fabstir-llm-node                                   │  │ │
│  │  │ - NVIDIA A16 2GB VRAM                                │  │ │
│  │  │ - Model: Llama-2-7B (GGUF)                           │  │ │
│  │  │ - WebSocket: wss://node1.yourplatform.ai:8080       │  │ │
│  │  │ - Registered on blockchain                          │  │ │
│  │  │ - Wallet: 0xAAA... (Host Provider A's wallet)       │  │ │
│  │  └──────────────────────────────────────────────────────┘  │ │
│  │                                                             │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  HOST PROVIDER B (Different operator - independent)              │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │                                                             │ │
│  │  Vultr Instance 2 (vgp-a16-8gb-50gb)                        │ │
│  │  ┌──────────────────────────────────────────────────────┐  │ │
│  │  │ GPU Node 2                                           │  │ │
│  │  │ - fabstir-llm-node                                   │  │ │
│  │  │ - NVIDIA A16 2GB VRAM                                │  │ │
│  │  │ - Model: Llama-2-7B (GGUF)                           │  │ │
│  │  │ - WebSocket: wss://node2.anotherhost.com:8080       │  │ │
│  │  │ - Registered on blockchain                          │  │ │
│  │  │ - Wallet: 0xBBB... (Host Provider B's wallet)       │  │ │
│  │  └──────────────────────────────────────────────────────┘  │ │
│  │                                                             │ │
│  └────────────────────────────────────────────────────────────┘ │
│                                                                   │
│                            ▲                                     │
│                            │                                     │
│                            │ Both register to                    │
│                            │                                     │
│                            ▼                                     │
│                ┌────────────────────────┐                        │
│                │ Base Blockchain        │                        │
│                │ NodeRegistry Contract  │                        │
│                └────────────────────────┘                        │
│                            ▲                                     │
│                            │                                     │
│                            │ Clients query                       │
│                            │                                     │
│  ┌─────────────────────────┴───────────────────────────┐        │
│  │                                                      │        │
│  │  FRONTEND UI (Optional - for end users)             │        │
│  │  ┌────────────────────────────────────────────────┐ │        │
│  │  │ fabstir-llm-ui3 (Next.js)                     │ │        │
│  │  │ - Deployed on Vercel/Netlify                  │ │        │
│  │  │ - URL: https://app.platformless.ai            │ │        │
│  │  │ - Discovers nodes via SDK                     │ │        │
│  │  │ - Connects directly to chosen node via WS     │ │        │
│  │  └────────────────────────────────────────────────┘ │        │
│  │                                                      │        │
│  └──────────────────────────────────────────────────────┘        │
│                                                                   │
└──────────────────────────────────────────────────────────────────┘
```

**Important Notes**:
- **No load balancer between nodes** - each node is independent
- **No central API gateway** - clients connect directly to nodes via WebSocket
- **Nodes don't know about each other** - discovery happens via blockchain only
- **Host selection**: SDK automatically selects compatible host based on model chosen; advanced users can manually select from list of compatible hosts
- **Frontend UI is optional** - developers can build their own using the SDK

---

## Cloud Deployment Guide

### Prerequisites

1. **Vultr Cloud GPU Account** (using Vultr for cloud service)
2. **Domain Name** (for SSL/DNS)
3. **Ethereum Wallet** (**Note**: Test account keys will be provided - DevOps does not need to handle wallet setup or staking)
4. **GPU Instance Access**: Vultr Cloud GPU with NVIDIA A16
5. **Contract Addresses**: Use addresses from project's `.env.test.local.example`

---

### Building fabstir-llm-node from Source

**⚠️ CRITICAL**: Production nodes MUST be built with `--features real-ezkl` to generate real STARK proofs!

**Prerequisites**:
- Rust 1.75+ (nightly toolchain)
- CUDA Toolkit 12.2+
- Git

**Build Steps**:
```bash
# 1. Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 2. Install nightly toolchain
rustup default nightly

# 3. Clone repository
git clone https://github.com/fabstir/fabstir-llm-node.git
cd fabstir-llm-node

# 4. Build with REAL STARK proofs (CRITICAL!)
cargo build --release --features real-ezkl -j 4

# The -j 4 flag limits parallel jobs to avoid OOM errors during Risc0 compilation

# 5. Verify version
./target/release/fabstir-llm-node --version
# Expected output: v8.1.6-websocket-error-logging-2025-10-15 (or later)

# 6. Check binary size
ls -lh target/release/fabstir-llm-node
# Expected: ~500MB (includes CUDA support and Risc0 zkVM)
```

**Verification**:
```bash
# Confirm real proofs are enabled
strings target/release/fabstir-llm-node | grep "real-ezkl"
# Should find references to Risc0 STARK proof generation
```

**⚠️ Common Mistakes**:
- Building without `--features real-ezkl` → Generates MOCK proofs (not production-ready!)
- Building on host machine without CUDA → Creates CPU-only binary
- Not using `-j 4` → Runs out of memory during compilation

---

### Deployment Steps

**Important**: This guide shows how to deploy **2 independent nodes** as if they were operated by different host providers. Each node is completely independent and doesn't know about the other.

---

#### Step 1: Deploy GPU Node 1 (Host Provider A)

**Cloud Provider**: Vultr Cloud GPU

**GPU Specs**:
- **Instance Type**: `vgp-a16-8gb-50gb`
- **GPU**: NVIDIA A16 with CUDA 12.2+
- **System RAM**: 8GB
- **Storage**: 50GB SSD
- **Price**: ~$0.059/GPU/hour (~$42.50/month)

**Note**: Consumer GPUs (RTX series) are NOT allowed on most cloud services

**1. Provision GPU Instance**:
```bash
# Via Vultr CLI
vultr-cli instance create \
  --region ewr \
  --plan vgp-a16-8gb-50gb \
  --os "Ubuntu 22.04 LTS" \
  --label "fabstir-node-1"

# Or via Vultr web console
# Navigate to: Compute → Deploy New Instance → Cloud GPU
```

**2. Install CUDA and Docker**:
```bash
# SSH into instance
ssh root@<instance-ip>

# Install NVIDIA drivers
ubuntu-drivers autoinstall
reboot

# Reconnect after reboot
ssh root@<instance-ip>

# Install Docker
curl -fsSL https://get.docker.com | sh

# Install nvidia-docker2
distribution=$(. /etc/os-release;echo $ID$VERSION_ID)
curl -s -L https://nvidia.github.io/nvidia-docker/gpgkey | apt-key add -
curl -s -L https://nvidia.github.io/nvidia-docker/$distribution/nvidia-docker.list | \
  tee /etc/apt/sources.list.d/nvidia-docker.list

apt update
apt install -y nvidia-docker2
systemctl restart docker

# Verify GPU access
docker run --rm --gpus all nvidia/cuda:12.2.0-base-ubuntu22.04 nvidia-smi
```

**3. Build fabstir-llm-node**:
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
rustup default nightly

# Clone repository
git clone https://github.com/fabstir/fabstir-llm-node.git
cd fabstir-llm-node

# Build with REAL STARK proofs (CRITICAL!)
cargo build --release --features real-ezkl -j 4

# Verify version
./target/release/fabstir-llm-node --version
```

**4. Download Model**:
```bash
# Use provided script to download TinyLlama test model (~700MB)
./scripts/phase_4_2_2/download_test_model.sh

# Verify model downloaded
ls -lh models/
# Should see tinyllama-1b.Q4_K_M.gguf or tiny-vicuna-1b.q4_k_m.gguf
```

**5. Configure Environment**:
```bash
cat > .env << 'EOF'
# Node Identity (provided by project team)
HOST_PRIVATE_KEY=0x<provided-test-key>

# Blockchain Configuration
RPC_URL=https://sepolia.base.org
CHAIN_ID=84532

# Contract Addresses (Base Sepolia testnet)
NODE_REGISTRY_FAB_ADDRESS=0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6
JOB_MARKETPLACE_FAB_WITH_S5_ADDRESS=0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E

# Model Configuration
MODEL_PATH=/app/models/tiny-vicuna-1b.q4_k_m.gguf
GPU_LAYERS=35

# Network Ports
P2P_PORT=9000
API_PORT=8080
PUBLIC_URL=https://node1.example.com

# Logging
RUST_LOG=info
EOF

chmod 600 .env  # Protect private key
```

**6. Build and Run Docker Container**:
```bash
# Build Docker image
docker build -f Dockerfile.production -t fabstir-llm-node .

# Run node
docker run -d \
  --name fabstir-node \
  --gpus all \
  --restart unless-stopped \
  -p 9000:9000 \
  -p 8080:8080 \
  -v $(pwd)/models:/app/models \
  --env-file .env \
  --log-opt max-size=10m \
  --log-opt max-file=3 \
  fabstir-llm-node

# Check logs
docker logs -f fabstir-node
# Look for: "🚀 Starting Fabstir LLM Node..."
```

**7. Configure Firewall**:
```bash
# Using UFW (Ubuntu Firewall)
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp     # SSH (restrict to your IP for security)
ufw allow 9000/tcp   # P2P (libp2p DHT)
ufw allow 8080/tcp   # WebSocket API
ufw enable

# Verify rules
ufw status
```

**8. Register Node on Blockchain**:
```bash
# Get approved model hash from project team
# Example TinyVicuna model hash: 0x<model-hash>

# Register using built-in CLI
docker exec -it fabstir-node /usr/local/bin/fabstir-llm-node register-node \
  --chain 84532 \
  --name "Node 1" \
  --api-url "https://node1.example.com:8080" \
  --models "0x<tinyllama-model-hash>" \
  --performance-tier standard

# Verify registration
docker exec -it fabstir-node /usr/local/bin/fabstir-llm-node registration-status --chain 84532

# Expected output:
# ✅ Node registered on chain 84532
# Name: Node 1
# API URL: https://node1.example.com:8080
# Models: [0x...]
```

**9. Test Node Health**:
```bash
curl http://localhost:8080/health

# Expected response:
# {"status":"healthy","model_loaded":true,"gpu_available":true,"active_sessions":0}
```

---

#### Step 2: Deploy GPU Node 2 (Host Provider B)

**IMPORTANT**: This is a completely independent deployment. Use a **different Ethereum wallet** (private key) to simulate a different host provider.

```bash
# 1. Provision SECOND GPU instance (separate from Node 1)
vultr-cli instance create \
  --region ewr \
  --plan vgp-a16-8gb-50gb \
  --os "Ubuntu 22.04 LTS" \
  --label "fabstir-node-2"

# 2. SSH into instance
ssh root@<node-2-instance-ip>

# 3-5. Repeat same setup as Node 1:
# - Install NVIDIA drivers (ubuntu-drivers autoinstall + reboot)
# - Install Docker (curl -fsSL https://get.docker.com | sh)
# - Install nvidia-docker2
# - Verify GPU access (nvidia-smi)

# 6. Install Rust and build fabstir-llm-node
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
rustup default nightly

git clone https://github.com/fabstir/fabstir-llm-node.git
cd fabstir-llm-node

# Build with REAL STARK proofs
cargo build --release --features real-ezkl -j 4

# 7. Download model (same as Node 1)
./scripts/phase_4_2_2/download_test_model.sh

# 8. Create .env file for Node 2 (DIFFERENT private key!)
cat > .env << 'EOF'
# Different host identity
HOST_PRIVATE_KEY=0x<host-provider-b-different-key>

# Same blockchain configuration
RPC_URL=https://sepolia.base.org
CHAIN_ID=84532

# Same contract addresses
NODE_REGISTRY_FAB_ADDRESS=0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6
JOB_MARKETPLACE_FAB_WITH_S5_ADDRESS=0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E

# Model configuration
MODEL_PATH=/app/models/tiny-vicuna-1b.q4_k_m.gguf
GPU_LAYERS=35

# Network ports (same)
P2P_PORT=9000
API_PORT=8080
PUBLIC_URL=https://node2.anotherhost.com

# Logging
RUST_LOG=info
EOF

chmod 600 .env

# 9. Build Docker image and run Node 2
docker build -f Dockerfile.production -t fabstir-llm-node .

docker run -d \
  --name fabstir-node-2 \
  --gpus all \
  --restart unless-stopped \
  -p 9000:9000 \
  -p 8080:8080 \
  -v $(pwd)/models:/app/models \
  --env-file .env \
  --log-opt max-size=10m \
  --log-opt max-file=3 \
  fabstir-llm-node

# 10. Configure firewall (same as Node 1)
ufw allow 22/tcp
ufw allow 9000/tcp
ufw allow 8080/tcp
ufw enable

# 11. Register Node 2 on blockchain (with Host Provider B's wallet)
docker exec -it fabstir-node-2 /usr/local/bin/fabstir-llm-node register-node \
  --chain 84532 \
  --name "Node 2" \
  --api-url "https://node2.anotherhost.com:8080" \
  --models "0x<tinyllama-model-hash>" \
  --performance-tier standard

# 12. Verify both nodes are registered
docker exec -it fabstir-node-2 /usr/local/bin/fabstir-llm-node registration-status --chain 84532
```

**Key Points**:
- Node 2 has **NO KNOWLEDGE** of Node 1
- They are completely independent (different wallets, different servers)
- Both register to the same blockchain contracts
- Clients discover both via `NodeRegistry` contract
- Each earns to their own wallet address
- No communication between nodes - only client-to-node connections

---

#### Step 3: Deploy Frontend UI (fabstir-llm-ui3)

**Option A: Vercel (Recommended - Easiest)**

```bash
# 1. Clone UI repository
git clone https://github.com/fabstir/fabstir-llm-ui3.git
cd fabstir-llm-ui3

# 2. Install Vercel CLI
npm install -g vercel

# 3. Create .env.local file
cat > .env.local << EOF
NEXT_PUBLIC_RPC_URL=https://sepolia.base.org
NEXT_PUBLIC_CHAIN_ID=84532
NEXT_PUBLIC_CONTRACT_JOB_MARKETPLACE=<from-.env.test>
NEXT_PUBLIC_CONTRACT_NODE_REGISTRY=<from-.env.test>
NEXT_PUBLIC_CONTRACT_USDC_TOKEN=<from-.env.test>
NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID=<your-walletconnect-project-id>
NEXT_PUBLIC_APP_NAME="Platformless AI"
EOF

# 4. Deploy to Vercel
vercel --prod

# 5. Configure custom domain (optional)
vercel domains add app.platformless.ai
```

**Option B: Netlify**

```bash
# 1. Clone and install dependencies
git clone https://github.com/fabstir/fabstir-llm-ui3.git
cd fabstir-llm-ui3
pnpm install

# 2. Build for production
pnpm build

# 3. Deploy to Netlify
# Via Netlify CLI:
npm install -g netlify-cli
netlify deploy --prod --dir=.next

# OR via Netlify UI:
# - Connect GitHub repo
# - Set build command: pnpm build
# - Set publish directory: .next
# - Add environment variables from .env.local
```

**Option C: Self-Host with Docker (Vultr)**

```bash
# 1. Provision a standard Vultr instance (no GPU needed)
vultr-cli instance create \
  --region ewr \
  --plan vc2-2c-4gb \
  --os "Ubuntu 22.04 LTS" \
  --label "fabstir-ui" \
  --firewall-group-id <your-firewall-id>

# 2. SSH into instance
ssh root@<ui-instance-ip>

# 3. Install Docker
curl -fsSL https://get.docker.com -o get-docker.sh
sh get-docker.sh

# 4. Clone UI repo
git clone https://github.com/fabstir/fabstir-llm-ui3.git
cd fabstir-llm-ui3

# 5. Create .env.production file
cat > .env.production << EOF
NEXT_PUBLIC_RPC_URL=https://sepolia.base.org
NEXT_PUBLIC_CHAIN_ID=84532
NEXT_PUBLIC_CONTRACT_JOB_MARKETPLACE=<from-.env.test>
NEXT_PUBLIC_CONTRACT_NODE_REGISTRY=<from-.env.test>
NEXT_PUBLIC_CONTRACT_USDC_TOKEN=<from-.env.test>
NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID=<your-walletconnect-project-id>
EOF

# 6. Build and run with Docker
docker build -t fabstir-ui .
docker run -d \
  --name fabstir-ui \
  --restart unless-stopped \
  -p 3000:3000 \
  --env-file .env.production \
  fabstir-ui:latest

# 7. Configure Nginx reverse proxy with SSL
apt install nginx certbot python3-certbot-nginx

cat > /etc/nginx/sites-available/fabstir-ui << EOF
server {
    server_name app.platformless.ai;

    location / {
        proxy_pass http://localhost:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade \$http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host \$host;
        proxy_cache_bypass \$http_upgrade;
    }
}
EOF

ln -s /etc/nginx/sites-available/fabstir-ui /etc/nginx/sites-enabled/
nginx -t
systemctl restart nginx

# 8. Get SSL certificate
certbot --nginx -d app.platformless.ai
```

**Test Frontend UI**:
```bash
# Visit https://app.platformless.ai
# Should show:
# 1. Wallet connection button (MetaMask, WalletConnect, Coinbase)
# 2. List of available nodes (Node 1 and Node 2)
# 3. Chat interface for inference
# 4. Balance and deposit sections
```

---

#### Step 4: Configure DNS and SSL

**DNS Setup** (Cloudflare, Route53, etc.):
```
# Node 1 (Host Provider A)
node1.yourplatform.ai       →  A record  →  <gpu-node-1-ip>

# Node 2 (Host Provider B - different domain)
node2.anotherhost.com       →  A record  →  <gpu-node-2-ip>

# Frontend UI (optional)
app.platformless.ai         →  A record  →  <ui-ip> (if self-hosted)
                            →  CNAME     →  vercel-alias (if Vercel)
```

**SSL Certificates** (Let's Encrypt via Certbot):
```bash
# On Node 1 VM
certbot --nginx -d node1.yourplatform.ai

# On Node 2 VM
certbot --nginx -d node2.anotherhost.com

# On UI VM (if self-hosted)
certbot --nginx -d app.platformless.ai

# Auto-renews every 90 days
```

---

### Scaling Strategies

#### Horizontal Scaling (Multiple Nodes)

**Benefits**:
- Handle more concurrent sessions
- Geographic distribution (lower latency)
- Redundancy (failover)

**Implementation**:
```bash
# Deploy nodes in multiple regions
# Region 1: us-east (Vultr EWR)
# Region 2: eu-west (Vultr AMS)
# Region 3: asia-pac (Vultr SYD)

# Each node registers independently on blockchain
# SDK automatically discovers all nodes via NodeRegistry
```

#### Vertical Scaling (More Nodes)

Since we're using A16 GPUs exclusively, scaling is achieved by adding more independent nodes rather than upgrading to larger GPUs:

1. **Small deployment**: 1-2 A16 nodes (what this guide covers)
2. **Medium deployment**: 5-10 A16 nodes across multiple regions
3. **Large deployment**: 20+ A16 nodes for high availability and geographic distribution

Each additional node increases total capacity and provides redundancy.

#### Auto-Scaling Considerations

**When to scale UP**:
- CPU/GPU utilization > 80% sustained
- Queue depth > 10 requests
- Response latency > 5 seconds

**When to scale DOWN**:
- CPU/GPU utilization < 20% sustained
- No active sessions for 15+ minutes
- Consider shutting down expensive instances

**Cost Optimization**:
- Use spot/preemptible instances for dev/test
- Reserved instances for production (save 30-50%)
- Monitor token usage to optimize model selection

---

## Monitoring & Operations

### Health Checks

**GPU Node**:
```bash
# Endpoint: GET /health
curl https://node1.yourplatform.ai/health

# Expected response:
{
  "status": "healthy",
  "gpu": {
    "available": true,
    "model": "NVIDIA A16",
    "vram_total": 2048,
    "vram_used": 1024,
    "temperature": 55
  },
  "model_loaded": true,
  "active_sessions": 1
}
```

### Metrics to Monitor

**System Metrics** (monitor locally with nvidia-smi, htop):
- CPU utilization (target: 50-70%)
- RAM usage (alert if > 90%)
- GPU utilization (target: 70-90% under load)
- GPU temperature (alert if > 85°C)
- Network bandwidth (in/out)
- Disk I/O and space

```bash
# Monitor GPU in real-time
watch -n 1 nvidia-smi

# Monitor system resources
htop
```

**Application Metrics**:
- Active WebSocket connections
- Request queue depth
- Average inference time per token
- Token throughput (tokens/second)
- Error rate (target: < 1%)
- Failed wallet signature verifications

**Blockchain Metrics**:
- Node stake amount (ensure > minimum)
- Earnings accumulated (verify proof submissions)
- Failed proof submissions (investigate if > 5%)
- Gas prices (optimize transaction timing)

### Logging

**Local Log Management** (no subscription services):

```bash
# Configure Docker logging driver with rotation
docker run -d \
  --log-driver=json-file \
  --log-opt max-size=10m \
  --log-opt max-file=3 \
  fabstir-node

# View logs
docker logs -f fabstir-node

# Rotate logs manually if needed
docker logs fabstir-node > /var/log/fabstir-node-$(date +%Y%m%d).log
```

**Log Levels**:
- `ERROR`: Failed operations, crashes
- `WARN`: Retries, degraded performance
- `INFO`: Session start/end, payments
- `DEBUG`: Detailed operation logs (dev only)

### Alerting Rules

**Critical Alerts** (PagerDuty, Opsgenie):
- GPU node down (no heartbeat for 5 min)
- GPU temperature > 90°C
- Out of memory (OOM kill)
- Blockchain sync lag > 100 blocks
- Node unstaked/deregistered from blockchain

**Warning Alerts** (Email, Slack):
- High CPU/GPU utilization (> 90% for 10 min)
- High error rate (> 5% for 5 min)
- Low disk space (< 10% free)
- SSL certificate expiring (< 30 days)
- Earnings not increasing (possible payment issue)

### Backup & Recovery

**GPU Node**:
- **Model weights**: Store in S3/GCS (download on startup)
- **Configuration**: Version control .env templates
- **State**: Stateless - no backup needed
- **Private key**: Store in HSM/Vault (NEVER in plaintext)

**Recovery Time Objective (RTO)**:
- GPU node: < 30 minutes (provision GPU + download model)
- Frontend UI: < 5 minutes (Vercel auto-redeploys on push)
- Client SDK: N/A (library, not deployed)

### Cost Optimization

**GPU Node Costs** (Vultr A16):
- Instance: $0.059/GPU/hour = ~$42.50/month per node (720 hours)
- Storage: Included in instance price (50GB)
- Bandwidth: Included in instance price
- **Total**: ~$42.50/month per node

**Multi-Node Deployment Costs**:
- 2 nodes (this guide): ~$85/month
- 5 nodes: ~$212.50/month
- 10 nodes: ~$425/month

**Ways to Reduce Costs**:
1. Use smaller quantized models (3B-7B GGUF) → better fit for A16
2. Batch requests → higher throughput per node
3. Auto-shutdown nodes during low usage periods
4. Monitor utilization and only run nodes when needed
5. Use efficient model formats (GGUF Q4_K_M quantization)

---

## Security Best Practices

### Network Security

```bash
# Firewall configuration (ufw example)
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp   # SSH (restrict to your IP)
ufw allow 8080/tcp # WebSocket API
ufw enable
```

### Secrets Management

**Never hardcode**:
- Private keys
- JWT secrets
- API keys
- Database passwords

**Use**:
- HashiCorp Vault
- Environment variables (for non-sensitive config)
- Encrypted `.env` files with restricted permissions (chmod 600)

### SSL/TLS

- Use Let's Encrypt for free SSL certificates
- Enable HSTS (HTTP Strict Transport Security)
- Use TLS 1.2+ only (disable TLS 1.0/1.1)
- Configure strong cipher suites

### DDoS Protection

- Use Cloudflare for auth service (free tier)
- Rate limiting at nginx/load balancer level
- WebSocket connection limits (max 100/IP)
- IP blacklisting for abusive clients

---

## Troubleshooting

### Common Issues

**"GPU out of memory"**:
```bash
# Check VRAM usage
nvidia-smi

# Solution 1: Reduce batch size
# Solution 2: Use smaller model
# Solution 3: Increase GPU VRAM (upgrade instance)
```

**"WebSocket connection refused"**:
```bash
# Check if node is running
docker ps

# Check firewall
ufw status

# Check logs
docker logs fabstir-node

# Solution: Ensure port 8080 is open and service is running
```

**"Authentication failed"** (wallet signature verification):
```bash
# Check node logs
docker logs fabstir-node

# Verify wallet signature is valid
# Client must sign challenge with correct private key

# Solution: Ensure client is using correct wallet and signing challenge properly
```

**"Transaction reverted"**:
```bash
# Check stake amount
# Check contract addresses in .env
# Check RPC_URL is correct
# Check wallet has enough ETH for gas

# Solution: Verify blockchain configuration and wallet balance
```

---

## Support & Resources

- **Documentation**: https://docs.platformless.ai
- **Discord**: https://discord.gg/fabstir
- **GitHub Issues**: https://github.com/fabstir/fabstir-llm-sdk/issues
- **Email Support**: devops@fabstir.com

---

## Appendix: Quick Reference

### Environment Variables Summary

| Service | Variable | Purpose | Example |
|---------|----------|---------|---------|
| Node | `HOST_PRIVATE_KEY` | Ethereum wallet (encryption, settlements) | `0x123...` |
| Node | `RPC_URL` | Blockchain RPC endpoint | `https://sepolia.base.org` |
| Node | `CHAIN_ID` | Blockchain chain ID | `84532` |
| Node | `MODEL_PATH` | GGUF model file path | `/app/models/tiny-vicuna-1b.q4_k_m.gguf` |
| Node | `P2P_PORT` | libp2p DHT port | `9000` |
| Node | `API_PORT` | WebSocket API port | `8080` |
| Node | `PUBLIC_URL` | Node's public URL | `https://node1.example.com` |
| Node | `GPU_LAYERS` | GPU acceleration layers | `35` |
| Node | `NODE_REGISTRY_FAB_ADDRESS` | NodeRegistry contract | `0xDFFDecDfa0CF5D6cbE299711C7e4559eB16F42D6` |
| Node | `JOB_MARKETPLACE_FAB_WITH_S5_ADDRESS` | JobMarketplace contract | `0xc6D44D7f2DfA8fdbb1614a8b6675c78D3cfA376E` |
| Node | `RUST_LOG` | Logging level | `info`, `debug`, `warn`, `error` |
| Node | `SESSION_KEY_TTL_SECONDS` | Encryption key expiration | `3600` (1 hour) |
| UI | `NEXT_PUBLIC_RPC_URL` | Blockchain RPC for client | `https://sepolia.base.org` |
| UI | `NEXT_PUBLIC_CHAIN_ID` | Chain ID | `84532` (Base Sepolia) |
| UI | `NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID` | WalletConnect Project ID | `abc123...` |

**Note**: Do NOT use `NODE_PRIVATE_KEY`, `AUTH_SERVICE_URL`, `ENABLE_PROOFS`, or `S5_SEED_PHRASE` - these are deprecated or non-existent.

### Port Reference

| Service | Port | Protocol | Purpose |
|---------|------|----------|---------|
| Node | 9000 | TCP | P2P networking (libp2p DHT) |
| Node | 8080 | WebSocket/WSS | Inference API |
| Frontend UI | 3000 | HTTP/HTTPS | Web app (if self-hosted) |
| Blockchain | 443 | HTTPS | RPC calls |
| SSH | 22 | SSH | Remote access (restrict to your IP!) |

### Vultr GPU Instance Specs

**Deployment Configuration:**
- Instance Type: `vgp-a16-8gb-50gb`
- GPU: NVIDIA A16 with CUDA 12.2+
- System RAM: 8GB
- Storage: 50GB SSD
- Price: $0.059/GPU/hour (~$42.50/month per node)
- Suitable for: Small models (1-7B parameters with GGUF Q4_K_M quantization)
- OS: Ubuntu 22.04 LTS

**Software Requirements:**
- Rust nightly toolchain
- Docker + nvidia-docker2
- NVIDIA drivers (automatically installed via ubuntu-drivers)

---

**Last Updated**: 2025-01-21
**Version**: 2.0 (Major update - corrected technology stack and deployment procedures)
**License**: BUSL-1.1
