# GPT-OSS-120B Upgrade Guide

Quick reference for upgrading from gpt-oss-20B to gpt-oss-120B with 131K context window.

## Overview

- **From**: openai_gpt-oss-20b-MXFP4.gguf (~12GB, 8K context)
- **To**: gpt-oss-120b-MXFP4 (~65GB, 131K context, 3-part split)
- **GPU**: NVIDIA RTX Pro 6000 Blackwell (97GB VRAM)
- **Expected VRAM**: ~85-90GB during inference

## Quick Start

### 1. Download Model (~2-4 hours)

```bash
cd /path/to/fabstir-llm-node
./scripts/download_gpt_oss_120b.sh
```

This downloads ~65GB and computes the composite hash for ModelRegistry.

### 2. Update Configuration

The configuration files have already been updated:
- ✅ `.env.prod` - Updated with 120B model path and 131K context
- ✅ `.env.prod.example` - Updated for documentation
- ✅ `restart-and-deploy-openai.sh` - Updated deployment script

**If using your own .env.prod**, ensure these settings:

```bash
MODEL_PATH=/models/gpt-oss-120b-GGUF/gpt-oss-120b-mxfp4-00001-of-00003.gguf
MAX_CONTEXT_LENGTH=131072
GPU_LAYERS=99
LLAMA_BATCH_SIZE=2048
MODEL_CHAT_TEMPLATE=harmony
```

### 3. Register Model (Contract Owner Only)

```bash
# Requires OWNER_PRIVATE_KEY in environment
export OWNER_PRIVATE_KEY=0x...
./scripts/register_gpt_oss_120b.sh
```

### 4. Deploy Node

```bash
./restart-and-deploy-openai.sh
```

Waits 60 seconds for model to load (120B model is larger than 20B).

### 5. Test Deployment

```bash
./scripts/test_gpt_oss_120b.sh
```

Runs comprehensive test suite including:
- Health checks
- GPU memory verification
- Functional tests
- Performance benchmarks

## Manual Testing

```bash
# Simple test
curl -s -X POST http://localhost:8080/v1/inference \
  -H 'Content-Type: application/json' \
  -d '{"model": "gpt-oss-120b", "prompt": "What is 2+2?", "max_tokens": 20, "temperature": 0.1, "chain_id": 84532}' | jq .

# Performance test
time curl -s -X POST http://localhost:8080/v1/inference \
  -H 'Content-Type: application/json' \
  -d '{"model": "gpt-oss-120b", "prompt": "Explain quantum computing", "max_tokens": 100, "temperature": 0.7, "chain_id": 84532}' | jq .
```

## Verification Checklist

- [ ] Model files downloaded (3 parts, ~65GB total)
- [ ] Composite hash computed and saved
- [ ] `.env.prod` updated with correct MODEL_PATH and MAX_CONTEXT_LENGTH
- [ ] Node started successfully (check Docker logs)
- [ ] Model loaded (check logs for "Model loaded")
- [ ] GPU layers configured (GPU_LAYERS=99)
- [ ] VRAM usage 85-90GB (check with `nvidia-smi`)
- [ ] Harmony template active (check logs for "channel" tags)
- [ ] Inference tests pass (run test_gpt_oss_120b.sh)
- [ ] Model registered in ModelRegistry (contract owner only)
- [ ] Node registration updated with model ID

## Key Differences from 20B Model

| Aspect | 20B Model | 120B Model |
|--------|-----------|------------|
| **Model Size** | ~12GB | ~65GB (3 parts) |
| **Context Window** | 8,192 tokens | 131,072 tokens |
| **VRAM Usage** | ~15-20GB | ~85-90GB |
| **GPU Layers** | 35 | 99 |
| **Load Time** | 5-10 seconds | 30-60 seconds |
| **Throughput** | 15-30 tokens/sec | 5-15 tokens/sec |
| **Model Files** | Single GGUF | 3-part split GGUF |
| **Hash Type** | Single SHA256 | Composite SHA256 |

## Performance Expectations

### VRAM Allocation
- Model: ~65GB
- Context buffer (131K): ~20-25GB
- Total: ~85-90GB
- Available: 97GB
- Safety margin: ~7-12GB ✅

### Inference Speed
- First token: 2-5 seconds
- Sustained: 5-15 tokens/second
- 100 token generation: 10-20 seconds

