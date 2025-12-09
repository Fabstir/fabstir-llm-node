# Deployment Guide

This guide covers deploying the Fabstir LLM Node in production environments with multi-chain support.

## Prerequisites

- Linux server (Ubuntu 20.04+ or similar)
- Docker and Docker Compose installed
- CUDA-capable GPU (optional but recommended)
- At least 16GB RAM
- 100GB+ SSD storage
- Stable internet connection
- Wallet with native tokens (ETH for Base Sepolia, BNB for opBNB)

## Deployment Options

### Option 1: Docker Deployment (Recommended)

#### 1. Clone Repository

```bash
git clone https://github.com/fabstir/fabstir-llm-node.git
cd fabstir-llm-node
```

#### 2. Configure Environment

Create production configuration files:

```bash
# Copy example configurations
cp .env.example .env
cp .env.contracts.example .env.contracts

# Edit with your settings
nano .env
```

**Important: Configure Encryption** (Recommended for Production)

```bash
# Generate or use existing Ethereum private key
# For testing:
openssl rand -hex 32 | sed 's/^/0x/' > .host_key

# For production: Use existing wallet or HSM

# Set private key (DO NOT commit to git)
export HOST_PRIVATE_KEY=0x1234567890abcdef...  # 66 characters (0x + 64 hex)

# Add to .env (gitignored)
echo "HOST_PRIVATE_KEY=$HOST_PRIVATE_KEY" >> .env

# Verify key format
echo $HOST_PRIVATE_KEY | wc -c  # Should be 67 (66 + newline)

# Configure session TTL (optional, default: 3600 seconds)
export SESSION_KEY_TTL_SECONDS=3600
echo "SESSION_KEY_TTL_SECONDS=3600" >> .env
```

**Security Note**:
- ✅ Keep `HOST_PRIVATE_KEY` in secrets management (Kubernetes secrets, AWS Secrets Manager, etc.)
- ✅ Use different keys for production and testing
- ✅ Never commit `.env` to version control
- ✅ Rotate keys quarterly
- ❌ Never log or print private keys

See `docs/ENCRYPTION_SECURITY.md` for comprehensive security guide.

#### 3. Build Docker Image

```bash
# Build with CUDA support
docker build -f Dockerfile.cuda -t fabstir-node:latest .

# Or build without CUDA
docker build -t fabstir-node:latest .
```

#### 4. Run with Docker Compose

```bash
# Start all services
docker-compose -f docker-compose.prod.yml up -d

# Check status
docker-compose ps

# View logs
docker-compose logs -f fabstir-node
```

### Option 2: Binary Deployment

#### 1. Build from Source

```bash
# Clone and build
git clone https://github.com/fabstir/fabstir-llm-node.git
cd fabstir-llm-node
cargo build --release

# Binary location
ls -la target/release/fabstir-llm-node
```

#### 2. Create Service File

```bash
sudo nano /etc/systemd/system/fabstir-node.service
```

```ini
[Unit]
Description=Fabstir LLM Node
After=network.target

[Service]
Type=simple
User=fabstir
Group=fabstir
WorkingDirectory=/opt/fabstir-node
Environment="RUST_LOG=info"
Environment="P2P_PORT=9000"
Environment="API_PORT=8080"
# Encryption configuration (load from secrets file)
EnvironmentFile=/opt/fabstir-node/.env.secrets
ExecStart=/opt/fabstir-node/fabstir-llm-node
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

#### 3. Start Service

```bash
# Reload systemd
sudo systemctl daemon-reload

# Enable and start service
sudo systemctl enable fabstir-node
sudo systemctl start fabstir-node

# Check status
sudo systemctl status fabstir-node
```

### Option 3: Kubernetes Deployment

#### 1. Create Secrets (for Encryption)

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: fabstir-secrets
type: Opaque
stringData:
  HOST_PRIVATE_KEY: "0x1234567890abcdef..."  # Your private key
```

**Create secret from file**:
```bash
# Store key in file (temporary)
echo "0x1234567890abcdef..." > host_key.txt

# Create Kubernetes secret
kubectl create secret generic fabstir-secrets \
  --from-file=HOST_PRIVATE_KEY=host_key.txt

# Delete temporary file
shred -u host_key.txt
```

