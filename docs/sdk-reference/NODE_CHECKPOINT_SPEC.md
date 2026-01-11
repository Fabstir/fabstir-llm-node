# Node Checkpoint Publishing Specification

## Overview

This document specifies how host nodes should publish conversation checkpoints to S5 storage for SDK recovery. When sessions timeout or disconnect, the SDK can recover conversation state up to the last proven checkpoint.

**SDK Version**: 1.9.0+
**Status**: Implementation Required
**Priority**: Critical for MVP

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         CHECKPOINT PUBLISHING FLOW                           │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  At each proof submission (~1000 tokens):                                    │
│                                                                              │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐                 │
│  │ 1. Generate  │ ──► │ 2. Upload    │ ──► │ 3. Update    │                 │
│  │    Delta     │     │    to S5     │     │    Index     │                 │
│  └──────────────┘     └──────────────┘     └──────────────┘                 │
│         │                    │                    │                          │
│         ▼                    ▼                    ▼                          │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐                 │
│  │ 4. Sign with │ ──► │ 5. Submit    │ ──► │ 6. Proof     │                 │
│  │    Host Key  │     │    Proof     │     │    Verified  │                 │
│  └──────────────┘     └──────────────┘     └──────────────┘                 │
│                                                                              │
│  IMPORTANT: Steps 1-4 MUST complete BEFORE step 5 (proof submission)        │
│                                                                              │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## S5 Path Convention

### Checkpoint Index Path
```
home/checkpoints/{hostAddress}/{sessionId}/index.json
```

### Delta Storage
Deltas are stored at S5 CIDs. The CID is recorded in the index.

### Path Examples
```
# Index for session 123 from host 0xABC...
home/checkpoints/0xabc123def456789012345678901234567890abcd/123/index.json

# Delta CID (stored separately, referenced in index)
s5://bafybeig123...  (content-addressed)
```

**Important**: Host addresses in paths MUST be lowercase.

---

## Data Formats

### CheckpointDelta (Delta File)

Each delta contains only the NEW messages since the last checkpoint.

```json
{
  "sessionId": "123",
  "checkpointIndex": 0,
  "proofHash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
  "startToken": 0,
  "endToken": 1000,
  "messages": [
    {
      "role": "user",
      "content": "Explain quantum computing",
      "timestamp": 1704844800000
    },
    {
      "role": "assistant",
      "content": "Quantum computing uses quantum mechanical phenomena...",
      "timestamp": 1704844805000,
      "metadata": {
        "partial": true
      }
    }
  ],
  "hostSignature": "0x..."
}
```

#### Field Descriptions

| Field | Type | Description |
|-------|------|-------------|
| `sessionId` | string | Session ID (matches on-chain session) |
| `checkpointIndex` | number | 0-based index of this checkpoint |
| `proofHash` | string | bytes32 keccak256 hash of proof data (matches on-chain) |
| `startToken` | number | Token count at start of this delta |
| `endToken` | number | Token count at end of this delta |
| `messages` | array | Messages added since last checkpoint |
| `hostSignature` | string | EIP-191 signature (see Signature Requirements) |

#### Message Object

| Field | Type | Description |
|-------|------|-------------|
| `role` | string | "user" or "assistant" |
| `content` | string | Message content |
| `timestamp` | number | Unix timestamp in milliseconds |
| `metadata` | object | Optional. Set `partial: true` if message continues in next delta |

### CheckpointIndex (Index File)

The index lists all checkpoints for a session.

```json
{
  "sessionId": "123",
  "hostAddress": "0xabc123def456789012345678901234567890abcd",
  "checkpoints": [
    {
      "index": 0,
      "proofHash": "0x1234...",
      "deltaCID": "s5://bafybeig1...",
      "tokenRange": [0, 1000],
      "timestamp": 1704844800000
    },
    {
      "index": 1,
      "proofHash": "0x5678...",
      "deltaCID": "s5://bafybeig2...",
      "tokenRange": [1000, 2000],
      "timestamp": 1704844860000
    }
  ],
  "hostSignature": "0x..."
}
```

#### Field Descriptions

