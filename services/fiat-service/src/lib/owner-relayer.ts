// OP1.3a: the owner relayer. "Add a passkey" (and "Remove") on a customer's
// Coinbase Smart Wallet v1.1 account, by one ERC-4337 v0.6 user operation that
// WE pay for: the relayer is its own bundler (EntryPoint.depositTo, then
// handleOps with itself as beneficiary). No outside bundler, no paymaster.
//
// Plan: docs/development/IMPLEMENTATION-OWN-PASSKEY-ACCOUNTS.md D14, T6, T7;
// check findings temp/own-passkey/V04-CHECK-FINDINGS.md C1 to C5, H2, M1 to M3.
//
// What protects the gas wallet (T6):
//  - The route lets in only the holder of the account's card credential whose
//    account has a credited purchase (the route checks; this module trusts its
//    caller's userId).
//  - The relayer builds the WHOLE operation itself (prepare). The client sends
//    an action, never callData, gas or fees; unknown body fields are refused.
//    submit takes only the signature, so the signed hash is over OUR op.
//  - Exactly two calls: execute(sender, 0, addOwnerPublicKey(x, y)) and
//    execute(sender, 0, removeOwnerAtIndex(index, owner)).
//  - Per-account and global daily caps, journaled (JSONL) under the data dir.
//  - One operation per account at a time; one relayer transaction at a time.
//  - A low-balance alarm line in the log.
// The relayer's key is never logged or returned; only its address is.
//
// This is the second (and last) file allowed to import viem
// (test/viem-confinement.test.ts).
import {
  concat,
  createPublicClient,
  createWalletClient,
  decodeErrorResult,
  decodeEventLog,
  decodeFunctionResult,
  encodeAbiParameters,
  encodeFunctionData,
  getAddress,
  getContractAddress,
  http,
  keccak256,
  pad,
  parseAbi,
  toHex,
  type Address,
  type Hex,
  type PublicClient,
  type StateOverride,
  type Transport,
  type WalletClient,
} from 'viem';
import { privateKeyToAccount, type PrivateKeyAccount } from 'viem/accounts';
import { entryPoint06Abi, entryPoint06Address, getUserOperationHash } from 'viem/account-abstraction';
import { base, baseSepolia } from 'viem/chains';
import { join } from 'node:path';
import { rpcUrl } from './balance';
import { vaultChainId } from './fiat-vault';
import { JsonlLedgerStore, type LedgerStore } from './ledger';

// ---------------------------------------------------------------------------
// Constants (the account scheme is frozen: the site computes the same address)

export const ENTRY_POINT_V06: Address = entryPoint06Address; // 0x5FF137D4b0FDCD49DcA30c7CF57E578a026d2789
export const FACTORY_V1_1: Address = '0xba5ed110efdba3d005bfc882d75358acbbb85842';
export const FACTORY_INIT_CODE_HASH: Hex = '0xa53d9968113cdf38ec78edc985b30bcb1a274ddaa221624c1d15604f70b0761a';
export const ACCOUNT_NONCE = 0n;
/** EntryPoint nonce key. Never the account's REPLAYABLE_NONCE_KEY (8453). */
export const NONCE_KEY = 0n;

/** The relayer's OWN gas limits (H2): the client never chooses them. Measured
 *  on Base Sepolia by simulation on 23 Nov 2026 (OP1.3a report, no funds): a
 *  fresh account's add (deployment + WebAuthn validation + addOwnerPublicKey)
 *  had preOpGas 316,278 and used about 414,680 gas in all; eth_estimateGas for
 *  the whole handleOps transaction was 1,322,063 (the EntryPoint wants gas in
 *  hand for every limit). viem's floor for a WebAuthn owner's verification
 *  limit is 800,000, kept. */
export const GAS = Object.freeze({
  callGasLimit: 200_000n,
  verificationGasLimit: 800_000n,
  preVerificationGas: 60_000n,
  /** The tip on the operation and on our own transactions (0.001 gwei). */
  maxPriorityFeePerGas: 1_000_000n,
  /** handleOps transaction gas: the op's limits plus the EntryPoint's own work. */
  handleOpsTxGas: 1_500_000n,
  depositTxGas: 80_000n,
});

export const PREPARED_TTL_MS = 10 * 60_000;
const DAY_MS = 86_400_000;
/** wrapSignature carries the owner index as a uint8. */
const MAX_OWNER_INDEX = 255;
const MAX_SIGNATURE_HEX = 16_384;

const ownerAbi = parseAbi([
  'function execute(address target, uint256 value, bytes data) payable',
  'function addOwnerPublicKey(bytes32 x, bytes32 y)',
  'function removeOwnerAtIndex(uint256 index, bytes owner)',
  'function isOwnerPublicKey(bytes32 x, bytes32 y) view returns (bool)',
  'function ownerAtIndex(uint256 index) view returns (bytes)',
  'function nextOwnerIndex() view returns (uint256)',
  'function removedOwnersCount() view returns (uint256)',
  'event AddOwner(uint256 indexed index, bytes owner)',
]);
const factoryAbi = parseAbi(['function createAccount(bytes[] owners, uint256 nonce) payable returns (address)']);

// ---------------------------------------------------------------------------
// Keys and the local address

const KEY_RE = /^0x[0-9a-fA-F]{128}$/;
const P = 0xffffffff00000001000000000000000000000000ffffffffffffffffffffffffn;
const B = 0x5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604bn;

/** A 64-byte x || y key that is a point on P-256 (the contract takes any 64
 *  bytes; a key off the curve is an owner nobody can ever sign for). */
export function isP256PublicKey(key: unknown): key is Hex {
  if (typeof key !== 'string' || !KEY_RE.test(key)) return false;
  const x = BigInt('0x' + key.slice(2, 66));
  const y = BigInt('0x' + key.slice(66));
  if (x >= P || y >= P) return false;
  const mod = (n: bigint) => ((n % P) + P) % P;
  return mod(y * y) === mod(x * x * x - 3n * x + B);
}

