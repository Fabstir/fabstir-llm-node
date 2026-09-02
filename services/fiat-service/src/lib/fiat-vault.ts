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
  jobMarketplaceAddress,
  sessionShapeFor,
} from './escrow';
import { rpcUrl, usdcTokenAddress } from './balance';
import { bigintEnv, type SessionKind } from './gatekeeper';

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

/** Reads the price the REGISTRY holds for this (host, model, token) triple.
 *  Pricing is per model per host: the LTX video model is 904, the chat models
 *  on the chat host are 10000. A single service-wide constant made every
 *  non-LTX model unopenable (the contract rejects an underpriced create with
 *  "Low price"), so the chain is the only honest source. */
export type ModelPriceReader = (host: string, modelId: string, token: string) => Promise<bigint>;

export interface CreateSessionParams {
  host: string;
  modelId: string;
  depositMicro: bigint;
  /** Selects the SERVICE-OWNED on-chain shape (escrow.ts `sessionShapeFor`).
   *  Absent = standard (chat/render). Never three raw numbers from a client. */
  kind?: SessionKind;
  /** Omit to use the registry price for (host, modelId, token) — the norm.
   *  Supply it only to pin a price deliberately; it is never taken from a
   *  client request (a price the caller chooses is a price an attacker
   *  chooses). */
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
  /** The registry price for (host, model, USDC). 0n = the host has not
   *  advertised this model, so no session for it can EVER open (a policy
   *  refusal, MODEL_NOT_PRICED); a throw = the chain could not be read just
   *  now (chain_error, worth a retry). The service asks this before it
   *  spends anything, and pins the answer into the create. */
  modelPrice(host: string, modelId: string): Promise<bigint>;
}

/** The standing approve() float: EXACTLY the configured value, never raised
 *  implicitly (Decision 3b: a small float, topped up as spend happens).
 *  `ensureAllowance` refuses a deposit above it as a loud config error, so a
 *  kind whose cap exceeds the float can never open — which is why
 *  `assertBootInvariants` (fiat-session-service) refuses to boot when an
 *  ENABLED kind's cap exceeds this figure (FT1 D5): the deploy raises
 *  FIAT_VAULT_ALLOWANCE_FLOAT_MICRO deliberately, in the same edit as
 *  FIAT_TRAINING_MODEL_IDS. Exported for that check. */
export function allowanceFloatMicro(): bigint {
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
  modelPrice: ModelPriceReader;
}

/** Real deps come from server env; tests inject all five. */
export function makeVaultChain(deps?: VaultChainDeps): VaultChain {
  const resolved = deps ?? realDeps();
  const { vaultAddress, marketplaceAddress, usdcAddress, usdc, marketplace, modelPrice } = resolved;
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
      // Registry price unless the caller pinned one. A failed read or a zero
      // price ABORTS: guessing here would either revert on chain ("Low price")
      // or, worse, open a session that bills the host at the wrong rate.
      let pricePerToken = params.pricePerToken;
      if (pricePerToken === undefined) {
        pricePerToken = await modelPrice(params.host, params.modelId, usdcAddress);
        if (pricePerToken <= 0n) {
          throw new Error(
            `no registered price for model ${params.modelId} on host ${params.host} — the host must advertise this model before a vault session can pay for it`
          );
        }
      }
      // The shape is chosen by KIND, server-side: a training run needs the
      // 14400 / 1000 / 3600 wallet-path shape or it fails the node's A.3 gate
      // after escrow (see escrow.ts).
      const shape = sessionShapeFor(params.kind);
      const send = () =>
        marketplace.createSessionJobForModelWithToken(
          params.host,
          params.modelId,
          usdcAddress,
          params.depositMicro,
          pricePerToken,
          shape.maxDuration,
          shape.proofInterval,
          shape.proofTimeoutWindow
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

    modelPrice(host: string, modelId: string): Promise<bigint> {
      return modelPrice(host, modelId, usdcAddress);
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
    modelPrice: makeChainModelPriceReader(provider),
  };
}

/** The registry read behind the price: NodeRegistry.getModelPricing(host,
 *  modelId, token), the same source the SDK quotes from, so a vault-paid
 *  session and a wallet-paid one are billed at the identical rate. */
export function makeChainModelPriceReader(provider: JsonRpcProvider): ModelPriceReader {
  const address = process.env.NEXT_PUBLIC_CONTRACT_NODE_REGISTRY;
  if (!address) {
    throw new Error(
      'NEXT_PUBLIC_CONTRACT_NODE_REGISTRY is not set — the fiat backend cannot price a session without the host directory'
    );
  }
  const registry = new Contract(
    address,
    ['function getModelPricing(address host, bytes32 modelId, address token) view returns (uint256)'],
    provider
  );
  const read = registry.getFunction('getModelPricing');
  return async (host, modelId, token) => BigInt(await read(host, modelId, token));
}