### Context Handling
- Maximum: 131,072 tokens
- Recommended: 8K-32K tokens for optimal speed
- Large contexts (>64K): May reduce throughput

## Troubleshooting

### Model Won't Load
```bash
# Check logs
docker logs llm-node-prod-1 2>&1 | grep -i "error\|failed"

# Check VRAM
nvidia-smi

# Verify all 3 parts exist
ls -lh models/gpt-oss-120b-GGUF/
```

### OOM Errors
- Ensure no other processes using GPU
- Verify 97GB+ VRAM available
- Check GPU_LAYERS setting (reduce if needed)

### Slow Performance
- Monitor VRAM: `watch -n 1 nvidia-smi`
- Check context size in requests
- Verify Flash Attention enabled (automatic)

### Wrong Model Loading
```bash
# Check MODEL_PATH in container
docker exec llm-node-prod-1 env | grep MODEL_PATH

# Verify binary sees correct path
docker logs llm-node-prod-1 2>&1 | grep "Model path"
```

## Rollback Procedure

If issues occur, revert to 20B model:

```bash
# Stop node
docker stop llm-node-prod-1 && docker rm llm-node-prod-1

# Edit restart-and-deploy-openai.sh
# Change MODEL_PATH back to:
# MODEL_PATH=/models/openai_gpt-oss-20b-MXFP4.gguf
# Remove MAX_CONTEXT_LENGTH=131072
# Change GPU_LAYERS=99 to GPU_LAYERS=35

# Restart
./restart-and-deploy-openai.sh
```

## Files Modified

### Configuration
- `.env.prod` - Production environment config
- `.env.prod.example` - Example config for documentation

### Scripts
- `restart-and-deploy-openai.sh` - Deployment script (updated model path, context, GPU layers)
- `scripts/download_gpt_oss_120b.sh` - New: Download and verify model
- `scripts/register_gpt_oss_120b.sh` - New: Register in ModelRegistry
- `scripts/test_gpt_oss_120b.sh` - New: Comprehensive test suite

### Documentation
- `docs/models/GPT-OSS-120B-REGISTRATION.md` - Detailed model documentation
- `docs/GPT-OSS-120B-UPGRADE-GUIDE.md` - This file

### No Code Changes Required
- `src/inference/engine.rs` - Already supports split files, large context ✅
- All other Rust source files - No changes needed ✅

## Success Criteria

All must pass before production deployment:

- ✅ Model loads successfully (65GB VRAM)
- ✅ Context window accepts 131K tokens
- ✅ VRAM usage stays below 95GB
- ✅ Inference produces clean, coherent responses
- ✅ No garbage output or premature truncation
- ✅ Harmony chat template formatting correct
- ✅ Tokens/second >= 5 (acceptable for 120B)
- ✅ No OOM errors during long context tests
- ✅ All tests in test_gpt_oss_120b.sh pass
- ✅ Model registered in ModelRegistry (for official deployment)
- ✅ Node registration updated with model ID

## Timeline

- **Download**: 2-4 hours (bandwidth dependent)
- **Configuration**: 5 minutes (already done)
- **Testing**: 20-30 minutes
- **Total**: 2.5-5 hours (mostly download time)

## Next Steps After Upgrade

1. **Monitor Performance**: Run `nvidia-smi` regularly to track VRAM
2. **Test Various Context Sizes**: Start small (8K), gradually increase
3. **Benchmark Throughput**: Document actual tokens/second for your workload
4. **Update Client Documentation**: Inform clients of 131K context capability
5. **Consider Pricing Adjustments**: 120B model may justify higher pricing

## Support Resources

- **Model Documentation**: `docs/models/GPT-OSS-120B-REGISTRATION.md`
- **API Documentation**: `docs/API.md`
- **Deployment Guide**: `docs/DEPLOYMENT.md`
- **Main Documentation**: `CLAUDE.md`
- **Hugging Face**: https://huggingface.co/ggml-org/gpt-oss-120b-GGUF

## Notes

- **No Rebuild Required**: All changes are configuration-only
- **Backward Compatible**: Can revert by changing environment variables
- **Split Files**: llama.cpp automatically handles multi-part GGUF files
- **Flash Attention**: Automatically enabled by llama-cpp-2 for Blackwell
- **Repetition Penalty**: Correctly disabled (not in sampler chain)
- **Chat Template**: Harmony format maintained