/** The account address for a first owner key, by the factory's CREATE2 rule
 *  (the same calculation as the site's passkey-account.ts). */
export function accountAddressFor(firstOwnerPublicKey: Hex): Address {
  const key = firstOwnerPublicKey.toLowerCase() as Hex;
  const salt = keccak256(encodeAbiParameters([{ type: 'bytes[]' }, { type: 'uint256' }], [[key], ACCOUNT_NONCE]));
  return getContractAddress({ opcode: 'CREATE2', from: FACTORY_V1_1, salt, bytecodeHash: FACTORY_INIT_CODE_HASH });
}

export function initCodeFor(firstOwnerPublicKey: Hex): Hex {
  return concat([
    FACTORY_V1_1,
    encodeFunctionData({ abi: factoryAbi, functionName: 'createAccount', args: [[firstOwnerPublicKey.toLowerCase() as Hex], ACCOUNT_NONCE] }),
  ]);
}

export type OwnerAction = { action: 'add'; publicKey: Hex } | { action: 'remove'; index: number; publicKey: Hex };

/** The ONLY two calls the relayer ever builds. */
export function ownerChangeCallData(sender: Address, action: OwnerAction): Hex {
  const inner =
    action.action === 'add'
      ? encodeFunctionData({
          abi: ownerAbi,
          functionName: 'addOwnerPublicKey',
          args: [`0x${action.publicKey.slice(2, 66)}` as Hex, `0x${action.publicKey.slice(66)}` as Hex],
        })
      : encodeFunctionData({ abi: ownerAbi, functionName: 'removeOwnerAtIndex', args: [BigInt(action.index), action.publicKey.toLowerCase() as Hex] });
  return encodeFunctionData({ abi: ownerAbi, functionName: 'execute', args: [sender, 0n, inner] });
}

// ---------------------------------------------------------------------------
// The user operation (v0.6)

export type UserOp = {
  sender: Address;
  nonce: bigint;
  initCode: Hex;
  callData: Hex;
  callGasLimit: bigint;
  verificationGasLimit: bigint;
  preVerificationGas: bigint;
  maxFeePerGas: bigint;
  maxPriorityFeePerGas: bigint;
  paymasterAndData: Hex;
  signature: Hex;
};

export type WireUserOp = { [K in keyof UserOp]: UserOp[K] extends bigint ? Hex : UserOp[K] };

export function toWire(op: UserOp): WireUserOp {
  return {
    sender: op.sender,
    nonce: toHex(op.nonce),
    initCode: op.initCode,
    callData: op.callData,
    callGasLimit: toHex(op.callGasLimit),
    verificationGasLimit: toHex(op.verificationGasLimit),
    preVerificationGas: toHex(op.preVerificationGas),
    maxFeePerGas: toHex(op.maxFeePerGas),
    maxPriorityFeePerGas: toHex(op.maxPriorityFeePerGas),
    paymasterAndData: op.paymasterAndData,
    signature: op.signature,
  };
}

export function userOpHash(op: UserOp, chainId: number): Hex {
  return getUserOperationHash({
    chainId,
    entryPointAddress: ENTRY_POINT_V06,
    entryPointVersion: '0.6',
    userOperation: { ...op, signature: '0x' },
  });
}

/** What the account must hold at the EntryPoint before handleOps (no paymaster: multiplier 1). */
export function requiredPrefund(op: UserOp): bigint {
  return (op.callGasLimit + op.verificationGasLimit + op.preVerificationGas) * op.maxFeePerGas;
}

// ---------------------------------------------------------------------------
// Config

export type RelayerConfig = {
  maxPerAccountPerDay: number;
  maxPerDay: number;
  lowBalanceWei: bigint;
  /** A fee above this is refused (fee_too_high), never paid. */
  maxFeePerGasWei: bigint;
  /** prepare calls one account may make in a UTC day (each costs RPC calls). In memory: a restart resets it. */
  maxPreparesPerAccountPerDay: number;
  /** Prepared operations one account may hold open; a newer prepare drops its oldest. */
  maxOpenPreparedPerAccount: number;
  /** Prepared operations held for everyone together. */
  maxPrepared: number;
};

export const DEFAULT_RELAYER_CONFIG: RelayerConfig = Object.freeze({
  maxPerAccountPerDay: 5,
  maxPerDay: 200,
  lowBalanceWei: 2_000_000_000_000_000n, // 0.002 ETH
  maxFeePerGasWei: 1_000_000_000n, // 1 gwei
  maxPreparesPerAccountPerDay: 30,
  maxOpenPreparedPerAccount: 3,
  maxPrepared: 1000,
});

function positiveInt(raw: string | undefined, fallback: number, name: string): number {
  const v = (raw ?? '').trim();
  if (v === '') return fallback;
  if (!/^[1-9]\d{0,6}$/.test(v)) throw new Error(`${name} must be a positive integer, got "${v}"`);
  return Number(v);
}

function weiAmount(raw: string | undefined, fallback: bigint, name: string): bigint {
  const v = (raw ?? '').trim();
  if (v === '') return fallback;
  if (!/^\d{1,30}$/.test(v)) throw new Error(`${name} must be a whole number of wei, got "${v}"`);
  return BigInt(v);
}

/** The relayer's settings. `privateKey` is null when FIAT_RELAYER_PRIVATE_KEY is
 *  unset or malformed (the routes then answer 503 relayer_not_configured); the
 *  value itself is never echoed in any message. */
