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
| `S5_SEED_PHRASE` | *required* | 12-word identity seed phrase. A regional host uses a fresh seed, never one used in another region |
| `S5_REGION` | *required, no default* | Storage region: `global` or `uk` (`eu` is not configured yet). See "Storage region guard" below |
| `S5_PORTAL_URL` | `https://s5.platformlessai.ai` | S5 portal gateway URL. Its host must be the `S5_REGION` portal host; a uk host sets it explicitly. Uploads are pinned to this portal's host: at boot the bridge drops every other portal account from its live list (`src/upload_pin.js`; `accounts.json` is untouched), and logs `Upload portal pinned to <host>` |
| `S5_INITIAL_PEERS` | `wss://...@s5.platformlessai.ai/s5/p2p` | WebSocket P2P peers (comma-separated). Our own node only; never the public S5 nodes. Every entry's host must be the `S5_REGION` peer host, and at least one is required |
| `LOG_LEVEL` | `info` | Logging level (trace, debug, info, warn, error) |
| `PRETTY_LOGS` | `true` | Enable pretty-printed logs |
| `REQUEST_TIMEOUT_MS` | `30000` | Request timeout (30 seconds) |
| `MAX_CONTENT_LENGTH` | `104857600` | Max upload size (100MB) |

## Storage region guard

A host has exactly one storage region (SR1 D3, `src/region_guard.js`). `S5_REGION` is required and has no default:

| `S5_REGION` | Portal host | Peer host |
|---|---|---|
| `global` | `s5.platformlessai.ai` | `s5.platformlessai.ai` |
| `uk` | `s5-uk.platformlessai.ai` | `s5-uk.platformlessai.ai` |

The bridge exits with a non-zero status and names the reason when:

- before the S5 client is created: `S5_REGION` is missing, empty or not in the table; the `S5_PORTAL_URL` host is not the region's portal host; any `S5_INITIAL_PEERS` entry cannot be parsed or its host is not the region's peer host; or no peer is given;
- after login and before the filesystem is initialised: the upload pin did not take (no account on the region portal), or any account left in the live account list is on another host.

The second check means a bridge whose portal registration fails now stops instead of running read only. Existing Global hosts set `S5_REGION=global` in the same deploy that brings this release (`docker-compose.prod.yml` already does).

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

1. Check the bridge log for the peer it connected to (it should be `s5.platformlessai.ai`)
2. Verify WebSocket access (port 443): `curl -I https://s5.platformlessai.ai`
3. Check `S5_INITIAL_PEERS` is set to our own node, not a public S5 node

### Uploads Landing on a Public Portal

Every `UPLOAD COMPLETE` log line names its `portalAccount`; it must be on `S5_PORTAL_URL`'s host. If the bridge exits with `upload pin did not take`, the identity has no account on our portal: fix the registration, never add a public portal back.

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
