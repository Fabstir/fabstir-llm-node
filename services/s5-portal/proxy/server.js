const http = require('http');
const { execSync } = require('child_process');
const dns = require('dns');
const { ethers } = require('ethers');

dns.setDefaultResultOrder('ipv4first');

const PORT = 5051;
const S5_HOST = '127.0.0.1';
const S5_PORT = 5050;

// On-chain verification config
const BASE_SEPOLIA_RPC = process.env.BASE_SEPOLIA_RPC_URL;
if (!BASE_SEPOLIA_RPC) { console.error('BASE_SEPOLIA_RPC_URL is required'); process.exit(1); }
const NODE_REGISTRY_ADDRESS = '0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22';
const NODE_REGISTRY_ABI = [
  {
    "type": "function",
    "name": "isActiveNode",
    "inputs": [{ "name": "operator", "type": "address" }],
    "outputs": [{ "name": "", "type": "bool" }],
    "stateMutability": "view"
  }
];

// Initialize provider and contract
const provider = new ethers.JsonRpcProvider(BASE_SEPOLIA_RPC);
const nodeRegistry = new ethers.Contract(NODE_REGISTRY_ADDRESS, NODE_REGISTRY_ABI, provider);

// Get master token for internal S5 calls
const MASTER_TOKEN = require('fs').readFileSync('/root/.s5-master-token', 'utf8').trim();

// Cap concurrent file lookups so missing files cannot fill S5's 50 request slots
// New-format blob IDs (prefix 5b 82) are misread by S5 v0.14.1, so resolve them from storage here
const B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
function base58Decode(str) {
  let n = 0n;
  for (const ch of str) { const i = B58.indexOf(ch); if (i < 0) return null; n = n * 58n + BigInt(i); }
  let hex = n.toString(16); if (hex.length % 2) hex = '0' + hex;
  let lead = 0; while (lead < str.length && str[lead] === '1') lead++;
  return Buffer.concat([Buffer.alloc(lead), Buffer.from(hex, 'hex')]);
}
function newFormatBlobKey(cidStr) {
  if (!cidStr || cidStr[0] !== 'z') return null;
  const b = base58Decode(cidStr.slice(1));
  if (!b || b.length < 35 || b[0] !== 0x5b || b[1] !== 0x82 || (b[2] !== 0x1f && b[2] !== 0x1e)) return null;
  return '1/' + Buffer.concat([Buffer.from([0x1f]), b.subarray(3, 35)]).toString('base64url');
}
// Legacy S5 CIDs (raw 0x26 / encrypted 0xae, multibase u|b|z): the store key is the BLAKE3 multihash of the
// stored blob, which sits at a fixed offset. Raw: 26 1f <32 hash> <1-8 size>. Encrypted: ae <alg> <chunk> 1f <32 hash of the encrypted blob> ...
const B32 = 'abcdefghijklmnopqrstuvwxyz234567';
function base32Decode(str) {
  let bits = 0, val = 0; const out = [];
  for (const ch of str) {
    const i = B32.indexOf(ch); if (i < 0) return null;
    val = (val << 5) | i; bits += 5;
    if (bits >= 8) { bits -= 8; out.push((val >> bits) & 0xff); val &= (1 << bits) - 1; }
  }
  return Buffer.from(out);
}
function legacyBlobKey(cidStr) {
  if (!cidStr || typeof cidStr !== 'string' || cidStr.length < 2) return null;
  const body = cidStr.slice(1);
  let b;
  if (cidStr[0] === 'u') { if (!/^[A-Za-z0-9_-]+$/.test(body)) return null; b = Buffer.from(body, 'base64url'); }
  else if (cidStr[0] === 'b') b = base32Decode(body);
  else if (cidStr[0] === 'z') b = base58Decode(body);
  else return null;
  if (!b) return null;
  let off;
  if (b[0] === 0x26 && b.length >= 35 && b.length <= 42 && b[1] === 0x1f) off = 1;
  else if (b[0] === 0xae && b.length >= 36 && b[3] === 0x1f) off = 3;
  else return null;
  return '1/' + b.subarray(off, off + 33).toString('base64url');
}
const STORAGE_BASE = process.env.S5_PROXY_STORAGE_BASE;
const STORAGE_HOST = process.env.S5_PROXY_STORAGE_HOST || '';
if (!STORAGE_BASE) { console.error('S5_PROXY_STORAGE_BASE is required'); process.exit(1); }
const CDN_BASE = process.env.S5_PROXY_CDN_BASE;
if (!CDN_BASE) { console.error('S5_PROXY_CDN_BASE is required'); process.exit(1); }
function storageHead(key) {
  return new Promise((resolve) => {
    const r = http.request(STORAGE_BASE + key, { method: 'HEAD', timeout: 10000, headers: STORAGE_HOST ? { Host: STORAGE_HOST } : {} }, (sr) => { sr.resume(); resolve(sr.statusCode); });
    r.on('timeout', () => r.destroy(new Error('timeout')));
    r.on('error', () => resolve(0));
    r.end();
  });
}