export function relayerConfigFromEnv(env: Record<string, string | undefined> = process.env): {
  privateKey: Hex | null;
  config: RelayerConfig;
} {
  const raw = (env.FIAT_RELAYER_PRIVATE_KEY ?? '').trim();
  let privateKey: Hex | null = null;
  if (raw !== '') {
    if (/^0x[0-9a-fA-F]{64}$/.test(raw)) privateKey = raw as Hex;
    else console.error('[owner-relayer] FIAT_RELAYER_PRIVATE_KEY is malformed (want 0x + 64 hex); the relayer is off');
  }
  return {
    privateKey,
    config: {
      maxPerAccountPerDay: positiveInt(env.FIAT_RELAYER_MAX_PER_ACCOUNT_PER_DAY, DEFAULT_RELAYER_CONFIG.maxPerAccountPerDay, 'FIAT_RELAYER_MAX_PER_ACCOUNT_PER_DAY'),
      maxPerDay: positiveInt(env.FIAT_RELAYER_MAX_PER_DAY, DEFAULT_RELAYER_CONFIG.maxPerDay, 'FIAT_RELAYER_MAX_PER_DAY'),
      lowBalanceWei: weiAmount(env.FIAT_RELAYER_LOW_BALANCE_WEI, DEFAULT_RELAYER_CONFIG.lowBalanceWei, 'FIAT_RELAYER_LOW_BALANCE_WEI'),
      maxFeePerGasWei: weiAmount(env.FIAT_RELAYER_MAX_FEE_PER_GAS_WEI, DEFAULT_RELAYER_CONFIG.maxFeePerGasWei, 'FIAT_RELAYER_MAX_FEE_PER_GAS_WEI'),
      maxPreparesPerAccountPerDay: positiveInt(
        env.FIAT_RELAYER_MAX_PREPARES_PER_ACCOUNT_PER_DAY,
        DEFAULT_RELAYER_CONFIG.maxPreparesPerAccountPerDay,
        'FIAT_RELAYER_MAX_PREPARES_PER_ACCOUNT_PER_DAY'
      ),
      maxOpenPreparedPerAccount: DEFAULT_RELAYER_CONFIG.maxOpenPreparedPerAccount,
      maxPrepared: DEFAULT_RELAYER_CONFIG.maxPrepared,
    },
  };
}

// ---------------------------------------------------------------------------
// The journal (daily caps and the same-day rule)

type ChangeEntry = { t: 'owner-change'; account: string; action: 'add' | 'remove'; index?: number; userOpHash: Hex; at: number };
type ResultEntry = { t: 'owner-change-result'; userOpHash: Hex; txHash?: Hex; ok: boolean; at: number };

const utcDay = (ms: number) => Math.floor(ms / DAY_MS);

// ---------------------------------------------------------------------------
// Answers

export type RelayerAnswer = { status: number; body: Record<string, unknown> };
const answer = (status: number, body: Record<string, unknown>): RelayerAnswer => ({ status, body });
const refuse = (status: number, error: string, extra: Record<string, unknown> = {}): RelayerAnswer => answer(status, { error, ...extra });

class Refusal extends Error {
  constructor(readonly answer: RelayerAnswer) {
    super(String(answer.body.error));
  }
}

type Prepared = {
  userId: string;
  action: OwnerAction;
  op: UserOp;
  hash: Hex;
  expiresAt: number;
  hadInitCode: boolean;
  firstOwnerPublicKey: Hex | null;
};

export type SimulationResult =
  | { ok: true; preOpGas: bigint; paid: bigint; handleOpsGas?: bigint }
  | { ok: false; reason: string };

const CHAINS = { [baseSepolia.id]: baseSepolia, [base.id]: base } as const;

/** The revert data in a viem error chain (custom and http transports put it in different places). */
function revertData(e: unknown): Hex | null {
  let x: unknown = e;
  for (let d = 0; x && typeof x === 'object' && d < 12; d++) {
    const data = (x as { data?: unknown }).data;
    if (typeof data === 'string' && /^0x[0-9a-fA-F]*$/.test(data) && data.length >= 10) return data as Hex;
    if (data && typeof data === 'object' && typeof (data as { data?: unknown }).data === 'string') {
      const inner = (data as { data: string }).data;
      if (/^0x[0-9a-fA-F]*$/.test(inner) && inner.length >= 10) return inner as Hex;
    }
    x = (x as { cause?: unknown }).cause;
  }
  return null;
}

function failedOpReason(e: unknown): string {
  const data = revertData(e);
  if (!data) return 'reverted';
  try {
    const decoded = decodeErrorResult({ abi: entryPoint06Abi, data });
    if (decoded.errorName === 'FailedOp') return String((decoded.args as readonly unknown[])[1]);
    return decoded.errorName;
  } catch {
    return 'reverted';
  }
}

// ---------------------------------------------------------------------------
// The relayer

export type OwnerRelayerOptions = {
  transport: Transport;
  chainId: number;
  privateKey: Hex;
  store: LedgerStore;
  config?: Partial<RelayerConfig>;
  now?: () => number;
  /** viem polling interval while waiting for a receipt (tests: a few ms) */
  pollingInterval?: number;
  receiptTimeoutMs?: number;
  log?: (line: string) => void;
  alarm?: (line: string) => void;
};

export class OwnerRelayer {
  readonly address: Address;
  readonly chainId: number;
  private readonly account: PrivateKeyAccount;
  private readonly client: PublicClient;
  private readonly wallet: WalletClient;
  private readonly config: RelayerConfig;
  private readonly now: () => number;
  private readonly log: (line: string) => void;
  private readonly alarm: (line: string) => void;
  private readonly receiptTimeoutMs: number;
  private readonly prepared = new Map<Hex, Prepared>();
  private readonly locks = new Set<string>();
  /** Cap slots taken by submits that have not recorded their change yet (review L8). */
  private readonly reserved = new Map<string, number>();
  private reservedTotal = 0;
  /** prepare calls per account in the current UTC day (review M4). */
  private readonly prepareCounts = new Map<string, { day: number; count: number }>();
  private changes: ChangeEntry[] = [];
  private sendQueue: Promise<unknown> = Promise.resolve();
  private storeQueue: Promise<unknown> = Promise.resolve();

