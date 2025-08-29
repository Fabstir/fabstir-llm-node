# Node Staking Guide

This guide covers everything about staking for Fabstir compute nodes, including requirements, strategies, and risk management.

## Staking Mechanism

Fabstir uses FAB token staking for node registration:
- **FAB Token Staking**: 1000 FAB tokens via NodeRegistryFAB
- **No ETH staking**: FAB is the exclusive staking token

## Prerequisites

- Base wallet with FAB tokens (1000 FAB minimum)
- Small amount of ETH for gas fees (~0.01 ETH)
- Understanding of [Running a Node](running-a-node.md)
- Hardware wallet (recommended for mainnet)

## FAB Token Staking

### Why FAB Staking?
- **Low Barrier**: 1000 FAB (~$1,000) entry cost
- **Platform Native**: Uses Fabstir's native token
- **Full Access**: Complete access to job marketplace
- **Easy Entry/Exit**: Unstake anytime when not processing jobs

### Requirements
- **Minimum Stake**: 1000 FAB tokens
- **Lock Period**: None (but active jobs prevent withdrawal)
- **Slashing Risk**: No (tokens returned on unregistration)
- **Rewards**: Earned through job completion in USDC
- **Payment Flow**: USDC earnings accumulated, withdrawn in batches

### Economic Model
```
Stake (1000 FAB) → Node Registration → Claim Jobs → Complete Jobs → Accumulate USDC → Withdraw Earnings
                                           ↓                                          ↓
                                    No Slashing Risk                        40-46% Gas Savings
```


## Step 1: Prepare Your Stake

#### Calculate Required FAB
```javascript
const calculateFABStakeRequirement = () => {
    const minimumStake = 1000; // FAB tokens
    const gasETH = 0.01;       // ETH for gas fees
    
    console.log("FAB Stake Breakdown:");
    console.log(`Minimum Stake: ${minimumStake} FAB`);
    console.log(`Gas Fees: ${gasETH} ETH`);
    console.log(`Estimated Value: ~$1,000 USD`);
    
    return { fab: minimumStake, eth: gasETH };
};
```

#### Acquire FAB Tokens
- **FAB Token Address**: `0xC78949004B4EB6dEf2D66e49Cd81231472612D62` (Base Sepolia)
- **Options**:
  1. Purchase from DEX (when available)
  2. Request from faucet (testnet)
  3. OTC purchase from community

#### Register with FAB Tokens
```javascript
const { ethers } = require("ethers");

async function registerWithFAB() {
    const provider = new ethers.JsonRpcProvider(process.env.BASE_RPC_URL);
    const wallet = new ethers.Wallet(process.env.PRIVATE_KEY, provider);
    
    // Contract addresses on Base Sepolia
    const FAB_TOKEN = "0xC78949004B4EB6dEf2D66e49Cd81231472612D62";
    const NODE_REGISTRY_FAB = "0x87516C13Ea2f99de598665e14cab64E191A0f8c4";
    
    // 1. Approve FAB tokens
    const fabToken = new ethers.Contract(FAB_TOKEN, [
        "function approve(address spender, uint256 amount) returns (bool)",
        "function balanceOf(address) view returns (uint256)"
    ], wallet);
    
    const stakeAmount = ethers.parseEther("1000"); // 1000 FAB
    
    // Check balance
    const balance = await fabToken.balanceOf(wallet.address);
    console.log("FAB Balance:", ethers.formatEther(balance));
    
    if (balance < stakeAmount) {
        console.log("Insufficient FAB tokens!");
        return;
    }
    
    // Approve
    const approveTx = await fabToken.approve(NODE_REGISTRY_FAB, stakeAmount);
    await approveTx.wait();
    console.log("FAB tokens approved");
    
    // 2. Register node
    const nodeRegistry = new ethers.Contract(NODE_REGISTRY_FAB, [
        "function registerNode(string memory metadata)"
    ], wallet);
    
    const metadata = "gpu:rtx4090,model:llama2,region:us-west";
    const registerTx = await nodeRegistry.registerNode(metadata);
    await registerTx.wait();
    
    console.log("Node registered with FAB stake!");
}
```


### Security Setup
```javascript
// Best practice: Use a dedicated staking wallet
const setupStakingWallet = () => {
    // 1. Create new wallet for node operations
    const nodeWallet = ethers.Wallet.createRandom();
    
    // 2. Use hardware wallet for stake custody
    // Connect Ledger/Trezor for mainnet
    
    // 3. Set up multisig for team operations
    // Consider Gnosis Safe on Base
};
```

## Step 2: Check Staking Requirements
```javascript
async function checkFABStakeRequirements() {
    const provider = new ethers.JsonRpcProvider(process.env.BASE_RPC_URL);
    const wallet = new ethers.Wallet(process.env.PRIVATE_KEY, provider);
    
    const fabToken = new ethers.Contract(
        "0xC78949004B4EB6dEf2D66e49Cd81231472612D62",
        ["function balanceOf(address) view returns (uint256)"],
        provider
    );
    
    const balance = await fabToken.balanceOf(wallet.address);
    const required = ethers.parseEther("1000");
    
    console.log("FAB Balance:", ethers.formatEther(balance));
    console.log("Required:", "1000 FAB");
    console.log("Can stake:", balance >= required ? "✓ Yes" : "✗ No");
    
    if (balance < required) {
        const needed = required - balance;
        console.log("Need", ethers.formatEther(needed), "more FAB");
    }
}
```


