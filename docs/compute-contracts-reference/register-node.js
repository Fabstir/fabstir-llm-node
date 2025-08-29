/**
 * Example: Register Node
 * Purpose: Demonstrates how to register as a compute node provider using FAB token staking
 * Prerequisites: 
 *   - 1000 FAB tokens for staking (minimum requirement)
 *   - Small amount of ETH for gas fees (~0.01 ETH)
 *   - Node with GPU capabilities
 */

const { ethers } = require('ethers');
const readline = require('readline');
require('dotenv').config({ path: '../.env' });

// Contract ABIs
const NODE_REGISTRY_FAB_ABI = [
    'function registerNode(string memory metadata)',
    'function unregisterNode()',
    'function nodes(address) view returns (address operator, uint256 stakedAmount, bool active, string memory metadata)',
    'function MIN_STAKE() view returns (uint256)',
    'event NodeRegistered(address indexed operator, uint256 stakedAmount, string metadata)',
    'event NodeUnregistered(address indexed operator, uint256 returnedAmount)'
];

const FAB_TOKEN_ABI = [
    'function approve(address spender, uint256 amount) returns (bool)',
    'function balanceOf(address account) view returns (uint256)',
    'function decimals() view returns (uint8)',
    'function symbol() view returns (string)'
];

// Configuration
const config = {
    rpcUrl: process.env.RPC_URL || 'https://sepolia.base.org',
    chainId: parseInt(process.env.CHAIN_ID || '84532'), // Base Sepolia
    nodeRegistry: process.env.NODE_REGISTRY || '0x87516C13Ea2f99de598665e14cab64E191A0f8c4',
    fabToken: process.env.FAB_TOKEN || '0xC78949004B4EB6dEf2D66e49Cd81231472612D62',
    
    // Node metadata
    metadata: {
        gpu: 'rtx4090',
        models: ['gpt-4', 'llama2', 'stable-diffusion'],
        region: 'us-west',
        memory: '24GB',
        bandwidth: '1Gbps'
    },
    
    // Gas settings
    gasLimit: 300000,
    maxFeePerGas: ethers.parseUnits('50', 'gwei'),
    maxPriorityFeePerGas: ethers.parseUnits('2', 'gwei')
};

// Helper function to confirm action
async function confirm(question) {
    const rl = readline.createInterface({
        input: process.stdin,
        output: process.stdout
    });
    
    return new Promise(resolve => {
        rl.question(question + ' (y/n): ', answer => {
            rl.close();
            resolve(answer.toLowerCase() === 'y');
        });
    });
}

// Format metadata for registration
function formatMetadata(metadata) {
    const parts = [];
    if (metadata.gpu) parts.push(`gpu:${metadata.gpu}`);
    if (metadata.models) parts.push(`models:${metadata.models.join(',')}`);
    if (metadata.region) parts.push(`region:${metadata.region}`);
    if (metadata.memory) parts.push(`memory:${metadata.memory}`);
    if (metadata.bandwidth) parts.push(`bandwidth:${metadata.bandwidth}`);
    return parts.join(';');
}

