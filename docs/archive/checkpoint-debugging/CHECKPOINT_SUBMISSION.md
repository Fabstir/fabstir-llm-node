# Checkpoint Submission for Payment Settlement

## Overview
The Fabstir LLM Node now includes automatic checkpoint submission for payment settlement. When nodes process inference requests with a `job_id`, they track token generation and automatically submit checkpoints to the blockchain for payment processing.

## How It Works

### 1. Token Tracking
- Each generated token is tracked per `job_id`
- Tokens accumulate until reaching the `CHECKPOINT_THRESHOLD` (default: 100 tokens)
- Both simple logging and full blockchain submission are supported

### 2. Checkpoint Submission
When the threshold is reached:
1. The node creates a proof of work (currently minimal)
2. Submits a checkpoint transaction to the ProofSystem contract
3. Smart contract automatically distributes payment:
   - 90% to the host (compute provider)
   - 10% to the treasury

### 3. Force Checkpoint
When a session ends, any remaining tokens are submitted as a final checkpoint to ensure hosts are paid for all work performed.

## Configuration

### Environment Variables
```bash
# Required for blockchain submission
HOST_PRIVATE_KEY=<your_ethereum_private_key>

# Optional (defaults shown)
RPC_URL=https://base-sepolia.g.alchemy.com/v2/...
CHAIN_ID=84532  # Base Sepolia
```

### Contract Addresses
The ProofSystem contract is deployed at:
```
0x2ACcc60893872A499700908889B38C5420CBcFD1  # Base Sepolia
```

## API Changes

### InferenceRequest Schema
The inference request now supports payment tracking fields:

```json
{
  "model": "tiny-vicuna",
  "prompt": "Your prompt here",
  "max_tokens": 100,
  "temperature": 0.7,
  "job_id": 12345,           // Optional: Blockchain job ID
  "session_id": "session-123" // Optional: Session identifier
}
```

### WebSocket Integration
The WebSocket handler automatically tracks tokens when a `job_id` is provided:

```javascript
// Example WebSocket request
const request = {
  type: "inference",
  request: {
    model: "tiny-vicuna",
    prompt: "Tell me a story",
    max_tokens: 200,
    job_id: 12345,  // Triggers checkpoint tracking
    session_id: "user-session-001"
  }
};
```

## Architecture

### Components

1. **TokenTracker** (`src/api/token_tracker.rs`)
   - Simple token tracking with logging
   - Used when Web3 is not configured

2. **CheckpointManager** (`src/contracts/checkpoint_manager.rs`)
   - Full blockchain integration
   - Submits actual transactions to smart contracts
   - Manages payment settlement

3. **ApiServer Integration** (`src/api/server.rs`)
   - Tracks tokens during streaming
   - Automatically chooses between TokenTracker and CheckpointManager
   - Forces checkpoint on session end

## Testing

### Run Test Script
```bash
# Test without blockchain (logging only)
cargo run --release

# In another terminal
python3 test_checkpoint_submission.py
```

### Test with Blockchain
```bash
# With actual blockchain submission
HOST_PRIVATE_KEY=<your_test_key> cargo run --release

# In another terminal
python3 test_checkpoint_submission.py
```

### Expected Output
When checkpoint threshold is reached:
```
🔔 CHECKPOINT NEEDED for job 12345 with 100 tokens!
CHECKPOINT SUBMISSION REQUIRED:
- Job ID: 12345
- Tokens to submit: 100
- Session ID: test-session-001
- Contract: ProofSystem at 0x2ACcc60893872A499700908889B38C5420CBcFD1
```

With blockchain enabled:
```
✅ Checkpoint submitted successfully for job 12345 - tx_hash: 0x...
✅ Checkpoint confirmed for job 12345 - payment distributed (90% host, 10% treasury)
```

## Implementation Status

### ✅ Completed
- Token tracking per job_id
- Checkpoint threshold detection
- WebSocket integration
- Force checkpoint on session end
- Simple logging tracker
- Full Web3 checkpoint manager
- Automatic manager selection based on configuration

### 🚧 TODO
- Proper proof generation (currently using placeholder)
- Batch checkpoint submission for efficiency
- Checkpoint recovery on failure
- Gas optimization
- Event monitoring for payment confirmation

## Troubleshooting

### No Checkpoints Being Submitted
1. Verify `job_id` is included in the request
2. Check that enough tokens are generated (>= 100)
3. Ensure HOST_PRIVATE_KEY is set for blockchain submission

### Transaction Failures
1. Check account has sufficient ETH for gas
2. Verify RPC_URL is correct
3. Ensure private key corresponds to registered host

### Logs to Check
```bash
# Token tracking
grep "Generated.*tokens for job" logs.txt

# Checkpoint submission
grep "CHECKPOINT" logs.txt

# Transaction results
grep "tx_hash" logs.txt
```

## Security Considerations
- Never commit HOST_PRIVATE_KEY to version control
- Use environment variables or secure key management
- Test with testnet keys first
- Monitor gas usage and optimize checkpoint frequency

## Contract Integration
The checkpoint submission calls:
```solidity
ProofSystem.submitCheckpoint(
    uint256 jobId,
    uint256 tokensGenerated,
    bytes calldata proof
)
```

This triggers automatic payment distribution according to the job's payment terms.