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

### 9. Encryption Issues

#### Symptoms
- ENCRYPTION_NOT_SUPPORTED errors
- DECRYPTION_FAILED errors
- INVALID_SIGNATURE errors
- SESSION_KEY_NOT_FOUND errors

#### Causes & Solutions

**Node Missing Private Key**
```bash
# Error: "ENCRYPTION_NOT_SUPPORTED"
# Cause: HOST_PRIVATE_KEY not configured

# Check if key is set
echo $HOST_PRIVATE_KEY

# Solution: Set private key
export HOST_PRIVATE_KEY=0x1234567890abcdef...  # 66 chars (0x + 64 hex)

# Restart node
cargo run --release

# Verify in logs
# Look for: "Private key loaded successfully"
```

**Invalid Private Key Format**
```bash
# Error: "Invalid private key format"

# Check key format (must be 0x-prefixed, 64 hex characters)
echo $HOST_PRIVATE_KEY | wc -c  # Should be 67 (66 + newline)

# Solution: Fix format
# Correct: 0x followed by 64 hex characters
export HOST_PRIVATE_KEY=0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef

# Incorrect formats:
# - Missing 0x prefix
# - Too short/long
# - Non-hex characters
```

**Decryption Failures**
```bash
# Error: "DECRYPTION_FAILED"
# Possible causes:
# 1. Wrong session key
# 2. Invalid nonce size
# 3. Corrupted ciphertext
# 4. Mismatched AAD

# Debug with logs
RUST_LOG=fabstir_llm_node::crypto=debug cargo run

# Look for specific error:
# - "Invalid nonce size: expected 24, got X"
# - "Authentication tag verification failed"
# - "Invalid hex encoding"

# Client-side checklist:
# ✓ Nonce is exactly 24 bytes (XChaCha20, not ChaCha20's 12 bytes)
# ✓ Session key matches the one from session init
# ✓ AAD format is correct (e.g., "message_0")
# ✓ Ciphertext is properly hex-encoded
```

**Signature Verification Failures**
```bash
# Error: "INVALID_SIGNATURE"
# Cause: Client signature doesn't verify

# Common issues:
# 1. Wrong message hash signed
# 2. Wrong private key used
# 3. Signature format incorrect

# Verify signature format
# Should be 65 bytes: r (32) + s (32) + v (1)
# In hex: 130 characters (or 132 with 0x prefix)

# Debug client-side:
const messageHash = sha256(ciphertext);  // Hash ciphertext, not plaintext
const signature = await wallet.signMessage(messageHash);
console.log('Signature length:', signature.length);  // Should be 65 or 130 (hex)

# Node logs will show recovered address
RUST_LOG=fabstir_llm_node::crypto=debug cargo run
# Look for: "Client address recovered: 0x..."
```

**Session Key Not Found**
```bash
# Error: "SESSION_KEY_NOT_FOUND"
# Cause: Session not initialized or expired

# Check session status
curl http://localhost:8080/v1/metrics/session_keys

# Solution 1: Initialize session first
# Send encrypted_session_init before encrypted_message

# Solution 2: Check TTL settings
export SESSION_KEY_TTL_SECONDS=3600  # 1 hour

# Solution 3: Verify session_id matches
# Client must use same session_id for init and messages
```

**Invalid Nonce Size**
```bash
# Error: "INVALID_NONCE_SIZE: expected 24, got 12"
# Cause: Using ChaCha20 nonce (12 bytes) instead of XChaCha20 (24 bytes)

# Solution: Generate 24-byte nonce
const nonce = crypto.getRandomValues(new Uint8Array(24));  // ✓ Correct
const nonce = crypto.getRandomValues(new Uint8Array(12));  // ✗ Wrong
```

**Hex Encoding Errors**
```bash
# Error: "INVALID_HEX_ENCODING"
# Cause: Malformed hex strings

# Common issues:
# - Odd-length hex string
# - Non-hex characters (g-z, special chars)
# - Missing bytes

# Solution: Verify hex encoding
function bytesToHex(bytes) {
  return Array.from(bytes)
    .map(b => b.toString(16).padStart(2, '0'))
    .join('');
}

# Optional: Add 0x prefix (node strips it)
const hex = '0x' + bytesToHex(bytes);
```

**Testing Encryption Locally**
```bash
# Run security tests
cargo test --test security_tests

# Test specific scenario
cargo test test_replay_attack_prevented -- --exact

# Verify encryption roundtrip
cargo test test_encrypt_decrypt_roundtrip -- --nocapture

# Enable crypto debug logging
RUST_LOG=fabstir_llm_node::crypto=trace cargo run
```

**Session Key Metrics**
```bash
# Check active sessions
curl http://localhost:8080/v1/metrics/session_keys

# Response shows:
# - active_sessions: number of sessions with stored keys
# - total_keys_stored: total keys in memory
# - memory_usage_estimate_bytes: approximate memory used
# - expired_keys_cleaned: number of expired keys removed

# If memory usage is high, reduce TTL:
export SESSION_KEY_TTL_SECONDS=1800  # 30 minutes
```

**Client-Side Debug Checklist**
- [ ] Nonces are unique per encryption (never reused)
- [ ] Nonce size is exactly 24 bytes
- [ ] Session key matches the one from session init
- [ ] Signature is over ciphertext hash, not plaintext
- [ ] AAD format matches server expectations
- [ ] Hex encoding is correct (even length, valid chars)
- [ ] session_init_ack received before sending encrypted_message
- [ ] Same session_id used for init and messages

**Security Best Practices**
```bash
# For node operators:
# ✓ Keep HOST_PRIVATE_KEY in secrets management
# ✓ Use different keys for production and testing
# ✓ Rotate keys quarterly
# ✓ Monitor session key metrics
# ✓ Set reasonable session TTL

# For SDK developers:
# ✓ Generate new nonce for EVERY encryption
# ✓ Clear session keys on disconnect
# ✓ Validate decryption success before processing
# ✓ Use CSPRNG for nonce generation
# ✓ Never log private keys or session keys
```

**See Also**:
- `docs/ENCRYPTION_SECURITY.md` - Comprehensive security guide
- `docs/sdk-reference/NODE_ENCRYPTION_GUIDE.md` - SDK integration
- `docs/API.md` - Encryption protocol documentation

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