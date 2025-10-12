# Node Encryption Implementation Guide

## Overview

**IMPORTANT**: As of SDK Phase 6.2 (January 2025), all client sessions use **end-to-end encryption by default**. Production nodes MUST support encrypted WebSocket messages to work with SDK clients.

This guide explains how to implement decryption on the node side to handle:
- Encrypted session initialization (`encrypted_session_init`)
- Encrypted prompt messages (`encrypted_message`)
- Encrypted response messages (`encrypted_chunk`, `encrypted_response`)

**Backward Compatibility**: Nodes should continue supporting plaintext messages for clients that explicitly opt-out with `encryption: false`.

---

## Encryption Protocol Summary

### Session Initialization (Ephemeral-Static ECDH)

1. **Client** generates ephemeral keypair (random, discarded after session)
2. **Client** performs ECDH with node's static public key → `shared_secret`
3. **Client** derives encryption key from `shared_secret` using HKDF-SHA256
4. **Client** encrypts session payload (includes random `sessionKey` for subsequent messages)
5. **Client** signs encrypted payload with wallet private key (ECDSA)
6. **Client** sends `encrypted_session_init` message

**Node receives**:
```json
{
  "type": "encrypted_session_init",
  "chain_id": 84532,
  "session_id": "123",
  "payload": {
    "ephPubHex": "0x...",      // Client's ephemeral public key (33 bytes compressed)
    "ciphertextHex": "0x...",   // Encrypted session data
    "nonceHex": "0x...",        // 24-byte nonce for XChaCha20-Poly1305
    "sigHex": "0x...",          // ECDSA signature (65 bytes)
    "aadHex": "0x..."           // Additional authenticated data (optional)
  }
}
```

**Node must**:
1. Perform ECDH with client's ephemeral public key + node's private key → `shared_secret`
2. Derive decryption key from `shared_secret` using HKDF-SHA256
3. Decrypt `ciphertextHex` using XChaCha20-Poly1305
4. Recover client address from `sigHex` signature
5. Verify client is authorized (check ClientManager allowlist if implemented)
6. Extract `sessionKey` from decrypted payload
7. Store `sessionKey` in memory for this session (used for subsequent messages)

**Decrypted payload structure**:
```json
{
  "jobId": "123",
  "modelName": "llama-3",
  "sessionKey": "0x...",        // 32-byte random key (hex-encoded)
  "pricePerToken": 2000
}
```

### Message Streaming (Symmetric Encryption)

After session initialization, all messages use **symmetric encryption** with the `sessionKey` extracted from the session init payload.

**Client sends**:
```json
{
  "type": "encrypted_message",
  "nonceHex": "0x...",          // 24-byte nonce
  "ciphertextHex": "0x...",     // Encrypted prompt
  "aadHex": "0x..."             // Authenticated data (message index + timestamp)
}
```

**Node must**:
1. Retrieve `sessionKey` from memory (set during session init)
2. Decrypt `ciphertextHex` using XChaCha20-Poly1305 with `sessionKey`
3. Verify AAD (prevents replay attacks)
4. Extract plaintext prompt
5. Process inference with LLM
6. Encrypt LLM response chunks with `sessionKey`
7. Send encrypted chunks back to client

**Node sends (streaming)**:
```json
{
  "type": "encrypted_chunk",
  "nonceHex": "0x...",
  "ciphertextHex": "0x...",     // Encrypted token
  "aadHex": "0x..."
}
```

**Node sends (complete response)**:
```json
{
  "type": "encrypted_response",
  "nonceHex": "0x...",
  "ciphertextHex": "0x...",     // Encrypted full response
  "aadHex": "0x..."
}
```

---

## Implementation Steps

### Step 1: Install Cryptographic Libraries

**Recommended (matches SDK)**:
```bash
npm install @noble/secp256k1 @noble/ciphers @noble/hashes
```

**Alternative (Node.js native)**:
```bash
# Use built-in crypto module (requires more manual work)
```

### Step 2: Implement ECDH Key Exchange

