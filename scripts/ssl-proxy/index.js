#!/usr/bin/env node
/**
 * SSL Proxy for Fabstir LLM Node
 *
 * This proxy provides SSL termination for the node's HTTP and WebSocket endpoints.
 * It accepts HTTPS/WSS connections from clients and proxies to plain HTTP/WS backend.
 *
 * Usage:
 *   node index.js
 *
 * Environment Variables:
 *   PROXY_PORT=443              - Port to listen on (default: 443)
 *   BACKEND_HOST=localhost      - Backend node host (default: localhost)
 *   BACKEND_PORT=8080          - Backend node port (default: 8080)
 *   SSL_CERT_PATH=/path/to/cert.pem
 *   SSL_KEY_PATH=/path/to/key.pem
 *   SSL_CA_PATH=/path/to/ca.pem (optional)
 */

const https = require('https');
const http = require('http');
const fs = require('fs');
const httpProxy = require('http-proxy');

// Configuration
const PROXY_PORT = process.env.PROXY_PORT || 443;
const BACKEND_HOST = process.env.BACKEND_HOST || 'localhost';
const BACKEND_PORT = process.env.BACKEND_PORT || 8080;
const SSL_CERT_PATH = process.env.SSL_CERT_PATH;
const SSL_KEY_PATH = process.env.SSL_KEY_PATH;
const SSL_CA_PATH = process.env.SSL_CA_PATH;

// Validate SSL configuration
if (!SSL_CERT_PATH || !SSL_KEY_PATH) {
  console.error('❌ Error: SSL_CERT_PATH and SSL_KEY_PATH environment variables are required');
  console.error('\nUsage:');
  console.error('  SSL_CERT_PATH=/etc/letsencrypt/live/your.domain/fullchain.pem \\');
  console.error('  SSL_KEY_PATH=/etc/letsencrypt/live/your.domain/privkey.pem \\');
  console.error('  node index.js');
  process.exit(1);
}

// Check if certificate files exist
if (!fs.existsSync(SSL_CERT_PATH)) {
  console.error(`❌ Error: Certificate file not found: ${SSL_CERT_PATH}`);
  process.exit(1);
}

if (!fs.existsSync(SSL_KEY_PATH)) {
  console.error(`❌ Error: Key file not found: ${SSL_KEY_PATH}`);
  process.exit(1);
}

// SSL options
const sslOptions = {
  cert: fs.readFileSync(SSL_CERT_PATH),
  key: fs.readFileSync(SSL_KEY_PATH),
};

// Add CA bundle if provided
if (SSL_CA_PATH && fs.existsSync(SSL_CA_PATH)) {
  sslOptions.ca = fs.readFileSync(SSL_CA_PATH);
  console.log('✅ CA bundle loaded');
}

// Create proxy server
const proxy = httpProxy.createProxyServer({
  target: `http://${BACKEND_HOST}:${BACKEND_PORT}`,
  ws: true, // Enable WebSocket proxying
  changeOrigin: true,
  // Preserve original host header
  preserveHeaderKeyCase: true,
  // Timeout settings
  proxyTimeout: 300000, // 5 minutes for long-running inference
  timeout: 300000,
});

// Error handling
proxy.on('error', (err, req, res) => {
  console.error('❌ Proxy error:', err.message);

  // Handle WebSocket errors
  if (req.headers.upgrade === 'websocket') {
    console.error('WebSocket proxy error');
    return;
  }

  // Handle HTTP errors
  if (res && !res.headersSent) {
    res.writeHead(502, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({
      error: 'Bad Gateway',
      message: 'Failed to connect to backend server',
      backend: `${BACKEND_HOST}:${BACKEND_PORT}`
    }));
  }
});

// Proxy request logging
proxy.on('proxyReq', (proxyReq, req, res, options) => {
  const isWebSocket = req.headers.upgrade === 'websocket';
  const protocol = isWebSocket ? 'WSS' : 'HTTPS';
  console.log(`${protocol} ${req.method} ${req.url} -> http://${BACKEND_HOST}:${BACKEND_PORT}${req.url}`);
});

// Create HTTPS server
const server = https.createServer(sslOptions, (req, res) => {
  // Add CORS headers for cross-origin requests
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
  res.setHeader('Access-Control-Allow-Headers', 'Content-Type, Authorization');

  // Handle preflight requests
  if (req.method === 'OPTIONS') {
    res.writeHead(204);
    res.end();
    return;
  }

  // Proxy the request
  proxy.web(req, res);
});

// Handle WebSocket upgrades
server.on('upgrade', (req, socket, head) => {
  console.log(`🔄 WebSocket upgrade: ${req.url}`);
  proxy.ws(req, socket, head);
});

// Handle WebSocket close
proxy.on('close', (res, socket, head) => {
  console.log('🔌 WebSocket connection closed');
});

// Start server
server.listen(PROXY_PORT, () => {
  console.log('🚀 SSL Proxy Server Started');
  console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━');
  console.log(`📡 Listening:  https://0.0.0.0:${PROXY_PORT}`);
  console.log(`🎯 Backend:    http://${BACKEND_HOST}:${BACKEND_PORT}`);
  console.log(`🔒 SSL Cert:   ${SSL_CERT_PATH}`);
  console.log(`🔑 SSL Key:    ${SSL_KEY_PATH}`);
  console.log('━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━');
  console.log('\nEndpoints:');
  console.log(`  HTTPS:      https://0.0.0.0:${PROXY_PORT}/v1/embed`);
  console.log(`  WebSocket:  wss://0.0.0.0:${PROXY_PORT}/v1/ws`);
  console.log('');
});

// Graceful shutdown
process.on('SIGTERM', () => {
  console.log('📴 SIGTERM received, shutting down gracefully...');
  server.close(() => {
    console.log('✅ Server closed');
    process.exit(0);
  });
});

process.on('SIGINT', () => {
  console.log('\n📴 SIGINT received, shutting down gracefully...');
  server.close(() => {
    console.log('✅ Server closed');
    process.exit(0);
  });
});
