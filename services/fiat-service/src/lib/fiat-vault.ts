// FC1.2 — the vault SIGNER (Decision 1): the backend signs
// createSessionJobForModelWithToken from the vault key, making the vault the
// on-chain depositor. Server-only module; the two private keys live in server
// env (FIAT_VAULT_PRIVATE_KEY funds, FIAT_BACKEND_AUTH_PRIVATE_KEY authorises
// — deliberately separate, Decision 7) and are never logged or returned.
//
// NOT src/lib/vault.ts — that is the CS1 envelope-encryption module.
import {
  Contract,
  Interface,
  JsonRpcProvider,
  SigningKey,
  Transaction,
  Wallet,
  getAddress,
  keccak256,
  toUtf8Bytes,
} from 'ethers';
import {
  ESCROW_INTERFACE,
  jobMarketplaceAddress,
  sessionShapeFor,
} from './escrow';
import { rpcUrl, usdcTokenAddress } from './balance';
import { bigintEnv, type SessionKind } from './gatekeeper';
import { DEFAULT_CHAIN_ID, OutboxAppendFailed, type SendResult, type VaultOutbox, type VaultRpc, type VaultSigner } from './vault-outbox';

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

/** Reads the price the REGISTRY holds for this (host, model, token) triple.
 *  Pricing is per model per host: the LTX video model is 904, the chat models
 *  on the chat host are 10000. A single service-wide constant made every
 *  non-LTX model unopenable (the contract rejects an underpriced create with
 *  "Low price"), so the chain is the only honest source. */
export type ModelPriceReader = (host: string, modelId: string, token: string) => Promise<bigint>;

/** The create's request bounds (D20 item 1): one 170 s deadline from arrival
 *  covers the lock wait, the allowance and the confirmation; the outbox append
 *  must START by 150 s. */
export const CREATE_APPEND_BY_MS = 150_000;
export const CREATE_DEADLINE_MS = 170_000;
/** An approve's confirmation is awaited under the create's lock, bounded. */
export const APPROVE_WAIT_MS = 120_000;

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
  /** The outbox entry's `link`: the hold this create is for. */
  link?: string;
  /** Checked FIRST under the lock: the hold must still be `held`. */
  assertHeld?: () => boolean;
  /** Absolute epoch-ms bounds (default: now + 150 s / now + 170 s). */
  appendByMs?: number;
  deadlineAtMs?: number;
  /** Called with the tx hash once the create's outbox entry is appended and its
   *  broadcast returned or threw (the lock already released). From here on a
   *  failure must NOT release the hold: the transaction may mine. */
  onSubmitted?: (txHash: string) => Promise<void>;
}

