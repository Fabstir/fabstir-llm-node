#!/bin/bash
# Quick installation script for SSL Proxy

set -e

echo "🚀 Fabstir LLM Node SSL Proxy Installation"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Check Node.js
if ! command -v node &> /dev/null; then
    echo "❌ Node.js not found. Installing..."
    curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash -
    sudo apt-get install -y nodejs
fi

echo "✅ Node.js $(node --version) detected"

# Install dependencies
echo "📦 Installing dependencies..."
npm install

# Check if SSL certificates exist
if [ -z "$SSL_CERT_PATH" ] || [ -z "$SSL_KEY_PATH" ]; then
    echo "⚠️  SSL certificates not configured"
    echo ""
    echo "Please set environment variables:"
    echo "  export SSL_CERT_PATH=/path/to/cert.pem"
    echo "  export SSL_KEY_PATH=/path/to/key.pem"
    echo ""
    echo "For Let's Encrypt certificates:"
    echo "  sudo apt install certbot"
    echo "  sudo certbot certonly --standalone -d your.domain.com"
    echo ""
    echo "Then set:"
    echo "  export SSL_CERT_PATH=/etc/letsencrypt/live/your.domain.com/fullchain.pem"
    echo "  export SSL_KEY_PATH=/etc/letsencrypt/live/your.domain.com/privkey.pem"
    echo ""
    exit 1
fi

# Verify certificate files exist
if [ ! -f "$SSL_CERT_PATH" ]; then
    echo "❌ Certificate file not found: $SSL_CERT_PATH"
    exit 1
fi

if [ ! -f "$SSL_KEY_PATH" ]; then
    echo "❌ Key file not found: $SSL_KEY_PATH"
    exit 1
fi

echo "✅ SSL certificates found"
echo "   Cert: $SSL_CERT_PATH"
echo "   Key:  $SSL_KEY_PATH"

# Check if backend is running
BACKEND_HOST=${BACKEND_HOST:-localhost}
BACKEND_PORT=${BACKEND_PORT:-8080}

echo ""
echo "🔍 Checking backend node at http://$BACKEND_HOST:$BACKEND_PORT..."
if curl -s -f "http://$BACKEND_HOST:$BACKEND_PORT/health" > /dev/null 2>&1; then
    echo "✅ Backend node is running"
else
    echo "⚠️  Backend node not responding at http://$BACKEND_HOST:$BACKEND_PORT"
    echo "   Make sure the Fabstir LLM Node is running first"
    echo ""
    read -p "Continue anyway? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ Installation complete!"
echo ""
echo "To start the proxy:"
echo "  sudo -E npm start"
echo ""
echo "Or install as systemd service:"
echo "  See README.md for instructions"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