#### 2. Create ConfigMap

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: fabstir-config
data:
  P2P_PORT: "9000"
  API_PORT: "8080"
  DEFAULT_CHAIN_ID: "84532"
  BASE_SEPOLIA_RPC: "https://sepolia.base.org"
  OPBNB_TESTNET_RPC: "https://opbnb-testnet-rpc.bnbchain.org"
  SESSION_KEY_TTL_SECONDS: "3600"  # 1 hour
```

#### 3. Create Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: fabstir-node
spec:
  replicas: 1
  selector:
    matchLabels:
      app: fabstir-node
  template:
    metadata:
      labels:
        app: fabstir-node
    spec:
      containers:
      - name: fabstir-node
        image: fabstir/llm-node:latest
        ports:
        - containerPort: 9000
          name: p2p
        - containerPort: 8080
          name: api
        envFrom:
        - configMapRef:
            name: fabstir-config
        - secretRef:
            name: fabstir-secrets  # Include encryption secrets
        resources:
          requests:
            memory: "8Gi"
            cpu: "2"
            nvidia.com/gpu: 1  # If GPU available
          limits:
            memory: "16Gi"
            cpu: "4"
            nvidia.com/gpu: 1
```

## Production Configuration

### 1. Network Setup

Configure firewall rules:

```bash
# Allow P2P port
sudo ufw allow 9000/tcp

# Allow API port
sudo ufw allow 8080/tcp

# Allow SSH (if needed)
sudo ufw allow 22/tcp

# Enable firewall
sudo ufw enable
```

### 2. SSL/TLS Setup

Use nginx as reverse proxy with SSL:

```nginx
server {
    listen 443 ssl http2;
    server_name node.yourdomain.com;

    ssl_certificate /etc/ssl/certs/your-cert.pem;
    ssl_certificate_key /etc/ssl/private/your-key.pem;

    location / {
        proxy_pass http://localhost:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }

    location /ws {
        proxy_pass http://localhost:8080/ws;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 3600s;
    }
}
```

### 3. Node Registration

Register on each chain:

```bash
# Register on Base Sepolia
./fabstir-cli register \
  --chain-id 84532 \
  --host-address $HOST_ADDRESS \
  --private-key $HOST_PRIVATE_KEY \
  --model-ids "llama-3-8b,mistral-7b" \
  --hourly-rate 100000000000000000

# Register on opBNB Testnet
./fabstir-cli register \
  --chain-id 5611 \
  --host-address $HOST_ADDRESS \
  --private-key $HOST_PRIVATE_KEY \
  --model-ids "llama-3-8b,mistral-7b" \
  --hourly-rate 50000000000000000
```

### 4. Model Setup

Download and configure models:

```bash
# Create models directory
mkdir -p /opt/fabstir-node/models

# Download models
cd /opt/fabstir-node/models
wget https://huggingface.co/TheBloke/Llama-2-7B-GGUF/resolve/main/llama-2-7b.Q4_K_M.gguf

# Set permissions
chown -R fabstir:fabstir /opt/fabstir-node/models
```

#### Inference Configuration

Configure inference parameters in your `.env` file:

```bash
# Batch size for inference (must be >= max prompt length)
LLAMA_BATCH_SIZE=2048  # Default: 2048

# GPU Configuration Examples:
# High VRAM (24GB+):  LLAMA_BATCH_SIZE=4096
# Medium VRAM (8-16GB): LLAMA_BATCH_SIZE=2048 (default)
# Low VRAM (4-8GB):   LLAMA_BATCH_SIZE=1024

# Model path (optional, defaults to ./models/)
MODEL_PATH=/opt/fabstir-node/models/llama-2-7b.Q4_K_M.gguf

# GPU selection (optional)
CUDA_VISIBLE_DEVICES=0  # Use first GPU
```