export interface VaultChain {
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
 *  A deposit above it is refused as a loud config error, so a kind whose cap
 *  exceeds the float can never open — which is why `assertBootInvariants`
 *  (fiat-session-service) refuses to boot when an ENABLED kind's cap exceeds
 *  this figure (FT1 D5): the deploy raises FIAT_VAULT_ALLOWANCE_FLOAT_MICRO
 *  deliberately, in the same edit as FIAT_TRAINING_MODEL_IDS. Exported for
 *  that check. */
export function allowanceFloatMicro(): bigint {
  return bigintEnv('FIAT_VAULT_ALLOWANCE_FLOAT_MICRO', 2_000_000n);
}

/** An ERC-20 allowance revert on the create's estimate — the signal to top up + retry. */
export function isAllowanceError(e: unknown): boolean {
  const msg = e instanceof Error ? e.message : String(e);
  return /exceeds allowance|insufficient allowance|allowance/i.test(msg);
}

const ERC20_INTERFACE = new Interface([
  'function allowance(address owner, address spender) view returns (uint256)',
  'function approve(address spender, uint256 amount) returns (bool)',
]);

export interface VaultChainDeps {
  /** The ONE outbox (shared deps): every vault send goes through it. */
  outbox: VaultOutbox;
  vaultAddress: string;
  marketplaceAddress: string;
  usdcAddress: string;
  /** USDC allowance vault → marketplace (a read). */
  readAllowance(): Promise<bigint>;
  modelPrice: ModelPriceReader;
  now?: () => number;
}

/** The deposit of an outbox create entry, from its own signed bytes. */
export function createEntryDeposit(raw: string): bigint {
  const tx = Transaction.from(raw);
  const args = ESCROW_INTERFACE.decodeFunctionData('createSessionJobForModelWithToken', tx.data);
  return BigInt(args[3]);
}

export function makeVaultChain(deps: VaultChainDeps): VaultChain {
  const { outbox, vaultAddress, marketplaceAddress, usdcAddress, readAllowance, modelPrice } = deps;
  const now = deps.now ?? Date.now;
  const createdTopic = ESCROW_INTERFACE.getEvent('SessionJobCreatedForModel')!.topicHash;

  /** The allowance minus the deposits of this service's unconfirmed creates:
   *  a create still in the mempool will consume allowance the RPC still shows. */
  function unconfirmedCreateDeposits(): bigint {
    return outbox.unresolved('create').reduce((sum, e) => sum + createEntryDeposit(e.raw), 0n);
  }

  /** A bounded approve, INSIDE the caller's lock (sendLocked, never vaultSend:
   *  the lock is non-reentrant). The create's gas estimate needs it mined. */
  async function approveLocked(appendBy: number, deadline: number): Promise<void> {
    const sent = await outbox.sendLocked(
      'approve',
      { to: usdcAddress, data: ERC20_INTERFACE.encodeFunctionData('approve', [marketplaceAddress, allowanceFloatMicro()]) },
      'allowance',
      { appendBy, deadline }
    );
    if (sent.broadcastError) {
      throw new Error(`approve broadcast failed (${String(sent.broadcastError)}) — the create is aborted before any send`);
    }
    const receipt = await outbox.waitForReceipt(sent.hash, Math.min(now() + APPROVE_WAIT_MS, deadline));
    if (!receipt) throw new Error(`approve ${sent.hash} unconfirmed after ${APPROVE_WAIT_MS / 1000} s — the create is aborted before any send`);
    if (receipt.status === 0) throw new Error(`approve ${sent.hash} reverted — the create is aborted before any send`);
  }

  return {
    async createSession(params: CreateSessionParams) {
      const start = now();
      const appendBy = params.appendByMs ?? start + CREATE_APPEND_BY_MS;
      const deadline = params.deadlineAtMs ?? start + CREATE_DEADLINE_MS;
      // Registry price unless the caller pinned one. A failed read or a zero
      // price ABORTS: guessing here would either revert on chain ("Low price")
      // or, worse, open a session that bills the host at the wrong rate.
      let pricePerToken = params.pricePerToken;
      if (pricePerToken === undefined) {
        pricePerToken = await outbox.timed(modelPrice(params.host, params.modelId, usdcAddress), 'price read');
        if (pricePerToken <= 0n) {
          throw new Error(
            `no registered price for model ${params.modelId} on host ${params.host} — the host must advertise this model before a vault session can pay for it`
          );
        }
      }
      const float = allowanceFloatMicro();
      if (params.depositMicro > float) {
        throw new Error(
          `deposit ${params.depositMicro} exceeds the allowance float ${float} — raise FIAT_VAULT_ALLOWANCE_FLOAT_MICRO deliberately, never implicitly`
        );
      }
      // The shape is chosen by KIND, server-side: a training run needs the
      // 14400 / 1000 / 3600 wallet-path shape or it fails the node's A.3 gate
      // after escrow (see escrow.ts).
      const shape = sessionShapeFor(params.kind);
      const data = ESCROW_INTERFACE.encodeFunctionData('createSessionJobForModelWithToken', [
        params.host,
        params.modelId,
        usdcAddress,
        params.depositMicro,
        pricePerToken,
        shape.maxDuration,
        shape.proofInterval,
        shape.proofTimeoutWindow,
      ]);
      const request = { to: marketplaceAddress, data };
      const link = params.link ?? 'unlinked';

      // ONE lock across every step up to the broadcast, released when the
      // broadcast returns or throws (never through the confirmation wait).
      let sent: SendResult;
      try {
        sent = await outbox.withVaultLock(
          async () => {
            if (params.assertHeld && !params.assertHeld()) {
              throw new Error('the hold is no longer held — nothing was sent');
            }
            const allowance = await outbox.timed(readAllowance(), 'allowance read', appendBy);
            if (allowance - unconfirmedCreateDeposits() < params.depositMicro) {
              await approveLocked(appendBy, deadline);
            }
            try {
              return await outbox.sendLocked('create', request, link, { appendBy, deadline });
            } catch (e) {
              // A stale or exact-boundary allowance read can make the check above
              // skip the top-up, so the create's ESTIMATE reverts "exceeds
              // allowance" — before any append, so nothing was sent. Approve the
              // float and populate ONCE more (safe: nothing can double-open).
              if (!isAllowanceError(e)) throw e;
              await approveLocked(appendBy, deadline);
              return outbox.sendLocked('create', request, link, { appendBy, deadline });
            }
          },
          { acquireBy: appendBy }
        );
      } catch (e) {
        // D20 — the create's outbox append threw (write or fsync): AMBIGUOUS,
        // the line may be on disk and a restart rebroadcasts it. Treated as a
        // throw AFTER the append: the hold stays held, the key pending.
        if (e instanceof OutboxAppendFailed && e.kind === 'create') {
          if (params.onSubmitted) await params.onSubmitted(e.hash);
          throw new Error(`${e.message} — the line may be on disk; the restart's replay decides (R5)`);
        }
        throw e;
      }

      // Appended (and broadcast, or not): from here a failure keeps the hold.
      if (params.onSubmitted) await params.onSubmitted(sent.hash);
      if (sent.broadcastError) {
        throw new Error(`create ${sent.hash} broadcast failed after its outbox append: ${String(sent.broadcastError)}`);
      }
      const receipt = await outbox.waitForReceipt(sent.hash, deadline);
      if (!receipt) {
        throw new Error(`create ${sent.hash} unconfirmed at the ${CREATE_DEADLINE_MS / 1000} s deadline — R5 resolves it`);
      }
      if (receipt.status === 0) throw new Error(`create ${sent.hash} reverted on chain`);
      const log = receipt.logs.find((l) => l.topics[0] === createdTopic);
      if (!log) {
        throw new Error(`no SessionJobCreatedForModel event in receipt ${sent.hash} — event inventory mismatch, stop`);
      }
      const jobId = BigInt(log.topics[1]!);
      const depositor = getAddress(`0x${log.topics[2]!.slice(26)}`);
      if (depositor.toLowerCase() !== vaultAddress.toLowerCase()) {
        throw new Error(`session ${jobId} depositor ${depositor} is not the vault ${vaultAddress} — mis-wired signer`);
      }
      return { jobId, depositor, txHash: sent.hash };
    },

    modelPrice(host: string, modelId: string): Promise<bigint> {
      return modelPrice(host, modelId, usdcAddress);
    },
  };
}

/** Base's GasPriceOracle predeploy (OP stack). */
export const GAS_PRICE_ORACLE = '0x420000000000000000000000000000000000000F';

/** The production VaultRpc over one ethers provider (`staticNetwork`). */
export function makeEthersVaultRpc(provider: JsonRpcProvider, vaultAddress: string): VaultRpc {
  const oracle = new Contract(GAS_PRICE_ORACLE, ['function getL1Fee(bytes) view returns (uint256)'], provider);
  return {
    getNonce: (tag) => provider.getTransactionCount(vaultAddress, tag),
    getBalance: () => provider.getBalance(vaultAddress),
    estimateGas: (tx) => provider.estimateGas(tx),
    async getFeeData() {
      const fees = await provider.getFeeData();
      if (fees.maxFeePerGas === null || fees.maxPriorityFeePerGas === null) {
        throw new Error('the RPC returned no EIP-1559 fee data');
      }
      return { maxFeePerGas: fees.maxFeePerGas, maxPriorityFeePerGas: fees.maxPriorityFeePerGas };
    },
    l1Fee: async (unsigned) => BigInt(await oracle.getFunction('getL1Fee')(unsigned)),
    async broadcast(raw) {
      await provider.broadcastTransaction(raw);
    },
    async receipt(hash) {
      const r = await provider.getTransactionReceipt(hash);
      if (!r) return null;
      return { status: r.status, blockNumber: r.blockNumber, logs: r.logs.map((l) => ({ topics: l.topics, data: l.data })) };
    },
    blockNumber: () => provider.getBlockNumber(),
  };
}

export interface ProductionVault {
  rpc: VaultRpc;
  signer: VaultSigner;
  chainId: bigint;
  vaultAddress: string;
  marketplaceAddress: string;
  usdcAddress: string;
  readAllowance(): Promise<bigint>;
  modelPrice: ModelPriceReader;
}

/** The vault's chain parts from server env. Throws the "not configured"
 *  error when the vault key is unset. */
export function productionVault(): ProductionVault {
  const key = process.env.FIAT_VAULT_PRIVATE_KEY;
  if (!key) {
    throw new Error('FIAT_VAULT_PRIVATE_KEY is not set — the fiat backend is not configured');
  }
  const chainId = BigInt(process.env.FIAT_CHAIN_ID ?? DEFAULT_CHAIN_ID.toString());
  // staticNetwork: one chain id, never re-detected per call (and never a
  // detection request hanging under the vault lock).
  const provider = new JsonRpcProvider(rpcUrl(), Number(chainId), { staticNetwork: true });
  const wallet = new Wallet(key, provider);
  const usdcAddress = usdcTokenAddress();
  const marketplaceAddress = jobMarketplaceAddress();
  const usdc = new Contract(usdcAddress, ERC20_INTERFACE, provider);
  return {
    rpc: makeEthersVaultRpc(provider, wallet.address),
    signer: { address: wallet.address, signTransaction: (tx) => wallet.signTransaction(tx) },
    chainId,
    vaultAddress: wallet.address,
    marketplaceAddress,
    usdcAddress,
    readAllowance: async () => BigInt(await usdc.getFunction('allowance')(wallet.address, marketplaceAddress)),
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