## Step 3: Register with FAB Stake

Use the FAB-based registration example from `/workspace/docs/examples/basic/register-node-fab.js`

## Step 4: Manage Your Stake

### Monitor Stake Status
```javascript
async function monitorStake() {
    const provider = new ethers.JsonRpcProvider(process.env.BASE_RPC_URL);
    
    const nodeRegistryFABABI = [
        "function nodes(address) view returns (address operator, uint256 stakedAmount, bool active, string metadata)"
    ];
    
    const nodeRegistry = new ethers.Contract(
        "0x87516C13Ea2f99de598665e14cab64E191A0f8c4", // NodeRegistryFAB
        nodeRegistryFABABI,
        provider
    );
    
    const address = process.env.NODE_ADDRESS;
    
    // Get node info
    const [operator, stakedAmount, active, metadata] = await nodeRegistry.nodes(address);
    
    console.log("Current stake:", ethers.formatEther(stakedAmount), "FAB");
    console.log("Node status:", {
        active: active,
        metadata: metadata
    });
    
    // Calculate stake value (assuming $1 per FAB)
    const fabPrice = 1; // $1 per FAB
    const stakeValue = parseFloat(ethers.formatEther(stakedAmount)) * fabPrice;
    console.log("Stake value: $", stakeValue.toFixed(2));
}
```

### Withdraw FAB Stake
```javascript
async function withdrawStake() {
    const provider = new ethers.JsonRpcProvider(process.env.BASE_RPC_URL);
    const wallet = new ethers.Wallet(process.env.PRIVATE_KEY, provider);
    
    const nodeRegistryFABABI = [
        "function unregisterNode()",
        "event NodeUnregistered(address indexed operator, uint256 returnedAmount)"
    ];
    
    const nodeRegistry = new ethers.Contract(
        "0x87516C13Ea2f99de598665e14cab64E191A0f8c4", // NodeRegistryFAB
        nodeRegistryFABABI,
        wallet
    );
    
    console.log("Unregistering node and withdrawing FAB stake...");
    
    const tx = await nodeRegistry.unregisterNode();
    console.log("Transaction:", tx.hash);
    
    const receipt = await tx.wait();
    console.log("FAB stake withdrawn successfully!");
}
```

## Step 5: Risk Management

### No Slashing Risk
With FAB staking, there is no slashing mechanism. Your staked FAB tokens are:
- Safe from slashing
- Returned in full when unregistering
- Only locked while actively processing jobs


## Staking ROI Calculator

```javascript
class StakingCalculator {
    constructor(stakeAmount, avgJobsPerDay, avgPaymentPerJob) {
        this.stakeAmount = stakeAmount; // in FAB
        this.avgJobsPerDay = avgJobsPerDay;
        this.avgPaymentPerJob = avgPaymentPerJob; // in USDC
        this.platformFee = 0.10; // 10%
        this.gasPerWithdrawal = 0.002; // ETH for earnings withdrawal
    }
    
    calculateDailyEarnings() {
        const grossEarnings = this.avgJobsPerDay * this.avgPaymentPerJob;
        const netEarnings = grossEarnings * (1 - this.platformFee);
        // Note: Earnings accumulate - no gas cost per job
        return netEarnings;
    }
    
    calculateGasSavings(days) {
        const jobsTotal = this.avgJobsPerDay * days;
        const traditionalGasPerJob = 0.003; // ETH (~$5)
        const accumulationGasPerJob = 0.0018; // ETH (~$3)
        const savings = jobsTotal * (traditionalGasPerJob - accumulationGasPerJob);
        return savings;
    }
    
    calculateROI(days) {
        const totalEarnings = this.calculateDailyEarnings() * days;
        const roi = (totalEarnings / this.stakeAmount) * 100;
        return {
            earnings: totalEarnings,
            roi: roi.toFixed(2) + '%',
            breakeven: this.stakeAmount / this.calculateDailyEarnings()
        };
    }
    
    projectReturns() {
        console.log("Staking Projections:");
        console.log("Stake:", this.stakeAmount, "FAB");
        console.log("Daily earnings:", this.calculateDailyEarnings().toFixed(2), "USDC");
        
        const periods = [30, 90, 180, 365];
        periods.forEach(days => {
            const projection = this.calculateROI(days);
            console.log(`\n${days} days:`);
            console.log("- Earnings:", projection.earnings.toFixed(2), "USDC");
            console.log("- ROI:", projection.roi);
        });
        
        console.log("\nBreakeven:", this.calculateROI(0).breakeven.toFixed(0), "days");
        
        // Show gas savings with earnings accumulation
        console.log("\n=== Gas Savings with Earnings Accumulation ===");
        periods.forEach(days => {
            const savings = this.calculateGasSavings(days);
            console.log(`${days} days: ${savings.toFixed(4)} ETH saved (~$${(savings * 1700).toFixed(0)})`);
        });
    }
}

// Example usage
const calculator = new StakingCalculator(
    1000,   // 1000 FAB stake
    50,     // 50 jobs per day
    10      // 10 USDC average per job
);
calculator.projectReturns();
```