**LLAMA_BATCH_SIZE Guidelines**:
- **Too Small**: Causes `InsufficientSpace` errors if prompt exceeds batch size
- **Too Large**: Increases VRAM usage, may cause OOM on smaller GPUs
- **Recommendation**: Set to 2x your expected max prompt length
- **Minimum**: 512 (for short prompts only)
- **Maximum**: Limited by GPU VRAM (4096+ on high-end GPUs)

### 5. Embedding Model Setup (Optional - Zero-Cost Embeddings)

The node supports host-side embedding generation using ONNX models, providing zero-cost embeddings as an alternative to external APIs (OpenAI, Cohere).

#### Download Embedding Models

```bash
# Automatic download (recommended)
cd /opt/fabstir-node
./scripts/download_embedding_model.sh

# Or manual download
mkdir -p /opt/fabstir-node/models/all-MiniLM-L6-v2-onnx
cd /opt/fabstir-node/models/all-MiniLM-L6-v2-onnx

# Download ONNX model (~90MB)
wget -O model.onnx \
  "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/refs%2Fpr%2F21/onnx/model.onnx"

# Download tokenizer (~500KB)
wget -O tokenizer.json \
  "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/raw/refs%2Fpr%2F21/tokenizer.json"

# Set permissions
chown -R fabstir:fabstir /opt/fabstir-node/models/all-MiniLM-L6-v2-onnx
```

#### Configure Environment Variables

Add to your `.env` file:

```bash
# Embedding model configuration (optional)
EMBEDDING_MODEL_PATH=/opt/fabstir-node/models/all-MiniLM-L6-v2-onnx/model.onnx
EMBEDDING_TOKENIZER_PATH=/opt/fabstir-node/models/all-MiniLM-L6-v2-onnx/tokenizer.json
EMBEDDING_DIMENSIONS=384  # Required for vector DB compatibility

# Enable/disable embedding endpoint
ENABLE_EMBEDDINGS=true    # Default: true if models found, false otherwise
```

#### Verify Installation

```bash
# Test embedding generation
curl -X POST http://localhost:8080/v1/embed \
  -H "Content-Type: application/json" \
  -d '{
    "texts": ["Test embedding generation"]
  }'

# Should return 384-dimensional embedding
# Response includes: embeddings, model, provider:"host", cost:0.0

# List available embedding models
curl "http://localhost:8080/v1/models?type=embedding"
```

#### Memory Requirements

**Embedding models add minimal overhead**:
- **Model Size**: ~90MB (all-MiniLM-L6-v2)
- **Runtime Memory**: ~200MB during inference
- **Total Impact**: <300MB additional RAM

**Combined Requirements** (LLM + Embeddings):
- TinyLlama + Embeddings: ~1GB total
- Llama-2-7B + Embeddings: ~4.5GB total
- Recommended: Add 500MB to existing RAM requirements

#### Docker Configuration for Embeddings

Update `docker-compose.prod.yml`:

```yaml
version: '3.8'

services:
  fabstir-node:
    image: fabstir-node:latest
    container_name: fabstir-llm-node
    restart: unless-stopped
    ports:
      - "8080:8080"
      - "9000:9000"
    volumes:
      - ./models:/app/models           # Mount models directory
      - ./data:/app/data
      - ./.env:/app/.env
    environment:
      - MODEL_PATH=/app/models/llama-2-7b.Q4_K_M.gguf
      - EMBEDDING_MODEL_PATH=/app/models/all-MiniLM-L6-v2-onnx/model.onnx
      - EMBEDDING_TOKENIZER_PATH=/app/models/all-MiniLM-L6-v2-onnx/tokenizer.json
      - EMBEDDING_DIMENSIONS=384
      - ENABLE_EMBEDDINGS=true
      - CHAIN_ID=84532
      - API_PORT=8080
      - P2P_PORT=9000
      - HOST_PRIVATE_KEY=${HOST_PRIVATE_KEY}
    deploy:
      resources:
        limits:
          memory: 8G              # Increase if using large LLM models
        reservations:
          memory: 4G
          devices:
            - driver: nvidia
              count: 1
              capabilities: [gpu]
```

#### Kubernetes ConfigMap for Embeddings

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: fabstir-node-config
  namespace: fabstir
