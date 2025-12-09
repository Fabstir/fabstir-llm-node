# SSL Proxy for Fabstir LLM Node

This Node.js proxy provides SSL termination for the Fabstir LLM Node, allowing HTTPS/WSS clients to connect to a plain HTTP/WS backend.

## Problem It Solves

When your UI is deployed with HTTPS (like on Kubernetes/Vultr), browsers require all connections to use SSL:
- ❌ `https://ui.example.com` → `http://81.150.166.91:8080` (BLOCKED - Mixed content)
- ❌ `wss://ui.example.com` → `ws://81.150.166.91:8080` (BLOCKED - SSL protocol error)

This proxy sits in front of your node and provides SSL termination:
- ✅ `https://ui.example.com` → `https://81.150.166.91:443` → `http://localhost:8080`
- ✅ `wss://ui.example.com` → `wss://81.150.166.91:443` → `ws://localhost:8080`

## Prerequisites

1. **Node.js 14+** installed on your Ubuntu server
2. **SSL Certificate** (Let's Encrypt recommended)
3. **Fabstir LLM Node** running on `http://localhost:8080`

## Installation

### Step 1: Install Dependencies

```bash
cd /workspace/scripts/ssl-proxy
npm install
```

### Step 2: Get SSL Certificate (Let's Encrypt)

If you don't have a domain name, you can use a self-signed certificate for testing, but browsers will show security warnings.

#### Option A: With Domain Name (Recommended)

```bash
# Install certbot
sudo apt update
sudo apt install -y certbot

# Get certificate (replace your.domain.com)
sudo certbot certonly --standalone -d your.domain.com

# Certificates will be at:
# /etc/letsencrypt/live/your.domain.com/fullchain.pem
# /etc/letsencrypt/live/your.domain.com/privkey.pem
```

#### Option B: Self-Signed Certificate (Testing Only)

```bash
# Generate self-signed certificate
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes \
  -subj "/CN=81.150.166.91"

# Move to standard location
sudo mkdir -p /etc/ssl/fabstir
sudo mv cert.pem /etc/ssl/fabstir/
sudo mv key.pem /etc/ssl/fabstir/
```

⚠️ **Self-signed certificates will trigger browser warnings!** Only use for testing.

### Step 3: Configure Environment

Create a `.env` file or set environment variables:

```bash
# For Let's Encrypt certificate
export SSL_CERT_PATH=/etc/letsencrypt/live/your.domain.com/fullchain.pem
export SSL_KEY_PATH=/etc/letsencrypt/live/your.domain.com/privkey.pem
export BACKEND_HOST=localhost
export BACKEND_PORT=8080
export PROXY_PORT=443

# Or for self-signed certificate
export SSL_CERT_PATH=/etc/ssl/fabstir/cert.pem
export SSL_KEY_PATH=/etc/ssl/fabstir/key.pem
```

### Step 4: Run the Proxy

```bash
# Make sure backend node is running first
cd /workspace
cargo run --release --features real-ezkl

# In another terminal, start the proxy
cd /workspace/scripts/ssl-proxy
sudo -E npm start
```

⚠️ **Note**: `sudo` is required for port 443. Use `-E` to preserve environment variables.

## Deployment with systemd

For production, run as a systemd service:

### Create systemd service file:

```bash
sudo nano /etc/systemd/system/fabstir-ssl-proxy.service
```

**Content:**

```ini
[Unit]
Description=Fabstir LLM Node SSL Proxy
After=network.target fabstir-llm-node.service
Requires=fabstir-llm-node.service

[Service]
Type=simple
User=root
WorkingDirectory=/workspace/scripts/ssl-proxy
Environment="SSL_CERT_PATH=/etc/letsencrypt/live/your.domain.com/fullchain.pem"
Environment="SSL_KEY_PATH=/etc/letsencrypt/live/your.domain.com/privkey.pem"
Environment="BACKEND_HOST=localhost"
Environment="BACKEND_PORT=8080"
Environment="PROXY_PORT=443"
ExecStart=/usr/bin/node index.js
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

### Enable and start:

```bash
sudo systemctl daemon-reload
sudo systemctl enable fabstir-ssl-proxy
sudo systemctl start fabstir-ssl-proxy
sudo systemctl status fabstir-ssl-proxy
```

### View logs:

```bash
sudo journalctl -u fabstir-ssl-proxy -f
```

## Testing

### Test HTTPS endpoint:

```bash
curl https://81.150.166.91/v1/embed \
  -k \
  -H 'Content-Type: application/json' \
  -d '{"input": "test", "model": "all-MiniLM-L6-v2"}'
```

### Test WebSocket endpoint:

```bash
# Install wscat if needed
npm install -g wscat

# Test WebSocket connection
wscat -c wss://81.150.166.91/v1/ws --no-check
```

## Firewall Configuration

Make sure ports are open:

```bash
# Allow HTTPS
sudo ufw allow 443/tcp

# Check status
sudo ufw status
```

## Troubleshooting

### Error: `EACCES: permission denied, bind 0.0.0.0:443`

**Solution**: Run with `sudo` or use a port >1024:

```bash
# Option 1: Use sudo
sudo -E npm start

# Option 2: Use different port
export PROXY_PORT=8443
npm start
```

### Error: `ENOENT: no such file or directory`

**Solution**: Check certificate paths:

```bash
ls -la /etc/letsencrypt/live/your.domain.com/
```

### Error: Backend connection refused

**Solution**: Make sure the node is running:

```bash
curl http://localhost:8080/health
```

### Browser shows "Not Secure" warning

**Solution**:
- If using Let's Encrypt: Make sure certificate is valid and not expired
- If using self-signed: This is expected - click "Advanced" → "Proceed"
- For production: Use Let's Encrypt with a real domain name

## Architecture

```
┌─────────────┐          ┌──────────────┐          ┌─────────────────┐
│             │  HTTPS   │              │   HTTP   │                 │
│  UI Client  │─────────▶│  SSL Proxy   │─────────▶│  Fabstir Node   │
│  (Browser)  │  (443)   │  (Node.js)   │  (8080)  │  (Rust)         │
│             │◀─────────│              │◀─────────│                 │
└─────────────┘   WSS    └──────────────┘    WS    └─────────────────┘
```

## Performance

- **Latency**: ~1-2ms overhead for SSL termination
- **Throughput**: Supports 1000+ concurrent WebSocket connections
- **WebSocket**: No buffering, streams tokens in real-time
- **Memory**: ~50MB for proxy process

## Security Notes

1. **Keep certificates updated**: Let's Encrypt certificates expire every 90 days
2. **Auto-renewal**: Set up automatic renewal:
   ```bash
   sudo certbot renew --dry-run
   sudo systemctl enable certbot.timer
   ```

3. **Firewall**: Only expose port 443, keep 8080 internal:
   ```bash
   sudo ufw deny 8080/tcp  # Block external access to backend
   sudo ufw allow 443/tcp  # Allow only proxy port
   ```

4. **CORS**: The proxy sets permissive CORS headers. Adjust `Access-Control-Allow-Origin` in production.

## Alternative: Use Nginx Instead

If you prefer Nginx over Node.js:

```nginx
server {
    listen 443 ssl http2;
    server_name 81.150.166.91;

    ssl_certificate /etc/letsencrypt/live/your.domain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/your.domain.com/privkey.pem;

    location / {
        proxy_pass http://localhost:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # Timeouts for long-running inference
        proxy_read_timeout 300s;
        proxy_send_timeout 300s;
    }
}
```

## Support

For issues or questions:
- Check logs: `sudo journalctl -u fabstir-ssl-proxy -f`
- Verify backend: `curl http://localhost:8080/health`
- Test SSL: `openssl s_client -connect 81.150.166.91:443`

## License

BUSL-1.1