```typescript
import { secp256k1 } from '@noble/curves/secp256k1';
import { hkdf } from '@noble/hashes/hkdf';
import { sha256 } from '@noble/hashes/sha256';
import { hexToBytes, bytesToHex } from '@noble/hashes/utils';

/**
 * Perform ECDH with client's ephemeral public key
 * @param clientEphPubHex - Client's ephemeral public key (hex string, 33 bytes compressed)
 * @param nodePrivateKey - Node's private key (32 bytes)
 * @returns Derived encryption key (32 bytes)
 */
function deriveSharedKey(
  clientEphPubHex: string,
  nodePrivateKey: Uint8Array
): Uint8Array {
  // 1. Parse client's ephemeral public key
  const clientEphPub = hexToBytes(clientEphPubHex);

  // 2. Perform ECDH: shared_point = client_eph_pub * node_private_key
  const sharedPoint = secp256k1.getSharedSecret(
    nodePrivateKey,
    clientEphPub,
    true // compressed
  );

  // 3. Derive encryption key using HKDF-SHA256
  // HKDF extracts entropy from shared secret and expands to 32-byte key
  const sharedSecret = sharedPoint.slice(1); // Remove 0x02/0x03 prefix
  const derivedKey = hkdf(sha256, sharedSecret, undefined, undefined, 32);

  return derivedKey;
}
```

### Step 3: Decrypt Session Init Payload

```typescript
import { xchacha20poly1305 } from '@noble/ciphers/chacha';

/**
 * Decrypt encrypted_session_init message
 * @param payload - Encrypted payload from client
 * @param nodePrivateKey - Node's private key
 * @returns Decrypted session data + client address
 */
async function decryptSessionInit(
  payload: {
    ephPubHex: string;
    ciphertextHex: string;
    nonceHex: string;
    sigHex: string;
    aadHex?: string;
  },
  nodePrivateKey: Uint8Array
): Promise<{
  jobId: string;
  modelName: string;
  sessionKey: string;
  pricePerToken: number;
  clientAddress: string;
}> {
  // 1. Derive shared key via ECDH
  const sharedKey = deriveSharedKey(payload.ephPubHex, nodePrivateKey);

  // 2. Decrypt ciphertext using XChaCha20-Poly1305
  const cipher = xchacha20poly1305(sharedKey, hexToBytes(payload.nonceHex));
  const aad = payload.aadHex ? hexToBytes(payload.aadHex) : undefined;
  const plaintext = cipher.decrypt(hexToBytes(payload.ciphertextHex), aad);

  // 3. Parse decrypted JSON
  const sessionData = JSON.parse(new TextDecoder().decode(plaintext));

  // 4. Recover client address from signature
  const messageHash = sha256(hexToBytes(payload.ciphertextHex));
  const signature = hexToBytes(payload.sigHex);
  const recoveredPubKey = secp256k1.Signature.fromCompact(signature.slice(0, 64))
    .addRecoveryBit(signature[64])
    .recoverPublicKey(messageHash);

  // 5. Derive Ethereum address from public key
  const pubKeyUncompressed = recoveredPubKey.toRawBytes(false).slice(1); // Remove 0x04 prefix
  const addressHash = sha256(pubKeyUncompressed);
  const clientAddress = '0x' + bytesToHex(addressHash.slice(-20));

  return {
    ...sessionData,
    clientAddress
  };
}
```

### Step 4: Store Session Key in Memory

```typescript
// In-memory session storage
const sessionKeys = new Map<string, Uint8Array>();

/**
 * Store session key for subsequent messages
 */
function storeSessionKey(sessionId: string, sessionKeyHex: string): void {
  const sessionKey = hexToBytes(sessionKeyHex);
  sessionKeys.set(sessionId, sessionKey);

  console.log(`[Node] Session key stored for session ${sessionId}`);
}

/**
 * Retrieve session key for decryption
 */
function getSessionKey(sessionId: string): Uint8Array | undefined {
  return sessionKeys.get(sessionId);
}

/**
 * Clean up session key after session ends
 */
function clearSessionKey(sessionId: string): void {
  sessionKeys.delete(sessionId);
  console.log(`[Node] Session key cleared for session ${sessionId}`);
}
```

### Step 5: Decrypt Incoming Messages