  private constructor(private readonly opts: OwnerRelayerOptions) {
    const chain = (CHAINS as Record<number, typeof baseSepolia | typeof base>)[opts.chainId];
    if (!chain) throw new Error(`The owner relayer supports Base Sepolia and Base only, not chain ${opts.chainId}`);
    this.chainId = opts.chainId;
    this.account = privateKeyToAccount(opts.privateKey);
    this.address = this.account.address;
    const pollingInterval = opts.pollingInterval ?? 2_000;
    this.client = createPublicClient({ chain, transport: opts.transport, pollingInterval }) as PublicClient;
    this.wallet = createWalletClient({ chain, transport: opts.transport, account: this.account, pollingInterval });
    this.config = { ...DEFAULT_RELAYER_CONFIG, ...opts.config };
    this.now = opts.now ?? Date.now;
    this.log = opts.log ?? ((l) => console.log(l));
    this.alarm = opts.alarm ?? ((l) => console.error(l));
    this.receiptTimeoutMs = opts.receiptTimeoutMs ?? 120_000;
  }

  static async open(opts: OwnerRelayerOptions): Promise<OwnerRelayer> {
    const relayer = new OwnerRelayer(opts);
    for (const line of await opts.store.load()) {
      let entry: ChangeEntry | ResultEntry;
      try {
        entry = JSON.parse(line);
      } catch {
        continue; // a torn line: the store repairs its tail
      }
      if (entry.t === 'owner-change') relayer.changes.push(entry);
    }
    relayer.log(`[owner-relayer] gas wallet ${relayer.address} on chain ${relayer.chainId}`);
    return relayer;
  }

  // ---- caps and the same-day rule

  private countToday(account?: string): number {
    const today = utcDay(this.now());
    return this.changes.filter((c) => utcDay(c.at) === today && (account === undefined || c.account === account)).length;
  }

  private addedToday(account: string): boolean {
    const today = utcDay(this.now());
    return this.changes.some((c) => c.account === account && c.action === 'add' && utcDay(c.at) === today);
  }

  /** The caps, counting the slots other submits hold but have not recorded yet. */
  private checkCaps(userId: string): void {
    if (this.countToday(userId) + (this.reserved.get(userId) ?? 0) >= this.config.maxPerAccountPerDay) throw new Refusal(refuse(429, 'account_daily_limit'));
    if (this.countToday() + this.reservedTotal >= this.config.maxPerDay) throw new Refusal(refuse(429, 'daily_limit'));
  }

  /** Check the caps and hold a slot in one synchronous step, so concurrent submits
   *  from different accounts cannot all pass a cap with one place left. The slot
   *  is let go once the change is journaled (it is counted there) or refused. */
  private reserveCap(userId: string): () => void {
    this.checkCaps(userId);
    this.reserved.set(userId, (this.reserved.get(userId) ?? 0) + 1);
    this.reservedTotal++;
    let released = false;
    return () => {
      if (released) return;
      released = true;
      const left = (this.reserved.get(userId) ?? 1) - 1;
      if (left <= 0) this.reserved.delete(userId);
      else this.reserved.set(userId, left);
      this.reservedTotal--;
    };
  }

  /** One prepare counted against the account's daily allowance. */
  private takePrepare(userId: string): boolean {
    const day = utcDay(this.now());
    const seen = this.prepareCounts.get(userId);
    const count = seen && seen.day === day ? seen.count : 0;
    if (count >= this.config.maxPreparesPerAccountPerDay) return false;
    this.prepareCounts.set(userId, { day, count: count + 1 });
    if (this.prepareCounts.size > 10_000) {
      for (const [k, v] of this.prepareCounts) if (v.day !== day) this.prepareCounts.delete(k);
    }
    return true;
  }

  private record(entry: ChangeEntry | ResultEntry): Promise<void> {
    const run = this.storeQueue.then(async () => {
      await this.opts.store.append(JSON.stringify(entry));
      if (entry.t === 'owner-change') this.changes.push(entry);
    });
    this.storeQueue = run.catch(() => undefined);
    return run;
  }

  // ---- chain reads

  private async isDeployed(sender: Address): Promise<boolean> {
    const code = await this.client.getCode({ address: sender });
    return !!code && code !== '0x';
  }

  private read<F extends 'isOwnerPublicKey' | 'ownerAtIndex' | 'nextOwnerIndex' | 'removedOwnersCount'>(
    sender: Address,
    functionName: F,
    args: readonly unknown[] = [],
    blockNumber?: bigint
  ) {
    return this.client.readContract({
      address: sender,
      abi: ownerAbi,
      functionName,
      args,
      ...(blockNumber !== undefined ? { blockNumber } : {}),
    } as never) as Promise<F extends 'isOwnerPublicKey' ? boolean : F extends 'ownerAtIndex' ? Hex : bigint>;
  }

  private nonceOf(sender: Address): Promise<bigint> {
    return this.client.readContract({ address: ENTRY_POINT_V06, abi: entryPoint06Abi, functionName: 'getNonce', args: [sender, NONCE_KEY] });
  }

  private depositOf(sender: Address): Promise<bigint> {
    return this.client.readContract({ address: ENTRY_POINT_V06, abi: entryPoint06Abi, functionName: 'balanceOf', args: [sender] });
  }

