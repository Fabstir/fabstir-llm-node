# Enhanced S5.js P2P Bridge Deployment Guide

This guide covers deploying the Enhanced S5.js bridge service for production use with Fabstir LLM Node.

## Overview

The Enhanced S5.js bridge provides decentralized P2P storage access for the Fabstir LLM Node. The bridge runs the Enhanced S5.js SDK and exposes a simple HTTP API.

### Architecture

```
Fabstir LLM Node (Rust) → HTTP API (localhost:5522) → Enhanced S5.js Bridge (Node.js)
                                                              ↓
                                                    WebSocket P2P Network
                                                              ↓
                                                     S5 Portal Gateway
                                                              ↓
                                              Decentralized Storage Network
```

**Key Points:**
- Bridge runs as separate Node.js process
- Connects to P2P network via WebSocket
- No centralized servers involved
- Identity managed via seed phrase
- Portal registration via on-chain verification (NodeRegistry)
- HTTP API localhost-only for security

## Prerequisites

- **Node.js v20+**: Required for Enhanced S5.js SDK
- **Network Access**:
  - WebSocket connectivity (port 443 for `wss://`)
  - HTTPS access to S5 portal
- **Storage**: ~500MB for Node.js + dependencies
- **Memory**: 256MB+ for bridge service

## Quick Start

### 1. Generate Seed Phrase

```bash
cd services/s5-bridge
npm install

# Generate new seed phrase
node -e "import('@julesl23/s5js').then(({S5}) => S5.generateSeedPhrase().then(console.log))"

# Example output:
# word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11 word12
```

**🔐 Security**: Save this seed phrase securely! It's your identity on the S5 network.

### 2. Configure Environment

```bash
cd services/s5-bridge
cp .env.example .env
nano .env
```

Set required environment variables:
```bash
# S5 identity (12-15 word seed phrase)
S5_SEED_PHRASE=word1 word2 word3 word4 word5 word6 word7 word8 word9 word10 word11 word12

# Host Ethereum private key (must be registered on NodeRegistry)
HOST_PRIVATE_KEY=0x...
```

**Note:** `HOST_PRIVATE_KEY` is required for on-chain verified portal registration. The host must be registered on the NodeRegistry contract before the bridge can register with the S5 portal.

### 3. Start Bridge

```bash
npm start
```

Expected output:
```
╔════════════════════════════════════════╗
║  Enhanced S5.js Bridge Service v1.2.0  ║
║  P2P Storage Bridge for Fabstir Node  ║
╚════════════════════════════════════════╝

🔧 Validating configuration...
📋 Bridge Configuration:
   Host: localhost
   Port: 5522
   Portal: https://s5.platformlessai.ai
   Peers: 3 configured
   Identity: ✅ Configured
   Host Key: ✅ Configured
   ...

🚀 Initializing Enhanced S5.js client...
📡 Connecting to 3 P2P peer(s)...
✅ S5 instance created
🔐 Recovering identity from seed phrase...
✅ Identity recovered
🔑 Checking for existing portal account: https://s5.platformlessai.ai
🌐 Registering with portal via on-chain verification: https://s5.platformlessai.ai
   S5 PubKey: xxxx...
   ETH Address: 0x...
   ✅ On-chain verification passed, got S5 challenge
✅ Portal registration complete (on-chain verified)
🔧 Initializing filesystem for read/write operations...
✅ Filesystem initialized - uploads and downloads ready
🎉 Enhanced S5.js client fully initialized

🚀 Starting HTTP server...
✅ Bridge service is ready!
📡 HTTP API: http://localhost:5522
```

### 4. Verify Health

```bash
curl http://localhost:5522/health | jq
```

Expected:
```json
{
  "status": "healthy",
  "service": "s5-bridge",
  "timestamp": "2025-11-14T12:00:00.000Z",
  "initialized": true,
  "connected": true,
  "peerCount": 1,
  "portal": "https://s5.platformlessai.ai"
}
```

### 5. Start Fabstir Node

```bash
# In workspace root
./scripts/start-with-s5-bridge.sh
```

Or manually:
```bash
cargo run --release --features real-ezkl -j 4
```

## Deployment Options

### Option 1: Direct Process (Development)

Best for development and testing.

```bash
cd services/s5-bridge
npm start
```

**Pros:**
- Easy to debug
- Fast iteration
- Direct log access

**Cons:**
- Manual process management
- No auto-restart
- Not suitable for production

### Option 2: Docker (Recommended for Production)

