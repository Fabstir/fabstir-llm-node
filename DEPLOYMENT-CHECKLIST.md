# GPT-OSS-120B Deployment Checklist

Use this checklist to track your upgrade progress.

## Pre-Deployment

- [ ] **System Requirements Verified**
  - [ ] NVIDIA RTX Pro 6000 Blackwell GPU (97GB VRAM) available
  - [ ] Sufficient disk space (>70GB free for model files)
  - [ ] Docker installed and configured with GPU support
  - [ ] `nvidia-smi` working and showing 97GB VRAM
  - [ ] Internet connection stable (for 65GB download)

- [ ] **Configuration Files Prepared**
  - [ ] `.env.prod` exists with real credentials (S5_SEED_PHRASE, HOST_PRIVATE_KEY)
  - [ ] RPC_URL configured (or using default Base Sepolia)
  - [ ] Contract addresses verified in `.env.local.test`

## Phase 1: Download Model (2-4 hours)

- [ ] **Run Download Script**
  ```bash
  cd /home/jules23/fabstir/fabstir-llm-node
  ./scripts/download_gpt_oss_120b.sh
  ```

- [ ] **Verify Download**
  - [ ] 3 files exist in `models/gpt-oss-120b-GGUF/`
  - [ ] Total size ~65GB (run: `du -sh models/gpt-oss-120b-GGUF/`)
  - [ ] Composite hash file exists: `gpt-oss-120b-composite-hash.txt`
  - [ ] Individual SHA256 hashes displayed
  - [ ] Composite hash computed and saved

- [ ] **Document Hashes** (for other hosts)
  - [ ] Save composite hash for later reference
  - [ ] Note model ID displayed by script

## Phase 2: ModelRegistry Registration (Contract Owner Only)

**Skip this section if you're not the contract owner.**

- [ ] **Prepare for Registration**
  - [ ] `OWNER_PRIVATE_KEY` available
  - [ ] Contract owner account has gas on Base Sepolia
  - [ ] `CONTRACT_MODEL_REGISTRY` address verified

- [ ] **Run Registration Script**
  ```bash
  export OWNER_PRIVATE_KEY=0x...
  ./scripts/register_gpt_oss_120b.sh
  ```

- [ ] **Verify Registration**
  - [ ] Transaction succeeded (TX hash displayed)
  - [ ] `isModelApproved()` returns true
  - [ ] `getModel()` returns correct hash
  - [ ] Model ID noted for node registration

- [ ] **Update Node Registration**
  ```bash
  # Add model to your node's supported models
  cast send $CONTRACT_NODE_REGISTRY \
      "updateSupportedModels(bytes32[])" \
      "[MODEL_ID]" \
      --rpc-url $BASE_SEPOLIA_RPC_URL \
      --private-key $HOST_PRIVATE_KEY \
      --legacy
  ```

## Phase 3: Deploy Node (10-15 minutes)

- [ ] **Pre-Deployment Checks**
  - [ ] Old container stopped (if running)
  - [ ] `.env.prod` has correct MODEL_PATH
  - [ ] `.env.prod` has MAX_CONTEXT_LENGTH=131072
  - [ ] `.env.prod` has GPU_LAYERS=99
  - [ ] Models directory mounted correctly in Docker

- [ ] **Run Deployment Script**
  ```bash
  ./restart-and-deploy-openai.sh
  ```

- [ ] **Monitor Startup**
  - [ ] Container starts: `docker ps | grep llm-node-prod-1`
  - [ ] Logs show model loading: `docker logs llm-node-prod-1 | grep "Model"`
  - [ ] No error messages in logs
  - [ ] Wait 60 seconds for model to fully load

## Phase 4: Verification (20-30 minutes)

- [ ] **Check Docker Logs**
  ```bash
  docker logs llm-node-prod-1 2>&1 | grep -E "Model loaded|GPU|Context"
  ```
  - [ ] "Model loaded" message present
  - [ ] GPU configuration correct (99 layers)
  - [ ] Context size shows 131072
  - [ ] Harmony template active (<|channel|> tags)
  - [ ] Encryption enabled (if HOST_PRIVATE_KEY set)

- [ ] **Check GPU Memory**
  ```bash
  nvidia-smi
  ```
  - [ ] VRAM usage ~65GB (model loaded)
  - [ ] Total VRAM shows 97GB
  - [ ] GPU utilization shows activity during inference

- [ ] **Run Test Suite**
  ```bash
  ./scripts/test_gpt_oss_120b.sh
  ```
  - [ ] All health checks pass
  - [ ] Simple math test passes (2+2)
  - [ ] Capital city test passes (Paris)
  - [ ] Longer generation test passes
  - [ ] Streaming test works
  - [ ] Performance benchmark completes
  - [ ] VRAM stays below 95GB during tests

- [ ] **Manual Tests**
  ```bash
  # Test 1: Simple inference
  curl -s -X POST http://localhost:8080/v1/inference \
    -H 'Content-Type: application/json' \
    -d '{"model": "gpt-oss-120b", "prompt": "What is 2+2?", "max_tokens": 20, "temperature": 0.1, "chain_id": 84532}' | jq .
  ```
  - [ ] Response contains "4" or "four"
  - [ ] No garbage output
  - [ ] No error messages
  - [ ] Response time reasonable (5-10 seconds)

  ```bash
  # Test 2: Knowledge query
  curl -s -X POST http://localhost:8080/v1/inference \
    -H 'Content-Type: application/json' \
    -d '{"model": "gpt-oss-120b", "prompt": "What is the capital of France?", "max_tokens": 50, "temperature": 0.1, "chain_id": 84532}' | jq .
  ```
  - [ ] Response mentions "Paris"
  - [ ] Response is coherent and complete
  - [ ] No premature truncation

  ```bash
  # Test 3: Performance test
  time curl -s -X POST http://localhost:8080/v1/inference \
    -H 'Content-Type: application/json' \
    -d '{"model": "gpt-oss-120b", "prompt": "Explain quantum computing", "max_tokens": 100, "temperature": 0.7, "chain_id": 84532}' | jq .
  ```
  - [ ] Completes in reasonable time (10-20 seconds)
  - [ ] Response is coherent
  - [ ] VRAM usage stays below 95GB (check with nvidia-smi)

