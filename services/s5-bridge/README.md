# Enhanced S5.js Bridge Service

HTTP bridge service that exposes [Enhanced S5.js](https://github.com/parajbs/s5-network) P2P storage via REST API for the Fabstir LLM Node.

## Architecture

```
Rust Node → HTTP Bridge (localhost:5522) → Enhanced S5.js SDK → P2P Network (WebSocket)
                                                  ↓
                                    S5 Portal (s5.platformlessai.ai)
                                                  ↓
                                      Sia Decentralized Storage
```

## Features

- **P2P Storage Access**: Direct peer-to-peer storage via Enhanced S5.js
- **Identity Management**: Seed phrase-based identity recovery
- **Portal Registration**: Automatic registration with S5 portal
- **REST API**: Simple HTTP interface for filesystem operations
- **Health Checks**: Built-in health monitoring endpoint

## Quick Start

### 1. Install Dependencies

```bash
cd services/s5-bridge
npm install
```

### 2. Configure Environment

```bash
# Copy example configuration
cp .env.example .env

# Generate seed phrase (or use existing)
node -e "import('@julesl23/s5js').then(({S5}) => S5.generateSeedPhrase().then(console.log))"

# Edit .env and set S5_SEED_PHRASE
nano .env
```

### 3. Start Service

```bash
npm start
```

The service will start on `http://localhost:5522` by default.

### 4. Verify Health

```bash
curl http://localhost:5522/health
```

Expected output:
```json
{
  "status": "healthy",
  "service": "s5-bridge",
  "initialized": true,
  "connected": true,
  "peerCount": 1,
  "portal": "https://s5.platformlessai.ai"
}
```

## API Endpoints

### Health Check
```bash
GET /health
```

Returns service status and P2P connectivity information.

### Download File
```bash
GET /s5/fs/{path}
```

Downloads a file from S5 storage.

Example:
```bash
curl http://localhost:5522/s5/fs/home/vector-databases/0xABC/manifest.json
```

### Upload File
```bash
PUT /s5/fs/{path}
```

Uploads a file to S5 storage.

Example:
```bash
curl -X PUT http://localhost:5522/s5/fs/home/test/file.txt \
  -H "Content-Type: application/octet-stream" \
  --data-binary "@file.txt"
```

### Delete File
```bash
DELETE /s5/fs/{path}
```

Deletes a file from S5 storage.

Example:
```bash
curl -X DELETE http://localhost:5522/s5/fs/home/test/file.txt
```

### List Directory
```bash
GET /s5/fs/{path}/
```

Lists files in a directory (note trailing slash).

Example:
```bash
curl http://localhost:5522/s5/fs/home/vector-databases/
```

## Configuration

Environment variables (see `.env.example`):

| Variable | Default | Description |
|----------|---------|-------------|
| `BRIDGE_PORT` | `5522` | HTTP server port |
| `BRIDGE_HOST` | `localhost` | Bind address (localhost for security) |
| `S5_SEED_PHRASE` | *required* | 12-word identity seed phrase |
| `S5_PORTAL_URL` | `https://s5.platformlessai.ai` | S5 portal gateway URL (Sia storage) |
| `S5_INITIAL_PEERS` | `wss://...node.sfive.net/s5/p2p,...` | WebSocket P2P peers (comma-separated) |
| `LOG_LEVEL` | `info` | Logging level (trace, debug, info, warn, error) |
| `PRETTY_LOGS` | `true` | Enable pretty-printed logs |
| `REQUEST_TIMEOUT_MS` | `30000` | Request timeout (30 seconds) |
| `MAX_CONTENT_LENGTH` | `104857600` | Max upload size (100MB) |

## Development

### Run in Watch Mode

```bash
npm run dev
```

### Run Tests

```bash
# Start bridge service in one terminal
npm start

# Run tests in another terminal
npm test
```

### Watch Tests

```bash
npm run test:watch
```

## Security Notes

- **Localhost Only**: Bridge binds to `localhost` by default for security
- **Seed Phrase**: Keep your seed phrase secure and backed up
- **Network Access**: Requires WebSocket connectivity to P2P peers
- **CORS**: Only allows requests from localhost origins

## Troubleshooting

### Bridge Won't Start

1. Check seed phrase is set: `echo $S5_SEED_PHRASE`
2. Verify Node.js version: `node --version` (requires 20+)
3. Check port is available: `lsof -i :5522`

### P2P Peers Not Connecting

1. Check network connectivity: `ping node.sfive.net`
2. Verify WebSocket access (port 443): `curl -I https://node.sfive.net`
3. Try alternative peers in `S5_INITIAL_PEERS`

### Portal Registration Failing

1. Check portal URL is accessible: `curl https://s5.platformlessai.ai/s5/version`
2. Verify seed phrase is valid (12 words)
3. Check logs for detailed error messages

### File Operations Timing Out

1. Increase `REQUEST_TIMEOUT_MS` in .env
2. Check P2P connectivity: `GET /health`
3. Verify file path format (e.g., `home/username/file.txt`)

## Production Deployment

See `docs/ENHANCED_S5_DEPLOYMENT.md` for production deployment guide including:
- Docker configuration
- Systemd service setup
- Monitoring and alerting
- High availability setup

## License

BUSL-1.1 - Copyright (c) 2025 Fabstir
