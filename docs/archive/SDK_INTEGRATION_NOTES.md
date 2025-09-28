# SDK Integration Notes - Multi-Chain Update

**Version**: Multi-chain support (Phases 1-6 + 11.1 complete)
**Date**: December 2024
**For**: SDK Developer integrating with fabstir-llm-node

## 🚨 Critical Changes in This Version

### 1. Multi-Chain Support is Now Required

The node now operates across multiple chains simultaneously. Your SDK MUST specify chain_id in all requests.

**Supported Chains:**
- **Base Sepolia**: Chain ID `84532` (Ethereum L2)
- **opBNB Testnet**: Chain ID `5611` (BSC L2)

### 2. Breaking API Changes

#### WebSocket Session Initialization
**OLD:**
```json
{
  "type": "session_init",
  "session_id": "sess_123",
  "job_id": "0x...",
  "user_address": "0x..."
}
```

**NEW (REQUIRED):**
```json
{
  "type": "session_init",
  "session_id": "sess_123",
  "chain_id": 84532,  // <-- MUST include chain_id
  "job_id": "0x...",
  "user_address": "0x...",
  "signature": "0x..."
}
```

#### HTTP Inference Requests
**OLD:**
```json
POST /inference
{
  "prompt": "Hello",
  "job_id": "0x...",
  "max_tokens": 100
}
```

**NEW (REQUIRED):**
```json
POST /inference
{
  "chain_id": 84532,  // <-- MUST include chain_id
  "prompt": "Hello",
  "job_id": "0x...",
  "max_tokens": 100
}
```

## 📋 Integration Checklist

### Phase 1: Update Request Structures
- [ ] Add `chain_id` field to all request types
- [ ] Update WebSocket message types to include `chain_id`
- [ ] Update session initialization to include chain parameter
- [ ] Add chain selection UI/configuration in your SDK

### Phase 2: Contract Address Management
- [ ] Store contract addresses PER CHAIN (not global)
- [ ] Update job marketplace address based on chain_id
- [ ] Update node registry address based on chain_id
- [ ] Handle different payment tokens (ETH vs BNB)

### Phase 3: Settlement Handling
- [ ] Expect automatic settlement when WebSocket disconnects
- [ ] Track settlements per chain
- [ ] Handle chain-specific gas costs
- [ ] Monitor settlement events on correct chain

## 💡 Key Integration Points

### 1. Chain Configuration

Your SDK should maintain a configuration like:

```javascript
const CHAIN_CONFIGS = {
  84532: {
    name: "Base Sepolia",
    rpc: "https://sepolia.base.org",
    contracts: {
      jobMarketplace: "0xaa38e7fcf5d7944ef7c836e8451f3bf93b98364f",
      nodeRegistry: "0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218",
      paymentEscrow: "0x908962e8c6CE72610021586f85ebDE09aAc97776"
    },
    nativeToken: "ETH"
  },
  5611: {
    name: "opBNB Testnet",
    rpc: "https://opbnb-testnet-rpc.bnbchain.org",
    contracts: {
      jobMarketplace: "0x...", // Get from node's /chains endpoint
      nodeRegistry: "0x...",
      paymentEscrow: "0x..."
    },
    nativeToken: "BNB"
  }
};
```

### 2. WebSocket Connection Flow

```javascript
// 1. Connect to WebSocket
const ws = new WebSocket('ws://node-address:8080/ws');

// 2. Initialize session with chain_id
ws.send(JSON.stringify({
  type: 'session_init',
  session_id: generateSessionId(),
  chain_id: 84532,  // CRITICAL: Specify chain
  job_id: '0x...',
  user_address: userAddress,
  signature: await signMessage(...)
}));

// 3. Handle session acceptance
ws.on('message', (msg) => {
  const data = JSON.parse(msg);
  if (data.type === 'session_accepted') {
    // Session ready, can send prompts
  }
});

// 4. Send prompts (chain context maintained in session)
ws.send(JSON.stringify({
  type: 'prompt',
  session_id: sessionId,
  content: 'Your prompt here'
}));

// 5. Disconnect triggers automatic settlement
ws.close(); // Settlement happens on the chain specified during init
```

### 3. Error Handling

New chain-specific errors to handle:

```javascript
const CHAIN_ERRORS = {
  'INVALID_CHAIN': 'Chain ID not supported by this node',
  'CHAIN_MISMATCH': 'Job exists on different chain',
  'NO_REGISTRATION': 'Node not registered on this chain',
  'SETTLEMENT_FAILED': 'Settlement failed on chain'
};
```

## 🧪 Testing Your Integration

### Step 1: Basic Multi-Chain Test
```javascript
// Test both chains work
async function testMultiChain() {
  // Test Base Sepolia
  const baseResponse = await node.inference({
    chain_id: 84532,
    job_id: baseSepoliaJobId,
    prompt: "Test on Base"
  });

  // Test opBNB
  const bnbResponse = await node.inference({
    chain_id: 5611,
    job_id: opBNBJobId,
    prompt: "Test on BNB"
  });

  assert(baseResponse.chain_id === 84532);
  assert(bnbResponse.chain_id === 5611);
}
```