  /** The rules for one action against the chain as it is NOW (prepare and again at submit). */
  private async checkAction(userId: string, sender: Address, action: OwnerAction, deployed: boolean, firstOwnerPublicKey: Hex | null): Promise<void> {
    if (!deployed && !firstOwnerPublicKey) throw new Refusal(refuse(400, 'first_owner_required'));
    if (action.action === 'add') {
      if (deployed) {
        const x = `0x${action.publicKey.slice(2, 66)}` as Hex;
        const y = `0x${action.publicKey.slice(66)}` as Hex;
        if (await this.read(sender, 'isOwnerPublicKey', [x, y])) throw new Refusal(refuse(409, 'already_owner'));
        if ((await this.read(sender, 'nextOwnerIndex')) > BigInt(MAX_OWNER_INDEX)) throw new Refusal(refuse(422, 'too_many_owners'));
      } else if (firstOwnerPublicKey && firstOwnerPublicKey.toLowerCase() === action.publicKey.toLowerCase()) {
        throw new Refusal(refuse(409, 'already_owner'));
      }
      return;
    }
    // remove: an undeployed account has exactly one owner, and the last owner is never removed.
    if (!deployed) throw new Refusal(refuse(422, 'last_owner'));
    const next = await this.read(sender, 'nextOwnerIndex');
    const removed = await this.read(sender, 'removedOwnersCount');
    if (next - removed <= 1n) throw new Refusal(refuse(422, 'last_owner'));
    if (BigInt(action.index) >= next) throw new Refusal(refuse(422, 'wrong_owner'));
    const owner = await this.read(sender, 'ownerAtIndex', [BigInt(action.index)]);
    if (owner.toLowerCase() !== action.publicKey.toLowerCase()) throw new Refusal(refuse(422, 'wrong_owner'));
    // T7: a passkey added today cannot remove the first one today.
    if (action.index === 0 && this.addedToday(userId)) throw new Refusal(refuse(422, 'first_passkey_locked_today'));
  }

  // ---- prepare

  async prepare(userId: string, body: unknown): Promise<RelayerAnswer> {
    try {
      return await this.prepareNow(userId, body);
    } catch (e) {
      return this.failure('prepare', e);
    }
  }

  private async prepareNow(userId: string, body: unknown): Promise<RelayerAnswer> {
    const parsed = parsePrepareBody(body);
    if ('error' in parsed) return refuse(400, parsed.error);
    const { action, firstOwnerPublicKey } = parsed;
    const sender = getAddress(userId);
    // R3: the address computed from the claimed first owner must be the credential's.
    if (firstOwnerPublicKey && accountAddressFor(firstOwnerPublicKey).toLowerCase() !== userId.toLowerCase()) {
      return refuse(403, 'wrong_account');
    }
    this.checkCaps(userId);
    // Counted before any chain read: one account cannot burn our RPC (review M4).
    if (!this.takePrepare(userId)) return refuse(429, 'prepare_limit');
    const deployed = await this.isDeployed(sender);
    await this.checkAction(userId, sender, action, deployed, firstOwnerPublicKey);
    const [nonce, block] = await Promise.all([this.nonceOf(sender), this.client.getBlock({ blockTag: 'latest' })]);
    const baseFee = block.baseFeePerGas;
    if (baseFee === null || baseFee === undefined) return refuse(503, 'chain_unavailable');
    const maxFeePerGas = baseFee * 2n + GAS.maxPriorityFeePerGas;
    if (maxFeePerGas > this.config.maxFeePerGasWei) return refuse(503, 'fee_too_high');
    const op: UserOp = {
      sender,
      nonce,
      initCode: deployed ? '0x' : initCodeFor(firstOwnerPublicKey as Hex),
      callData: ownerChangeCallData(sender, action),
      callGasLimit: GAS.callGasLimit,
      verificationGasLimit: GAS.verificationGasLimit,
      preVerificationGas: GAS.preVerificationGas,
      maxFeePerGas,
      maxPriorityFeePerGas: GAS.maxPriorityFeePerGas,
      paymasterAndData: '0x',
      signature: '0x',
    };
    const hash = userOpHash(op, this.chainId);
    this.prune();
    // One account holds at most a few open operations: a newer one drops its oldest (review M4).
    const mine = [...this.prepared.values()].filter((p) => p.userId === userId).sort((a, b) => a.expiresAt - b.expiresAt);
    while (mine.length >= this.config.maxOpenPreparedPerAccount) this.prepared.delete(mine.shift()!.hash);
    if (this.prepared.size >= this.config.maxPrepared) return refuse(503, 'busy');
    const expiresAt = this.now() + PREPARED_TTL_MS;
    this.prepared.set(hash, { userId, action, op, hash, expiresAt, hadInitCode: !deployed, firstOwnerPublicKey });
    return answer(200, {
      userOperation: toWire(op),
      userOpHash: hash,
      entryPoint: ENTRY_POINT_V06,
      chainId: this.chainId,
      expiresAt,
    });
  }

  private prune(): void {
    const now = this.now();
    for (const [h, p] of this.prepared) if (p.expiresAt <= now) this.prepared.delete(h);
  }

  // ---- submit

  async submit(userId: string, body: unknown): Promise<RelayerAnswer> {
    const parsed = parseSubmitBody(body);
    if ('error' in parsed) return refuse(400, parsed.error);
    const prepared = this.prepared.get(parsed.userOpHash);
    if (!prepared || prepared.expiresAt <= this.now() || prepared.userId !== userId) {
      if (prepared && prepared.expiresAt <= this.now()) this.prepared.delete(prepared.hash);
      return refuse(404, 'unknown_operation');
    }
    if (this.locks.has(userId)) return refuse(409, 'busy');
    let release: () => void;
    try {
      release = this.reserveCap(userId);
    } catch (e) {
      return this.failure('submit', e);
    }
    this.locks.add(userId);
    try {
      return await this.submitNow(userId, prepared, parsed.signature);
    } catch (e) {
      return this.failure('submit', e);
    } finally {
      this.locks.delete(userId);
      release();
    }
  }