| Field | Type | Description |
|-------|------|-------------|
| `sessionId` | string | Session ID |
| `hostAddress` | string | Host's Ethereum address (lowercase) |
| `checkpoints` | array | List of checkpoint entries |
| `hostSignature` | string | EIP-191 signature of checkpoints array |

#### Checkpoint Entry

| Field | Type | Description |
|-------|------|-------------|
| `index` | number | 0-based checkpoint index |
| `proofHash` | string | bytes32 proof hash (matches on-chain) |
| `deltaCID` | string | S5 CID where delta is stored |
| `tokenRange` | [number, number] | [startToken, endToken] tuple |
| `timestamp` | number | Unix timestamp when checkpoint was created |

---

## Signature Requirements

### Signing Algorithm
- **Standard**: EIP-191 (Ethereum Signed Message)
- **Curve**: secp256k1
- **Hash**: keccak256

### What to Sign

#### Delta Signature
Sign the JSON-stringified messages array with sorted keys:

```python
import json
from eth_account.messages import encode_defunct
from web3 import Web3

def sign_delta(messages, private_key):
    # Sort keys deterministically
    sorted_messages = json.dumps(messages, sort_keys=True, separators=(',', ':'))

    # EIP-191 sign
    message = encode_defunct(text=sorted_messages)
    signed = Web3().eth.account.sign_message(message, private_key)

    return signed.signature.hex()
```

#### Index Signature
Sign the JSON-stringified checkpoints array:

```python
def sign_index(checkpoints, private_key):
    sorted_checkpoints = json.dumps(checkpoints, sort_keys=True, separators=(',', ':'))

    message = encode_defunct(text=sorted_checkpoints)
    signed = Web3().eth.account.sign_message(message, private_key)

    return signed.signature.hex()
```

### Signature Format
- 65 bytes: r (32) + s (32) + v (1)
- Hex string with 0x prefix: `0x` + 130 hex characters
- Example: `0x1234...abcd` (132 characters total)

---

## Timing Requirements

### Critical Order of Operations

```
1. Generate proof data
2. Compute proofHash = keccak256(proof_data)
3. Create delta with messages since last checkpoint
4. Sign delta
5. Upload delta to S5 → get deltaCID
6. Update index with new checkpoint entry
7. Sign index
8. Upload index to S5
9. Submit proof to chain (with proofHash)
```

**IMPORTANT**: Steps 3-8 MUST complete BEFORE step 9.

If S5 upload fails, do NOT submit the proof. Retry S5 upload first.

### Why This Order Matters
- SDK verifies `checkpoint.proofHash === onChainProof.proofHash`
- If proof is on-chain but checkpoint isn't in S5, SDK cannot recover
- Unrecoverable data is worthless to the user

---

## Error Handling

### S5 Upload Failure

```python
MAX_RETRIES = 3
RETRY_DELAY = 1.0  # seconds

async def upload_with_retry(s5_client, path, data):
    for attempt in range(MAX_RETRIES):
        try:
            return await s5_client.fs.put(path, data)
        except Exception as e:
            if attempt < MAX_RETRIES - 1:
                await asyncio.sleep(RETRY_DELAY * (attempt + 1))
            else:
                raise CheckpointUploadError(f"S5 upload failed after {MAX_RETRIES} attempts: {e}")

    # DO NOT submit proof if upload failed
```

### Index Update Failure

If index update fails but delta uploaded:
1. Log the orphaned delta CID
2. Retry index update
3. If still failing, do NOT submit proof
4. Clean up orphaned delta later

### Proof Hash Mismatch

If `proofHash` in checkpoint doesn't match computed hash:
1. This indicates a bug in your code
2. Recompute proofHash before signing
3. Never publish mismatched hashes

---

## Cleanup Policy

### When to Delete Checkpoints

| Event | Action |
|-------|--------|
| Session completed normally | Keep for 7 days, then delete |
| Session timeout | Keep for 30 days (dispute window) |
| Session cancelled | Delete immediately |
| Dispute opened | Keep until dispute resolved + 7 days |

### How to Delete