### Step 2: Chain Switching Test
```javascript
// Test switching chains with different sessions
async function testChainSwitching() {
  // Session 1 on Base Sepolia
  const session1 = await createSession(84532, baseJob);
  await session1.prompt("Hello from Base");

  // Session 2 on opBNB (concurrent)
  const session2 = await createSession(5611, bnbJob);
  await session2.prompt("Hello from BNB");

  // Both should work independently
  await session1.close(); // Settles on Base
  await session2.close(); // Settles on BNB
}
```

### Step 3: Settlement Verification
```javascript
// Verify settlement happens on correct chain
async function testSettlement() {
  const session = await createSession(84532, jobId);
  const initialBalance = await getBalance(84532, userAddress);

  await session.prompt("Generate tokens");
  await session.close(); // Triggers settlement

  // Wait for settlement
  await sleep(5000);

  // Check payment on Base Sepolia (not other chains!)
  const finalBalance = await getBalance(84532, userAddress);
  assert(finalBalance < initialBalance); // Payment made
}
```

## ⚠️ Common Integration Issues

### 1. "Invalid Chain" Error
**Problem**: Node doesn't support the chain_id you're sending
**Solution**: Use `/chains` endpoint to get supported chains dynamically

### 2. "Job Not Found" on Valid Job
**Problem**: Job exists but on different chain than specified
**Solution**: Ensure chain_id matches where job was created

### 3. Settlement Not Happening
**Problem**: WebSocket closes but no settlement transaction
**Solution**: Check you're monitoring the correct chain for settlement events

### 4. Gas Estimation Failures
**Problem**: Different chains have different gas requirements
**Solution**: Use chain-specific gas settings (Base: low gas, opBNB: higher gas)

## 📊 Performance Considerations

1. **RPC Calls**: Each chain has separate RPC limits
   - Base Sepolia: ~10 req/sec on public RPC
   - opBNB Testnet: ~5 req/sec on public RPC
   - Consider using paid RPC services for production

2. **Settlement Timing**:
   - Base Sepolia: ~2 second blocks
   - opBNB Testnet: ~1 second blocks
   - Plan UI accordingly

3. **Connection Pooling**:
   - Node maintains separate connection pools per chain
   - Don't overwhelm single chain with all traffic

## 🔍 Debugging Tools

### Get Node's Chain Status
```bash
curl http://node:8080/chains
```

### Check Specific Chain Config
```bash
curl http://node:8080/chain/84532
```

### Monitor WebSocket Messages
```javascript
// Enable debug logging in your SDK
ws.on('message', (msg) => {
  console.log('[WS Received]', JSON.parse(msg));
});
```

### Verify Job Chain
```javascript
// Before connecting, verify job is on expected chain
const job = await jobMarketplace.getJob(jobId);
assert(job.chainId === expectedChainId);
```

## 📚 Documentation References

- **Configuration**: `docs/MULTI_CHAIN_CONFIG.md` - Detailed chain setup
- **API Reference**: `docs/API.md` - Complete API documentation
- **Troubleshooting**: `docs/TROUBLESHOOTING.md` - Common issues
- **Deployment**: `docs/DEPLOYMENT.md` - Production deployment

## 🚀 Quick Start Example

```javascript
// Minimal working example
const FabstirClient = require('@fabstir/sdk');

async function main() {
  // Initialize client
  const client = new FabstirClient({
    nodeUrl: 'http://localhost:8080',
    chainId: 84532  // Default to Base Sepolia
  });

  // Create job on Base Sepolia first
  const job = await createJobOnChain(84532, {
    model: 'llama-3-8b',
    maxTokens: 1000,
    payment: ethers.utils.parseEther('0.01')
  });

  // Connect to node with job
  const session = await client.createSession({
    chainId: 84532,
    jobId: job.id,
    userAddress: wallet.address
  });

  // Stream inference
  const response = await session.prompt('Explain Web3');

  // Settlement happens automatically on disconnect
  await session.close();
}
```

## 🆘 Support Channels

1. **GitHub Issues**: File bugs with reproduction steps
2. **Discord**: Real-time help in #sdk-integration channel
3. **Direct Contact**: sdk-support@fabstir.com

## 🎯 Next Steps After Integration

Once your SDK integration is working:

1. **Report Issues**: Any problems become our Phase 10 test cases
2. **Performance Feedback**: Help us optimize Phase 7 (Gas Management)
3. **Error Scenarios**: Your edge cases inform Phase 8 (Error Handling)
4. **Monitoring Needs**: Tell us what metrics you need (Phase 9)

Your integration experience directly shapes our remaining development phases!

---

**Remember**: The node is multi-chain by default now. There is no single-chain mode. Every request must specify which blockchain to use via `chain_id`.