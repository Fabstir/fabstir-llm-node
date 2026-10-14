// A scripted Base chain for the vault outbox tests (D20, BM1.2c). It plays the
// RPC (nonces, balance, estimate, fees, L1 fee, broadcast, receipts, blocks),
// a mempool keyed by nonce, and the three contracts the vault touches (USDC
// allowance, the marketplace create and triggerSessionTimeout). Nothing here
// touches a network: signing is offline with a fixed test key.
import { Interface, Transaction, Wallet, type TransactionLike } from 'ethers';
import { ESCROW_INTERFACE } from '../../src/lib/escrow';
import type { ChainReceipt, VaultRpc } from '../../src/lib/vault-outbox';
import type { ProductionVault } from '../../src/lib/fiat-vault';

export const VAULT_KEY = `0x${'22'.repeat(32)}`;
export const MARKETPLACE = '0x00000000000000000000000000000000000000aa';
export const USDC = '0x00000000000000000000000000000000000000cc';
export const HOST = '0xabcd000000000000000000000000000000000001';
export const MODEL = `0x${'ab'.repeat(32)}`;

const ERC20 = new Interface([
  'function allowance(address owner, address spender) view returns (uint256)',
  'function approve(address spender, uint256 amount) returns (bool)',
]);
const TIMEOUT = new Interface(['function triggerSessionTimeout(uint256 jobId)']);

export type Decoded =
  | { kind: 'approve'; spender: string; amount: bigint }
  | { kind: 'create'; deposit: bigint; host: string; args: readonly unknown[] }
  | { kind: 'sweep'; jobId: bigint }
  | { kind: 'other' };

export function decodeCall(to: string | null | undefined, data: string): Decoded {
  const target = (to ?? '').toLowerCase();
  if (target === USDC.toLowerCase()) {
    const args = ERC20.decodeFunctionData('approve', data);
    // The spender, lowercased: tests assert it is the marketplace, and the
    // scripted USDC raises the allowance only for that spender.
    return { kind: 'approve', spender: String(args[0]).toLowerCase(), amount: BigInt(args[1]) };
  }
  if (target === MARKETPLACE.toLowerCase()) {
    const selector = data.slice(0, 10);
    if (selector === ESCROW_INTERFACE.getFunction('createSessionJobForModelWithToken')!.selector) {
      const args = ESCROW_INTERFACE.decodeFunctionData('createSessionJobForModelWithToken', data);
      return { kind: 'create', deposit: BigInt(args[3]), host: String(args[0]), args: [...args] };
    }
    const args = TIMEOUT.decodeFunctionData('triggerSessionTimeout', data);
    return { kind: 'sweep', jobId: BigInt(args[0]) };
  }
  return { kind: 'other' };
}

export class ScriptedChain implements VaultRpc {
  readonly wallet = new Wallet(VAULT_KEY);
  readonly vaultAddress = this.wallet.address;
  readonly chainId = 84532n;

  // chain state
  latest = 0;
  block = 100;
  ethBalance = 10n ** 18n;
  allowance = 100_000_000n;
  price = 904n;
  jobCounter = 1000n;
  readonly sessions = new Map<bigint, 0 | 1 | 2>();
  readonly mempool = new Map<number, string>();
  readonly mined = new Map<string, ChainReceipt>();

  // knobs
  gasLimit = 100_000n;
  maxFeePerGas = 1_000_000_000n;
  l1FeeWei = 0n;
  /** A lagging load-balanced RPC: a fixed pending/latest answer. */
  pendingOverride: number | undefined;
  latestOverride: number | undefined;
  /** Receipts a lagging replica cannot see yet. */
  readonly hiddenReceipts = new Set<string>();
  /** Every broadcast is refused (a full pool, an RPC fee floor). */
  rejectBroadcasts = false;
  /** Accept but lose: the broadcast never reaches the mempool. */
  loseBroadcast: ((d: Decoded) => boolean) | undefined;
  autoMine = false;
  /** A stale allowance READ (the create's estimate still sees `allowance`). */
  staleAllowanceRead: bigint | undefined;
  /** The process "dies" inside the broadcast: it never returns, nothing reaches the pool. */
  hangBroadcast = false;
  readonly estimates: Decoded[] = [];
  /** Called on every accepted broadcast, before auto-mining. */
  onBroadcast: ((raw: string, d: Decoded) => void | Promise<void>) | undefined;
  /** Called on every estimate (throw to revert, await to delay). */
  onEstimate: ((d: Decoded) => void | Promise<void>) | undefined;
  /** Called on every receipt read (await to delay). */
  onReceipt: ((hash: string) => void | Promise<void>) | undefined;

  // records
  readonly broadcasts: Array<{ raw: string; hash: string; nonce: number; decoded: Decoded; accepted: boolean }> = [];
  readonly replaced: Array<{ nonce: number; was: string; now: string }> = [];

  vault(): ProductionVault {
    return {
      rpc: this,
      signer: { address: this.vaultAddress, signTransaction: (tx: TransactionLike<string>) => this.wallet.signTransaction(tx) },
      chainId: this.chainId,
      vaultAddress: this.vaultAddress,
      marketplaceAddress: MARKETPLACE,
      usdcAddress: USDC,
      readAllowance: async () => this.staleAllowanceRead ?? this.allowance,
      modelPrice: async () => this.price,
    };
  }

