# Troubleshooting Guide

This guide helps diagnose and resolve common issues with the Fabstir LLM Node.

## Quick Diagnostics

Run the diagnostic script to check system status:

```bash
# Check all systems
cargo run --bin fabstir-cli -- diagnose

# Check specific component
cargo run --bin fabstir-cli -- diagnose --component p2p
cargo run --bin fabstir-cli -- diagnose --component chain
cargo run --bin fabstir-cli -- diagnose --component inference
```

## Common Issues

### 1. Node Won't Start

#### Symptoms
- Node fails to start
- Process exits immediately
- No logs generated

#### Causes & Solutions

**Port Already in Use**
```bash
# Check if ports are in use
netstat -tulpn | grep -E '(9000|8080)'

# Solution: Use different ports
P2P_PORT=9001 API_PORT=8081 cargo run --release
```

**Missing Configuration**
```bash
# Check required files exist
ls -la .env .env.contracts

# Solution: Copy from examples
cp .env.example .env
cp .env.contracts.example .env.contracts
```

**Insufficient Permissions**
```bash
# Check file ownership
ls -la /opt/fabstir-node

# Solution: Fix permissions
sudo chown -R $USER:$USER /opt/fabstir-node
```

### 2. Chain Connection Issues

#### Symptoms
- "Failed to connect to chain" errors
- RPC timeout messages
- Transaction failures

#### Causes & Solutions

**Invalid RPC Endpoint**
```bash
# Test RPC connection
curl -X POST $BASE_SEPOLIA_RPC \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Solution: Update RPC URL
export BASE_SEPOLIA_RPC=https://sepolia.base.org
```

**Rate Limiting**
```bash
# Symptoms in logs
ERROR fabstir_llm_node::contracts: RPC rate limit exceeded

# Solution: Use authenticated RPC or reduce request frequency
export RPC_REQUEST_DELAY_MS=1000
```

**Wrong Chain ID**
```bash
# Verify chain ID
curl -X POST $RPC_URL \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'

# Solution: Ensure correct chain ID in config
export CHAIN_ID=84532  # For Base Sepolia
```

### 3. Registration Failures

#### Symptoms
- "Registration failed" error
- Transaction reverts
- Node not appearing in registry

#### Causes & Solutions

**Insufficient Balance**
```bash
# Check wallet balance
cast balance $HOST_ADDRESS --rpc-url $RPC_URL

# Solution: Fund wallet from faucet
# Base Sepolia: https://www.coinbase.com/faucets
# opBNB Testnet: https://www.bnbchain.org/en/testnet-faucet
```

**Already Registered**
```bash
# Check registration status
cargo run --bin fabstir-cli -- status \
  --chain-id 84532 \
  --host-address $HOST_ADDRESS

# Solution: Update registration instead
cargo run --bin fabstir-cli -- update-registration ...
```

**Invalid Model IDs**
```bash
# Error: "Model not in registry"

# Solution: Use valid model IDs
cargo run --bin fabstir-cli -- list-models --chain-id 84532
```

### 4. WebSocket Connection Issues

#### Symptoms
- WebSocket disconnections
- "Connection refused" errors
- Session timeout issues

#### Causes & Solutions

**Firewall Blocking**
```bash
# Check firewall rules
sudo ufw status

# Solution: Allow WebSocket port
sudo ufw allow 8080/tcp
```

**Proxy Configuration**
```nginx
# Nginx configuration for WebSocket
location /ws {
    proxy_pass http://localhost:8080/ws;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_read_timeout 3600s;  # Important!
}
```

**Session Limits**
```bash
# Error: "Max sessions reached"

# Solution: Increase limits
export MAX_WEBSOCKET_SESSIONS=1000
export SESSION_TIMEOUT_MS=3600000
```

### 5. Inference Issues

#### Symptoms
- Model loading failures
- Out of memory errors
- Slow inference

#### Causes & Solutions

**Model Not Found**
```bash
# Check model exists
ls -la models/

# Solution: Download model
wget -P models/ https://url-to-model/model.gguf
```

**GPU Not Available**
```bash
# Check GPU status
nvidia-smi

# Solution: Install CUDA drivers
sudo apt install nvidia-driver-470 cuda-toolkit-11-4
```

**Memory Issues**
```bash
# Monitor memory usage
watch -n 1 free -h

# Solution: Reduce model size or batch size
export MAX_CONTEXT_LENGTH=2048
export BATCH_SIZE=1
```

### 6. Settlement Problems

#### Symptoms
- Payments not received
- Settlement transactions failing
- Incorrect payment amounts

#### Causes & Solutions

**Gas Too Low**
```bash
# Check gas price
cast gas-price --rpc-url $RPC_URL

# Solution: Increase gas settings
export MAX_GAS_PRICE=20000000000  # 20 Gwei
export GAS_MULTIPLIER=1.2
```

**Nonce Issues**
```bash
# Reset nonce
cast nonce $HOST_ADDRESS --rpc-url $RPC_URL

# Solution: Clear pending transactions or wait
export NONCE_MANAGER=automatic
```

**Contract State Mismatch**
```bash
# Verify job state
cargo run --bin fabstir-cli -- check-job \
  --chain-id 84532 \
  --job-id 0x...

# Solution: Sync with blockchain
cargo run --bin fabstir-cli -- sync-state
```

### 7. P2P Network Issues

#### Symptoms
- No peer connections
- Discovery failures
- Message routing errors

#### Causes & Solutions