```python
async def cleanup_checkpoints(s5_client, host_address, session_id):
    index_path = f"home/checkpoints/{host_address.lower()}/{session_id}/index.json"

    # Fetch index to get delta CIDs
    try:
        index = await s5_client.fs.get(index_path)

        # Delete all deltas
        for checkpoint in index.get('checkpoints', []):
            try:
                await s5_client.fs.delete(checkpoint['deltaCID'])
            except:
                pass  # Ignore if already deleted

        # Delete index
        await s5_client.fs.delete(index_path)
    except:
        pass  # Index doesn't exist
```

---

## Complete Python Example

```python
import json
import asyncio
from dataclasses import dataclass, asdict
from typing import List, Optional
from eth_account.messages import encode_defunct
from web3 import Web3

@dataclass
class Message:
    role: str
    content: str
    timestamp: int
    metadata: Optional[dict] = None

@dataclass
class CheckpointDelta:
    sessionId: str
    checkpointIndex: int
    proofHash: str
    startToken: int
    endToken: int
    messages: List[dict]
    hostSignature: str

@dataclass
class CheckpointEntry:
    index: int
    proofHash: str
    deltaCID: str
    tokenRange: List[int]
    timestamp: int

@dataclass
class CheckpointIndex:
    sessionId: str
    hostAddress: str
    checkpoints: List[dict]
    hostSignature: str


class CheckpointPublisher:
    def __init__(self, s5_client, host_address: str, private_key: str):
        self.s5 = s5_client
        self.host_address = host_address.lower()
        self.private_key = private_key
        self.w3 = Web3()

    def _sign_message(self, data: str) -> str:
        """EIP-191 sign a message."""
        message = encode_defunct(text=data)
        signed = self.w3.eth.account.sign_message(message, self.private_key)
        return signed.signature.hex()

    def _sort_and_stringify(self, obj) -> str:
        """Deterministic JSON stringification with sorted keys."""
        return json.dumps(obj, sort_keys=True, separators=(',', ':'))

    async def publish_checkpoint(
        self,
        session_id: str,
        checkpoint_index: int,
        proof_hash: str,
        start_token: int,
        end_token: int,
        messages: List[Message],
        proof_data: bytes
    ) -> str:
        """
        Publish a checkpoint. Call this BEFORE submitting proof to chain.

        Returns: deltaCID
        """
        # 1. Create delta
        messages_dict = [asdict(m) for m in messages]

        # 2. Sign messages
        messages_json = self._sort_and_stringify(messages_dict)
        delta_signature = self._sign_message(messages_json)

        delta = CheckpointDelta(
            sessionId=session_id,
            checkpointIndex=checkpoint_index,
            proofHash=proof_hash,
            startToken=start_token,
            endToken=end_token,
            messages=messages_dict,
            hostSignature=delta_signature
        )

        # 3. Upload delta to S5
        delta_cid = await self._upload_with_retry(asdict(delta))

        # 4. Update index
        await self._update_index(
            session_id=session_id,
            checkpoint_index=checkpoint_index,
            proof_hash=proof_hash,
            delta_cid=delta_cid,
            token_range=[start_token, end_token]
        )

        return delta_cid

    async def _upload_with_retry(self, data: dict, max_retries: int = 3) -> str:
        """Upload data to S5 with retry."""
        for attempt in range(max_retries):
            try:
                # S5 returns CID after upload
                result = await self.s5.upload(data)
                return result['cid']
            except Exception as e:
                if attempt < max_retries - 1:
                    await asyncio.sleep(1.0 * (attempt + 1))
                else:
                    raise Exception(f"S5 upload failed: {e}")

    async def _update_index(
        self,
        session_id: str,
        checkpoint_index: int,
        proof_hash: str,
        delta_cid: str,
        token_range: List[int]
    ):
        """Update the checkpoint index."""
        index_path = f"home/checkpoints/{self.host_address}/{session_id}/index.json"

        # Fetch existing index or create new
        try:
            index_data = await self.s5.fs.get(index_path)
            checkpoints = index_data.get('checkpoints', [])
        except:
            checkpoints = []

        # Add new checkpoint entry
        import time
        checkpoints.append({
            'index': checkpoint_index,
            'proofHash': proof_hash,
            'deltaCID': delta_cid,
            'tokenRange': token_range,
            'timestamp': int(time.time() * 1000)
        })

        # Sign checkpoints array
        checkpoints_json = self._sort_and_stringify(checkpoints)
        index_signature = self._sign_message(checkpoints_json)

        # Create index
        index = CheckpointIndex(
            sessionId=session_id,
            hostAddress=self.host_address,
            checkpoints=checkpoints,
            hostSignature=index_signature
        )

        # Upload index
        await self.s5.fs.put(index_path, asdict(index))


# Usage example
async def on_proof_interval(publisher, session_id, messages_since_last, proof_data):
    """Called every ~1000 tokens when proof is ready."""

    # Compute proof hash (must match what goes on-chain)
    proof_hash = Web3.keccak(proof_data).hex()

    # Publish checkpoint FIRST
    delta_cid = await publisher.publish_checkpoint(
        session_id=session_id,
        checkpoint_index=current_checkpoint_index,
        proof_hash=proof_hash,
        start_token=last_checkpoint_tokens,
        end_token=current_tokens,
        messages=messages_since_last,
        proof_data=proof_data
    )

    # THEN submit proof to chain
    await submit_proof_to_chain(
        session_id=session_id,
        tokens_claimed=current_tokens - last_checkpoint_tokens,
        proof_hash=proof_hash,
        signature=host_signature,
        proof_cid=proof_cid
    )
```

