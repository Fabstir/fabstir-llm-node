# Checkpoint/Proof Submission Implementation Guide

## Critical Information - READ THIS FIRST

### The Production Deployment Process
**ALWAYS follow this exact sequence when deploying changes:**

1. **Build in dev container**:
   ```bash
   cargo build --release
   ```

2. **Copy binary from dev container to host**:
   ```bash
   docker cp fabstir-llm-marketplace-node-dev-1:/workspace/target/release/fabstir-llm-node ./fabstir-llm-node-new
   ```

3. **Verify the binary timestamp** (CRITICAL):
   ```bash
   ls -la --full-time ./fabstir-llm-node-new
   # Should show current time, not old timestamp
   ```

4. **Use the binary for Docker image**:
   ```bash
   cp ./fabstir-llm-node-new target/release/fabstir-llm-node
   ```

5. **Force rebuild Docker image** (MUST use --no-cache):
   ```bash
   docker build --no-cache -t llm-node-prod:latest -f Dockerfile.production .
   ```

6. **Restart nodes**:
   ```bash
   ./restart-nodes-with-payments.sh
   ```

## Contract Integration Details

### Contract Address
- **JobMarketplace**: `0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944`
- **NOT ProofSystem**: ~~`0x2ACcc60893872A499700908889B38C5420CBcFD1`~~ (DO NOT USE)

### Correct Function to Call
```solidity
submitProofOfWork(uint256 jobId, uint256 tokensClaimed, bytes calldata proof)
```
- Function selector: `0x5c1baa89`
- **DO NOT USE** ~~submitCheckpoint~~ - This function doesn't exist!

### Implementation Location
File: `/workspace/src/contracts/checkpoint_manager.rs`

Key components:
1. **PROOF_SYSTEM_ADDRESS** - Must be set to JobMarketplace address
2. **encode_checkpoint_call()** - Must encode `submitProofOfWork` function
3. **Proof data format** - JSON with timestamp, tokensUsed, hostAddress, jobId

## Token Tracking Implementation

### Critical Fix Required
The non-streaming inference path MUST track tokens. This was missing initially.

Location: `/workspace/src/api/server.rs` in `handle_inference_request()` method

After generating response (around line 387), add:
```rust
// Track tokens for checkpoint submission (non-streaming path)
let job_id = request.job_id.or_else(|| {
    request.session_id.as_ref().and_then(|sid| {
        sid.trim_end_matches('n').parse::<u64>().ok()
    })
});

if let Some(jid) = job_id {
    if let Some(cm) = self.checkpoint_manager.read().await.as_ref() {
        let _ = cm.track_tokens(jid, response.tokens_used as u64, request.session_id.clone()).await;
    }
}
```

### WebSocket Session ID Handling
The WebSocket handler converts session_id to job_id (line 127 in `/workspace/src/api/http_server.rs`):
```rust
if let Ok(parsed_id) = sid.trim_end_matches('n').parse::<u64>() {
    request.job_id = Some(parsed_id);
}
```

## Testing and Verification

### How to Verify Correct Binary is Deployed

1. **Add version string to checkpoint manager**:
   In `CheckpointManager::new()`, add:
   ```rust
   eprintln!("📝 CONTRACT VERSION: Using JobMarketplace at {}", PROOF_SYSTEM_ADDRESS);
   eprintln!("🔖 BUILD VERSION: v3-submitProofOfWork-fix");
   ```

2. **Check tracking messages**:
   ```bash
   docker logs llm-node-prod-1 2>&1 | grep "📊 Tracking.*tokens for job.*non-streaming"
   ```
   If you see this message, the token tracking fix is deployed.

3. **Verify contract address**:
   ```bash
   docker logs llm-node-prod-1 2>&1 | grep "CONTRACT VERSION"
   ```
   Should show: `Using JobMarketplace at 0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944`

### Test Commands

1. **Test with session_id** (simulates UI):
   ```bash
   curl -X POST http://localhost:8080/v1/inference \
     -H "Content-Type: application/json" \
     -d '{
       "model": "tiny-vicuna-1b",
       "prompt": "Count to 100",
       "max_tokens": 110,
       "temperature": 0.7,
       "stream": false,
       "session_id": "163"
     }'
   ```

2. **Check logs for checkpoint trigger**:
   ```bash
   docker logs llm-node-prod-1 2>&1 | grep -E "job 163|TRIGGERING|submitProofOfWork"
   ```

## Common Issues and Solutions