**NAT/Firewall Issues**
```bash
# Test P2P port
nc -zv external_ip 9000

# Solution: Configure port forwarding
# Router: Forward TCP 9000 to node IP
# Or use UPnP
export ENABLE_UPNP=true
```

**Bootstrap Node Issues**
```bash
# Check bootstrap nodes
grep BOOTSTRAP .env

# Solution: Add working bootstrap nodes
export BOOTSTRAP_NODES="/ip4/1.2.3.4/tcp/9000/p2p/QmPeer1,/ip4/5.6.7.8/tcp/9000/p2p/QmPeer2"
```

### 8. Storage Issues

#### Symptoms
- S5 storage errors
- Vector DB connection failures
- Cache misses

#### Causes & Solutions

**S5 Node Offline**
```bash
# Check S5 connection
curl http://localhost:5522/health

# Solution: Restart S5 service
docker-compose restart s5-server
```

**Vector DB Issues**
```bash
# Check Vector DB
curl http://localhost:8081/health

# Solution: Reset Vector DB
docker-compose down vector-db
docker-compose up -d vector-db
```

## Performance Issues

### High CPU Usage

```bash
# Profile CPU usage
perf top -p $(pgrep fabstir)

# Solutions:
# - Reduce worker threads
export NUM_WORKERS=4
# - Enable rate limiting
export ENABLE_RATE_LIMITING=true
```

### High Memory Usage

```bash
# Check memory breakdown
pmap -x $(pgrep fabstir)

# Solutions:
# - Clear caches
cargo run --bin fabstir-cli -- clear-cache
# - Reduce cache sizes
export MODEL_CACHE_SIZE=2
export RESULT_CACHE_SIZE=100
```

### Slow Inference

```bash
# Benchmark inference
cargo run --bin fabstir-cli -- benchmark

# Solutions:
# - Use quantized models (Q4_K_M)
# - Enable GPU acceleration
export CUDA_VISIBLE_DEVICES=0
# - Reduce batch size
export INFERENCE_BATCH_SIZE=1
```

## Debug Commands

### Enable Debug Logging

```bash
# Full debug mode
RUST_LOG=debug cargo run

# Module-specific debugging
RUST_LOG=fabstir_llm_node::contracts=trace cargo run
RUST_LOG=fabstir_llm_node::p2p=debug cargo run
RUST_LOG=fabstir_llm_node::inference=trace cargo run
```

### Test Specific Components

```bash
# Test P2P connectivity
cargo test --test p2p_tests

# Test chain connection
cargo test --test contracts_tests

# Test WebSocket
cargo test --test websocket_tests

# Test inference
cargo test --test inference_tests
```

### Check System Resources

```bash
# Overall system check
htop

# Network connections
ss -tuln

# Disk usage
df -h

# Process details
ps aux | grep fabstir
```

## Log Analysis

### Important Log Patterns

```bash
# Find errors
grep ERROR /var/log/fabstir/node.log

# Find chain issues
grep -E "(chain|rpc|contract)" /var/log/fabstir/node.log

# Find settlement events
grep -E "(settlement|payment|escrow)" /var/log/fabstir/node.log

# Find performance warnings
grep WARN /var/log/fabstir/node.log | grep -E "(slow|timeout|memory)"
```

### Log Rotation

```bash
# Configure logrotate
cat > /etc/logrotate.d/fabstir << EOF
/var/log/fabstir/*.log {
    daily
    rotate 7
    compress
    missingok
    notifempty
    create 644 fabstir fabstir
    postrotate
        systemctl reload fabstir-node
    endscript
}
EOF
```

## Recovery Procedures

### Emergency Shutdown

```bash
# Graceful shutdown
systemctl stop fabstir-node

# Force kill if needed
pkill -9 fabstir
```

### Data Recovery

```bash
# Backup critical data
tar -czf backup.tar.gz \
  /opt/fabstir-node/data \
  /opt/fabstir-node/.env*

# Restore from backup
tar -xzf backup.tar.gz -C /
```

### State Reset

```bash
# Clear all caches
rm -rf /opt/fabstir-node/data/cache/*

# Reset P2P identity (new peer ID)
rm -rf /opt/fabstir-node/data/identity

# Resync with blockchain
cargo run --bin fabstir-cli -- sync --full
```

## Getting Help

If issues persist:

1. **Check Documentation**
   - [Configuration Guide](MULTI_CHAIN_CONFIG.md)
   - [Deployment Guide](DEPLOYMENT.md)
   - [API Documentation](API.md)

2. **Gather Information**
   ```bash
   # Create diagnostic report
   cargo run --bin fabstir-cli -- diagnose --output report.txt
   ```

3. **Contact Support**
   - GitHub Issues: Include diagnostic report
   - Discord: #node-operators channel
   - Email: support@fabstir.com

## Common Error Codes

| Code | Description | Solution |
|------|-------------|----------|
| E001 | Port binding failed | Change port or kill conflicting process |
| E002 | RPC connection failed | Check RPC URL and network |
| E003 | Registration failed | Check balance and gas |
| E004 | Model load failed | Verify model file exists |
| E005 | GPU init failed | Install CUDA drivers |
| E006 | Session limit reached | Increase max sessions |
| E007 | Settlement failed | Check gas and nonce |
| E008 | P2P discovery failed | Check NAT/firewall |
| E009 | Storage unreachable | Restart storage services |
| E010 | Invalid configuration | Fix configuration file |