  private async submitNow(userId: string, prepared: Prepared, signature: Hex): Promise<RelayerAnswer> {
    const { op: unsigned, action } = prepared;
    const sender = unsigned.sender;
    // The caps were checked, and a slot held, by submit (reserveCap).
    // R4: everything again, against the chain NOW.
    const deployed = await this.isDeployed(sender);
    if (prepared.hadInitCode && deployed) {
      // Someone deployed it meanwhile (the factory is public): initCode would fail (AA10).
      this.prepared.delete(prepared.hash);
      return refuse(409, 'prepare_again');
    }
    if (!prepared.hadInitCode && !deployed) {
      this.prepared.delete(prepared.hash);
      return refuse(409, 'prepare_again');
    }
    if ((await this.nonceOf(sender)) !== unsigned.nonce) {
      this.prepared.delete(prepared.hash);
      return refuse(409, 'prepare_again');
    }
    // The fee was set at prepare, up to PREPARED_TTL_MS ago (review L7). If the
    // base fee has risen so the op's maxFeePerGas no longer covers base fee + tip,
    // the op would not repay our transaction (or sit unmined): build it again.
    const block = await this.client.getBlock({ blockTag: 'latest' });
    if (block.baseFeePerGas === null || block.baseFeePerGas === undefined) return refuse(503, 'chain_unavailable');
    if (unsigned.maxFeePerGas < block.baseFeePerGas + unsigned.maxPriorityFeePerGas) {
      this.prepared.delete(prepared.hash);
      return refuse(409, 'prepare_again');
    }
    await this.checkAction(userId, sender, action, deployed, prepared.firstOwnerPublicKey);
    // The op is OURS, from the prepared record; only the signature comes from the client.
    const op: UserOp = { ...unsigned, signature };
    if (userOpHash(op, this.chainId) !== prepared.hash) return refuse(500, 'internal_error');

    const sim = await this.simulate(op, action);
    if (!sim.ok) return refuse(422, 'simulation_failed', { reason: sim.reason });

    const needed = requiredPrefund(op);
    const balance = await this.client.getBalance({ address: this.address });
    const worstCase = needed + (GAS.handleOpsTxGas + GAS.depositTxGas) * (op.maxFeePerGas + GAS.maxPriorityFeePerGas);
    if (balance < worstCase) {
      this.alarm(`[owner-relayer] ALARM gas wallet ${this.address} has ${balance} wei, needs ${worstCase} wei for one operation`);
      return refuse(503, 'relayer_low_balance');
    }

    // Counted BEFORE anything is sent: a crash after the send still counts.
    await this.record({ t: 'owner-change', account: userId, action: action.action, ...(action.action === 'remove' ? { index: action.index } : {}), userOpHash: prepared.hash, at: this.now() });
    this.prepared.delete(prepared.hash);

    const sent = await this.serial(() => this.send(op));
    // From here the change may be on chain: nothing below may turn it into a
    // failure answer (review M2). The journal line is for the record only.
    try {
      await this.record({ t: 'owner-change-result', userOpHash: prepared.hash, ...(sent.txHash ? { txHash: sent.txHash } : {}), ok: sent.ok, at: this.now() });
    } catch (e) {
      this.alarm(`[owner-relayer] ALARM ${prepared.hash}: result not journaled: ${e instanceof Error ? e.message.split('\n')[0] : String(e)}`);
    }
    void this.checkBalance();
    if (sent.pending) return refuse(502, 'operation_pending', sent.txHash ? { txHash: sent.txHash } : {});
    if (!sent.ok) return refuse(502, 'operation_failed', sent.txHash ? { txHash: sent.txHash } : {});

    // The index of an add comes from the AddOwner event in OUR receipt, matched to
    // the key (review M1): never from state after the block, which another add in
    // the same block would move. No matching event: the answer says so.
    let ownerIndex: number | null;
    if (action.action === 'add') {
      ownerIndex = addOwnerIndex(sent.logs ?? [], sender, action.publicKey);
      if (ownerIndex === null) this.alarm(`[owner-relayer] ALARM ${prepared.hash}: mined, but no AddOwner event for the key in the receipt`);
    } else {
      ownerIndex = action.index;
    }
    this.log(`[owner-relayer] ${action.action} for ${sender} index ${ownerIndex ?? 'unknown'} tx ${sent.txHash}`);
    return answer(200, { txHash: sent.txHash, ownerIndex, confirmed: true, ...(ownerIndex === null ? { note: 'owner_index_unknown' } : {}) });
  }

  private serial<T>(fn: () => Promise<T>): Promise<T> {
    const run = this.sendQueue.then(fn);
    this.sendQueue = run.catch(() => undefined);
    return run;
  }

  /** depositTo (only if the account's deposit is short), then handleOps; waits for both receipts. */
  private async send(op: UserOp): Promise<{ ok: boolean; pending?: boolean; txHash?: Hex; logs?: readonly ReceiptLog[] }> {
    const fees = { maxFeePerGas: op.maxFeePerGas + GAS.maxPriorityFeePerGas, maxPriorityFeePerGas: GAS.maxPriorityFeePerGas };
    const deposit = await this.depositOf(op.sender);
    const needed = requiredPrefund(op);
    if (deposit < needed) {
      const depositHash = await this.wallet.sendTransaction({
        account: this.account,
        chain: this.client.chain,
        to: ENTRY_POINT_V06,
        data: encodeFunctionData({ abi: entryPoint06Abi, functionName: 'depositTo', args: [op.sender] }),
        value: needed - deposit,
        gas: GAS.depositTxGas,
        ...fees,
      });
      const r = await this.receipt(depositHash);
      if (!r) return { ok: false, pending: true, txHash: depositHash };
      if (r.status !== 'success') return { ok: false, txHash: depositHash };
    }
    const txHash = await this.wallet.sendTransaction({
      account: this.account,
      chain: this.client.chain,
      to: ENTRY_POINT_V06,
      data: encodeFunctionData({ abi: entryPoint06Abi, functionName: 'handleOps', args: [[op], this.address] }),
      gas: GAS.handleOpsTxGas,
      ...fees,
    });
    const receipt = await this.receipt(txHash);
    if (!receipt) return { ok: false, pending: true, txHash };
    if (receipt.status !== 'success') return { ok: false, txHash };
    const hash = userOpHash(op, this.chainId).toLowerCase();
    for (const log of receipt.logs) {
      if (log.address.toLowerCase() !== ENTRY_POINT_V06.toLowerCase()) continue;
      try {
        const ev = decodeEventLog({ abi: entryPoint06Abi, data: log.data, topics: log.topics });
        if (ev.eventName === 'UserOperationEvent' && String(ev.args.userOpHash).toLowerCase() === hash) {
          return { ok: ev.args.success === true, txHash, logs: receipt.logs };
        }
      } catch {
        // another event
      }
    }
    return { ok: false, txHash };
  }