const BLOB_CAP = 20;
let blobInFlight = 0;

// Per-minute traffic summary by request type, sender and app
const stats = new Map();
function count(key) { stats.set(key, (stats.get(key) || 0) + 1); }
setInterval(() => {
  if (stats.size === 0) return;
  const top = [...stats.entries()].sort((x, y) => y[1] - x[1]).slice(0, 8);
  console.log('[Stats 60s] blobInFlight=' + blobInFlight + ' | ' + top.map(([k, v]) => v + ' ' + k).join(' | '));
  stats.clear();
}, 60000);


// Verify ETH signature
function verifySignature(message, signature, expectedAddress) {
  try {
    const recoveredAddress = ethers.verifyMessage(message, signature);
    return recoveredAddress.toLowerCase() === expectedAddress.toLowerCase();
  } catch (e) {
    console.error('[Proxy] Signature verification failed:', e.message);
    return false;
  }
}

// Check if address is registered on NodeRegistry
async function isRegisteredHost(address) {
  try {
    const isActive = await nodeRegistry.isActiveNode(address);
    console.log(`[Proxy] NodeRegistry check for ${address}: ${isActive}`);
    return isActive;
  } catch (e) {
    console.error('[Proxy] NodeRegistry check failed:', e.message);
    return false;
  }
}

// Helper to query SQLite for auth token
function getAuthToken(pubKeyBase64) {
  try {
    const pubKeyBuffer = Buffer.from(pubKeyBase64.replace(/-/g, '+').replace(/_/g, '/'), 'base64');
    const pubKeyHex = pubKeyBuffer.toString('hex').toUpperCase();
    const query = `SELECT a.token FROM AuthToken a JOIN PublicKey p ON a.account_id = p.account_id WHERE UPPER(HEX(p.public_key)) = '${pubKeyHex}' ORDER BY a.created_at DESC LIMIT 1;`;
    const result = execSync(`sqlite3 -cmd ".timeout 2000" /opt/s5/db/accounts.db "${query}"`, { encoding: 'utf8' }).trim();
    return result || null;
  } catch (e) {
    console.error('DB query error:', e.message);
    return null;
  }
}