data:
  # LLM Configuration
  MODEL_PATH: "/models/llama-2-7b.Q4_K_M.gguf"
  CHAIN_ID: "84532"
  API_PORT: "8080"
  P2P_PORT: "9000"

  # Embedding Configuration (NEW)
  EMBEDDING_MODEL_PATH: "/models/all-MiniLM-L6-v2-onnx/model.onnx"
  EMBEDDING_TOKENIZER_PATH: "/models/all-MiniLM-L6-v2-onnx/tokenizer.json"
  EMBEDDING_DIMENSIONS: "384"
  ENABLE_EMBEDDINGS: "true"

  # Blockchain Configuration (v8.4.21+ with PRICE_PRECISION=1000)
  RPC_URL: "https://sepolia.base.org"
  JOB_MARKETPLACE_ADDRESS: "0x0c942eADAF86855F69Ee4fa7f765bc6466f254A1"
  NODE_REGISTRY_ADDRESS: "0x48aa4A8047A45862Da8412FAB71ef66C17c7766d"

---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: fabstir-models-pvc
  namespace: fabstir
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 50Gi  # Increase for LLM + embedding models
  storageClassName: fast-ssd

---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: fabstir-node
  namespace: fabstir
spec:
  replicas: 1
  selector:
    matchLabels:
      app: fabstir-node
  template:
    metadata:
      labels:
        app: fabstir-node
    spec:
      containers:
      - name: fabstir-node
        image: fabstir-node:latest
        ports:
        - containerPort: 8080
          name: http
        - containerPort: 9000
          name: p2p
        envFrom:
        - configMapRef:
            name: fabstir-node-config
        - secretRef:
            name: fabstir-node-secrets  # Contains HOST_PRIVATE_KEY
        volumeMounts:
        - name: models
          mountPath: /models
          readOnly: true  # Models are read-only
        resources:
          requests:
            memory: "4Gi"
            cpu: "2"
          limits:
            memory: "8Gi"
            cpu: "4"
            nvidia.com/gpu: 1  # Optional GPU support
      volumes:
      - name: models
        persistentVolumeClaim:
          claimName: fabstir-models-pvc
```

#### Production Deployment Checklist

**Before deploying with embeddings**:

- [ ] Download embedding models (automatic script or manual)
- [ ] Verify model files exist and have correct sizes:
  - `model.onnx`: ~90MB
  - `tokenizer.json`: ~500KB
- [ ] Set environment variables (EMBEDDING_MODEL_PATH, etc.)
- [ ] Increase memory limits if needed (+500MB)
- [ ] Test embedding endpoint with curl
- [ ] Verify 384-dimensional output
- [ ] Monitor memory usage during first requests
- [ ] Check logs for model loading confirmation

**Environment Variable Reference**:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `EMBEDDING_MODEL_PATH` | No | Auto-detect | Path to ONNX model file |
| `EMBEDDING_TOKENIZER_PATH` | No | Auto-detect | Path to tokenizer.json file |
| `EMBEDDING_DIMENSIONS` | No | 384 | Output dimensions (must be 384) |
| `ENABLE_EMBEDDINGS` | No | auto | Enable embedding endpoint (true/false/auto) |

**Auto-detection behavior**:
- If `ENABLE_EMBEDDINGS=auto` (default): Enabled if model files found in `models/all-MiniLM-L6-v2-onnx/`
- If `ENABLE_EMBEDDINGS=false`: Embedding endpoint returns 503 Service Unavailable
- If model files missing: Node starts without embedding support (graceful degradation)

## Monitoring Setup

### 1. Prometheus Configuration

```yaml
# prometheus.yml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'fabstir-node'
    static_configs:
      - targets: ['localhost:9090']
```

### 2. Grafana Dashboard

Import the provided dashboard:

```bash
# Copy dashboard
cp monitoring/grafana-dashboard.json /var/lib/grafana/dashboards/