```typescript
/**
 * Decrypt encrypted_message from client
 * @param message - Encrypted message payload
 * @param sessionId - Session ID to retrieve session key
 * @returns Decrypted prompt text
 */
function decryptMessage(
  message: {
    type: 'encrypted_message';
    nonceHex: string;
    ciphertextHex: string;
    aadHex: string;
  },
  sessionId: string
): string {
  // 1. Retrieve session key
  const sessionKey = getSessionKey(sessionId);
  if (!sessionKey) {
    throw new Error(`Session key not found for session ${sessionId}`);
  }

  // 2. Decrypt with XChaCha20-Poly1305
  const cipher = xchacha20poly1305(sessionKey, hexToBytes(message.nonceHex));
  const aad = hexToBytes(message.aadHex);
  const plaintext = cipher.decrypt(hexToBytes(message.ciphertextHex), aad);

  // 3. Convert to string
  return new TextDecoder().decode(plaintext);
}
```

### Step 6: Encrypt Outgoing Responses

```typescript
/**
 * Encrypt LLM response chunk for streaming
 * @param chunk - Response text chunk
 * @param sessionId - Session ID to retrieve session key
 * @param messageIndex - Sequential message index (for AAD)
 * @returns Encrypted message payload
 */
function encryptChunk(
  chunk: string,
  sessionId: string,
  messageIndex: number
): {
  type: 'encrypted_chunk';
  nonceHex: string;
  ciphertextHex: string;
  aadHex: string;
} {
  // 1. Retrieve session key
  const sessionKey = getSessionKey(sessionId);
  if (!sessionKey) {
    throw new Error(`Session key not found for session ${sessionId}`);
  }

  // 2. Generate random nonce (24 bytes for XChaCha20)
  const nonce = crypto.getRandomValues(new Uint8Array(24));

  // 3. Prepare AAD (authenticated data)
  const aad = new TextEncoder().encode(
    JSON.stringify({
      message_index: messageIndex,
      timestamp: Date.now(),
      session_id: sessionId
    })
  );

  // 4. Encrypt chunk
  const cipher = xchacha20poly1305(sessionKey, nonce);
  const plaintext = new TextEncoder().encode(chunk);
  const ciphertext = cipher.encrypt(plaintext, aad);

  return {
    type: 'encrypted_chunk',
    nonceHex: bytesToHex(nonce),
    ciphertextHex: bytesToHex(ciphertext),
    aadHex: bytesToHex(aad)
  };
}

/**
 * Encrypt complete LLM response (non-streaming)
 */
function encryptResponse(
  response: string,
  sessionId: string,
  messageIndex: number
): {
  type: 'encrypted_response';
  nonceHex: string;
  ciphertextHex: string;
  aadHex: string;
} {
  const encrypted = encryptChunk(response, sessionId, messageIndex);
  return {
    ...encrypted,
    type: 'encrypted_response'
  };
}
```

### Step 7: WebSocket Message Handler

```typescript
import { WebSocket } from 'ws';

/**
 * Handle incoming WebSocket message
 */
async function handleWebSocketMessage(
  ws: WebSocket,
  message: any,
  nodePrivateKey: Uint8Array
): Promise<void> {
  const data = JSON.parse(message.toString());

  // Check message type
  if (data.type === 'encrypted_session_init') {
    // Decrypt session init
    const { jobId, modelName, sessionKey, pricePerToken, clientAddress } =
      await decryptSessionInit(data.payload, nodePrivateKey);

    // Verify client is authorized (optional - check ClientManager contract)
    // const isAuthorized = await verifyClientAllowlist(clientAddress);
    // if (!isAuthorized) {
    //   ws.send(JSON.stringify({ type: 'error', message: 'Unauthorized client' }));
    //   ws.close();
    //   return;
    // }

    // Store session key for subsequent messages
    storeSessionKey(data.session_id, sessionKey);

    console.log(`[Node] Session ${data.session_id} initialized (encrypted)`);
    console.log(`  Client: ${clientAddress}`);
    console.log(`  Model: ${modelName}`);
    console.log(`  Price: ${pricePerToken}`);

    // Send acknowledgment (plaintext OK for non-sensitive metadata)
    ws.send(JSON.stringify({
      type: 'session_init_ack',
      session_id: data.session_id,
      status: 'active'
    }));

  } else if (data.type === 'encrypted_message') {
    // Decrypt prompt
    const prompt = decryptMessage(data, data.session_id || '');

    console.log(`[Node] Received encrypted prompt: ${prompt.substring(0, 50)}...`);

    // Process LLM inference
    const llmResponse = await generateLLMResponse(prompt, data.model);

    // Encrypt and send response chunks (streaming)
    let messageIndex = 0;
    for (const chunk of llmResponse.split(' ')) {
      const encrypted = encryptChunk(chunk + ' ', data.session_id, messageIndex++);
      ws.send(JSON.stringify(encrypted));
    }

    // Send completion marker
    ws.send(JSON.stringify({
      type: 'stream_complete',
      session_id: data.session_id
    }));

  } else if (data.type === 'session_init') {
    // Plaintext session init (backward compatible)
    console.warn(`[Node] DEPRECATED: Plaintext session ${data.session_id} (encryption recommended)`);

    // Handle plaintext session (existing logic)
    // ...

  } else if (data.type === 'prompt') {
    // Plaintext prompt (backward compatible)
    console.warn(`[Node] DEPRECATED: Plaintext prompt in session ${data.session_id}`);

    // Handle plaintext prompt (existing logic)
    // ...
  }
}
```

