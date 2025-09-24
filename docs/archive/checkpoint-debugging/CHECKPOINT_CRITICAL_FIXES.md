# Critical Checkpoint Implementation Fixes
## Date: September 22, 2025

## Executive Summary
Production nodes were failing to submit proofs to blockchain despite accumulating 100+ tokens. This document details the four critical bugs that prevented checkpoint submission and their fixes.

## Critical Bug #1: Missing Token Tracking in Non-Streaming Path
**File:** `/workspace/src/api/server.rs`
**Lines:** 393-414

### Problem
The `handle_inference_request()` method wasn't tracking tokens at all. Only streaming requests had token tracking implemented.

### Fix
```rust
// Added token tracking after generating response (line 393)
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

## Critical Bug #2: Premature Tracker Cleanup
**File:** `/workspace/src/api/server.rs`
**Lines:** 585-596

### Problem
The streaming handler was calling `cleanup_job()` after EVERY response, which deleted the token tracker and reset the count to 0. This prevented tokens from ever accumulating to the 100-token threshold.

### Original Buggy Code
```rust
if let Some(jid) = job_id {
    if let Some(cm) = checkpoint_manager.as_ref() {
        let _ = cm.force_checkpoint(jid).await;
        cm.cleanup_job(jid).await;  // BUG: Deletes tracker!
    }
}
```

### Fix
```rust
// Try to submit checkpoint if we have enough tokens
// BUT DON'T CLEANUP - the session might continue!
if let Some(jid) = job_id {
    if let Some(cm) = checkpoint_manager.as_ref() {
        let _ = cm.force_checkpoint(jid).await;
        // DON'T cleanup here - session continues across multiple prompts!
        // Cleanup should only happen when websocket disconnects
    }
}
```

## Critical Bug #3: Wrong Contract Address
**File:** `/workspace/src/contracts/checkpoint_manager.rs`

### Problem
Node was using ProofSystem contract address (0x2ACcc...) instead of JobMarketplace (0x1273E...)

### Fix
```rust
// Changed from ProofSystem to JobMarketplace address
const PROOF_SYSTEM_ADDRESS: &str = "0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944";
```

## Critical Bug #4: Non-Existent Function Name
**File:** `/workspace/src/contracts/checkpoint_manager.rs`

### Problem
Code was calling "submitCheckpoint" which doesn't exist in the contract ABI.

### Fix
```rust
let function = Function {
    name: "submitProofOfWork".to_string(),  // Correct function name
    inputs: vec![
        // jobId: uint256
        // tokensClaimed: uint256
        // proof: bytes
    ],
};
```

## Impact
These four bugs combined to completely prevent proof submission:
1. Tokens weren't being tracked for most requests
2. When they were tracked, the counter was reset after each response
3. Even if tokens accumulated, submission used wrong contract
4. Even with correct contract, the function name was wrong

## Verification Methods

### Check Token Tracking
```bash
# Generate tokens and watch logs
curl -X POST http://localhost:8080/v1/inference \
  -H "Content-Type: application/json" \
  -d '{"model": "tiny-vicuna-1b", "prompt": "Test", "max_tokens": 150, "session_id": "TEST_SESSION"}'

# Look for tracking messages
docker logs llm-node-prod-1 2>&1 | grep "TEST_SESSION"
```

### Check Version
```bash
# Added version strings to identify deployments
docker logs llm-node-prod-1 2>&1 | grep VERSION
# Should show: v4-no-cleanup-on-streaming-2024-09-22-03:49
```

### Check Blockchain Events
Monitor Base Sepolia for ProofOfWork events from JobMarketplace contract (0x1273E...)

## Lessons Learned

1. **Always Verify Contract ABIs**: The biggest time waste was trying to call a non-existent function. Always check the actual ABI first.

2. **Token Lifecycle Management**: Understanding when to cleanup vs persist state is critical. Sessions span multiple requests.

3. **WebSocket vs HTTP**: Production UI uses WebSocket, not HTTP. Must ensure fixes apply to both paths.

4. **Binary Deployment Verification**: Added version strings to definitively know which code is running.

5. **Docker Build Context Issues**: Target directory not accessible to Docker build. Solution: use `docker cp` to copy binary directly.

## Key Files Modified
- `/workspace/src/api/server.rs` - Token tracking and cleanup fixes
- `/workspace/src/contracts/checkpoint_manager.rs` - Contract address and function name
- `/workspace/src/api/http_server.rs` - WebSocket handler (already had session conversion)

## Testing Checklist
- [ ] Token tracking works for HTTP requests
- [ ] Token tracking works for WebSocket requests
- [ ] Tokens accumulate across multiple prompts (not reset)
- [ ] Checkpoint triggers at 100+ tokens
- [ ] Proof submission uses correct contract address
- [ ] Proof submission uses correct function name
- [ ] Blockchain events confirm submission