```bash
cd services/s5-bridge

# Build image
docker build -t fabstir/s5-bridge:latest .

# Run container
docker-compose up -d

# Check logs
docker logs s5-bridge -f

# Check health
curl http://localhost:5522/health
```

**Pros:**
- Isolated environment
- Auto-restart on failure
- Easy updates
- Consistent deployment

**Cons:**
- Slightly more resource usage
- Docker dependency

### Option 3: Systemd Service (Production)

Best for production on Linux servers.

#### Create Service File

```bash
sudo nano /etc/systemd/system/s5-bridge.service
```

```ini
[Unit]
Description=Enhanced S5.js Bridge Service
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=fabstir
Group=fabstir
WorkingDirectory=/opt/fabstir/services/s5-bridge
Environment=NODE_ENV=production
EnvironmentFile=/opt/fabstir/services/s5-bridge/.env
ExecStart=/usr/bin/node src/server.js
Restart=on-failure
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=s5-bridge

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/fabstir/services/s5-bridge

[Install]
WantedBy=multi-user.target
```

#### Enable and Start

```bash
sudo systemctl daemon-reload
sudo systemctl enable s5-bridge
sudo systemctl start s5-bridge

# Check status
sudo systemctl status s5-bridge

# View logs
sudo journalctl -u s5-bridge -f
```

### Option 4: Orchestrated Startup (Recommended)

Start bridge and node together:

```bash
./scripts/start-with-s5-bridge.sh
```

This script:
1. Checks seed phrase is set
2. Starts bridge service
3. Waits for health check
4. Starts Rust node

For daemon mode:
```bash
./scripts/start-with-s5-bridge.sh --daemon
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `BRIDGE_PORT` | `5522` | HTTP server port |
| `BRIDGE_HOST` | `localhost` | Bind address (localhost for security) |
| `S5_SEED_PHRASE` | *required* | 12-15 word identity seed phrase |
| `HOST_PRIVATE_KEY` | *required* | Host ETH private key (must be registered on NodeRegistry) |
| `S5_PORTAL_URL` | `https://s5.platformlessai.ai` | S5 portal gateway URL |
| `S5_INITIAL_PEERS` | `wss://...node.sfive.net/s5/p2p,...` | WebSocket P2P peers (comma-separated) |
| `LOG_LEVEL` | `info` | Logging level (trace, debug, info, warn, error) |
| `PRETTY_LOGS` | `true` | Enable pretty-printed logs (false in production) |
| `REQUEST_TIMEOUT_MS` | `30000` | Request timeout (30 seconds) |
| `MAX_CONTENT_LENGTH` | `104857600` | Max upload size (100MB) |

### Production Configuration

For production, update `.env`:

```bash
# Production settings
BRIDGE_HOST=localhost  # Keep localhost for security
LOG_LEVEL=info
PRETTY_LOGS=false
REQUEST_TIMEOUT_MS=60000
MAX_CONTENT_LENGTH=104857600

# Use multiple peers for redundancy
S5_INITIAL_PEERS=wss://peer1.example.com/s5/p2p,wss://peer2.example.com/s5/p2p
```

## On-Chain Verified Registration

Starting with v1.1.0, the S5 bridge uses **on-chain verification** for portal registration. This eliminates the need for a master token and ensures only legitimate hosts can register.

**v1.2.0 Update:** Uses S5.js v0.9.0-beta.32 high-level APIs (`getSigningPublicKey()`, `sign()`, `storePortalCredentials()`) for simplified code and better maintainability.

### How It Works

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  Host (s5-bridge)│     │   S5 Portal      │     │  NodeRegistry   │
└────────┬────────┘     └────────┬─────────┘     └────────┬────────┘
         │                       │                        │
         │ 1. POST /s5/account/register-host              │
         │    { pubKey, ethAddress, signature, message }  │
         │──────────────────────>│                        │
         │                       │                        │
         │                       │ 2. Verify ETH signature│
         │                       │                        │
         │                       │ 3. isActiveNode(addr)  │
         │                       │───────────────────────>│
         │                       │                        │
         │                       │ 4. true/false          │
         │                       │<───────────────────────│
         │                       │                        │
         │ 5. { challenge }      │                        │
         │<──────────────────────│                        │
         │                       │                        │
         │ 6. POST /s5/account/register-host/complete     │
         │    { pubKey, challenge, response, ... }        │
         │──────────────────────>│                        │
         │                       │                        │
         │ 7. { authToken }      │                        │
         │<──────────────────────│                        │