---

## Error Handling

### Decryption Failures

```typescript
try {
  const decrypted = cipher.decrypt(ciphertext, aad);
} catch (error) {
  console.error('[Node] Decryption failed:', error);

  // Send error to client
  ws.send(JSON.stringify({
    type: 'error',
    code: 'DECRYPTION_FAILED',
    message: 'Failed to decrypt message. Session may be corrupted.',
    session_id: sessionId
  }));

  // Close session
  clearSessionKey(sessionId);
  ws.close();
}
```

### Invalid Signature

```typescript
try {
  const clientAddress = recoverAddressFromSignature(signature, messageHash);
} catch (error) {
  console.error('[Node] Signature recovery failed:', error);

  ws.send(JSON.stringify({
    type: 'error',
    code: 'INVALID_SIGNATURE',
    message: 'Failed to verify client signature'
  }));

  ws.close();
}
```

### Missing Session Key

```typescript
const sessionKey = getSessionKey(sessionId);
if (!sessionKey) {
  console.error(`[Node] Session key not found for ${sessionId}`);

  ws.send(JSON.stringify({
    type: 'error',
    code: 'SESSION_KEY_NOT_FOUND',
    message: 'Session not initialized. Send encrypted_session_init first.'
  }));

  return;
}
```

---

## Backward Compatibility

### Supporting Both Encrypted and Plaintext Sessions

```typescript
function handleMessage(ws: WebSocket, data: any): void {
  // Check message type prefix
  if (data.type.startsWith('encrypted_')) {
    // Encrypted message - use new crypto logic
    handleEncryptedMessage(ws, data);
  } else {
    // Plaintext message - use existing logic
    handlePlaintextMessage(ws, data);

    // Log deprecation warning
    console.warn(
      `[Node] Plaintext session ${data.session_id || 'unknown'} detected. ` +
      `Encryption is recommended for privacy and security.`
    );
  }
}
```

### Graceful Degradation

If node doesn't support encryption yet:

```typescript
if (data.type === 'encrypted_session_init') {
  // Inform client that encryption is not supported
  ws.send(JSON.stringify({
    type: 'error',
    code: 'ENCRYPTION_NOT_SUPPORTED',
    message: 'This node does not support encrypted sessions. Please disable encryption.'
  }));

  ws.close();
}
```

**Note**: This should be temporary. All production nodes should implement encryption ASAP.

---

## Testing

### Unit Tests