# Restart Grafana
sudo systemctl restart grafana-server
```

### 3. Health Checks

Configure health monitoring:

```bash
# Health check script
#!/bin/bash
curl -f http://localhost:8080/health || exit 1
```

### 4. S5 Vector Loading Metrics

The node exposes Prometheus metrics for monitoring S5 vector database loading performance. These metrics are automatically collected when using host-side RAG (Sub-phase 5.4).

**Available Metrics:**

```prometheus
# Download performance
s5_download_duration_seconds (histogram)
  - Tracks S5 download latency
  - Buckets: 0.1s, 0.5s, 1.0s, 2.0s, 5.0s, 10.0s
  - Labels: none

s5_download_errors_total (counter)
  - Total number of S5 download failures
  - Labels: none

# Vector loading
s5_vectors_loaded_total (counter)
  - Total vectors loaded from S5
  - Labels: none

# Index building
vector_index_build_duration_seconds (histogram)
  - Time to build HNSW indexes from loaded vectors
  - Buckets: 0.01s, 0.05s, 0.1s, 0.5s, 1.0s, 5.0s
  - Labels: none

# Cache performance
vector_index_cache_hits_total (counter)
  - Number of index cache hits
  - Labels: none

vector_index_cache_misses_total (counter)
  - Number of index cache misses
  - Labels: none
```

**Prometheus Queries:**

```prometheus
# Average S5 download time (last 5 minutes)
rate(s5_download_duration_seconds_sum[5m]) / rate(s5_download_duration_seconds_count[5m])

# S5 error rate (last 5 minutes)
rate(s5_download_errors_total[5m])

# Cache hit rate
rate(vector_index_cache_hits_total[5m]) /
  (rate(vector_index_cache_hits_total[5m]) + rate(vector_index_cache_misses_total[5m]))

# P95 download latency
histogram_quantile(0.95, rate(s5_download_duration_seconds_bucket[5m]))

# P99 download latency
histogram_quantile(0.99, rate(s5_download_duration_seconds_bucket[5m]))

# Vectors loaded per minute
rate(s5_vectors_loaded_total[1m]) * 60
```

**Alert Rules:**

```yaml
groups:
  - name: s5_vector_loading
    rules:
      - alert: HighS5ErrorRate
        expr: rate(s5_download_errors_total[5m]) > 0.1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High S5 download error rate"
          description: "S5 download error rate is {{ $value }} errors/sec"

      - alert: SlowS5Downloads
        expr: histogram_quantile(0.95, rate(s5_download_duration_seconds_bucket[5m])) > 5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Slow S5 downloads"
          description: "P95 download latency is {{ $value }}s"

      - alert: LowCacheHitRate
        expr: |
          rate(vector_index_cache_hits_total[5m]) /
          (rate(vector_index_cache_hits_total[5m]) + rate(vector_index_cache_misses_total[5m])) < 0.5
        for: 10m
        labels:
          severity: info
        annotations:
          summary: "Low index cache hit rate"
          description: "Cache hit rate is {{ $value | humanizePercentage }}"
```

**Grafana Panels:**

```json
{
  "panels": [
    {
      "title": "S5 Download Latency (P95/P99)",
      "targets": [
        {
          "expr": "histogram_quantile(0.95, rate(s5_download_duration_seconds_bucket[5m]))",
          "legendFormat": "P95"
        },
        {
          "expr": "histogram_quantile(0.99, rate(s5_download_duration_seconds_bucket[5m]))",
          "legendFormat": "P99"
        }
      ]
    },
    {
      "title": "S5 Error Rate",
      "targets": [
        {
          "expr": "rate(s5_download_errors_total[5m])",
          "legendFormat": "Errors/sec"
        }
      ]
    },
    {
      "title": "Index Cache Hit Rate",
      "targets": [
        {
          "expr": "rate(vector_index_cache_hits_total[5m]) / (rate(vector_index_cache_hits_total[5m]) + rate(vector_index_cache_misses_total[5m]))",
          "legendFormat": "Hit Rate"
        }
      ]
    },
    {
      "title": "Vectors Loaded/Min",
      "targets": [
        {
          "expr": "rate(s5_vectors_loaded_total[1m]) * 60",
          "legendFormat": "Vectors/min"
        }
      ]
    }
  ]
}
```

**Structured Logging:**

The VectorLoader also emits structured logs for monitoring:

```log
# Loading started
INFO  🚀 Starting S5 vector database loading manifest_path="home/vector-databases/0xABC/docs/manifest.json" user_address="0xABC..."