  /** The receipt, or null when it did not come in time (the transaction may still be mined). */
  private async receipt(hash: Hex) {
    try {
      return await this.client.waitForTransactionReceipt({ hash, timeout: this.receiptTimeoutMs });
    } catch (e) {
      this.alarm(`[owner-relayer] no receipt for ${hash}: ${e instanceof Error ? e.message.split('\n')[0] : String(e)}`);
      return null;
    }
  }

  /** Simulation with NO funds (eth_call only): the account's EntryPoint deposit
   *  is set by a state override to what depositTo will give it. Two calls:
   *  handleOps (validation, so the signature, must pass) and simulateHandleOp
   *  with the account as target (the change must have happened). */
  async simulate(op: UserOp, action: OwnerAction): Promise<SimulationResult> {
    const prefund = requiredPrefund(op);
    let current = 0n;
    try {
      current = await this.depositOf(op.sender);
    } catch {
      return { ok: false, reason: 'chain_unavailable' };
    }
    const depositValue = current > prefund ? current : prefund;
    const stateOverride: StateOverride = [
      { address: ENTRY_POINT_V06, stateDiff: [{ slot: depositSlot(op.sender), value: pad(toHex(depositValue), { size: 32 }) }] },
      { address: this.address, balance: 10n ** 18n },
    ];
    // The override must land where the EntryPoint keeps the deposit.
    try {
      const seen = decodeFunctionResult({
        abi: entryPoint06Abi,
        functionName: 'balanceOf',
        data: (
          await this.client.call({
            to: ENTRY_POINT_V06,
            data: encodeFunctionData({ abi: entryPoint06Abi, functionName: 'balanceOf', args: [op.sender] }),
            stateOverride,
          })
        ).data as Hex,
      });
      if (seen !== depositValue) return { ok: false, reason: 'deposit_override_unsupported' };
    } catch {
      return { ok: false, reason: 'deposit_override_unsupported' };
    }
    const handleOps = encodeFunctionData({ abi: entryPoint06Abi, functionName: 'handleOps', args: [[op], this.address] });
    try {
      await this.client.call({ account: this.address, to: ENTRY_POINT_V06, data: handleOps, gas: GAS.handleOpsTxGas, stateOverride });
    } catch (e) {
      return { ok: false, reason: failedOpReason(e) };
    }
    const target =
      action.action === 'add'
        ? encodeFunctionData({ abi: ownerAbi, functionName: 'isOwnerPublicKey', args: [`0x${action.publicKey.slice(2, 66)}` as Hex, `0x${action.publicKey.slice(66)}` as Hex] })
        : encodeFunctionData({ abi: ownerAbi, functionName: 'ownerAtIndex', args: [BigInt(action.index)] });
    let result: { preOpGas: bigint; paid: bigint; targetSuccess: boolean; targetResult: Hex } | null = null;
    try {
      await this.client.call({
        account: this.address,
        to: ENTRY_POINT_V06,
        data: encodeFunctionData({ abi: entryPoint06Abi, functionName: 'simulateHandleOp', args: [op, op.sender, target] }),
        gas: GAS.handleOpsTxGas * 2n,
        stateOverride,
      });
      return { ok: false, reason: 'simulation_did_not_revert' };
    } catch (e) {
      const data = revertData(e);
      if (!data) return { ok: false, reason: 'reverted' };
      try {
        const decoded = decodeErrorResult({ abi: entryPoint06Abi, data });
        if (decoded.errorName !== 'ExecutionResult') {
          return { ok: false, reason: decoded.errorName === 'FailedOp' ? String((decoded.args as readonly unknown[])[1]) : decoded.errorName };
        }
        const [preOpGas, paid, , , targetSuccess, targetResult] = decoded.args as readonly [bigint, bigint, number, number, boolean, Hex];
        result = { preOpGas, paid, targetSuccess, targetResult };
      } catch {
        return { ok: false, reason: 'reverted' };
      }
    }
    if (!result.targetSuccess) return { ok: false, reason: 'change_not_made' };
    try {
      if (action.action === 'add') {
        if (decodeFunctionResult({ abi: ownerAbi, functionName: 'isOwnerPublicKey', data: result.targetResult }) !== true) {
          return { ok: false, reason: 'change_not_made' };
        }
      } else if (decodeFunctionResult({ abi: ownerAbi, functionName: 'ownerAtIndex', data: result.targetResult }) !== '0x') {
        return { ok: false, reason: 'change_not_made' };
      }
    } catch {
      return { ok: false, reason: 'change_not_made' };
    }
    let handleOpsGas: bigint | undefined;
    try {
      handleOpsGas = await this.client.estimateGas({ account: this.address, to: ENTRY_POINT_V06, data: handleOps, stateOverride });
    } catch {
      handleOpsGas = undefined; // an RPC without estimateGas overrides: the figure is for the record only
    }
    return { ok: true, preOpGas: result.preOpGas, paid: result.paid, ...(handleOpsGas !== undefined ? { handleOpsGas } : {}) };
  }

  /** The low-balance alarm line. Never throws. */
  async checkBalance(): Promise<bigint | null> {
    try {
      const balance = await this.client.getBalance({ address: this.address });
      if (balance < this.config.lowBalanceWei) {
        this.alarm(`[owner-relayer] ALARM gas wallet ${this.address} balance ${balance} wei is below FIAT_RELAYER_LOW_BALANCE_WEI (${this.config.lowBalanceWei})`);
      }
      return balance;
    } catch {
      return null;
    }
  }

