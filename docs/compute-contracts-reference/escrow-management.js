/**
 * Example: Deposit/Withdrawal Management (Multi-Chain)
 * Purpose: Demonstrates the deposit/withdrawal pattern for multi-chain/wallet support
 * Prerequisites:
 *   - Understanding of deposit/withdrawal pattern
 *   - Native tokens (ETH/BNB) or ERC20 tokens (USDC)
 *   - Works with both EOA and Smart Wallets
 *
 * Last Updated: January 25, 2025
 */

const { ethers } = require('ethers');
require('dotenv').config({ path: '../.env' });

// Contract ABIs - JobMarketplaceWithModels deposit/withdrawal functions
const JOB_MARKETPLACE_ABI = [
    'function depositNative() payable',
    'function depositToken(address token, uint256 amount)',
    'function withdrawNative(uint256 amount)',
    'function withdrawToken(address token, uint256 amount)',
    'function userDepositsNative(address user) view returns (uint256)',
    'function userDepositsToken(address user, address token) view returns (uint256)',
    'function getUserBalances(address user, address[] memory tokens) view returns (uint256[] memory)',
    'function createSessionFromDeposit(address host, address token, uint256 deposit, uint256 pricePerToken, uint256 duration, uint256 proofInterval) returns (uint256)',
    'event DepositReceived(address indexed depositor, uint256 amount, address indexed token)',
    'event WithdrawalProcessed(address indexed depositor, uint256 amount, address indexed token)',
    'event SessionCreatedByDepositor(uint256 indexed sessionId, address indexed depositor, address indexed host, uint256 deposit)'
];

const ERC20_ABI = [
    'function approve(address spender, uint256 amount) returns (bool)',
    'function allowance(address owner, address spender) view returns (uint256)',
    'function balanceOf(address account) view returns (uint256)',
    'function symbol() view returns (string)',
    'function decimals() view returns (uint8)'
];

const JOB_MARKETPLACE_ABI = [
    'function getJob(uint256 jobId) view returns (tuple(uint256 id, address poster, string modelId, uint256 payment, uint256 maxTokens, uint256 deadline, address assignedHost, uint8 status, bytes inputData, bytes outputData, uint256 postedAt, uint256 completedAt))',
    'function cancelJob(uint256 jobId)',
    'function disputeJob(uint256 jobId, string reason)'
];

// Configuration - Multi-chain deposit/withdrawal system
const config = {
    // Base Sepolia (ETH)
    baseSepolia: {
        rpcUrl: process.env.BASE_RPC_URL || 'https://sepolia.base.org',
        chainId: 84532,
        nativeSymbol: 'ETH',
        jobMarketplace: '0xaa38e7fcf5d7944ef7c836e8451f3bf93b98364f', // Multi-chain support
        hostEarnings: '0x908962e8c6CE72610021586f85ebDE09aAc97776',
        tokens: {
            USDC: '0x036CbD53842c5426634e7929541eC2318f3dCF7e',
            FAB: '0xC78949004B4EB6dEf2D66e49Cd81231472612D62'
        }
    },

    // opBNB Testnet (BNB) - Future deployment
    opBNB: {
        rpcUrl: process.env.OPBNB_RPC_URL || 'https://opbnb-testnet-rpc.bnbchain.org',
        chainId: 5611,
        nativeSymbol: 'BNB',
        jobMarketplace: 'TBD', // Post-MVP
        tokens: {
            USDC: 'TBD'
        }
    },
    
    // Gas settings
    gasLimit: 200000,
    maxFeePerGas: ethers.parseUnits('50', 'gwei'),
    maxPriorityFeePerGas: ethers.parseUnits('2', 'gwei')
};

// Escrow manager class
class EscrowManager {
    constructor(escrowContract, provider) {
        this.escrow = escrowContract;
        this.provider = provider;
        this.tokenCache = new Map();
    }
    
    async getTokenInfo(tokenAddress) {
        
        if (!this.tokenCache.has(tokenAddress)) {
            const token = new ethers.Contract(tokenAddress, ERC20_ABI, this.provider);
            const [symbol, decimals] = await Promise.all([
                token.symbol(),
                token.decimals()
            ]);
            this.tokenCache.set(tokenAddress, { symbol, decimals });
        }
        
        return this.tokenCache.get(tokenAddress);
    }
    