const server = http.createServer(async (req, res) => {
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
  res.setHeader('Access-Control-Allow-Headers', 'Content-Type, Authorization');
  res.setHeader('Access-Control-Allow-Credentials', 'true');
  res.setHeader('Access-Control-Expose-Headers', 'Retry-After, X-S5-Blob-Status');

  if (req.method === 'OPTIONS') {
    res.writeHead(204);
    res.end();
    return;
  }

  // Traffic accounting
  {
    const ip = (req.headers['x-forwarded-for'] || req.socket.remoteAddress || '-').split(',')[0].trim();
    const ua = (req.headers['user-agent'] || '-').slice(0, 40);
    const kind = req.url.startsWith('/s5/blob/') ? 'blob' : req.url.startsWith('/s5/upload') ? 'upload' : req.url.startsWith('/s5/account') ? 'account' : 'other';
    req._who = ip + ' ' + ua;
    if (kind !== 'other') count(kind + ' ' + req._who);
  }

  // New-format blob IDs: answer from storage, never via S5
  if ((req.method === 'GET' || req.method === 'HEAD') && req.url.startsWith('/s5/blob/')) {
    const key = newFormatBlobKey(req.url.slice(9).split(/[?#.]/)[0]);
    if (key) {
      const st = await storageHead(key);
      if (st === 200) {
        count('blob-direct ' + req._who);
        res.writeHead(307, { Location: CDN_BASE + key });
        res.end();
      } else if (st === 404) {
        count('blob-notfound ' + req._who);
        res.writeHead(404, { 'Content-Type': 'application/json', 'X-S5-Blob-Status': 'not-found', 'Cache-Control': 'no-store' });
        res.end(req.method === 'HEAD' ? undefined : JSON.stringify({ error: 'blob-not-found', source: 'portal-storage' }));
      } else {
        count('blob-storage-error ' + req._who);
        res.writeHead(503, { 'Content-Type': 'application/json', 'Retry-After': '5', 'X-S5-Blob-Status': 'unavailable' });
        res.end(req.method === 'HEAD' ? undefined : JSON.stringify({ error: 'blob-unavailable' }));
      }
      return;
    }
    // Legacy ids: the blob is usually in our own store under the same key scheme. Only a 200 answers here;
    // anything else falls through to the S5 node unchanged. Never log the id.
    const lkey = legacyBlobKey(req.url.slice(9).split(/[?#.]/)[0]);
    if (lkey) {
      const lst = await storageHead(lkey);
      if (lst === 200) {
        count('blob-direct-legacy ' + req._who);
        res.writeHead(307, { Location: CDN_BASE + lkey });
        res.end();
        return;
      }
      count('blob-legacy-fallthrough ' + req._who);
    }
  }

  // File lookups: forward at most BLOB_CAP at once, reject the rest immediately
  if (req.method === 'GET' && req.url.startsWith('/s5/blob/')) {
    if (blobInFlight >= BLOB_CAP) {
      count('blob-rejected ' + req._who);
      res.setHeader('Retry-After', '5');
      res.writeHead(503);
      res.end(JSON.stringify({ error: 'Portal busy, retry shortly' }));
      return;
    }
    blobInFlight++;
    const started = Date.now();
    let done = false, pr = null;
    const release = () => {
      if (done) return;
      done = true;
      blobInFlight--;
      if (Date.now() - started > 30000) count('blob-slow ' + req._who);
    };
    res.on('close', () => { if (pr) pr.resume(); });
    const proxyReq = http.request({
      hostname: S5_HOST, port: S5_PORT, path: req.url, method: 'GET',
      headers: { ...req.headers, host: 'localhost' }, family: 4
    }, (proxyRes) => {
      pr = proxyRes;
      proxyRes.on('end', release);
      proxyRes.on('close', release);
      if (res.destroyed) { proxyRes.resume(); return; }
      res.writeHead(proxyRes.statusCode, proxyRes.headers);
      proxyRes.pipe(res);
    });
    proxyReq.setTimeout(90000, () => proxyReq.destroy(new Error('S5 timeout')));
    proxyReq.on('error', (e) => {
      release();
      if (!res.headersSent) { res.writeHead(502); res.end(JSON.stringify({ error: 'Proxy error: ' + e.message })); }
    });
    proxyReq.end();
    return;
  }

  // NEW: Handle host registration with on-chain verification
  if (req.method === 'POST' && req.url === '/s5/account/register-host') {
    let reqBody = '';
    req.on('data', chunk => reqBody += chunk);
    req.on('end', async () => {
      try {
        const data = JSON.parse(reqBody);
        const { pubKey, ethAddress, signature, message } = data;

        if (!pubKey || !ethAddress || !signature || !message) {
          res.writeHead(400);
          res.end(JSON.stringify({ error: 'Missing required fields: pubKey, ethAddress, signature, message' }));
          return;
        }

        console.log(`[Proxy] Host registration request from ${ethAddress}`);

        // Step 1: Verify the signature
        if (!verifySignature(message, signature, ethAddress)) {
          console.log(`[Proxy] Invalid signature for ${ethAddress}`);
          res.writeHead(401);
          res.end(JSON.stringify({ error: 'Invalid signature' }));
          return;
        }

        // Step 2: Check NodeRegistry on-chain
        const isRegistered = await isRegisteredHost(ethAddress);
        if (!isRegistered) {
          console.log(`[Proxy] ${ethAddress} is not registered on NodeRegistry`);
          res.writeHead(403);
          res.end(JSON.stringify({ error: 'Host not registered on NodeRegistry' }));
          return;
        }

        console.log(`[Proxy] ${ethAddress} verified - proceeding with S5 registration`);

        // Step 3: Get challenge from S5 (using master token)
        const challengeRes = await new Promise((resolve, reject) => {
          const options = {
            hostname: S5_HOST,
            port: S5_PORT,
            path: `/s5/account/register?pubKey=${encodeURIComponent(pubKey)}`,
            method: 'GET',
            headers: { 
              'Authorization': `Bearer ${MASTER_TOKEN}`,
              'Host': 'localhost'
            },
            family: 4
          };

          const challengeReq = http.request(options, (challengeResponse) => {
            let body = '';
            challengeResponse.on('data', chunk => body += chunk);
            challengeResponse.on('end', () => resolve({ status: challengeResponse.statusCode, body }));
          });
          challengeReq.on('error', reject);
          challengeReq.end();
        });

        if (challengeRes.status !== 200) {
          res.writeHead(challengeRes.status);
          res.end(challengeRes.body);
          return;
        }

        // Return challenge to host for signing
        res.setHeader('Content-Type', 'application/json');
        res.writeHead(200);
        res.end(challengeRes.body);

      } catch (e) {
        console.error('[Proxy] Host registration error:', e);
        res.writeHead(500);
        res.end(JSON.stringify({ error: 'Internal server error: ' + e.message }));
      }
    });
    return;
  }

  // NEW: Complete host registration
  if (req.method === 'POST' && req.url === '/s5/account/register-host/complete') {
    let reqBody = '';
    req.on('data', chunk => reqBody += chunk);
    req.on('end', async () => {
      try {
        const data = JSON.parse(reqBody);
        const { pubKey, challenge, response, ethAddress, signature, message, s5Signature } = data;

        if (!pubKey || !challenge || !response || !ethAddress || !signature || !message || !s5Signature) {
          res.writeHead(400);
          res.end(JSON.stringify({ error: 'Missing required fields' }));
          return;
        }

        // Re-verify signature and on-chain status
        if (!verifySignature(message, signature, ethAddress)) {
          res.writeHead(401);
          res.end(JSON.stringify({ error: 'Invalid signature' }));
          return;
        }

        const isRegistered = await isRegisteredHost(ethAddress);
        if (!isRegistered) {
          res.writeHead(403);
          res.end(JSON.stringify({ error: 'Host not registered on NodeRegistry' }));
          return;
        }

        // Complete S5 registration with master token
        const registerRes = await new Promise((resolve, reject) => {
          const options = {
            hostname: S5_HOST,
            port: S5_PORT,
            path: '/s5/account/register',
            method: 'POST',
            headers: { 
              'Content-Type': 'application/json',
              'Authorization': `Bearer ${MASTER_TOKEN}`,
              'Host': 'localhost'
            },
            family: 4
          };

          const registerReq = http.request(options, (registerResponse) => {
            let body = '';
            registerResponse.on('data', chunk => body += chunk);
            registerResponse.on('end', () => resolve({ status: registerResponse.statusCode, body, headers: registerResponse.headers }));
          });
          registerReq.on('error', reject);
          registerReq.write(JSON.stringify({ pubKey, challenge, response, signature: data.s5Signature, label: data.label || 'host-' + ethAddress.slice(0, 8) }));
          registerReq.end();
        });

        // Handle empty response body bug
        let responseData = {};
        try {
          if (registerRes.body && registerRes.body.trim()) {
            responseData = JSON.parse(registerRes.body);
          }
        } catch (e) {}

        // If no authToken, fetch from DB
        // Only for a registration the node ACCEPTED (2xx). Without this check an unauthenticated
        // request carrying any known pubKey was handed that account's token from the database.
        if (!responseData.authToken && pubKey && registerRes.status >= 200 && registerRes.status < 300) {
          console.log(`[Proxy] No authToken in response, checking DB for: ${pubKey.substring(0, 20)}...`);
          await new Promise(resolve => setTimeout(resolve, 150));
          const dbToken = getAuthToken(pubKey);
          if (dbToken) {
            console.log(`[Proxy] Found token in DB: ${dbToken.substring(0, 20)}...`);
            responseData.authToken = dbToken;
          }
        }

        res.setHeader('Content-Type', 'application/json');
        res.writeHead(registerRes.status);
        res.end(JSON.stringify(responseData));

      } catch (e) {
        console.error('[Proxy] Host registration complete error:', e);
        res.writeHead(500);
        res.end(JSON.stringify({ error: 'Internal server error: ' + e.message }));
      }
    });
    return;
  }

  // Existing: Handle POST to /s5/account/register or /s5/account/login (with master token)
  const isAccountEndpoint = req.method === 'POST' && 
    (req.url === '/s5/account/register' || req.url === '/s5/account/login');

  if (isAccountEndpoint) {
    let reqBody = '';
    req.on('data', chunk => reqBody += chunk);
    req.on('end', () => {
      let pubKey = null;
      try {
        const data = JSON.parse(reqBody);
        pubKey = data.pubKey;
      } catch (e) {}

      // Check for Authorization header - pass through if present
      const authHeader = req.headers['authorization'];

      const options = {
        hostname: S5_HOST,
        port: S5_PORT,
        path: req.url,
        method: 'POST',
        headers: authHeader 
          ? { 'Content-Type': 'application/json', 'Authorization': authHeader, 'Host': 'localhost' }
          : { 'Content-Type': 'application/json', 'Host': 'localhost' },
        family: 4
      };

      const proxyReq = http.request(options, (proxyRes) => {
        let proxyBody = '';
        proxyRes.on('data', chunk => proxyBody += chunk);
        proxyRes.on('end', () => {
          Object.keys(proxyRes.headers).forEach(key => {
            if (key.toLowerCase() !== 'content-length') {
              res.setHeader(key, proxyRes.headers[key]);
            }
          });

          let responseData = {};
          try {
            if (proxyBody && proxyBody.trim()) {
              responseData = JSON.parse(proxyBody);
            }
          } catch (e) {}

          // Only for a request the node ACCEPTED (2xx); see the note in register-host/complete.
          const accepted = proxyRes.statusCode >= 200 && proxyRes.statusCode < 300;
          if (accepted && !responseData.authToken && pubKey && (req.url === '/s5/account/register' || req.url === '/s5/account/login')) {
            console.log(`[Proxy] No authToken, checking DB for: ${pubKey.substring(0, 20)}...`);
            setTimeout(() => {
              const dbToken = getAuthToken(pubKey);
              if (dbToken) {
                console.log(`[Proxy] Found token: ${dbToken.substring(0, 20)}...`);
                responseData.authToken = dbToken;
              }
              res.setHeader('Content-Type', 'application/json');
              res.writeHead(proxyRes.statusCode);
              res.end(JSON.stringify(responseData));
            }, 150);
          } else {
            res.setHeader('Content-Type', 'application/json');
            res.writeHead(proxyRes.statusCode);
            res.end(JSON.stringify(responseData));
          }
        });
      });

      proxyReq.on('error', (e) => {
        console.error('[Proxy] POST Error:', e.message);
        res.writeHead(502);
        res.end(JSON.stringify({ error: 'Proxy error: ' + e.message }));
      });

      proxyReq.write(reqBody);
      proxyReq.end();
    });
    return;
  }

  // For all other requests, pipe directly
  const options = {
    hostname: S5_HOST,
    port: S5_PORT,
    path: req.url,
    method: req.method,
    headers: { ...req.headers, host: 'localhost' },
    family: 4
  };

  const proxyReq = http.request(options, (proxyRes) => {
    Object.keys(proxyRes.headers).forEach(key => {
      res.setHeader(key, proxyRes.headers[key]);
    });
    res.writeHead(proxyRes.statusCode);
    proxyRes.pipe(res);
  });

  proxyReq.on('error', (e) => {
    console.error('[Proxy] Error:', e.message);
    res.writeHead(502);
    res.end(JSON.stringify({ error: 'Proxy error: ' + e.message }));
  });

  req.pipe(proxyReq);
});

server.listen(PORT, process.env.S5_PROXY_BIND || '127.0.0.1', () => {
  console.log(`S5 Auth Proxy running on port ${PORT}`);
  console.log(`NodeRegistry: ${NODE_REGISTRY_ADDRESS}`);
  console.log(`Chain: Base Sepolia`);
});

if (require.main !== module) module.exports = { newFormatBlobKey, legacyBlobKey };