  // ---- VaultRpc ----
  async getNonce(tag: 'latest' | 'pending'): Promise<number> {
    if (tag === 'latest') return this.latestOverride ?? this.latest;
    if (this.pendingOverride !== undefined) return this.pendingOverride;
    let n = this.latest;
    while (this.mempool.has(n)) n += 1;
    return n;
  }
  async getBalance(): Promise<bigint> {
    return this.ethBalance;
  }
  async estimateGas(tx: { to: string; data: string }): Promise<bigint> {
    const d = decodeCall(tx.to, tx.data);
    this.estimates.push(d);
    if (this.onEstimate) await this.onEstimate(d);
    if (d.kind === 'create' && this.allowance < d.deposit) {
      throw new Error('execution reverted: "ERC20: insufficient allowance"');
    }
    if (d.kind === 'sweep' && this.sessions.get(d.jobId) !== 0) {
      throw new Error('execution reverted: session not active');
    }
    return this.gasLimit;
  }
  async getFeeData() {
    return { maxFeePerGas: this.maxFeePerGas, maxPriorityFeePerGas: 1n };
  }
  async l1Fee(): Promise<bigint> {
    return this.l1FeeWei;
  }
  async broadcast(raw: string): Promise<void> {
    if (this.hangBroadcast) return new Promise<void>(() => {});
    const tx = Transaction.from(raw);
    const d = decodeCall(tx.to, tx.data);
    const record = { raw, hash: tx.hash!, nonce: tx.nonce, decoded: d, accepted: false };
    this.broadcasts.push(record);
    if (this.rejectBroadcasts) throw new Error('txpool is full');
    const cost = tx.gasLimit * (tx.maxFeePerGas ?? 0n) + this.l1FeeWei;
    if (this.ethBalance < cost) throw new Error('insufficient funds for gas * price + value');
    if (tx.nonce < this.latest) throw new Error('nonce too low');
    if (this.mined.has(tx.hash!)) throw new Error('already known');
    record.accepted = true;
    if (this.loseBroadcast?.(d)) return;
    const occupant = this.mempool.get(tx.nonce);
    if (occupant && occupant !== raw) this.replaced.push({ nonce: tx.nonce, was: occupant, now: raw });
    this.mempool.set(tx.nonce, raw);
    if (this.onBroadcast) await this.onBroadcast(raw, d);
    if (this.autoMine) this.mineAll();
  }
  async receipt(hash: string): Promise<ChainReceipt | null> {
    if (this.onReceipt) await this.onReceipt(hash);
    if (this.hiddenReceipts.has(hash)) return null;
    return this.mined.get(hash) ?? null;
  }
  async blockNumber(): Promise<number> {
    return this.block;
  }

  // ---- scripting ----
  /** Put a transaction the outbox does not own into the mempool at `nonce`. */
  foreign(nonce: number): string {
    const raw = `foreign-${nonce}`;
    this.mempool.set(nonce, raw);
    return raw;
  }

  /** Mine every contiguous mempool transaction from the latest nonce. */
  mineAll(): number {
    let count = 0;
    while (this.mempool.has(this.latest)) {
      this.mineOne();
      count += 1;
    }
    return count;
  }

  mineOne(): ChainReceipt | undefined {
    const raw = this.mempool.get(this.latest);
    if (raw === undefined) return undefined;
    this.mempool.delete(this.latest);
    this.latest += 1;
    this.block += 1;
    if (raw.startsWith('foreign-')) return undefined;
    const tx = Transaction.from(raw);
    const d = decodeCall(tx.to, tx.data);
    let receipt: ChainReceipt = { status: 1, blockNumber: this.block, logs: [] };
    if (d.kind === 'approve') {
      // `allowance` is the vault's allowance FOR THE MARKETPLACE: an approve of
      // any other spender mines but leaves it untouched.
      if (d.spender === MARKETPLACE.toLowerCase()) this.allowance = d.amount;
    } else if (d.kind === 'create') {
      if (this.allowance < d.deposit) {
        receipt = { status: 0, blockNumber: this.block, logs: [] };
      } else {
        this.allowance -= d.deposit;
        this.jobCounter += 1n;
        const jobId = this.jobCounter;
        this.sessions.set(jobId, 0);
        const log = ESCROW_INTERFACE.encodeEventLog('SessionJobCreatedForModel', [
          jobId,
          this.vaultAddress,
          d.host,
          MODEL,
          d.deposit,
        ]);
        receipt = { status: 1, blockNumber: this.block, logs: [{ topics: log.topics, data: log.data }] };
      }
    } else if (d.kind === 'sweep') {
      if (this.sessions.get(d.jobId) === 0) this.sessions.set(d.jobId, 2);
      else receipt = { status: 0, blockNumber: this.block, logs: [] };
    }
    this.ethBalance -= tx.gasLimit * 1_000_000n;
    this.mined.set(tx.hash!, receipt);
    return receipt;
  }

  /** Drop a mempool transaction (evicted). */
  evict(nonce: number): void {
    this.mempool.delete(nonce);
  }

  jobIdOf(hash: string): bigint | undefined {
    const r = this.mined.get(hash);
    const log = r?.logs[0];
    return log ? BigInt(log.topics[1]!) : undefined;
  }

  /** A SessionStateReader over the scripted sessions. */
  sessionReader() {
    return {
      session: async (jobId: bigint) => {
        const s = this.sessions.get(jobId);
        if (s === undefined) return undefined;
        return { ended: s !== 0, refundedToUser: 0n };
      },
    };
  }

  broadcastsOf(kind: Decoded['kind']) {
    return this.broadcasts.filter((b) => b.decoded.kind === kind);
  }
}
