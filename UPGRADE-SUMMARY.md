# GPT-OSS-120B Upgrade - Implementation Summary

**Date**: 2026-01-28
**Model**: GPT-OSS-120B with 131K context window
**Status**: ✅ Configuration Complete - Ready for Download & Deployment

## What Was Implemented

This upgrade transitions from gpt-oss-20B (12GB, 8K context) to gpt-oss-120B (65GB, 131K context) to fully utilize the NVIDIA RTX Pro 6000 Blackwell GPU (97GB VRAM).

### Key Changes

1. **Model Configuration**
   - Updated MODEL_PATH to point to 120B model (3-part split GGUF)
   - Increased MAX_CONTEXT_LENGTH from 8,192 to 131,072 tokens
   - Set GPU_LAYERS to 99 (all layers on GPU for Blackwell)
   - Maintained LLAMA_BATCH_SIZE at 2048 (optimal)
   - Maintained MODEL_CHAT_TEMPLATE as harmony

2. **No Code Changes Required**
   - ✅ Existing inference engine already supports split GGUF files
   - ✅ Flash Attention automatically enabled by llama-cpp-2
   - ✅ Repetition penalty correctly disabled (verified in sampler chain)
   - ✅ Large context support already implemented
   - ✅ Harmony chat template correctly configured

3. **Configuration Files Updated**
   - `.env.prod` - New production config with 120B settings
   - `.env.prod.example` - Updated example for documentation

4. **Deployment Scripts Updated**
   - `restart-and-deploy-openai.sh` - Updated for 120B deployment

5. **New Scripts Created**
   - `scripts/download_gpt_oss_120b.sh` - Download and verify model
   - `scripts/register_gpt_oss_120b.sh` - Register in ModelRegistry
   - `scripts/test_gpt_oss_120b.sh` - Comprehensive test suite

6. **New Documentation Created**
   - `docs/models/GPT-OSS-120B-REGISTRATION.md` - Detailed model docs
   - `docs/GPT-OSS-120B-UPGRADE-GUIDE.md` - Quick reference guide

## Files Created

```
/workspace/
├── .env.prod (new)
├── scripts/
│   ├── download_gpt_oss_120b.sh (new, executable)
│   ├── register_gpt_oss_120b.sh (new, executable)
│   └── test_gpt_oss_120b.sh (new, executable)
├── docs/
│   ├── models/
│   │   └── GPT-OSS-120B-REGISTRATION.md (new)
│   └── GPT-OSS-120B-UPGRADE-GUIDE.md (new)
└── UPGRADE-SUMMARY.md (this file)
```

## Files Modified

```
/workspace/
├── .env.prod.example (updated: model config section)
└── restart-and-deploy-openai.sh (updated: model path, env vars, wait time)
```

## What You Need to Do Next

### Step 1: Download Model (~2-4 hours)

**IMPORTANT**: This downloads ~65GB. Ensure you have:
- Sufficient disk space (>70GB free)
- Stable internet connection
- Time for download (2-4 hours)

```bash
cd /home/jules23/fabstir/fabstir-llm-node
./scripts/download_gpt_oss_120b.sh
```

This script will:
- Download all 3 model parts (~21GB each)
- Compute SHA256 hashes for integrity verification
- Calculate composite hash for ModelRegistry
- Save composite hash to `models/gpt-oss-120b-GGUF/gpt-oss-120b-composite-hash.txt`

### Step 2: Update Your .env.prod (If Custom)

If you have a custom `.env.prod` file with real credentials, merge these settings:

```bash
MODEL_PATH=/models/gpt-oss-120b-GGUF/gpt-oss-120b-mxfp4-00001-of-00003.gguf
MAX_CONTEXT_LENGTH=131072
GPU_LAYERS=99
LLAMA_BATCH_SIZE=2048
MODEL_CHAT_TEMPLATE=harmony
```

The template `.env.prod` has been created, but you'll need to add your real:
- `S5_SEED_PHRASE`
- `HOST_PRIVATE_KEY`
- `RPC_URL` (if using custom endpoint)

### Step 3: Register Model (Contract Owner Only)

**Skip this step if you're not the contract owner.**

If you have `OWNER_PRIVATE_KEY`:

```bash
export OWNER_PRIVATE_KEY=0x...
./scripts/register_gpt_oss_120b.sh
```

This registers the model in ModelRegistry with the composite hash.

### Step 4: Deploy Node

```bash
./restart-and-deploy-openai.sh
```

This will:
- Stop old container (if running)
- Start new container with 120B model
- Wait 60 seconds for model to load
- Display verification commands

### Step 5: Test Deployment

```bash
./scripts/test_gpt_oss_120b.sh
```

This comprehensive test suite verifies:
- Model loading
- GPU configuration
- VRAM usage
- Inference correctness
- Streaming functionality
- Performance benchmarks

### Step 6: Monitor VRAM Usage

```bash
watch -n 1 nvidia-smi
```

Expected VRAM usage:
- Idle (model loaded): ~65GB
- During inference: ~85-90GB
- Should stay below 95GB