# Manifest downloaded
INFO  📦 Manifest downloaded and decrypted manifest_name="My Docs" owner="0xABC..." dimensions=384 vector_count=10000 chunk_count=10

# Owner verification
DEBUG ✅ Owner verification passed expected_owner="0xABC..." actual_owner="0xABC..."

# Memory check
DEBUG ✅ Memory limit check passed estimated_mb=15 limit_mb=100

# Manifest download details
DEBUG 📥 Manifest downloaded from S5 path="..." duration_ms=234 size_bytes=1024

# Chunk downloads
TRACE 📥 Chunk downloaded chunk_id=1 path="..." duration_ms=567 size_bytes=102400
DEBUG ✅ Chunk processed chunk_id=1 total_chunks=10 vectors_so_far=1000

# All chunks complete
INFO  📦 All chunks downloaded and decrypted total_vectors=10000 total_chunks=10

# Loading complete
INFO  ✅ S5 vector database loading complete manifest_path="..." vector_count=10000 duration_ms=5234 chunk_count=10

# Errors
ERROR ❌ Failed to download manifest from S5 path="..." error="..."
ERROR ❌ Owner verification failed expected="0xABC..." actual="0xDEF..."
ERROR ❌ Memory limit would be exceeded required_mb=150 limit_mb=100
WARN  ⚠️  Rate limit exceeded current=10 limit=10 window_sec=60
ERROR ❌ Loading operation timed out manifest_path="..." timeout_sec=300
```

**Log Aggregation:**

Configure log shipping to aggregate structured logs:

```bash
# Fluentd configuration
<source>
  @type tail
  path /var/log/fabstir-node/*.log
  pos_file /var/log/td-agent/fabstir-node.pos
  tag fabstir.node
  <parse>
    @type json
  </parse>
</source>

<filter fabstir.node>
  @type grep
  <regexp>
    key message
    pattern /S5 vector database loading|Manifest downloaded|Chunk downloaded/
  </regexp>
</filter>

<match fabstir.node>
  @type elasticsearch
  host elasticsearch
  port 9200
  index_name fabstir-node
</match>
```

## Backup and Recovery

### 1. Data Backup

```bash
# Backup script
#!/bin/bash
BACKUP_DIR="/backups/fabstir-node"
mkdir -p $BACKUP_DIR

# Backup configuration
cp /opt/fabstir-node/.env* $BACKUP_DIR/

# Backup node identity
cp -r /opt/fabstir-node/data/identity $BACKUP_DIR/

# Backup database (if applicable)
pg_dump fabstir_node > $BACKUP_DIR/database.sql
```

### 2. Recovery Procedure

```bash
# Restore configuration
cp /backups/fabstir-node/.env* /opt/fabstir-node/

# Restore identity
cp -r /backups/fabstir-node/identity /opt/fabstir-node/data/

# Restart node
sudo systemctl restart fabstir-node
```

## Performance Tuning

### 1. System Limits

```bash
# /etc/security/limits.conf
fabstir soft nofile 65536
fabstir hard nofile 65536
fabstir soft nproc 32768
fabstir hard nproc 32768
```

### 2. Network Tuning

```bash
# /etc/sysctl.conf
net.core.rmem_max = 134217728
net.core.wmem_max = 134217728
net.ipv4.tcp_rmem = 4096 87380 134217728
net.ipv4.tcp_wmem = 4096 65536 134217728
net.core.netdev_max_backlog = 5000
```

### 3. GPU Optimization

```bash
# Set GPU persistence mode
nvidia-smi -pm 1

# Set power limit (optional)
nvidia-smi -pl 250

# Monitor GPU usage
nvidia-smi dmon -s pucvmet
```

## Security Hardening

### 1. Private Key Management

**For Encryption (HOST_PRIVATE_KEY)**:

```bash
# Production: Use secrets management
# AWS Secrets Manager
aws secretsmanager create-secret \
  --name fabstir-node-private-key \
  --secret-string "0x..."

# HashiCorp Vault
vault kv put secret/fabstir/private-key value="0x..."

# Kubernetes Secrets
kubectl create secret generic fabstir-secrets \
  --from-literal=HOST_PRIVATE_KEY="0x..."

# Azure Key Vault
az keyvault secret set \
  --vault-name fabstir-vault \
  --name host-private-key \
  --value "0x..."
```

**Key Rotation Procedure** (Recommended: Quarterly):

```bash
# 1. Generate new key
openssl rand -hex 32 | sed 's/^/0x/' > new_key.txt

# 2. Update secrets management
kubectl create secret generic fabstir-secrets-new \
  --from-file=HOST_PRIVATE_KEY=new_key.txt

# 3. Update deployment to use new secret
kubectl patch deployment fabstir-node \
  -p '{"spec":{"template":{"spec":{"containers":[{"name":"fabstir-node","envFrom":[{"secretRef":{"name":"fabstir-secrets-new"}}]}]}}}}'

# 4. Verify node restarts and loads new key
kubectl logs -f deployment/fabstir-node | grep "Private key loaded"

# 5. Delete old secret
kubectl delete secret fabstir-secrets

# 6. Securely delete temporary file
shred -u new_key.txt
```

**Best Practices**:
- ✅ Use HSM or key management services in production
- ✅ Never store private keys in plain text files
- ✅ Rotate keys quarterly or after security incidents
- ✅ Use different keys for each environment (dev/staging/prod)
- ✅ Restrict access to secrets (RBAC)
- ✅ Audit all secret access
- ❌ Never log private keys
- ❌ Never commit keys to version control
- ❌ Never share keys between nodes

**Monitoring Encryption Status**:

```bash
# Check if encryption is enabled
curl http://localhost:8080/v1/metrics/session_keys

# Expected response when encryption is enabled:
# {
#   "active_sessions": 5,
#   "total_keys_stored": 5,
#   "memory_usage_estimate_bytes": 640
# }

# If encryption is disabled, encrypted_session_init will return:
# "ENCRYPTION_NOT_SUPPORTED"
```

### 2. Access Control

```bash
# Restrict API access
iptables -A INPUT -p tcp --dport 8080 -s trusted_ip -j ACCEPT
iptables -A INPUT -p tcp --dport 8080 -j DROP
```

### 3. Audit Logging

Enable comprehensive logging:

```bash
# rsyslog configuration
:programname, isequal, "fabstir-node" /var/log/fabstir/node.log
& stop
```

## Troubleshooting Deployment

### Common Issues

1. **Port conflicts**: Use `netstat -tulpn` to check port usage
2. **Memory issues**: Monitor with `htop` or `free -h`
3. **GPU not detected**: Check with `nvidia-smi`
4. **Connection refused**: Verify firewall rules

### Debug Mode

```bash
# Run in debug mode
RUST_LOG=debug ./fabstir-llm-node

# Enable trace logging for specific modules
RUST_LOG=fabstir_llm_node::p2p=trace ./fabstir-llm-node
```

## Scaling Considerations

### Horizontal Scaling

Deploy multiple nodes behind a load balancer:

```nginx
upstream fabstir_nodes {
    least_conn;
    server node1.internal:8080;
    server node2.internal:8080;
    server node3.internal:8080;
}
```

### Vertical Scaling

- Increase CPU cores for better parallelism
- Add more RAM for model caching
- Use multiple GPUs for inference

## Maintenance

### Regular Tasks

1. **Daily**: Check logs for errors
2. **Weekly**: Update models if needed
3. **Monthly**: Security updates and patches
4. **Quarterly**: Performance review and optimization

### Update Procedure

```bash
# Stop service
sudo systemctl stop fabstir-node

# Backup current version
cp /opt/fabstir-node/fabstir-llm-node /opt/fabstir-node/fabstir-llm-node.backup

# Update binary
cp target/release/fabstir-llm-node /opt/fabstir-node/

# Restart service
sudo systemctl start fabstir-node
```

## Support

For deployment issues:
- Check [Troubleshooting Guide](TROUBLESHOOTING.md)
- Review logs in `/var/log/fabstir/`
- Open an issue on GitHub
- Contact support team