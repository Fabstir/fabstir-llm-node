# Max Tokens Truncation Fix - v8.4.1

## Problem

User's SDK developer reported that responses were being truncated at ~250 tokens despite the SDK sending `max_tokens: 4000` in WebSocket requests.

## Root Cause

The encrypted message handling in `/workspace/src/api/server.rs` had a bug at **lines 1461-1463**:

```rust
"max_tokens": json_msg.get("max_tokens")  // ← Looking in WRONG place!
    .and_then(|v| v.as_u64())
    .unwrap_or(100),  // ← Always defaulted to 100
```

### Why This Failed

1. **SDK sends encrypted JSON payload** containing the entire request (including `max_tokens: 4000`)
2. **Node decrypts only the prompt string** (line 1441), not the full JSON object
3. **Node looks for `max_tokens`** in the outer encrypted message wrapper (`json_msg`), not in the decrypted payload
4. **Since not found**, it defaults to 100

### Message Flow

**SDK sends:**
```json
{
  "type": "encrypted_message",
  "session_id": "...",
  "payload": {
    "ciphertextHex": "0xABC...",  // Contains: {"prompt": "...", "max_tokens": 4000}
    "nonceHex": "0x...",
    "aadHex": "0x..."
  }
  // NO max_tokens here!
}
```

**Node behavior:**
- ❌ **Old code**: Looks for `max_tokens` in outer wrapper → Not found → Defaults to 100
- ✅ **New code**: Parses decrypted payload as JSON → Extracts `max_tokens: 4000` → Uses 4000

## The Fix

### 1. Encrypted Message Path (lines 1438-1500)

**Changed:**
- Parse decrypted payload as JSON, not just as a prompt string
- Extract `max_tokens`, `temperature`, `stream`, etc. from decrypted JSON
- Fall back to outer message fields if not in decrypted payload
- Increased default from 100 to 4000

**Priority order:**
1. Look in decrypted JSON first
2. Fall back to outer message wrapper
3. Use default: 4000 (was 100)

### 2. Plaintext Fallback Path (lines 1936-1944)

**Changed:**
- Added missing required `"model"` field
- Increased `max_tokens` default from 100 to 4000

## Evidence from Production Logs

```
2025-11-19T05:49:18.125020Z  INFO fabstir_llm_node::api::server:
  Streaming inference request: model=..., prompt_len=956, max_tokens=100
```

All requests showed `max_tokens=100` even though SDK was sending 4000.

## Testing

1. **Build command:**
   ```bash
   cargo build --release --features real-ezkl -j 4
   ```

2. **Binary size:** 986M (includes Risc0 guest program)

3. **Tarball:** `fabstir-llm-node-v8.4.1-MAX-TOKENS-FIX.tar.gz` (555M)

## Expected Behavior After Fix

When SDK sends `max_tokens: 4000` in encrypted payload, logs should show:

```
Streaming inference request: model=..., prompt_len=..., max_tokens=4000
```

Responses should no longer be truncated at ~250 tokens.

## Files Modified

- `/workspace/src/api/server.rs` (lines 1438-1500, 1936-1944)

## Deployment

1. Extract tarball on production server
2. Restart Docker container
3. Monitor logs for `max_tokens=4000` in streaming requests
4. Test with SDK to verify full responses (no truncation)

## Version

- Binary version: v8.4.1-s5-integration-tests-2025-11-15
- Fix applied: 2025-11-19
- Tarball: `fabstir-llm-node-v8.4.1-MAX-TOKENS-FIX.tar.gz`