### Issue: "execution reverted"
**Causes:**
1. Job doesn't exist on chain - session must be created first
2. Wrong function being called - ensure using `submitProofOfWork`
3. Host not authorized - check host address matches job assignment

### Issue: Node crashes after checkpoint
The inference engine has mutex poisoning issues. After successful checkpoint submission, the node may crash. Temporary workaround: restart the node.

### Issue: No tracking messages in logs
Binary wasn't properly updated. Follow the deployment process exactly, especially the `--no-cache` flag.

## Environment Variables

Required for checkpoint submission:
```bash
HOST_PRIVATE_KEY=0xe7855c0ea54ccca55126d40f97d90868b2a73bad0363e92ccdec0c4fbd6c0ce2
RPC_URL=https://base-sepolia.g.alchemy.com/v2/1pZoccdtgU8CMyxXzE3l_ghnBBaJABMR
```

## WebSocket vs HTTP

- **WebSocket**: Used by UI, requires session_id conversion to job_id
- **HTTP**: Used for testing, can provide job_id directly
- Both paths MUST track tokens for checkpoint submission

## Critical Mistakes to Avoid

1. **DO NOT assume function names** - Always check the contract ABI
2. **DO NOT trust cached Docker images** - Always use `--no-cache`
3. **DO NOT skip binary timestamp verification** - Old binaries can persist
4. **DO NOT forget to track tokens in non-streaming path**
5. **DO NOT use ProofSystem contract** - Use JobMarketplace

## Checkpoint Threshold

- Current threshold: 100 tokens
- Checkpoint triggers when `tokens_since_last_checkpoint >= 100`
- Each token is tracked individually in streaming mode
- Bulk tracking in non-streaming mode

## Contract Requirements

Per the contract developer's instructions:
- Only the assigned host can submit proofs
- Minimum 100 tokens per submission (MIN_PROVEN_TOKENS)
- Cannot claim more than 2x expected tokens based on time elapsed
- Event emitted: `ProofOfWork` (not ProofSubmitted)

## Production Deployment Checklist

Before deploying to production, ALWAYS verify:

### 1. Contract Function Names
- [ ] Check the actual contract ABI for function names
- [ ] Never assume function names - always verify
- [ ] Use contract explorer or ABI files to confirm

### 2. Binary Verification
- [ ] Build timestamp matches current time
- [ ] Version strings appear in logs
- [ ] Docker image rebuilt with --no-cache
- [ ] Test endpoints respond with expected behavior

### 3. Token Tracking
- [ ] Both streaming AND non-streaming paths track tokens
- [ ] Session ID to Job ID conversion working
- [ ] Tracking messages appear in logs

### 4. Contract Integration
- [ ] Correct contract address configured
- [ ] Function selector matches ABI
- [ ] Transaction receipts show success status
- [ ] Events emitted on blockchain

## Debugging Flowchart

```
1. No checkpoints submitting?
   ├── Check logs for "📊 Tracking" messages
   │   └── No? Token tracking not implemented
   ├── Check logs for "🚨 TRIGGERING" messages
   │   └── No? Threshold not reached or already in progress
   └── Check logs for transaction hashes
       └── No? Contract call failing

2. Transaction sent but no proofs on chain?
   ├── Check function name matches contract ABI
   ├── Check contract address is correct
   └── Check transaction receipt status

3. Binary not updating?
   ├── Verify build timestamp
   ├── Check Docker build used --no-cache
   └── Verify container restart actually happened
```

## Emergency Recovery Procedures

### If checkpoints stop working:
1. Check the checkpoint manager initialization logs
2. Verify contract addresses match deployment
3. Check host private key is configured
4. Monitor for "execution reverted" errors
5. Restart node if mutex poisoning occurs

### If wrong binary deployed:
1. Stop production containers immediately
2. Rebuild with --no-cache flag
3. Verify binary timestamp before deployment
4. Test with curl before full deployment
5. Monitor version strings in logs

## Lessons Learned Summary

### Critical Mistakes Made:
1. **Assuming function names without checking ABI** - Always verify contract interfaces
2. **Not tracking tokens in all code paths** - Both streaming and non-streaming need tracking
3. **Docker cache causing stale deployments** - Always use --no-cache for production
4. **Not having version verification** - Add version strings to critical components
5. **Incomplete testing** - Test both WebSocket and HTTP paths

### Best Practices Established:
1. **Always check the contract ABI before implementing calls**
2. **Add comprehensive logging for debugging**
3. **Implement version strings for deployment verification**
4. **Test with actual contract state, not just logs**
5. **Document the exact deployment process**
6. **Have rollback procedures ready**