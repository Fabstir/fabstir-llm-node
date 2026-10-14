// The real D20 stack over a scripted chain: ledger + idempotency + credentials
// + the ONE outbox + the outbox-backed vault chain + openFiatSession + R5 and
// the settlement tick. `reboot()` re-opens every journal from the same stores,
// exactly as a process restart would.
import { CreditsLedger, MemoryLedgerStore, type LedgerStore } from '../../src/lib/ledger';
import { IdempotencyStore } from '../../src/lib/idempotency';
import { FiatCredentials } from '../../src/lib/fiat-credentials';
import { makeGatekeeper, type GatekeeperConfig } from '../../src/lib/gatekeeper';
import { makeVaultChain } from '../../src/lib/fiat-vault';
import { openFiatSession, type FiatSessionDeps, type FiatSessionRequest } from '../../src/lib/fiat-session-service';
import { VaultOutbox } from '../../src/lib/vault-outbox';
import { reconcileVault } from '../../src/lib/fiat-reconcile';
import { makeReclaimer, startSettlementListener } from '../../src/lib/settlement-listener';
import { HOST, MARKETPLACE, MODEL, ScriptedChain } from './scripted-chain';

export const USER = '0xb5e859a491607d8970bbd4d9ddd317d5c3357a80';
export const CLIENT = '0x1a84ef2650c4299659f522c1961c6be4bc22cb14';

export const CONFIG: GatekeeperConfig = {
  allowedHosts: [HOST],
  maxDepositPerSessionMicro: 2_000_000n,
  maxDailySpendMicro: 100_000_000n,
  maxOpensPerMinute: 1000,
  perKind: { training: { maxDepositPerSessionMicro: 2_000_000n, maxDailySpendMicro: 100_000_000n } },
  trainingModelIds: [`0x${'7e'.repeat(32)}`],
};
export const TRAINING_MODEL = `0x${'7e'.repeat(32)}`;

export interface Stores {
  ledger: LedgerStore;
  idempotency: LedgerStore;
  outbox: LedgerStore;
  credentials: LedgerStore;
}

export function memoryStores(): Stores {
  return {
    ledger: new MemoryLedgerStore(),
    idempotency: new MemoryLedgerStore(),
    outbox: new MemoryLedgerStore(),
    credentials: new MemoryLedgerStore(),
  };
}

/** A process crash: the dead process's later appends never reach the disk. */
class CrashableStore implements LedgerStore {
  dead = false;
  constructor(private readonly inner: LedgerStore) {}
  load() {
    return this.inner.load();
  }
  async append(line: string) {
    if (this.dead) throw new Error('process crashed');
    return this.inner.append(line);
  }
}

function crashable(stores: Stores): { stores: Stores; crash(): void } {
  const wrapped = {
    ledger: new CrashableStore(stores.ledger),
    idempotency: new CrashableStore(stores.idempotency),
    outbox: new CrashableStore(stores.outbox),
    credentials: new CrashableStore(stores.credentials),
  };
  return {
    stores: wrapped,
    crash: () => {
      for (const s of Object.values(wrapped)) s.dead = true;
    },
  };
}

export interface Harness {
  chain: ScriptedChain;
  stores: Stores;
  ledger: CreditsLedger;
  idempotency: IdempotencyStore;
  credentials: FiatCredentials;
  outbox: VaultOutbox;
  deps: FiatSessionDeps;
  token: string;
  alarms: string[];
  request(overrides?: Partial<FiatSessionRequest>): FiatSessionRequest;
  open(overrides?: Partial<FiatSessionRequest>): ReturnType<typeof openFiatSession>;
  reconcile(): Promise<void>;
  /** One settlement tick (outbox resolution + boot wait + R5 + state sweep + reclaim). */
  tick(): Promise<void>;
  reboot(overrides?: Partial<HarnessOptions>): Promise<Harness>;
  /** Kill this process: none of its later appends persist. */
  crash(): void;
}

export interface HarnessOptions {
  chain?: ScriptedChain;
  stores?: Stores;
  balanceMicro?: bigint;
  migrated?: boolean;
  pollMs?: number;
  reclaimAfterMs?: number;
  trainingReclaimAfterMs?: number;
  passesReclaimAfterMs?: number;
  tickTimeoutMs?: number;
  solvency?: { holdings(): Promise<bigint> };
  /** Skip the boot nonce check (most tests start with latest = pending). */
  keepBootWait?: boolean;
  token?: string;
}