async function main() {
    try {
        console.log('✅ Fabstir Node Registration\n');
        
        // 1. Setup connection
        console.log('1️⃣ Setting up connection...');
        const provider = new ethers.JsonRpcProvider(config.rpcUrl);
        const wallet = new ethers.Wallet(process.env.PRIVATE_KEY, provider);
        
        console.log(`   Node address: ${wallet.address}`);
        console.log(`   Network: Base Sepolia`);
        
        // 2. Initialize contracts
        console.log('\n2️⃣ Connecting to contracts...');
        const nodeRegistry = new ethers.Contract(
            config.nodeRegistry,
            NODE_REGISTRY_FAB_ABI,
            wallet
        );
        
        const fabToken = new ethers.Contract(
            config.fabToken,
            FAB_TOKEN_ABI,
            wallet
        );
        
        // 3. Check if already registered
        console.log('\n3️⃣ Checking registration status...');
        const nodeInfo = await nodeRegistry.nodes(wallet.address);
        
        if (nodeInfo.operator !== ethers.ZeroAddress) {
            console.log('   ⚠️  Node is already registered!');
            console.log(`   Staked: ${ethers.formatEther(nodeInfo.stakedAmount)} FAB`);
            console.log(`   Active: ${nodeInfo.active}`);
            console.log(`   Metadata: ${nodeInfo.metadata}`);
            
            const shouldUnregister = await confirm('\nDo you want to unregister and withdraw your stake?');
            if (shouldUnregister) {
                console.log('\n📤 Unregistering node...');
                const tx = await nodeRegistry.unregisterNode({
                    gasLimit: config.gasLimit,
                    maxFeePerGas: config.maxFeePerGas,
                    maxPriorityFeePerGas: config.maxPriorityFeePerGas
                });
                console.log(`   Transaction: ${tx.hash}`);
                await tx.wait();
                console.log('   ✅ Node unregistered and FAB returned!');
            }
            return;
        }
        
        // 4. Check FAB balance and requirements
        console.log('\n4️⃣ Checking FAB requirements...');
        const minStake = await nodeRegistry.MIN_STAKE();
        const fabBalance = await fabToken.balanceOf(wallet.address);
        const ethBalance = await provider.getBalance(wallet.address);
        
        console.log(`   Required stake: ${ethers.formatEther(minStake)} FAB`);
        console.log(`   Your FAB balance: ${ethers.formatEther(fabBalance)} FAB`);
        console.log(`   Your ETH balance: ${ethers.formatEther(ethBalance)} ETH (for gas)`);
        
        if (fabBalance < minStake) {
            throw new Error(`Insufficient FAB tokens. Need ${ethers.formatEther(minStake)} FAB, have ${ethers.formatEther(fabBalance)} FAB`);
        }
        
        if (ethBalance < ethers.parseEther('0.01')) {
            throw new Error('Insufficient ETH for gas. Need at least 0.01 ETH');
        }
        
        // 5. Display node configuration
        console.log('\n5️⃣ Node Configuration:');
        console.log(`   GPU: ${config.metadata.gpu}`);
        console.log(`   Models: ${config.metadata.models.join(', ')}`);
        console.log(`   Region: ${config.metadata.region}`);
        console.log(`   Memory: ${config.metadata.memory}`);
        console.log(`   Bandwidth: ${config.metadata.bandwidth}`);
        
        const metadataString = formatMetadata(config.metadata);
        console.log(`   Metadata string: "${metadataString}"`);
        
        // 6. Confirm registration
        console.log('\n⚠️  Registration Summary:');
        console.log(`   Stake amount: ${ethers.formatEther(minStake)} FAB (~$1,000)`);
        console.log(`   Node address: ${wallet.address}`);
        console.log(`   Metadata: ${metadataString}`);
        
        const shouldProceed = await confirm('\nDo you want to proceed with registration?');
        if (!shouldProceed) {
            console.log('❌ Registration cancelled');
            return;
        }
        
        // 7. Approve FAB tokens
        console.log('\n6️⃣ Approving FAB tokens...');
        const currentAllowance = await fabToken.allowance(wallet.address, config.nodeRegistry);
        
        if (currentAllowance < minStake) {
            const approveTx = await fabToken.approve(
                config.nodeRegistry,
                minStake,
                {
                    gasLimit: 100000,
                    maxFeePerGas: config.maxFeePerGas,
                    maxPriorityFeePerGas: config.maxPriorityFeePerGas
                }
            );
            console.log(`   Approval transaction: ${approveTx.hash}`);
            console.log('   Waiting for confirmation...');
            await approveTx.wait();
            console.log('   ✅ FAB tokens approved');
        } else {
            console.log('   ✅ FAB tokens already approved');
        }
        
        // 8. Register node
        console.log('\n7️⃣ Registering node...');
        const registerTx = await nodeRegistry.registerNode(
            metadataString,
            {
                gasLimit: config.gasLimit,
                maxFeePerGas: config.maxFeePerGas,
                maxPriorityFeePerGas: config.maxPriorityFeePerGas
            }
        );
        
        console.log(`   Transaction hash: ${registerTx.hash}`);
        console.log('   Waiting for confirmation...');
        
        // 9. Wait for confirmation
        const receipt = await registerTx.wait();
        console.log(`   ✅ Transaction confirmed in block ${receipt.blockNumber}`);
        
        // 10. Parse registration event
        const event = receipt.logs
            .map(log => {
                try {
                    return nodeRegistry.interface.parseLog(log);
                } catch {
                    return null;
                }
            })
            .find(e => e && e.name === 'NodeRegistered');
        
        if (event) {
            console.log(`\n✅ Node Registered Successfully!`);
            console.log(`   Operator: ${event.args[0]}`);
            console.log(`   Staked: ${ethers.formatEther(event.args[1])} FAB`);
            console.log(`   Metadata: ${event.args[2]}`);
        }
        
        // 11. Verify registration
        console.log('\n8️⃣ Verifying registration...');
        const newNodeInfo = await nodeRegistry.nodes(wallet.address);
        console.log(`   Status: ${newNodeInfo.active ? '✅ Active' : '❌ Inactive'}`);
        console.log(`   Staked: ${ethers.formatEther(newNodeInfo.stakedAmount)} FAB`);
        
        // 12. Next steps
        console.log('\n📋 Next Steps:');
        console.log('   1. Ensure your node is running and accessible');
        console.log('   2. Monitor for job assignments');
        console.log('   3. Complete jobs to earn USDC payments');
        console.log('   4. Maintain good reputation for more jobs');
        
        console.log('\n🎉 Congratulations! Your node is registered and ready to earn!');
        console.log('💡 You can now claim and complete jobs for USDC payments.');
        
        // Show BaseScan link
        console.log(`\n🔗 View on BaseScan:`);
        console.log(`   https://sepolia.basescan.org/tx/${registerTx.hash}`);
        
    } catch (error) {
        console.error('\n❌ Error:', error.message);
        
        // Helpful error messages
        if (error.message.includes('Insufficient FAB')) {
            console.error('💡 You need 1000 FAB tokens to register. Contact the team or use the faucet.');
        } else if (error.message.includes('Insufficient ETH')) {
            console.error('💡 You need ETH for gas fees. Use the Base Sepolia faucet.');
        } else if (error.message.includes('Already registered')) {
            console.error('💡 This address is already registered as a node.');
        }
        
        process.exit(1);
    }
}