## Phase 5: Quality Validation

- [ ] **Check Output Quality**
  - [ ] No ellipsis "..." in unexpected places
  - [ ] No repeated characters or loops
  - [ ] No premature truncation
  - [ ] Proper sentence structure
  - [ ] Accurate factual responses

- [ ] **Check Chat Formatting**
  - [ ] Harmony template applied correctly
  - [ ] <|channel|> tags present in logs
  - [ ] <|message|> tags structured correctly
  - [ ] No template-related errors

- [ ] **Stress Test** (optional but recommended)
  ```bash
  # Long context test
  curl -s -X POST http://localhost:8080/v1/inference \
    -H 'Content-Type: application/json' \
    -d '{"model": "gpt-oss-120b", "prompt": "[Long prompt with 10K+ tokens]", "max_tokens": 500, "temperature": 0.7, "chain_id": 84532}'
  ```
  - [ ] Accepts long prompts (8K+ tokens)
  - [ ] No OOM errors
  - [ ] VRAM stays below 95GB
  - [ ] Response quality maintained

## Phase 6: Production Readiness

- [ ] **Performance Benchmarks Documented**
  - [ ] Tokens/second measured: _____ tokens/sec (target: >=5)
  - [ ] First token latency: _____ seconds (target: 2-5s)
  - [ ] Model load time: _____ seconds (target: 30-60s)
  - [ ] Average VRAM usage: _____ GB (target: 85-90GB)

- [ ] **Monitoring Set Up**
  - [ ] VRAM monitoring script/tool configured
  - [ ] Docker logs being collected
  - [ ] Alert thresholds defined (VRAM >95GB, OOM errors)

- [ ] **Documentation Updated**
  - [ ] Composite hash recorded in registry docs
  - [ ] Model ID documented
  - [ ] Performance metrics documented
  - [ ] Any issues encountered documented

- [ ] **Rollback Plan Ready**
  - [ ] Old 20B model files still available
  - [ ] Rollback procedure documented
  - [ ] Backup deployment script saved

## Success Criteria

All must be checked before considering deployment complete:

- [ ] ✅ Model loads successfully (65GB VRAM)
- [ ] ✅ Context window accepts 131K tokens
- [ ] ✅ VRAM usage stays below 95GB during inference
- [ ] ✅ Inference produces clean, coherent responses
- [ ] ✅ No garbage output or premature truncation
- [ ] ✅ Harmony chat template formatting correct
- [ ] ✅ Tokens/second >= 5 (acceptable for 120B model)
- [ ] ✅ No OOM errors during long context tests
- [ ] ✅ All tests in test_gpt_oss_120b.sh pass
- [ ] ✅ Model registered in ModelRegistry (if applicable)
- [ ] ✅ Node registration updated with model ID (if applicable)

## Post-Deployment

- [ ] **Monitor First 24 Hours**
  - [ ] Check VRAM stability every few hours
  - [ ] Review Docker logs for any warnings
  - [ ] Track inference performance
  - [ ] Watch for OOM errors

- [ ] **Client Communication**
  - [ ] Inform clients of 131K context capability
  - [ ] Update API documentation
  - [ ] Announce performance characteristics
  - [ ] Update pricing (if applicable)

- [ ] **Continuous Monitoring**
  - [ ] Set up automated VRAM monitoring
  - [ ] Configure log aggregation
  - [ ] Set up performance dashboards
  - [ ] Define alert thresholds

## Troubleshooting Reference

If issues occur, check:

1. **Model won't load**
   - Verify all 3 GGUF files present and co-located
   - Check disk space
   - Review Docker logs for errors
   - Verify MODEL_PATH correct

2. **OOM errors**
   - Check VRAM with nvidia-smi
   - Ensure no other GPU processes
   - Consider reducing GPU_LAYERS if needed
   - Verify 97GB VRAM available

3. **Slow performance**
   - Monitor VRAM usage
   - Check context size in requests
   - Verify Flash Attention enabled
   - Review batch size setting

4. **Quality issues**
   - Verify MODEL_CHAT_TEMPLATE=harmony
   - Check logs for UTF-8 warnings
   - Review sampling parameters
   - Verify no repeat_penalty enabled

## Rollback

If critical issues occur:

```bash
# Stop upgraded node
docker stop llm-node-prod-1 && docker rm llm-node-prod-1

# Edit restart-and-deploy-openai.sh:
# MODEL_PATH=/models/openai_gpt-oss-20b-MXFP4.gguf
# Remove MAX_CONTEXT_LENGTH=131072
# GPU_LAYERS=35

# Restart with old config
./restart-and-deploy-openai.sh
```

---

**Deployment Started**: [DATE/TIME]
**Deployment Completed**: [DATE/TIME]
**Total Time**: [DURATION]
**Final Status**: [SUCCESS/ROLLBACK/ISSUES]

**Notes**:
[Add any deployment notes, issues encountered, or observations here]
