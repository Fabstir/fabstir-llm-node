# SDK Compatibility Guide: Model Authorization Enforcement

**Version**: v8.14.0-model-validation
**Date**: February 4, 2026
**Audience**: SDK Developers (Client-side)
**Host Node Version**: v8.14.0+

---

## Executive Summary

**Good News: ✅ NO BREAKING CHANGES for SDK developers**

The host node's model authorization enforcement is a **server-side security feature** that requires **ZERO changes** to existing SDK code. Your current job creation, session management, and WebSocket code will continue to work without modification.

### What Changed (Host-Side Only)

| Component | Change | SDK Impact |
|-----------|--------|------------|
| Host startup | Validates MODEL_PATH against registered models | None - transparent to SDK |
| SHA256 verification | Validates model file integrity against on-chain hash | None - prevents tampered models |
| Dynamic discovery | Models queried from contract (not hardcoded) | None - new models supported automatically |
| Job claiming | Hosts only claim jobs for registered models | None - better job reliability |
| Inference | Runtime model authorization checks | None - prevents model switching fraud |
| Feature flag | `REQUIRE_MODEL_VALIDATION` env var | None - host configuration only |

### What Stays the Same (SDK-Side)

✅ Job creation API - unchanged
✅ Session creation API - unchanged
✅ WebSocket protocol - unchanged
✅ Model ID format - unchanged
✅ Contract addresses - unchanged
✅ Payment flow - unchanged

---

## Table of Contents