## Common Issues & Solutions

### Issue: Transaction Fails with "Insufficient FAB"
```javascript
// Solution: Check FAB balance and requirement
const fabToken = new ethers.Contract(FAB_ADDRESS, FAB_ABI, provider);
const balance = await fabToken.balanceOf(wallet.address);
const required = ethers.parseEther("1000");

console.log("FAB balance:", ethers.formatEther(balance));
console.log("Required:", "1000 FAB");
```

### Issue: Can't Withdraw Stake
```javascript
// Check for active jobs
async function checkActiveJobs(nodeAddress) {
    const jobMarketplaceABI = ["function getActiveJobIds() view returns (uint256[])"];
    const jobMarketplace = new ethers.Contract(JOB_MARKETPLACE_ADDRESS, jobMarketplaceABI, provider);
    
    const activeJobs = await jobMarketplace.getActiveJobIds();
    // Check if any assigned to your node
    
    if (hasActiveJobs) {
        console.log("Cannot withdraw: Active jobs in progress");
    }
}
```


## Earnings Withdrawal (NEW)

With the new earnings accumulation system, hosts no longer receive direct payments per job. Instead:

### How It Works
1. Complete jobs to accumulate USDC earnings
2. Check accumulated balance anytime
3. Withdraw when convenient (batch withdrawal saves gas)

### Check Accumulated Earnings
```javascript
async function checkEarnings() {
    const provider = new ethers.JsonRpcProvider(process.env.BASE_RPC_URL);
    const wallet = new ethers.Wallet(process.env.PRIVATE_KEY, provider);
    
    const HOST_EARNINGS = "0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E";
    const USDC = "0x036CbD53842c5426634e7929541eC2318f3dCF7e";
    
    const hostEarnings = new ethers.Contract(HOST_EARNINGS, [
        "function getBalance(address host, address token) view returns (uint256)"
    ], provider);
    
    const balance = await hostEarnings.getBalance(wallet.address, USDC);
    console.log("Accumulated USDC:", ethers.formatUnits(balance, 6));
}
```

### Withdraw Earnings
```javascript
async function withdrawEarnings() {
    const provider = new ethers.JsonRpcProvider(process.env.BASE_RPC_URL);
    const wallet = new ethers.Wallet(process.env.PRIVATE_KEY, provider);
    
    const HOST_EARNINGS = "0xcbD91249cC8A7634a88d437Eaa083496C459Ef4E";
    const USDC = "0x036CbD53842c5426634e7929541eC2318f3dCF7e";
    
    const hostEarnings = new ethers.Contract(HOST_EARNINGS, [
        "function withdrawAll(address token)",
        "function withdraw(uint256 amount, address token)"
    ], wallet);
    
    // Option 1: Withdraw all accumulated earnings
    const tx = await hostEarnings.withdrawAll(USDC);
    await tx.wait();
    console.log("All earnings withdrawn!");
    
    // Option 2: Withdraw specific amount
    // const amount = ethers.parseUnits("100", 6); // 100 USDC
    // const tx = await hostEarnings.withdraw(amount, USDC);
}
```

### Gas Optimization Benefits
- **Traditional**: ~115,000 gas per job completion
- **With Accumulation**: ~69,000 gas per job completion
- **Savings**: 40% reduction in gas costs
- **Example**: Complete 10 jobs, withdraw once = 460,000 gas saved

## Best Practices

### 1. Stake Management
- Ensure you have 1000 FAB tokens before registering
- Monitor your node's active status
- Keep some ETH for gas fees
- Use hardware wallets for secure FAB storage

### 2. Earnings Management (NEW)
- Let earnings accumulate before withdrawing
- Withdraw during low gas periods
- Track accumulated balance regularly
- Consider tax implications of batch withdrawals

### 2. Risk Mitigation
- No slashing risk with FAB staking
- Maintain high uptime for more jobs
- Complete jobs reliably for reputation
- Monitor your node performance

### 3. Tax Considerations
```javascript
// Track all staking events for tax
const trackStakingEvents = () => {
    // Log:
    // - Initial FAB stake date and amount
    // - USDC earnings from jobs
    // - Withdrawal dates
    // - FAB/USDC prices at each event
};
```

## Next Steps

1. **[Claiming Jobs](claiming-jobs.md)** - Maximize earnings
2. **[Node Monitoring](../advanced/monitoring-setup.md)** - Protect your stake
3. **[Governance](../advanced/governance-participation.md)** - Influence staking rules

## Resources

- [Ethereum Staking Calculator](https://stakingcalculator.com)
- [Base Network Statistics](https://basescan.org/stat/supply)
- [DeFi Hedging Strategies](https://defillama.com)
- [Tax Guide for Stakers](https://tokentax.co)

---

Questions about staking? Join our [Discord](https://discord.gg/fabstir) →