    async formatBalance(amount, tokenAddress) {
        const info = await this.getTokenInfo(tokenAddress);
        return `${ethers.formatUnits(amount, info.decimals)} ${info.symbol}`;
    }
    
    async checkAllBalances(account) {
        console.log(`\n💰 Escrow Balances for ${account}:`);
        
        const supportedTokens = await this.escrow.getSupportedTokens();
        const balances = [];
        
        // Check token balances
        for (const token of supportedTokens) {
            const balance = await this.escrow.getBalance(account, token);
            if (balance > 0n) {
                balances.push({
                    token,
                    balance,
                    formatted: await this.formatBalance(balance, token)
                });
            }
        }
        
        if (balances.length === 0) {
            console.log('   No balances in escrow');
        } else {
            balances.forEach(b => {
                console.log(`   • ${b.formatted}`);
            });
        }
        
        // Check locked funds
        const totalLocked = await this.escrow.getTotalLocked(account);
        if (totalLocked > 0n) {
            console.log(`   🔒 Total locked: ${ethers.formatEther(totalLocked)} ETH equivalent`);
        }
        
        return balances;
    }
}

// Example: Deposit USDC to escrow
async function depositExample(escrow, wallet) {
    console.log('\n📥 USDC Deposit Example');
    
    const usdcAddress = config.tokens.USDC;
    const usdc = new ethers.Contract(usdcAddress, ERC20_ABI, wallet);
    const amount = ethers.parseUnits('10', 6); // 10 USDC
    
    console.log(`   Depositing 10 USDC...`);
    
    // Approve escrow to spend USDC
    console.log('   Approving USDC transfer...');
    const approveTx = await usdc.approve(escrow.target, amount);
    await approveTx.wait();
    
    // Deposit USDC
    const tx = await escrow.deposit(usdcAddress, amount, {
        gasLimit: config.gasLimit,
        maxFeePerGas: config.maxFeePerGas,
        maxPriorityFeePerGas: config.maxPriorityFeePerGas
    });
    
    console.log(`   Transaction: ${tx.hash}`);
    const receipt = await tx.wait();
    console.log(`   ✅ USDC deposit successful!`);
    
    // Check new balance
    const newBalance = await escrow.getBalance(wallet.address, usdcAddress);
    console.log(`   New escrow balance: ${ethers.formatUnits(newBalance, 6)} USDC`);
    
    return receipt;
}

// Example: Deposit ERC20 tokens
async function depositTokenExample(escrow, wallet, tokenAddress, amount) {
    console.log('\n🪙 Token Deposit Example');
    
    const token = new ethers.Contract(tokenAddress, ERC20_ABI, wallet);
    const tokenInfo = await new EscrowManager(escrow, wallet.provider).getTokenInfo(tokenAddress);
    
    console.log(`   Depositing ${ethers.formatUnits(amount, tokenInfo.decimals)} ${tokenInfo.symbol}...`);
    
    // Check token balance
    const tokenBalance = await token.balanceOf(wallet.address);
    if (tokenBalance < amount) {
        throw new Error(`Insufficient ${tokenInfo.symbol} balance`);
    }
    
    // Approve escrow to spend tokens
    console.log('   Approving token transfer...');
    const approveTx = await token.approve(escrow.target, amount);
    await approveTx.wait();
    
    // Deposit tokens
    console.log('   Depositing tokens...');
    const depositTx = await escrow.deposit(tokenAddress, amount, {
        gasLimit: config.gasLimit,
        maxFeePerGas: config.maxFeePerGas,
        maxPriorityFeePerGas: config.maxPriorityFeePerGas
    });
    
    console.log(`   Transaction: ${depositTx.hash}`);
    const receipt = await depositTx.wait();
    console.log(`   ✅ Token deposit successful!`);
    
    return receipt;
}

