// FC1.2 — the vault SIGNER (Decision 1): the backend signs
// createSessionJobForModelWithToken from the vault key, making the vault the
// on-chain depositor. Server-only module; the two private keys live in server
// env (FIAT_VAULT_PRIVATE_KEY funds, FIAT_BACKEND_AUTH_PRIVATE_KEY authorises
// — deliberately separate, Decision 7) and are never logged or returned.
//
// NOT src/lib/vault.ts — that is the CS1 envelope-encryption module.
import { Contract, JsonRpcProvider, SigningKey, Wallet, getAddress, keccak256, toUtf8Bytes } from 'ethers';
import {
  ESCROW_INTERFACE,
  SESSION_MAX_DURATION,
  SESSION_PROOF_INTERVAL,
  SESSION_PROOF_TIMEOUT_WINDOW,
  escrowPricePerToken,
  jobMarketplaceAddress,
} from './escrow';
import { rpcUrl, usdcTokenAddress } from './balance';
import { bigintEnv } from './gatekeeper';

export const SESSION_AUTH_SCHEME = 'fc1-session-auth-v1';

/** The FC1.6 authorisation: proves to the node that the backend blessed
 *  `clientAddress` to use vault-paid session `sessionId`. */
export interface SessionAuthorisation {
  scheme: string;
  /** 65-byte compact ECDSA signature (r||s||v, v in {27,28}) over sessionAuthDigest. */
  signature: string;
  clientAddress: string;
}

/**
 * The signed message, locked cross-repo (the node's Rust verifier hashes the
 * SAME string): keccak256(utf8("FC1-SESSION-AUTH:<sessionId decimal>:<client
 * address lowercase 0x-hex>")). No EIP-191 prefix — the node verifies the raw
 * digest with its existing recover_client_address.
 */
export function sessionAuthDigest(sessionId: bigint, clientAddress: string): string {
  return keccak256(toUtf8Bytes(`FC1-SESSION-AUTH:${sessionId}:${clientAddress.toLowerCase()}`));
}

function requireAuthKey(): string {
  const key = process.env.FIAT_BACKEND_AUTH_PRIVATE_KEY;
  if (!key) {
    throw new Error('FIAT_BACKEND_AUTH_PRIVATE_KEY is not set — the fiat backend is not configured');
  }
  return key;
}

export function signSessionAuthorisation(sessionId: bigint, clientAddress: string): SessionAuthorisation {
  const client = clientAddress.toLowerCase();
  const signature = new SigningKey(requireAuthKey()).sign(sessionAuthDigest(sessionId, client));
  return { scheme: SESSION_AUTH_SCHEME, signature: signature.serialized, clientAddress: client };
}

/** The address the node must be configured with to verify authorisations. */
export function backendAuthAddress(): string {
  return new Wallet(requireAuthKey()).address;
}

// Minimal contract surfaces so unit tests can inject fakes (no chain in CI).
export interface Erc20Like {
  allowance(owner: string, spender: string): Promise<bigint>;
  approve(spender: string, amount: bigint): Promise<{ wait(): Promise<unknown> }>;
}
export interface MarketplaceLike {
  createSessionJobForModelWithToken(
    host: string,
    modelId: string,
    token: string,
    deposit: bigint,
    pricePerToken: bigint,
    maxDuration: bigint,
    proofInterval: bigint,
    proofTimeoutWindow: bigint
  ): Promise<{ hash: string; wait(): Promise<{ logs: Array<{ topics: readonly string[]; data: string }> } | null> }>;
}

export interface CreateSessionParams {
  host: string;
  modelId: string;
  depositMicro: bigint;
  /** Defaults to the proven env price (NEXT_PUBLIC_SESSION_PRICE_PER_TOKEN). */
  pricePerToken?: bigint;
  /** Called with the tx hash the instant the create is SUBMITTED (before the
   *  confirmation wait), so the caller can durably record a crash-recoverable
   *  orphan marker (M2 / R5 reconciliation). */
  onSubmitted?: (txHash: string) => Promise<void>;
}

export interface VaultChain {
  /** Keep the marketplace allowance a SMALL float (Decision 3b): tops up to
   *  exactly the configured float when it cannot cover `depositMicro`. */
  ensureAllowance(depositMicro: bigint): Promise<void>;
  createSession(params: CreateSessionParams): Promise<{ jobId: bigint; depositor: string; txHash: string }>;
}