---

## Sequence Diagram

```
┌──────┐          ┌──────┐          ┌──────┐          ┌──────────┐
│ Node │          │  S5  │          │Chain │          │   SDK    │
└──┬───┘          └──┬───┘          └──┬───┘          └────┬─────┘
   │                 │                 │                    │
   │  1. Generate Delta                │                    │
   │─────────────────►                 │                    │
   │                 │                 │                    │
   │  2. Upload Delta                  │                    │
   │────────────────►│                 │                    │
   │                 │                 │                    │
   │  3. deltaCID    │                 │                    │
   │◄────────────────│                 │                    │
   │                 │                 │                    │
   │  4. Update Index                  │                    │
   │────────────────►│                 │                    │
   │                 │                 │                    │
   │  5. Submit Proof                  │                    │
   │─────────────────────────────────►│                    │
   │                 │                 │                    │
   │                 │                 │  6. proofHash      │
   │                 │                 │    on-chain        │
   │                 │                 │                    │
   │                 │                 │                    │
   │                 │    [TIMEOUT/DISCONNECT]              │
   │                 │                 │                    │
   │                 │                 │  7. Fetch Index    │
   │                 │◄────────────────────────────────────│
   │                 │                 │                    │
   │                 │  8. Index       │                    │
   │                 │────────────────────────────────────►│
   │                 │                 │                    │
   │                 │                 │  9. Verify proofHash
   │                 │                 │◄───────────────────│
   │                 │                 │                    │
   │                 │                 │  10. Match         │
   │                 │                 │────────────────────►
   │                 │                 │                    │
   │                 │  11. Fetch Deltas                    │
   │                 │◄────────────────────────────────────│
   │                 │                 │                    │
   │                 │  12. Deltas     │                    │
   │                 │────────────────────────────────────►│
   │                 │                 │                    │
   │                 │                 │  13. Verify & Merge│
   │                 │                 │    ┌───────────────┤
   │                 │                 │    │               │
   │                 │                 │    └───────────────►
   │                 │                 │                    │
   │                 │                 │  14. Recovered!    │
   │                 │                 │                    │
```

---

## Verification Checklist

Before submitting a proof, verify:

- [ ] Delta contains only messages since last checkpoint
- [ ] `proofHash` matches `keccak256(proof_data)`
- [ ] Delta signature is valid EIP-191
- [ ] Delta uploaded to S5 successfully
- [ ] Index updated with new checkpoint entry
- [ ] Index signature is valid EIP-191
- [ ] Index uploaded to S5 successfully

---

## Questions?

If you have questions about this specification, please:
1. Check the SDK implementation in `packages/sdk-core/src/utils/checkpoint-recovery.ts`
2. Review the test cases in `packages/sdk-core/tests/unit/checkpoint-recovery.test.ts`
3. Contact the SDK team

---

## Changelog

| Version | Date | Changes |
|---------|------|---------|
| 1.0.0 | 2026-01-11 | Initial specification |