// Execute if run directly
if (require.main === module) {
    main();
}

// Export for use in other modules
module.exports = { main, config };

/**
 * Usage Examples:
 * 
 * # Register with default configuration
 * node register-node-fab.js
 * 
 * # Set custom environment variables
 * NODE_REGISTRY=0x... FAB_TOKEN=0x... node register-node-fab.js
 * 
 * Expected Output:
 * 
 * ✅ Fabstir Node Registration (FAB Staking)
 * 
 * 1️⃣ Setting up connection...
 *    Node address: 0x742d35Cc6634C0532925a3b844Bc9e7595f6789
 *    Network: Base Sepolia
 * 
 * 2️⃣ Connecting to contracts...
 * 
 * 3️⃣ Checking registration status...
 *    Not registered
 * 
 * 4️⃣ Checking FAB requirements...
 *    Required stake: 1000.0 FAB
 *    Your FAB balance: 1500.0 FAB
 *    Your ETH balance: 0.05 ETH
 * 
 * 5️⃣ Node Configuration:
 *    GPU: rtx4090
 *    Models: gpt-4, llama2, stable-diffusion
 *    Region: us-west
 * 
 * 6️⃣ Approving FAB tokens...
 *    ✅ FAB tokens approved
 * 
 * 7️⃣ Registering node...
 *    ✅ Transaction confirmed
 * 
 * ✅ Node Registered Successfully!
 *    Staked: 1000.0 FAB
 * 
 * 🎉 Your node is registered and ready to earn!
 */