```

### Requirements

1. **Host must be registered on NodeRegistry** - Call `registerNode()` with active stake before starting the bridge
2. **HOST_PRIVATE_KEY must match** - The ETH private key must correspond to the registered node address
3. **S5_SEED_PHRASE for S5 identity** - Used to derive S5 keys for signing challenges

### Contract Details

- **Chain**: Base Sepolia
- **NodeRegistry (UUPS Proxy)**: `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22`
- **Function used**: `isActiveNode(address) returns (bool)`

### Security Benefits

- **No master token exposure** - On-chain state is the authorization
- **Sybil-resistant** - Only staked hosts can register
- **Fully automated** - No manual approval needed
- **Decentralized** - No central authority for registration

## Identity Management

### Seed Phrase Security

Your seed phrase is your identity on the S5 network. **Treat it like a private key.**

**Best Practices:**
- ✅ Store in secrets management (Kubernetes secrets, AWS Secrets Manager, Vault)
- ✅ Use different phrases for production/staging/development
- ✅ Back up securely (encrypted backup, offline storage)
- ✅ Rotate quarterly
- ❌ Never commit to version control
- ❌ Never log or print
- ❌ Never share or transmit insecurely

### Backing Up Seed Phrase

```bash
# Encrypt seed phrase with GPG
echo "your twelve word phrase here" | gpg --symmetric --armor > seed_phrase.gpg

# Store in secure location
mv seed_phrase.gpg /path/to/secure/backup/

# Decrypt when needed
gpg --decrypt /path/to/secure/backup/seed_phrase.gpg
```

### Rotating Seed Phrase

1. Generate new seed phrase
2. Update `.env` with new phrase
3. Restart bridge service
4. Verify connectivity
5. Securely destroy old phrase

**⚠️  Warning**: Changing seed phrase changes your identity. Existing data under old identity won't be accessible.

## Monitoring

### Health Checks

Add to monitoring system:

```bash
# Check every 30 seconds
*/30 * * * * curl -sf http://localhost:5522/health || alert "S5 bridge unhealthy"
```

### Prometheus Metrics

Bridge doesn't currently export Prometheus metrics, but you can scrape logs:

```bash
# Count errors in logs
journalctl -u s5-bridge --since "1 hour ago" | grep ERROR | wc -l
```

### Log Monitoring

```bash
# Tail logs
journalctl -u s5-bridge -f

# Search for errors
journalctl -u s5-bridge | grep -i error