  private failure(where: string, e: unknown): RelayerAnswer {
    if (e instanceof Refusal) return e.answer;
    // The message can carry RPC detail, never the key (viem never puts a key in an error).
    const message = e instanceof Error ? e.message.split('\n')[0] : String(e);
    this.alarm(`[owner-relayer] ${where} failed: ${message}`);
    return refuse(503, 'chain_unavailable');
  }

  /** Tests only. */
  __preparedCount(): number {
    return this.prepared.size;
  }
}

// ---------------------------------------------------------------------------
// The one relayer of this process (both route bundles share it, as getFiatDeps does)

const g = globalThis as typeof globalThis & {
  __ownerRelayer?: Promise<OwnerRelayer | null>;
  __ownerRelayerForTest?: OwnerRelayer | null;
};

/** Tests only: a relayer (or null: not configured) in place of the env-built one; undefined restores it. */
export function setOwnerRelayerForTest(relayer: OwnerRelayer | null | undefined): void {
  if (relayer === undefined) delete g.__ownerRelayerForTest;
  else g.__ownerRelayerForTest = relayer;
  g.__ownerRelayer = undefined;
}

/** The relayer, or null when FIAT_RELAYER_PRIVATE_KEY is not set. Its journal is
 *  owner-relayer.jsonl under FIAT_DATA_DIR; its RPC and chain are the vault's. */
export function getOwnerRelayer(): Promise<OwnerRelayer | null> {
  if (g.__ownerRelayerForTest !== undefined) return Promise.resolve(g.__ownerRelayerForTest);
  g.__ownerRelayer ??= (async () => {
    const { privateKey, config } = relayerConfigFromEnv();
    if (!privateKey) return null;
    const dataDir = process.env.FIAT_DATA_DIR ?? './data/fiat';
    const relayer = await OwnerRelayer.open({
      transport: http(rpcUrl()),
      chainId: Number(vaultChainId()),
      privateKey,
      store: new JsonlLedgerStore(join(dataDir, 'owner-relayer.jsonl')),
      config,
    });
    void relayer.checkBalance();
    return relayer;
  })().catch((e) => {
    g.__ownerRelayer = undefined; // try again on the next request
    throw e;
  });
  return g.__ownerRelayer;
}

type ReceiptLog = { address: string; data: Hex; topics: readonly Hex[] };

/** The index in the account's AddOwner(index, owner) event for `publicKey`, from
 *  one receipt's logs; null when no such event is there. */
export function addOwnerIndex(logs: readonly ReceiptLog[], sender: Address, publicKey: Hex): number | null {
  for (const log of logs) {
    if (log.address.toLowerCase() !== sender.toLowerCase()) continue;
    try {
      const ev = decodeEventLog({ abi: ownerAbi, data: log.data, topics: log.topics as [Hex, ...Hex[]] });
      if (ev.eventName === 'AddOwner' && ev.args.owner.toLowerCase() === publicKey.toLowerCase()) return Number(ev.args.index);
    } catch {
      // another event
    }
  }
  return null;
}

/** EntryPoint v0.6 keeps `deposits` (StakeManager) at storage slot 0; the deposit is the low 112 bits. */
export function depositSlot(account: Address): Hex {
  return keccak256(encodeAbiParameters([{ type: 'address' }, { type: 'uint256' }], [account, 0n]));
}

// ---------------------------------------------------------------------------
// Bodies: exactly these fields, nothing else (no callData, gas or fees can come in)

const PREPARE_FIELDS = new Set(['action', 'publicKey', 'index', 'firstOwnerPublicKey']);
const SUBMIT_FIELDS = new Set(['userOpHash', 'signature']);

function onlyFields(body: Record<string, unknown>, allowed: Set<string>): string | null {
  for (const k of Object.keys(body)) if (!allowed.has(k)) return 'unexpected_field';
  return null;
}

export function parsePrepareBody(body: unknown): { action: OwnerAction; firstOwnerPublicKey: Hex | null } | { error: string } {
  if (!body || typeof body !== 'object' || Array.isArray(body)) return { error: 'bad_request' };
  const b = body as Record<string, unknown>;
  const extra = onlyFields(b, PREPARE_FIELDS);
  if (extra) return { error: extra };
  if (!isP256PublicKey(b.publicKey)) return { error: 'bad_public_key' };
  const publicKey = b.publicKey.toLowerCase() as Hex;
  let firstOwnerPublicKey: Hex | null = null;
  if (b.firstOwnerPublicKey !== undefined) {
    if (!isP256PublicKey(b.firstOwnerPublicKey)) return { error: 'bad_public_key' };
    firstOwnerPublicKey = b.firstOwnerPublicKey.toLowerCase() as Hex;
  }
  if (b.action === 'add') {
    if (b.index !== undefined) return { error: 'unexpected_field' };
    return { action: { action: 'add', publicKey }, firstOwnerPublicKey };
  }
  if (b.action === 'remove') {
    if (typeof b.index !== 'number' || !Number.isInteger(b.index) || b.index < 0 || b.index > MAX_OWNER_INDEX) return { error: 'bad_index' };
    return { action: { action: 'remove', index: b.index, publicKey }, firstOwnerPublicKey };
  }
  return { error: 'bad_action' };
}

export function parseSubmitBody(body: unknown): { userOpHash: Hex; signature: Hex } | { error: string } {
  if (!body || typeof body !== 'object' || Array.isArray(body)) return { error: 'bad_request' };
  const b = body as Record<string, unknown>;
  const extra = onlyFields(b, SUBMIT_FIELDS);
  if (extra) return { error: extra };
  if (typeof b.userOpHash !== 'string' || !/^0x[0-9a-fA-F]{64}$/.test(b.userOpHash)) return { error: 'bad_hash' };
  if (typeof b.signature !== 'string' || !/^0x[0-9a-fA-F]+$/.test(b.signature) || b.signature.length % 2 !== 0 || b.signature.length > MAX_SIGNATURE_HEX) {
    return { error: 'bad_signature' };
  }
  return { userOpHash: b.userOpHash.toLowerCase() as Hex, signature: b.signature as Hex };
}
