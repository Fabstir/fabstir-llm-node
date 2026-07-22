// Read-only balance lookup: the user's "credits" ARE their on-chain USDC.
// No signer, no server — a public RPC read of balanceOf(smartAccount).
// Contract addresses come from env (never inlined — stale addresses are
// silent bugs); copy .env.example to .env.local for local dev.
import { Contract, JsonRpcProvider } from 'ethers';

const ERC20_BALANCE_ABI = ['function balanceOf(address) view returns (uint256)'];

export function usdcTokenAddress(): string {
  const address = process.env.NEXT_PUBLIC_USDC_TOKEN;
  if (!address) {
    throw new Error('NEXT_PUBLIC_USDC_TOKEN is not set — copy .env.example to .env.local');
  }
  return address;
}

export function rpcUrl(): string {
  return process.env.NEXT_PUBLIC_BASE_SEPOLIA_RPC_URL ?? 'https://sepolia.base.org';
}

/** USDC balance of `address` in micro-USDC (6-decimal integer, straight off the chain). */
export async function fetchUsdcMicroBalance(address: string): Promise<bigint> {
  const provider = new JsonRpcProvider(rpcUrl());
  const usdc = new Contract(usdcTokenAddress(), ERC20_BALANCE_ABI, provider);
  return usdc.balanceOf(address);
}