1. [How Model Validation Works](#how-model-validation-works)
2. [SDK Behavior Changes](#sdk-behavior-changes)
3. [Optional SDK Improvements](#optional-sdk-improvements)
4. [Error Handling Guide](#error-handling-guide)
5. [Code Examples](#code-examples)
6. [Best Practices](#best-practices)
7. [Migration Checklist](#migration-checklist)
8. [FAQ](#faq)

---

## How Model Validation Works

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                        CLIENT (SDK)                                  │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  1. Create Job: createSessionJob(host, modelId, ...)           │ │
│  │     → Specifies which model to use                             │ │
│  └────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
                                ↓
┌─────────────────────────────────────────────────────────────────────┐
│                    BLOCKCHAIN (Smart Contracts)                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  • Job created on-chain with modelId                           │ │
│  │  • NodeRegistry.nodeSupportsModel(host, modelId) → bool        │ │
│  │  • ModelRegistry.isModelApproved(modelId) → bool               │ │
│  └────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
                                ↓
┌─────────────────────────────────────────────────────────────────────┐
│                     HOST NODE (Server-Side)                          │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  2. Host sees job on-chain                                     │ │
│  │  3. ✅ NEW: Validates if host supports job.modelId             │ │
│  │     → Query: nodeSupportsModel(myAddress, job.modelId)         │ │
│  │     → If false: Skip job (don't claim)                         │ │
│  │     → If true: Claim and execute                               │ │
│  │  4. ✅ NEW: Before inference, verify model authorization       │ │
│  │  5. Execute inference with validated model                     │ │
│  └────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
                                ↓
┌─────────────────────────────────────────────────────────────────────┐
│                        CLIENT (SDK)                                  │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  6. Receive inference results                                  │ │
│  │     → Guaranteed to be from the requested model!               │ │
│  └────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

### Four Validation Points (Host-Side)

**1. Startup Validation**
- Host node checks if `MODEL_PATH` matches a registered model
- **Dynamic model discovery**: Node queries `getApprovedModelIds()` to build model map (no hardcoded list)
- If not authorized → node refuses to start
- SDK doesn't see failed hosts (they never come online)

**2. SHA256 Hash Verification** (NEW in v8.14.0)
- Host computes SHA256 hash of the model file on disk
- Queries `getModel(modelId)` to retrieve on-chain hash
- If hashes don't match → node refuses to start
- Prevents tampered or corrupted model files from running

**3. Job Claim Validation**
- Host queries `nodeSupportsModel(myAddress, job.modelId)`
- If not supported → host skips the job
- SDK sees job remain unclaimed longer (normal behavior)

**4. Inference Validation**
- Host verifies job's model matches loaded model
- If mismatch → returns error
- SDK receives error response (rare, only if host misconfigured)

### Dynamic Model Discovery (Host-Side)

**Important**: Host nodes no longer have hardcoded model lists. At startup, the node:

1. Queries `ModelRegistry.getApprovedModelIds()` to get all approved model IDs
2. For each model ID, queries `ModelRegistry.getModel(modelId)` to get:
   - `repo`: HuggingFace repository (e.g., "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF")
   - `filename`: Model file name (e.g., "tinyllama-1b.Q4_K_M.gguf")
   - `sha256Hash`: Expected file hash for integrity verification
   - `isApproved`: Whether model is still approved
3. Builds an internal map: `filename → { modelId, repo, sha256Hash }`
4. Extracts filename from `MODEL_PATH` and looks up the model ID
5. Verifies the local file's SHA256 matches the on-chain hash

**SDK Implication**: New models can be added to the contract without node software updates. Hosts simply need to:
1. Download the new model file
2. Ensure their node supports the model in NodeRegistry
3. Restart with `MODEL_PATH` pointing to the new file

---

## SDK Behavior Changes

### Before Model Validation (Security Vulnerability)

```typescript
// SDK creates job for GPT-4
const tx = await marketplace.createSessionJob(
  hostAddress,
  GPT_4_MODEL_ID,
  pricePerToken,
  maxDuration
);

// ❌ PROBLEM: Host claims job even if only has TinyLlama loaded
// ❌ Host runs TinyLlama but charges GPT-4 prices
// ❌ Client pays premium, gets cheap model
// ❌ No error - client doesn't know they were defrauded
```

### After Model Validation (Secure)

```typescript
// SDK creates job for GPT-4 - SAME CODE
const tx = await marketplace.createSessionJob(
  hostAddress,
  GPT_4_MODEL_ID,
  pricePerToken,
  maxDuration
);

// ✅ Host validates: Do I support GPT-4?
//    → If NO: Host doesn't claim (job stays pending)
//    → If YES: Host claims and runs GPT-4 (verified)
// ✅ Client guaranteed to get the model they paid for
// ✅ Better error handling (unclaimed jobs are visible)
```

### User Experience Improvements

| Scenario | Before Validation | After Validation |
|----------|------------------|------------------|
| Request supported model | Job claimed, inference runs | ✅ Same - works perfectly |
| Request unsupported model | Host claims, runs wrong model | ✅ Job unclaimed, clear error |
| Host fraud attempt | Succeeds silently | ✅ Prevented - host can't claim |
| Model mismatch | Client gets wrong results | ✅ Validation error returned |

---

## Optional SDK Improvements

While no changes are required, these improvements provide better UX:

### 1. Pre-Flight Validation (Recommended)

Check if host supports a model **before** creating the job:

```typescript
import { ethers } from 'ethers';

async function createJobWithValidation(
  nodeRegistry: ethers.Contract,
  marketplace: ethers.Contract,
  hostAddress: string,
  modelId: string,
  ...jobParams
) {
  // OPTIONAL: Pre-flight check (better UX)
  const supports = await nodeRegistry.nodeSupportsModel(hostAddress, modelId);

  if (!supports) {
    throw new Error(
      `Host ${hostAddress} doesn't support model ${modelId}. ` +
      `Please choose a different host or model.`
    );
  }

  // Create job (existing code)
  return await marketplace.createSessionJob(
    hostAddress,
    modelId,
    ...jobParams
  );
}
```

**Benefits**:
- Immediate feedback to user (don't wait for job to be unclaimed)
- Better error messages
- Saves gas (don't create unclaimed jobs)

### 2. Host Discovery by Model (Recommended)

Find all hosts that support a specific model:

```typescript
async function findHostsForModel(
  nodeRegistry: ethers.Contract,
  modelId: string
): Promise<string[]> {
  // Get all active hosts
  const allHosts = await nodeRegistry.getAllActiveNodes();

  // Filter by model support
  const supportingHosts = [];
  for (const host of allHosts) {
    const supports = await nodeRegistry.nodeSupportsModel(host, modelId);
    if (supports) {
      supportingHosts.push(host);
    }
  }

  return supportingHosts;
}

// Usage
const hosts = await findHostsForModel(nodeRegistry, GPT_4_MODEL_ID);
console.log(`${hosts.length} hosts support GPT-4`);
```

**Benefits**:
- Smart host selection
- Better marketplace UX
- Show users which hosts support which models

### 3. Bulk Model Query (Performance Optimization)

Get all models a host supports in one call:

```typescript
async function getHostCapabilities(
  nodeRegistry: ethers.Contract,
  hostAddress: string
) {
  // Single contract call returns all host info
  const [
    operator,
    stakedAmount,
    active,
    metadata,
    apiUrl,
    supportedModels,  // ← All models in one call!
    minPriceNative,
    minPriceStable
  ] = await nodeRegistry.getNodeFullInfo(hostAddress);

  return {
    address: operator,
    active,
    supportedModels,  // Array of model IDs (bytes32[])
    pricing: {
      native: minPriceNative,
      stable: minPriceStable
    }
  };
}

// Usage
const capabilities = await getHostCapabilities(nodeRegistry, hostAddress);
console.log('Host supports models:', capabilities.supportedModels);
```

**Benefits**:
- One contract call vs many (gas savings)
- Faster UI updates
- Complete host profile in one query

### 4. Model Metadata Helper

Display user-friendly model names.

**IMPORTANT**: The list below is for **display purposes only**. The source of truth for approved models is the **ModelRegistry contract**. Query `getAllApprovedModels()` to get the current list dynamically.

```typescript
interface ModelInfo {
  id: string;           // bytes32 model ID
  name: string;         // Human-readable name
  repo: string;         // HuggingFace repo
  filename: string;     // Model file
}

// EXAMPLE ONLY - Query contract for current list!
// Use: modelRegistry.getAllApprovedModels() for production
const EXAMPLE_MODELS: ModelInfo[] = [
  {
    id: '0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced',
    name: 'TinyVicuna 1B',
    repo: 'CohereForAI/TinyVicuna-1B-32k-GGUF',
    filename: 'tiny-vicuna-1b.q4_k_m.gguf'
  },
  {
    id: '0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca',
    name: 'TinyLlama 1.1B',
    repo: 'TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF',
    filename: 'tinyllama-1b.Q4_K_M.gguf'
  }
  // More models may be added to the contract over time
  // Always query the contract for the current list
];

// Recommended: Fetch models dynamically from contract
async function getApprovedModels(
  modelRegistry: ethers.Contract
): Promise<ModelInfo[]> {
  const modelIds = await modelRegistry.getAllApprovedModels();
  const models: ModelInfo[] = [];

  for (const id of modelIds) {
    const [repo, filename, , isApproved] = await modelRegistry.getModel(id);
    if (isApproved) {
      models.push({
        id,
        name: extractModelName(filename), // Your helper function
        repo,
        filename
      });
    }
  }
  return models;
}

function getModelName(modelId: string, models: ModelInfo[]): string {
  const model = models.find(m => m.id === modelId);
  return model?.name || 'Unknown Model';
}
```

---

## Error Handling Guide

### Error Scenario 1: Job Not Claimed

**Symptom**: Job stays in "pending" state, no host claims it

**Cause**: No hosts support the requested model (or all hosts offline)

**SDK Response**:
```typescript
// Poll job status with timeout
async function waitForJobClaim(
  marketplace: ethers.Contract,
  jobId: bigint,
  timeoutMs: number = 60000
): Promise<void> {
  const startTime = Date.now();

  while (Date.now() - startTime < timeoutMs) {
    const job = await marketplace.sessionJobs(jobId);

    if (job.host !== ethers.ZeroAddress) {
      console.log('Job claimed by:', job.host);
      return;
    }

    await new Promise(resolve => setTimeout(resolve, 2000));
  }

  throw new Error(
    'Job not claimed within timeout. ' +
    'Possible reasons: ' +
    '1. No hosts support this model, ' +
    '2. All hosts are offline, ' +
    '3. Price too low'
  );
}
```

### Error Scenario 2: Host Validation Error (Rare)

**Symptom**: WebSocket connection established, but inference fails

**Cause**: Host misconfiguration (running wrong model)

**SDK Response**:
```typescript
// WebSocket error handling
websocket.on('error', (error) => {
  if (error.message.includes('Model validation')) {
    console.error('Host validation error:', error);
    // Reconnect to different host
    reconnectWithDifferentHost();
  }
});
```

### Error Scenario 3: Model Not Approved

**Symptom**: No jobs created, or jobs immediately rejected

**Cause**: Trying to use a model not in ModelRegistry

**SDK Response**:
```typescript
async function validateModelApproved(
  modelRegistry: ethers.Contract,
  modelId: string
): Promise<void> {
  const isApproved = await modelRegistry.isModelApproved(modelId);

  if (!isApproved) {
    // Query current approved models dynamically
    const approvedIds = await modelRegistry.getAllApprovedModels();
    throw new Error(
      `Model ${modelId} is not approved. ` +
      `${approvedIds.length} models currently approved in ModelRegistry.`
    );
  }
}
```

### Error Scenario 4: Model Hash Mismatch (Host-Side Security)

**Symptom**: Host node fails to start with "ModelHashMismatch" error

**Cause**: Model file on disk has different SHA256 hash than registered on-chain

**SDK Impact**: None directly - host never comes online

**Why This Matters**:
- Prevents hosts from running tampered model files
- Detects corrupted downloads
- Ensures cryptographic proof of model integrity

**Host operator action required**:
```bash
# Host must re-download the correct model file
# The SHA256 hash must match what's registered in ModelRegistry

# Check on-chain hash:
cast call $MODEL_REGISTRY "getModel(bytes32)" $MODEL_ID
# Returns: (repo, filename, sha256Hash, isApproved)

# Compute local file hash:
sha256sum ./models/your-model.gguf
```

---

## Code Examples

### Complete Job Creation with Validation

```typescript
import { ethers } from 'ethers';

class ValidatedJobCreator {
  constructor(
    private marketplace: ethers.Contract,
    private nodeRegistry: ethers.Contract,
    private modelRegistry: ethers.Contract
  ) {}

  async createValidatedJob(params: {
    hostAddress: string;
    modelId: string;
    pricePerToken: bigint;
    maxDuration: number;
    paymentToken: string;
    depositAmount: bigint;
  }): Promise<{ jobId: bigint; txHash: string }> {

    // Step 1: Validate model is globally approved
    const isApproved = await this.modelRegistry.isModelApproved(params.modelId);
    if (!isApproved) {
      throw new Error(`Model ${params.modelId} not approved by ModelRegistry`);
    }

    // Step 2: Validate host supports this model
    const hostSupports = await this.nodeRegistry.nodeSupportsModel(
      params.hostAddress,
      params.modelId
    );
    if (!hostSupports) {
      throw new Error(
        `Host ${params.hostAddress} doesn't support model ${params.modelId}`
      );
    }

    // Step 3: Get host's pricing
    const [, , , , , , minPriceNative, minPriceStable] =
      await this.nodeRegistry.getNodeFullInfo(params.hostAddress);

    const minPrice = params.paymentToken === ethers.ZeroAddress
      ? minPriceNative
      : minPriceStable;

    if (params.pricePerToken < minPrice) {
      throw new Error(
        `Price too low. Host minimum: ${minPrice}, offered: ${params.pricePerToken}`
      );
    }

    // Step 4: Create job (all validations passed)
    const tx = await this.marketplace.createSessionForModelAsDelegate(
      params.hostAddress,
      params.modelId,
      params.paymentToken,
      params.depositAmount,
      params.pricePerToken,
      params.maxDuration,
      100, // proofInterval
      300  // proofTimeoutWindow
    );

    const receipt = await tx.wait();
    const event = receipt.logs.find(
      (log: any) => log.fragment?.name === 'SessionJobCreated'
    );
    const jobId = event?.args?.jobId;

    return {
      jobId,
      txHash: receipt.hash
    };
  }
}
```

### Host Selection UI Helper

```typescript
interface HostOption {
  address: string;
  name: string;
  supportedModels: string[];
  pricing: {
    native: bigint;
    stable: bigint;
  };
  online: boolean;
}

async function getAvailableHostsForModel(
  nodeRegistry: ethers.Contract,
  modelId: string
): Promise<HostOption[]> {
  const allHosts = await nodeRegistry.getAllActiveNodes();
  const hostOptions: HostOption[] = [];

  for (const hostAddress of allHosts) {
    try {
      const [
        ,
        ,
        active,
        metadata,
        ,
        supportedModels,
        minPriceNative,
        minPriceStable
      ] = await nodeRegistry.getNodeFullInfo(hostAddress);

      // Only include if host supports the model
      if (supportedModels.includes(modelId)) {
        hostOptions.push({
          address: hostAddress,
          name: JSON.parse(metadata).name || 'Unknown Host',
          supportedModels,
          pricing: {
            native: minPriceNative,
            stable: minPriceStable
          },
          online: active
        });
      }
    } catch (error) {
      console.warn(`Failed to fetch info for host ${hostAddress}:`, error);
    }
  }

  return hostOptions;
}

// Usage in UI
const hostsForGPT4 = await getAvailableHostsForModel(nodeRegistry, GPT_4_MODEL_ID);
console.log(`Found ${hostsForGPT4.length} hosts supporting GPT-4`);
hostsForGPT4.forEach(host => {
  console.log(`- ${host.name} (${host.address})`);
  console.log(`  Price: ${ethers.formatUnits(host.pricing.stable, 6)} USDC/million tokens`);
});
```

---

## Best Practices

### 1. Always Validate Before Job Creation

```typescript
// ❌ BAD: Create job without checking
await marketplace.createSessionJob(randomHost, randomModel, ...);

// ✅ GOOD: Validate first
const supports = await nodeRegistry.nodeSupportsModel(host, modelId);
if (supports) {
  await marketplace.createSessionJob(host, modelId, ...);
} else {
  // Show error or offer alternative hosts
}
```

### 2. Cache Model Lists

```typescript
// ✅ GOOD: Cache approved models (changes rarely)
let APPROVED_MODELS_CACHE: string[] | null = null;

async function getApprovedModels(
  modelRegistry: ethers.Contract
): Promise<string[]> {
  if (!APPROVED_MODELS_CACHE) {
    APPROVED_MODELS_CACHE = await modelRegistry.getAllApprovedModels();
  }
  return APPROVED_MODELS_CACHE;
}
```

### 3. Handle Graceful Degradation

```typescript
// ✅ GOOD: Fallback if validation fails
async function createJobWithFallback(
  nodeRegistry: ethers.Contract,
  marketplace: ethers.Contract,
  preferredHost: string,
  modelId: string,
  ...params
) {
  try {
    // Try preferred host
    const supports = await nodeRegistry.nodeSupportsModel(preferredHost, modelId);
    if (supports) {
      return await marketplace.createSessionJob(preferredHost, modelId, ...params);
    }
  } catch (error) {
    console.warn('Preferred host check failed:', error);
  }

  // Fallback: Find any supporting host
  const supportingHosts = await findHostsForModel(nodeRegistry, modelId);
  if (supportingHosts.length === 0) {
    throw new Error('No hosts support this model');
  }

  return await marketplace.createSessionJob(supportingHosts[0], modelId, ...params);
}
```

### 4. Monitor Job Status

```typescript
// ✅ GOOD: Track job lifecycle
class JobMonitor {
  async monitorJob(jobId: bigint) {
    const states = {
      PENDING: 'Waiting for host to claim',
      CLAIMED: 'Host claimed, ready for inference',
      ACTIVE: 'Inference in progress',
      COMPLETED: 'Job finished'
    };

    // Check every 5 seconds
    const interval = setInterval(async () => {
      const job = await marketplace.sessionJobs(jobId);

      if (job.host === ethers.ZeroAddress) {
        console.log(states.PENDING);
      } else if (!job.completed) {
        console.log(states.ACTIVE);
      } else {
        console.log(states.COMPLETED);
        clearInterval(interval);
      }
    }, 5000);
  }
}
```

---

## Migration Checklist

### For Existing SDK Code

- [ ] ✅ **No changes required** - existing code works as-is
- [ ] Review error handling for unclaimed jobs (optional improvement)
- [ ] Consider adding pre-flight validation (optional improvement)
- [ ] Test with hosts running v8.14.0+ nodes

### For New Features

- [ ] Implement `nodeSupportsModel()` pre-flight check
- [ ] Add `getNodeFullInfo()` for host capability discovery
- [ ] Update UI to show which hosts support which models
- [ ] Add model approval check before job creation
- [ ] Implement graceful fallback for unsupported models

### Testing Scenarios

- [ ] Create job for supported model → Should work normally
- [ ] Create job for unsupported model → Should remain unclaimed or error
- [ ] Query host capabilities → Should return accurate model list
- [ ] Handle validation errors gracefully → User-friendly messages

---

## FAQ

### Q: Do I need to update my SDK?

**A: No.** The validation is transparent. Existing SDK code continues to work without changes.

### Q: Will jobs fail more often now?

**A: No.** Jobs only "fail" if you request a model the host doesn't support - which was always invalid, but previously undetected. Now you get clear feedback instead of silent fraud.

### Q: How do I know which hosts support which models?

**A: Query the contract:**
```typescript
const supports = await nodeRegistry.nodeSupportsModel(hostAddress, modelId);
const allModels = await nodeRegistry.getNodeModels(hostAddress);
```

### Q: What if no hosts support my model?

**A: Three options:**
1. Choose a different (supported) model
2. Wait for hosts to register with that model
3. Contact host operators to add support

### Q: Can hosts bypass this validation?

**A: No.** Validation happens in the host node software. Hosts that bypass it:
- Can't claim jobs (validation fails)
- Can't run inference (runtime checks fail)
- Risk slashing for misbehavior

### Q: What's the performance impact?

**A: Minimal:**
- Pre-flight validation: +1 contract read (~50-100ms)
- Job claiming: No SDK impact (host-side only)
- Inference: No SDK impact (host-side only)

### Q: Does this affect WebSocket protocol?

**A: No.** WebSocket messages remain unchanged. Validation happens server-side before responses are sent.

### Q: What if a host lies about their models?

**A: They can't.**
1. Models are registered on-chain (immutable)
2. Host node validates against contract
3. Runtime checks prevent model switching
4. SHA256 hash verification prevents tampered files
5. Future: Slashing for proven fraud

### Q: What is SHA256 verification?

**A:** At startup, the host node:
1. Computes SHA256 hash of the model file on disk
2. Queries `ModelRegistry.getModel(modelId)` for the expected hash
3. If hashes don't match, the node refuses to start

This ensures hosts can't run modified/tampered model files. The integrity of the model is cryptographically verified against the on-chain hash.

### Q: Are model lists hardcoded in the node?

**A: No.** As of v8.14.0, the node dynamically queries the ModelRegistry contract at startup to discover approved models. This means:
- New models can be added to the contract without node software updates
- Hosts just download the new model and configure `MODEL_PATH`
- The node automatically recognizes any model approved in the contract

---

## Contract Reference

### NodeRegistry Functions (Model Validation)

```solidity
// Check if host supports a specific model
function nodeSupportsModel(address nodeAddress, bytes32 modelId)
    external view returns (bool);

// Get all models a host supports
function getNodeModels(address nodeAddress)
    external view returns (bytes32[] memory);

// Get complete host info (including supported models)
function getNodeFullInfo(address operator) external view returns (
    address operator,
    uint256 stakedAmount,
    bool active,
    string memory metadata,
    string memory apiUrl,
    bytes32[] memory supportedModels,  // ← Model list here
    uint256 minPricePerTokenNative,
    uint256 minPricePerTokenStable
);

// Get all active hosts
function getAllActiveNodes() external view returns (address[] memory);
```

### ModelRegistry Functions

```solidity
// Check if model is globally approved
function isModelApproved(bytes32 modelId)
    external view returns (bool);

// Calculate model ID from repo and filename
function getModelId(string memory repo, string memory filename)
    external pure returns (bytes32);

// Get all approved models
function getAllApprovedModels()
    external view returns (bytes32[] memory);
```

---

## Support

**Questions or Issues?**
- Check the main implementation doc: `/workspace/docs/IMPLEMENTATION-MODEL-VALIDATION.md`
- Contract API reference: `/workspace/docs/compute-contracts-reference/API_REFERENCE.md`
- GitHub Issues: https://github.com/fabstirp2p/contracts/issues

**Version Compatibility**:
- Host Node: v8.14.0+ (model validation enabled)
- Contracts: Remediation proxies (Feb 4, 2026)
- SDK: All versions compatible (no changes needed)

---

**Document Version**: 1.1.0
**Last Updated**: February 4, 2026
**Status**: Ready for SDK Integration

**Changelog**:
- v1.1.0: Added SHA256 hash verification, dynamic model discovery, ModelHashMismatch error documentation
- v1.0.0: Initial release
