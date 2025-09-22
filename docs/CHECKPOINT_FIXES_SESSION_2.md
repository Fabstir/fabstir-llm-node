# Checkpoint Implementation Fixes - Session 2
## Date: September 22, 2025 - 3:50 AM

## Summary of What We Fixed

### Initial State
- UI showed session 164 with 103+ tokens needing checkpoint submission
- Production nodes were NOT submitting any proofs to blockchain
- Contract developer confirmed no proofs on-chain

### Core Issues Found and Fixed

#### 1. Token Tracking Not Implemented in Non-Streaming Path
**Problem:** The `handle_inference_request()` method in `/workspace/src/api/server.rs` wasn't tracking tokens at all. Only streaming requests tracked tokens.

**Fix Applied (lines 389-410 in server.rs):**
```rust
// Track tokens for checkpoint submission (non-streaming path)
let job_id = request.job_id.or_else(|| {
    request.session_id.as_ref().and_then(|sid| {
        let parsed = sid.trim_end_matches('n').parse::<u64>().ok();
        if parsed.is_some() {
            eprintln!("📋 Converted session_id {} to job_id {:?}", sid, parsed);
        }
        parsed
    })
});

if let Some(jid) = job_id {
    if let Some(cm) = self.checkpoint_manager.read().await.as_ref() {
        eprintln!("📊 Tracking {} tokens for job {} (non-streaming)", response.tokens_used, jid);
        let _ = cm.track_tokens(jid, response.tokens_used as u64, request.session_id.clone()).await;
    }
}
```

#### 2. Wrong Contract Address
**Problem:** Node was using ProofSystem contract (0x2ACcc...) instead of JobMarketplace (0x1273E...)

**Fix:** Changed in `/workspace/src/contracts/checkpoint_manager.rs`:
```rust
const PROOF_SYSTEM_ADDRESS: &str = "0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944";
```

#### 3. Wrong Function Name
**Problem:** Code was calling non-existent "submitCheckpoint" function

**Fix:** Updated to use correct function "submitProofOfWork" in checkpoint_manager.rs:
```rust
let function = Function {
    name: "submitProofOfWork".to_string(),
    inputs: vec![
        // jobId: uint256
        // tokensClaimed: uint256
        // proof: bytes
    ],
};
```

#### 4. CRITICAL BUG: Tracker Cleanup After Every Response
**Problem:** The streaming handler was calling `cleanup_job()` after EVERY response, deleting the token tracker. This caused tokens to reset to 0 after each prompt.

**Location:** `/workspace/src/api/server.rs` lines 585-596

**Original Buggy Code:**
```rust
// Force checkpoint and cleanup if session ends with a job_id
if let Some(jid) = job_id {
    if let Some(cm) = checkpoint_manager.as_ref() {
        let _ = cm.force_checkpoint(jid).await;
        cm.cleanup_job(jid).await;  // BUG: Deletes tracker!
    }
}
```

**Fix Applied:**
```rust
// Try to submit checkpoint if we have enough tokens
// BUT DON'T CLEANUP - the session might continue!
if let Some(jid) = job_id {
    if let Some(cm) = checkpoint_manager.as_ref() {
        let _ = cm.force_checkpoint(jid).await;
        // DON'T cleanup here - session continues across multiple prompts!
    }
}
```

### Deployment Issues and Solutions

#### Docker Build Context Problem
**Problem:** Docker build was only transferring 116B instead of 841MB binary
**Cause:** The `target/` directory wasn't accessible to Docker build context
**Solution:** Use `docker cp` to copy binary directly into running containers

#### Deployment Process That Works
1. Build in dev container: `cargo build --release`
2. Copy to host: `docker cp fabstir-llm-marketplace-node-dev-1:/workspace/target/release/fabstir-llm-node target/release/fabstir-llm-node`
3. Stop containers: `docker stop llm-node-prod-1 llm-node-prod-2`
4. Remove containers: `docker rm llm-node-prod-1 llm-node-prod-2`
5. Start new containers: `./restart-nodes-with-payments.sh`
6. Copy binary into containers: `docker cp target/release/fabstir-llm-node llm-node-prod-1:/usr/local/bin/fabstir-llm-node`
7. Restart: `docker restart llm-node-prod-1`

### Version Tracking
Added version strings for verification:
- v1: Initial broken version
- v2: Added JobMarketplace address
- v3: Added token tracking (v3-token-tracking-fixed-2024-09-22-02:54)
- v4: Fixed cleanup bug (v4-no-cleanup-on-streaming-2024-09-22-03:49)

### Key Files Modified
1. `/workspace/src/api/server.rs` - Added token tracking, fixed cleanup bug
2. `/workspace/src/contracts/checkpoint_manager.rs` - Fixed contract address and function name
3. `/workspace/src/api/http_server.rs` - WebSocket handler (already had session conversion)

### Testing Commands
```bash
# Generate tokens for testing
curl -X POST http://localhost:8080/v1/inference \
  -H "Content-Type: application/json" \
  -d '{"model": "tiny-vicuna-1b", "prompt": "Test", "max_tokens": 150, "temperature": 0.7, "stream": false, "session_id": "TEST_ID"}'

# Check logs for tracking
docker logs llm-node-prod-1 2>&1 | grep "TEST_ID"

# Check for version
docker logs llm-node-prod-1 2>&1 | grep VERSION
```

### Contract Requirements
- Minimum 100 tokens per submission (MIN_PROVEN_TOKENS)
- Only assigned host can submit proofs
- Function: submitProofOfWork(uint256 jobId, uint256 tokensClaimed, bytes proof)
- Event emitted: ProofOfWork

### Common Error Messages
- "Must claim minimum tokens" - Less than 100 tokens
- "execution reverted" - Usually job doesn't exist or wrong host

### Scripts Created
- `/workspace/restart-and-deploy.sh` - Full restart with binary deployment
- `/workspace/deploy-binary.sh` - Quick binary update only
- `/workspace/test_token_tracking.py` - Comprehensive testing script

### Current Status
- Token tracking: ✅ Working for both HTTP and WebSocket
- Session ID conversion: ✅ Working
- Checkpoint triggering: ✅ Fixed (v4 doesn't cleanup prematurely)
- Contract submission: ✅ Using correct function and address

### Remaining Known Issues
- WebSocket disconnection should trigger final cleanup (not implemented)
- No retry mechanism for failed checkpoint submissions
- Line ending issues with scripts (need dos2unix or sed -i 's/\r$//')

## CRITICAL LESSON LEARNED
**ALWAYS CHECK THE CONTRACT ABI** - Don't assume function names exist. The biggest time waste was trying to call "submitCheckpoint" which didn't exist. The correct function was "submitProofOfWork".

## Testing the Fix
After deploying v4, test with UI:
1. Start a new session
2. Generate multiple prompts/responses
3. Tokens should accumulate (not reset to 0)
4. At 100+ total tokens, checkpoint should trigger
5. Check blockchain for ProofOfWork event