// Example: Withdraw funds
async function withdrawExample(escrow, wallet, tokenAddress, amount) {
    console.log('\n📤 Withdraw Example');
    
    const manager = new EscrowManager(escrow, wallet.provider);
    const formatted = await manager.formatBalance(amount, tokenAddress);
    
    console.log(`   Withdrawing ${formatted}...`);
    
    // Check balance
    const balance = await escrow.getBalance(wallet.address, tokenAddress);
    if (balance < amount) {
        throw new Error('Insufficient escrow balance');
    }
    
    // Withdraw
    const tx = await escrow.withdraw(tokenAddress, amount, {
        gasLimit: config.gasLimit,
        maxFeePerGas: config.maxFeePerGas,
        maxPriorityFeePerGas: config.maxPriorityFeePerGas
    });
    
    console.log(`   Transaction: ${tx.hash}`);
    const receipt = await tx.wait();
    console.log(`   ✅ Withdrawal successful!`);
    
    // Check new balance
    const newBalance = await escrow.getBalance(wallet.address, tokenAddress);
    const newFormatted = await manager.formatBalance(newBalance, tokenAddress);
    console.log(`   Remaining escrow balance: ${newFormatted}`);
    
    return receipt;
}

// Example: Handle job payment flow
async function jobPaymentFlow(contracts, jobId) {
    console.log(`\n💼 Job Payment Flow for Job #${jobId}`);
    
    const job = await contracts.marketplace.getJob(jobId);
    const manager = new EscrowManager(contracts.escrow, contracts.marketplace.provider);
    
    console.log('   Job Details:');
    console.log(`   • Poster: ${job.poster}`);
    console.log(`   • Host: ${job.assignedHost || 'Not assigned'}`);
    console.log(`   • Payment: ${ethers.formatUnits(job.payment, 6)} USDC`);
    console.log(`   • Status: ${['Posted', 'Claimed', 'Completed', 'Cancelled'][job.status]}`);
    
    // Check locked funds
    try {
        const locked = await contracts.escrow.getLockedFunds(jobId);
        console.log('\n   Locked Funds:');
        console.log(`   • Amount: ${await manager.formatBalance(locked.amount, locked.token)}`);
        console.log(`   • From: ${locked.from}`);
        console.log(`   • To: ${locked.to || 'Not assigned'}`);
        console.log(`   • Released: ${locked.released ? 'Yes' : 'No'}`);
        
        if (!locked.released && job.status === 2) { // Completed
            console.log('\n   ⚠️  Job completed but funds not released!');
        }
    } catch (error) {
        console.log('   No locked funds for this job');
    }
    
    return job;
}

// Example: Multi-token payment job
async function multiTokenJobExample(contracts, wallet) {
    console.log('\n🌈 Multi-Token Payment Example');
    
    // Simulate a job that accepts USDC payment
    const paymentOptions = [
        { token: config.tokens.USDC, amount: ethers.parseUnits('10', 6) }, // 10 USDC
        { token: config.tokens.USDC, amount: ethers.parseUnits('25', 6) }, // 25 USDC
        { token: config.tokens.USDC, amount: ethers.parseUnits('50', 6) } // 50 USDC
    ];
    
    console.log('   Payment options for job:');
    const manager = new EscrowManager(contracts.escrow, wallet.provider);
    
    for (const option of paymentOptions) {
        const formatted = await manager.formatBalance(option.amount, option.token);
        console.log(`   • ${formatted}`);
    }
    
    // Check which tokens user has in escrow
    console.log('\n   Checking available balances...');
    const balances = await manager.checkAllBalances(wallet.address);
    
    // Find best payment option
    const availableOption = paymentOptions.find(option => {
        const balance = balances.find(b => b.token === option.token);
        return balance && balance.balance >= option.amount;
    });
    
    if (availableOption) {
        const formatted = await manager.formatBalance(availableOption.amount, availableOption.token);
        console.log(`\n   ✅ Can pay with ${formatted}`);
    } else {
        console.log('\n   ❌ Insufficient balance for any payment option');
    }
    
    return availableOption;
}