## Expected Performance

### VRAM Allocation
- Model: ~65GB
- Context buffer (131K): ~20-25GB
- Total: ~85-90GB
- Available: 97GB
- Safety margin: ~7-12GB ✅ Safe

### Inference Speed
- Model load time: 30-60 seconds
- First token latency: 2-5 seconds
- Throughput: 5-15 tokens/second
- 100 token generation: 10-20 seconds

### Context Handling
- Maximum: 131,072 tokens (131K)
- Recommended: 8K-32K tokens for optimal speed
- Large contexts (>64K): May reduce throughput but should work

## Verification Checklist

Before considering deployment complete:

- [ ] Model downloaded (3 parts, ~65GB total)
- [ ] Composite hash computed and saved
- [ ] `.env.prod` updated with real credentials
- [ ] Docker container started successfully
- [ ] Model loaded (check Docker logs)
- [ ] GPU_LAYERS=99 configured (check logs)
- [ ] VRAM usage 85-90GB (check nvidia-smi)
- [ ] Harmony template active (check logs for "channel")
- [ ] Test suite passes (run test_gpt_oss_120b.sh)
- [ ] Simple inference works (2+2 test)
- [ ] Streaming works (count to 5 test)
- [ ] Performance acceptable (>=5 tokens/sec)

## Rollback Plan

If issues occur, revert to 20B model:

```bash
# Stop upgraded node
docker stop llm-node-prod-1 && docker rm llm-node-prod-1

# Restore old config in restart-and-deploy-openai.sh:
MODEL_PATH=/models/openai_gpt-oss-20b-MXFP4.gguf
# Remove MAX_CONTEXT_LENGTH=131072
# Change GPU_LAYERS=99 to GPU_LAYERS=35

# Restart with old config
./restart-and-deploy-openai.sh
```

## Technical Details

### Why Composite Hash?

The model uses 3 split GGUF files. We register a **composite hash** in ModelRegistry:

```
composite_hash = SHA256(hash_part1 + hash_part2 + hash_part3)
```

This ensures:
- ✅ All 3 parts are verified (not just first file)
- ✅ Any tampering of any part is detectable
- ✅ Complete model authenticity for marketplace
- ✅ Standard approach for split-file models

### Why No Code Changes?

The existing codebase already supports:
- ✅ Split GGUF files (llama.cpp auto-loads parts 2 and 3)
- ✅ Large context windows (MAX_CONTEXT_LENGTH is configurable)
- ✅ High GPU layer counts (GPU_LAYERS=99)
- ✅ Correct sampling (no repeat_penalty in chain)
- ✅ Harmony chat template (MODEL_CHAT_TEMPLATE=harmony)

This is a **configuration-only upgrade** - no Rust code modifications needed.

### Critical Model Requirements

From OpenAI/expert recommendations:
1. **No repetition penalty** ✅ Verified in sampler chain
2. **Flash Attention** ✅ Automatic in llama-cpp-2 for Blackwell
3. **Harmony template** ✅ Already configured
4. **Split GGUF support** ✅ llama.cpp handles automatically
5. **Sampling**: temp=1.0, top_p=1.0, top_k=0 ✅ Configurable per request

## Documentation

- **Quick Start**: `docs/GPT-OSS-120B-UPGRADE-GUIDE.md`
- **Detailed Docs**: `docs/models/GPT-OSS-120B-REGISTRATION.md`
- **Download Script**: `scripts/download_gpt_oss_120b.sh`
- **Registration Script**: `scripts/register_gpt_oss_120b.sh`
- **Test Suite**: `scripts/test_gpt_oss_120b.sh`

## Support

For issues:
1. Check Docker logs: `docker logs llm-node-prod-1`
2. Run test suite: `./scripts/test_gpt_oss_120b.sh`
3. Review documentation in `docs/`
4. Check VRAM: `nvidia-smi`

## Timeline

- **Configuration**: ✅ Complete
- **Download**: 2-4 hours (you need to run)
- **Registration**: 5-10 minutes (contract owner only)
- **Deployment**: 5-10 minutes
- **Testing**: 20-30 minutes
- **Total**: 3-5 hours (mostly download time)

## Success Criteria

All must pass before production use:

- ✅ Model loads successfully (65GB VRAM)
- ✅ Context window accepts 131K tokens
- ✅ VRAM usage stays below 95GB
- ✅ Inference produces clean responses
- ✅ No garbage output or truncation
- ✅ Harmony formatting correct
- ✅ Tokens/second >= 5
- ✅ No OOM errors during tests
- ✅ All tests pass

## Next Steps After Upgrade

1. **Document Performance**: Track actual tokens/sec for your workload
2. **Test Various Context Sizes**: Start small (8K), increase gradually
3. **Monitor VRAM Over Time**: Ensure stable under load
4. **Update Client Documentation**: Inform clients of 131K capability
5. **Consider Pricing**: 120B model may justify higher pricing

---

**Ready to Deploy**: All configuration complete. Run download script to begin.