function allowanceFloatMicro(): bigint {
  // TODO(Jules): set with the cap numbers.
  return bigintEnv('FIAT_VAULT_ALLOWANCE_FLOAT_MICRO', 2_000_000n);
}

/** An ERC-20 allowance revert on the create — the signal to top up + retry. */
function isAllowanceError(e: unknown): boolean {
  const msg = e instanceof Error ? e.message : String(e);
  return /exceeds allowance|insufficient allowance|allowance/i.test(msg);
}

export interface VaultChainDeps {
  vaultAddress: string;
  marketplaceAddress: string;
  usdcAddress: string;
  usdc: Erc20Like;
  marketplace: MarketplaceLike;
}

/** Real deps come from server env; tests inject all five. */
export function makeVaultChain(deps?: VaultChainDeps): VaultChain {
  const resolved = deps ?? realDeps();
  const { vaultAddress, marketplaceAddress, usdcAddress, usdc, marketplace } = resolved;
  const createdTopic = ESCROW_INTERFACE.getEvent('SessionJobCreatedForModel')!.topicHash;

  return {
    async ensureAllowance(depositMicro: bigint): Promise<void> {
      const float = allowanceFloatMicro();
      if (depositMicro > float) {
        throw new Error(
          `deposit ${depositMicro} exceeds the allowance float ${float} — raise FIAT_VAULT_ALLOWANCE_FLOAT_MICRO deliberately, never implicitly`
        );
      }
      const current = await usdc.allowance(vaultAddress, marketplaceAddress);
      if (current < depositMicro) {
        const tx = await usdc.approve(marketplaceAddress, float);
        await tx.wait();
      }
    },

    async createSession(params: CreateSessionParams) {
      const send = () =>
        marketplace.createSessionJobForModelWithToken(
          params.host,
          params.modelId,
          usdcAddress,
          params.depositMicro,
          params.pricePerToken ?? escrowPricePerToken(),
          SESSION_MAX_DURATION,
          SESSION_PROOF_INTERVAL,
          SESSION_PROOF_TIMEOUT_WINDOW
        );
      let tx: Awaited<ReturnType<MarketplaceLike['createSessionJobForModelWithToken']>>;
      try {
        tx = await send();
      } catch (e) {
        // A stale or exact-boundary allowance read can make ensureAllowance skip
        // the top-up, so the create reverts "exceeds allowance" — at gas
        // estimation, BEFORE any session exists. Force the float and retry ONCE
        // (safe: nothing was created, so this can't double-open).
        if (!isAllowanceError(e)) throw e;
        const approveTx = await usdc.approve(marketplaceAddress, allowanceFloatMicro());
        await approveTx.wait();
        tx = await send();
      }
      // Submitted — record the pending create BEFORE the confirmation wait, so a
      // crash during the wait leaves a deterministically recoverable orphan (M2).
      if (params.onSubmitted) await params.onSubmitted(tx.hash);
      const receipt = await tx.wait();
      const log = receipt?.logs.find((l) => l.topics[0] === createdTopic);
      if (!log) {
        throw new Error(`no SessionJobCreatedForModel event in receipt ${tx.hash} — event inventory mismatch, stop`);
      }
      const jobId = BigInt(log.topics[1]!);
      const depositor = getAddress(`0x${log.topics[2]!.slice(26)}`);
      if (depositor.toLowerCase() !== vaultAddress.toLowerCase()) {
        throw new Error(`session ${jobId} depositor ${depositor} is not the vault ${vaultAddress} — mis-wired signer`);
      }
      return { jobId, depositor, txHash: tx.hash };
    },
  };
}

function realDeps(): VaultChainDeps {
  const key = process.env.FIAT_VAULT_PRIVATE_KEY;
  if (!key) {
    throw new Error('FIAT_VAULT_PRIVATE_KEY is not set — the fiat backend is not configured');
  }
  const provider = new JsonRpcProvider(rpcUrl());
  const wallet = new Wallet(key, provider);
  const usdcAddress = usdcTokenAddress();
  const marketplaceAddress = jobMarketplaceAddress();
  return {
    vaultAddress: wallet.address,
    marketplaceAddress,
    usdcAddress,
    usdc: new Contract(
      usdcAddress,
      [
        'function allowance(address owner, address spender) view returns (uint256)',
        'function approve(address spender, uint256 amount) returns (bool)',
      ],
      wallet
    ) as unknown as Erc20Like,
    marketplace: new Contract(marketplaceAddress, ESCROW_INTERFACE, wallet) as unknown as MarketplaceLike,
  };
}