// Example: Handle refunds
async function refundExample(contracts, jobId, reason) {
    console.log(`\n💸 Refund Example for Job #${jobId}`);
    console.log(`   Reason: ${reason}`);
    
    // In a real implementation, this would be called by an admin or dispute resolver
    const job = await contracts.marketplace.getJob(jobId);
    
    if (job.status === 2) {
        console.log('   ❌ Cannot refund completed job');
        return;
    }
    
    try {
        const locked = await contracts.escrow.getLockedFunds(jobId);
        
        if (locked.released) {
            console.log('   ❌ Funds already released');
            return;
        }
        
        const manager = new EscrowManager(contracts.escrow, contracts.marketplace.provider);
        const formatted = await manager.formatBalance(locked.amount, locked.token);
        
        console.log(`   Refunding ${formatted} to ${locked.from}...`);
        
        // Note: In production, only authorized addresses can call refund
        const tx = await contracts.escrow.refundFunds(
            locked.from,
            locked.token,
            locked.amount,
            jobId
        );
        
        console.log(`   Transaction: ${tx.hash}`);
        const receipt = await tx.wait();
        console.log(`   ✅ Refund successful!`);
        
        return receipt;
        
    } catch (error) {
        console.log(`   ❌ Refund failed: ${error.message}`);
    }
}

// Main function
async function main() {
    try {
        console.log('💎 Fabstir Escrow Management Example\n');
        
        // 1. Setup
        console.log('1️⃣ Setting up connection...');
        const provider = new ethers.JsonRpcProvider(config.rpcUrl);
        const wallet = new ethers.Wallet(process.env.PRIVATE_KEY, provider);
        
        console.log(`   Account: ${wallet.address}`);
        console.log(`   Network: ${config.chainId === 8453 ? 'Base Mainnet' : 'Base Sepolia'}`);
        
        // 2. Initialize contracts
        console.log('\n2️⃣ Initializing contracts...');
        const escrow = new ethers.Contract(
            config.paymentEscrow,
            PAYMENT_ESCROW_ABI,
            wallet
        );
        
        const marketplace = new ethers.Contract(
            config.jobMarketplace,
            JOB_MARKETPLACE_ABI,
            wallet
        );
        
        const contracts = { escrow, marketplace };
        
        // 3. Check initial balances
        console.log('\n3️⃣ Checking escrow balances...');
        const manager = new EscrowManager(escrow, provider);
        await manager.checkAllBalances(wallet.address);
        
        // 4. Demonstrate deposit
        await depositExample(escrow, wallet);
        
        // 5. Demonstrate withdrawal
        const withdrawAmount = ethers.parseUnits('5', 6); // 5 USDC
        await withdrawExample(escrow, wallet, config.tokens.USDC, withdrawAmount);
        
        // 6. Check job payments (example job ID)
        const exampleJobId = 42;
        await jobPaymentFlow(contracts, exampleJobId);
        
        // 7. Multi-token example
        await multiTokenJobExample(contracts, wallet);
        
        // 8. Monitor escrow events
        console.log('\n📡 Setting up event monitoring...');
        
        escrow.on('FundsDeposited', (account, token, amount, event) => {
            if (account.toLowerCase() === wallet.address.toLowerCase()) {
                manager.formatBalance(amount, token).then(formatted => {
                    console.log(`\n🔔 Funds Deposited: ${formatted}`);
                });
            }
        });
        
        escrow.on('FundsLocked', (jobId, from, token, amount, event) => {
            manager.formatBalance(amount, token).then(formatted => {
                console.log(`\n🔔 Funds Locked for Job #${jobId}: ${formatted}`);
            });
        });
        
        escrow.on('FundsReleased', (jobId, to, token, amount, event) => {
            if (to.toLowerCase() === wallet.address.toLowerCase()) {
                manager.formatBalance(amount, token).then(formatted => {
                    console.log(`\n🔔 Payment Received for Job #${jobId}: ${formatted}`);
                });
            }
        });
        
        // 9. Check Host Earnings (NEW)
        console.log('\n💰 Host Earnings Accumulation (NEW):');
        const HOST_EARNINGS_ABI = [
            'function getBalance(address host, address token) view returns (uint256)',
            'function withdrawAll(address token)',
            'function withdraw(uint256 amount, address token)'
        ];
        
        const hostEarnings = new ethers.Contract(
            config.hostEarnings,
            HOST_EARNINGS_ABI,
            wallet
        );
        
        const earningsBalance = await hostEarnings.getBalance(wallet.address, config.tokens.USDC);
        console.log(`   Accumulated USDC earnings: ${ethers.formatUnits(earningsBalance, 6)} USDC`);
        console.log('   💡 Earnings accumulate from completed jobs (90% after 10% fee)');
        console.log('   💡 Withdraw anytime with hostEarnings.withdrawAll(USDC)');
        console.log('   💡 40-46% gas savings vs direct transfers!');
        
        // 10. Summary
        console.log('\n📊 Escrow Management Summary:');
        console.log('   ✅ Demonstrated deposits and withdrawals');
        console.log('   ✅ Showed multi-token support');
        console.log('   ✅ Explained job payment flow with earnings accumulation');
        console.log('   ✅ Highlighted 40-46% gas savings with new system');
        console.log('   ✅ Set up event monitoring');
        
        console.log('\n💡 Best Practices:');
        console.log('   • Always check balances before operations');
        console.log('   • Use appropriate gas limits for token operations');
        console.log('   • Monitor events for payment notifications');
        console.log('   • Let earnings accumulate before withdrawing (gas savings)');
        console.log('   • Withdraw during low gas periods for maximum efficiency');
        console.log('   • Track accumulated earnings regularly');
        console.log('   • Handle multiple payment tokens for flexibility');
        console.log('   • Implement proper error handling for failed transactions');
        
        // Keep listening for events
        console.log('\n👂 Listening for escrow events... (Press Ctrl+C to exit)');
        
        // Prevent script from exiting
        await new Promise(() => {});
        
    } catch (error) {
        console.error('\n❌ Error:', error.message);
        process.exit(1);
    }
}