export async function makeHarness(opts: HarnessOptions = {}): Promise<Harness> {
  const chain = opts.chain ?? new ScriptedChain();
  const disk = opts.stores ?? memoryStores();
  const process = crashable(disk);
  const stores = process.stores;
  const firstBoot = opts.token === undefined;
  const alarms: string[] = [];
  const onAlarm = (m: string) => alarms.push(m);
  const ledger = await CreditsLedger.open(stores.ledger);
  if (opts.migrated !== false) await ledger.writeMigrationMarker();
  const idempotency = await IdempotencyStore.open(stores.idempotency);
  const credentials = await FiatCredentials.open(stores.credentials);
  const token = opts.token ?? (await credentials.issue(USER));
  if (firstBoot && opts.balanceMicro !== 0n) {
    await ledger.purchase(USER, opts.balanceMicro ?? 10_000_000n, 'seed-event');
  }
  const outbox = await VaultOutbox.open(stores.outbox, {
    rpc: chain,
    signer: chain.vault().signer,
    chainId: chain.chainId,
    onAlarm,
    pollMs: opts.pollMs ?? 1_000,
  });
  if (!opts.keepBootWait) await outbox.checkBootNonce();
  const vault = chain.vault();
  const deps: FiatSessionDeps = {
    ledger,
    credentials,
    gatekeeper: makeGatekeeper(CONFIG),
    chain: makeVaultChain({
      outbox,
      vaultAddress: vault.vaultAddress,
      marketplaceAddress: vault.marketplaceAddress,
      usdcAddress: vault.usdcAddress,
      readAllowance: vault.readAllowance,
      modelPrice: vault.modelPrice,
    }),
    signAuth: (sessionId, clientAddress) => ({
      scheme: 'fc1-session-auth-v1',
      signature: `0xsig-${sessionId}-${clientAddress}`,
      clientAddress,
    }),
    idempotency,
    outbox,
    config: CONFIG,
    marketplaceAddress: MARKETPLACE,
  };
  const reclaimDelays = {
    reclaimAfterMs: opts.reclaimAfterMs ?? 2 * 60 * 60 * 1000,
    reclaimAfterMsByKind: {
      training: opts.trainingReclaimAfterMs ?? 6 * 60 * 60 * 1000,
      passes: opts.passesReclaimAfterMs ?? 6 * 60 * 60 * 1000,
    },
  };
  const listener = startSettlementListener({
    ledger,
    source: { latestBlock: async () => 10, query: async () => [] },
    cursor: { load: async () => undefined, save: async () => {} },
    fromBlock: 1,
    onAlarm,
    manual: true,
    vault: { outbox, idempotency },
    stateSweep: chain.sessionReader(),
    reclaim: makeReclaimer(outbox, MARKETPLACE),
    ...reclaimDelays,
    tickTimeoutMs: opts.tickTimeoutMs ?? 120_000,
    ...(opts.solvency ? { solvency: opts.solvency } : {}),
  });
  const request = (overrides: Partial<FiatSessionRequest> = {}): FiatSessionRequest => ({
    credential: token,
    host: HOST,
    modelId: MODEL,
    depositMicro: 500_000n,
    clientAddress: CLIENT,
    ...overrides,
  });
  const harness: Harness = {
    chain,
    stores,
    ledger,
    idempotency,
    credentials,
    outbox,
    deps,
    token,
    alarms,
    request,
    open: (overrides) => openFiatSession(deps, request(overrides)),
    reconcile: () => reconcileVault({ ledger, idempotency, outbox, onAlarm, ...reclaimDelays }),
    tick: () => listener.tick(),
    reboot: (overrides = {}) => makeHarness({ ...opts, ...overrides, chain, stores: disk, token }),
    crash: () => process.crash(),
  };
  return harness;
}

/** Let pending promise chains run (no timers advanced). */
export async function flush(times = 20): Promise<void> {
  for (let i = 0; i < times; i++) await Promise.resolve();
}

/** The outbox-backed vault chain alone, over a scripted chain that mines on
 *  broadcast — for the pre-D20 vault tests (shape, price, allowance rules). */
export async function scriptedVaultChain(
  opts: { chain?: ScriptedChain; modelPrice?: (host: string, modelId: string, token: string) => Promise<bigint> } = {}
) {
  const chain = opts.chain ?? new ScriptedChain();
  chain.autoMine = true;
  const outbox = await VaultOutbox.open(new MemoryLedgerStore(), {
    rpc: chain,
    signer: chain.vault().signer,
    chainId: chain.chainId,
    onAlarm: () => {},
    pollMs: 10,
  });
  const v = chain.vault();
  const vaultChain = makeVaultChain({
    outbox,
    vaultAddress: v.vaultAddress,
    marketplaceAddress: v.marketplaceAddress,
    usdcAddress: v.usdcAddress,
    readAllowance: v.readAllowance,
    modelPrice: opts.modelPrice ?? v.modelPrice,
  });
  /** Every create sent, decoded: [host, modelId, token, deposit, price, maxDuration, proofInterval, proofTimeoutWindow]. */
  const creates = () =>
    chain.broadcastsOf('create').map((b) =>
      (b.decoded as { args: readonly unknown[] }).args.map((a) => (typeof a === 'string' ? a.toLowerCase() : a))
    );
  const approvals = () => chain.broadcastsOf('approve').map((b) => (b.decoded as { amount: bigint }).amount);
  return { chain, outbox, vaultChain, creates, approvals };
}