```typescript
describe('Node Encryption', () => {
  it('should decrypt session init payload', async () => {
    const nodePrivateKey = secp256k1.utils.randomPrivateKey();
    const clientPrivateKey = secp256k1.utils.randomPrivateKey();

    // Client encrypts payload
    const encrypted = await clientEncryptSessionInit(
      nodePublicKey,
      { jobId: '123', modelName: 'llama-3', sessionKey: '0x...', pricePerToken: 2000 },
      clientPrivateKey
    );

    // Node decrypts payload
    const decrypted = await decryptSessionInit(encrypted.payload, nodePrivateKey);

    expect(decrypted.jobId).toBe('123');
    expect(decrypted.modelName).toBe('llama-3');
    expect(decrypted.clientAddress).toBe(getAddressFromPrivateKey(clientPrivateKey));
  });

  it('should encrypt and decrypt messages', () => {
    const sessionKey = crypto.getRandomValues(new Uint8Array(32));
    const message = 'What is 2+2?';

    // Client encrypts message
    const encrypted = encryptMessage(sessionKey, message, 0);

    // Node decrypts message
    const decrypted = decryptMessage(encrypted, sessionKey);

    expect(decrypted).toBe(message);
  });
});
```

### Integration Tests with SDK

```bash
# Test encrypted session flow
npm test tests/integration/encrypted-session.test.ts

# Test plaintext fallback
npm test tests/integration/plaintext-session.test.ts
```

---

## Performance Considerations

### Encryption Overhead

- **Session init**: ~10ms (one-time per session)
- **Message encryption/decryption**: <1ms per message
- **Impact**: Negligible compared to LLM inference time (1-10 seconds)

### Optimization Tips

1. **Reuse session keys**: Don't re-derive keys for each message
2. **Cache derived keys**: ECDH is expensive, cache shared secrets if possible
3. **Parallel encryption**: Encrypt response chunks in parallel for streaming

---

## Security Considerations

### Do's ✅

- **Always verify client signatures** to prevent impersonation
- **Store session keys in memory only** (never persist to disk)
- **Clear session keys** when session ends
- **Use authenticated encryption** (XChaCha20-Poly1305 includes MAC)
- **Validate AAD** to prevent replay attacks
- **Log encryption errors** for debugging

### Don'ts ❌

- **Never reuse nonces** with the same key
- **Never log session keys** or decrypted data
- **Never skip signature verification**
- **Never persist session keys** to database or files
- **Never use ECB mode** or unauthenticated ciphers

---

## Troubleshooting

### Problem: "Decryption failed" errors

**Cause**: Key mismatch or corrupted ciphertext

**Solution**:
1. Verify node's public key is correctly registered on-chain
2. Check that client is using latest SDK version (Phase 6.2+)
3. Ensure nonces are not being reused
4. Log ciphertext hex for debugging (but never log plaintext!)

### Problem: "Session key not found"

**Cause**: Session init not processed or session expired

**Solution**:
1. Check that `encrypted_session_init` was received and processed
2. Verify session key is stored in memory after init
3. Check session timeout logic (keys may be cleared prematurely)

### Problem: "Invalid signature"

**Cause**: Signature recovery failing or wrong message hash

**Solution**:
1. Ensure signature is 65 bytes (r + s + v)
2. Verify recovery bit (v) is correct (27/28 or 0/1)
3. Hash the correct data (ciphertext, not plaintext)

---

## Migration Path

### Phase 1: Add Encryption Support (Week 1-2)
- Implement ECDH and XChaCha20-Poly1305 decryption
- Handle `encrypted_session_init` and `encrypted_message`
- Test with SDK clients (encrypted sessions)

### Phase 2: Deprecate Plaintext (Week 3-4)
- Log warnings for plaintext sessions
- Update documentation to recommend encryption
- Monitor adoption metrics

### Phase 3: Enforce Encryption (Future)
- Reject plaintext sessions (return error)
- Remove plaintext handling code
- Require all clients to use encryption

---

## Additional Resources

- **SDK Encryption Guide**: `docs/ENCRYPTION_GUIDE.md`
- **SDK Encryption FAQ**: `docs/ENCRYPTION_FAQ.md`
- **Implementation Plan**: `docs/IMPLEMENTATION-CRYPTO.md`
- **Noble Crypto Libraries**: https://github.com/paulmillr/noble-ciphers
- **XChaCha20-Poly1305 Spec**: https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-xchacha

---

## Support

For questions or issues:
- **SDK Issues**: https://github.com/fabstir/fabstir-llm-sdk/issues
- **Node Issues**: https://github.com/fabstir/fabstir-llm-node/issues
- **Security**: security@fabstir.com (DO NOT disclose publicly)