// Execute if run directly
if (require.main === module) {
    main();
}

// Export for use in other modules
module.exports = { 
    main, 
    config,
    EscrowManager,
    depositExample,
    withdrawExample,
    jobPaymentFlow
};

/**
 * Expected Output:
 * 
 * 💎 Fabstir Escrow Management Example
 * 
 * 1️⃣ Setting up connection...
 *    Account: 0x742d35Cc6634C0532925a3b844Bc9e7595f6789
 *    Network: Base Mainnet
 * 
 * 2️⃣ Initializing contracts...
 * 
 * 3️⃣ Checking escrow balances...
 * 
 * 💰 Escrow Balances for 0x742d35Cc6634C0532925a3b844Bc9e7595f6789:
 *    • 500.00 USDC
 *    🔒 Total locked: 10 USDC
 * 
 * 📥 USDC Deposit Example
 *    Depositing 10 USDC...
 *    Approving USDC transfer...
 *    Transaction: 0xabc123...
 *    ✅ USDC deposit successful!
 *    New escrow balance: 510.00 USDC
 * 
 * 📤 Withdraw Example
 *    Withdrawing 5.0 USDC...
 *    Transaction: 0xdef456...
 *    ✅ Withdrawal successful!
 *    Remaining escrow balance: 505.00 USDC
 * 
 * 💼 Job Payment Flow for Job #42
 *    Job Details:
 *    • Poster: 0x1234...5678
 *    • Host: 0x742d35Cc6634C0532925a3b844Bc9e7595f6789
 *    • Payment: 15.00 USDC
 *    • Status: Claimed
 * 
 *    Locked Funds:
 *    • Amount: 15.00 USDC
 *    • From: 0x1234...5678
 *    • To: 0x742d35Cc6634C0532925a3b844Bc9e7595f6789
 *    • Released: No
 * 
 * 🌈 Multi-Token Payment Example
 *    Payment options for job:
 *    • 10.0 USDC
 *    • 25.0 USDC
 *    • 50.0 USDC
 * 
 *    Checking available balances...
 * 
 * 💰 Escrow Balances for 0x742d35Cc6634C0532925a3b844Bc9e7595f6789:
 *    • 505.00 USDC
 * 
 *    ✅ Can pay with 10.0 USDC
 * 
 * 📡 Setting up event monitoring...
 * 
 * 📊 Escrow Management Summary:
 *    ✅ Demonstrated deposits and withdrawals
 *    ✅ Showed multi-token support
 *    ✅ Explained job payment flow
 *    ✅ Set up event monitoring
 * 
 * 💡 Best Practices:
 *    • Always check balances before operations
 *    • Use appropriate gas limits for token operations
 *    • Monitor events for payment notifications
 *    • Handle multiple payment tokens for flexibility
 *    • Implement proper error handling for failed transactions
 * 
 * 👂 Listening for escrow events... (Press Ctrl+C to exit)
 * 
 * 🔔 Payment Received for Job #42: 15.00 USDC
 */