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

#### 1. Create ConfigMap

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
```

#### 2. Create Deployment

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

- Use hardware security modules (HSM) or key management services
- Never store private keys in plain text
- Rotate keys regularly

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