# Check initialization
journalctl -u s5-bridge | grep "fully initialized"
```

## Troubleshooting

### Bridge Won't Start

**Symptom**: Process exits immediately

**Causes & Solutions**:

1. **Missing seed phrase**
   ```bash
   # Check if set
   echo $S5_SEED_PHRASE
   # If empty, set it
   export S5_SEED_PHRASE="your twelve word phrase"
   ```

2. **Invalid seed phrase** (not 12 words)
   ```bash
   # Count words
   echo $S5_SEED_PHRASE | wc -w
   # Should be 12
   ```

3. **Port already in use**
   ```bash
   # Check port
   lsof -i :5522
   # Kill existing process or change BRIDGE_PORT
   ```

4. **Node.js version too old**
   ```bash
   # Check version
   node --version
   # Should be v20.0.0 or higher
   ```

### P2P Peers Not Connecting

**Symptom**: `"connected": false` in health check

**Causes & Solutions**:

1. **Network connectivity issues**
   ```bash
   # Test peer connectivity
   curl -I https://node.sfive.net
   # Should return 200 OK

   # Test WebSocket (requires wscat)
   wscat -c wss://node.sfive.net/s5/p2p
   ```

2. **Firewall blocking WebSocket**
   ```bash
   # Allow outbound HTTPS/WSS (port 443)
   sudo ufw allow out 443/tcp
   ```

3. **Peer URL incorrect**
   ```bash
   # Verify peer URL format
   echo $S5_INITIAL_PEERS
   # Should be: wss://...@hostname/s5/p2p
   ```

4. **Try alternative peers**
   ```bash
   # Update peers in .env
   S5_INITIAL_PEERS=wss://peer1.example.com/s5/p2p,wss://peer2.example.com/s5/p2p
   ```

### Portal Registration Failing

**Symptom**: Error during "Registering with portal via on-chain verification"

**Causes & Solutions**:

1. **Host not registered on NodeRegistry**
   ```
   ❌ Host not registered on NodeRegistry
      Register your host via CLI/TUI first, then restart the bridge
   ```
   **Solution**: Register your host on the NodeRegistry contract before starting the bridge. The ETH address from `HOST_PRIVATE_KEY` must be an active node.

2. **Invalid ETH signature**
   ```
   ❌ ETH signature verification failed
      Check HOST_PRIVATE_KEY is correct
   ```
   **Solution**: Verify `HOST_PRIVATE_KEY` is set correctly and matches the registered node address.

3. **Missing HOST_PRIVATE_KEY**
   ```
   ⚠️  HOST_PRIVATE_KEY not set - cannot register with portal
   📥 Bridge will operate in read-only mode (downloads only)
   ```
   **Solution**: Set `HOST_PRIVATE_KEY` in your `.env` file.

4. **Portal unreachable**
   ```bash
   # Test portal
   curl https://s5.platformlessai.ai
   ```

5. **Invalid portal URL**
   ```bash
   # Check URL format
   echo $S5_PORTAL_URL
   # Should be: https://s5.platformlessai.ai
   ```

6. **Seed phrase invalid**
   - Regenerate seed phrase
   - Verify it's 12-15 words
   - Check for typos

### File Operations Timing Out

**Symptom**: Requests to `/s5/fs/*` timeout

**Causes & Solutions**:

1. **Increase timeout**
   ```bash
   # In .env
   REQUEST_TIMEOUT_MS=60000
   ```

2. **Check P2P connectivity**
   ```bash
   curl http://localhost:5522/health
   # Verify connected=true
   ```

3. **Verify path format**
   ```bash
   # Correct format
   /s5/fs/home/username/file.txt

   # Incorrect
   /s5/fs//home/username/file.txt  # Double slash
   /s5/home/username/file.txt      # Missing /fs/
   ```

### Rust Node Can't Connect to Bridge

**Symptom**: "Connection refused" errors in Rust node

**Causes & Solutions**:

1. **Bridge not running**
   ```bash
   curl http://localhost:5522/health
   # Start bridge if fails
   ```

2. **Wrong URL configured**
   ```bash
   # Check Rust node config
   echo $ENHANCED_S5_URL
   # Should be: http://localhost:5522
   ```

3. **Port mismatch**
   ```bash
   # Ensure BRIDGE_PORT matches ENHANCED_S5_URL port
   ```

## Performance Tuning

### Node.js Heap Size

For large file operations:

```bash
# Increase heap size
NODE_OPTIONS="--max-old-space-size=2048" npm start
```

### Request Timeout

For slow network:

```bash
# In .env
REQUEST_TIMEOUT_MS=120000  # 2 minutes
```

### Concurrent Requests

Bridge handles concurrent requests via Node.js event loop. No special configuration needed.

## High Availability

### Multiple Bridge Instances

**Not recommended.** Each bridge instance has its own identity (seed phrase). Instead:

- Use systemd auto-restart
- Monitor health and alert on failures
- Have backup seed phrase ready

### Backup and Recovery

#### Backup

```bash
# Backup seed phrase
gpg --symmetric --armor .env > /backup/s5-bridge-env.gpg

# Backup logs (optional)
journalctl -u s5-bridge > /backup/s5-bridge.log
```

#### Recovery

```bash
# Restore seed phrase
gpg --decrypt /backup/s5-bridge-env.gpg > .env

# Restart bridge
sudo systemctl restart s5-bridge
```

## Security Considerations

### Network Security

- Bridge binds to `localhost` by default (never change to `0.0.0.0` in production)
- Only Rust node on same machine can access
- No authentication needed (localhost trust)

### Seed Phrase Security

- Store in secrets management
- Encrypt at rest
- Rotate quarterly
- Never log or transmit

### Process Isolation

- Run as non-root user
- Use systemd security features
- Limit file system access

### Monitoring

- Monitor health endpoint
- Alert on failures
- Track request rates
- Log all errors

## Upgrading

### Upgrade Bridge Service

```bash
cd services/s5-bridge

# Pull latest code
git pull

# Update dependencies
npm install

# Restart service
sudo systemctl restart s5-bridge

# Verify
curl http://localhost:5522/health
```

### Upgrade Enhanced S5.js SDK

When `@julesl23/s5js` moves from beta to stable:

```bash
cd services/s5-bridge

# Update package.json
# Change: "@julesl23/s5js": "beta"
# To:     "@s5-dev/s5js": "^1.0.0"

npm install
sudo systemctl restart s5-bridge
```

## See Also

- Bridge service README: `services/s5-bridge/README.md`
- Enhanced S5.js SDK: https://github.com/parajbs/s5-network
- Rust client docs: `src/storage/enhanced_s5_client.rs`
- Main deployment guide: `docs/DEPLOYMENT.md`
