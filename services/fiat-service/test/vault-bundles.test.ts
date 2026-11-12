// D20 item 1 (BM1.2c) — ONE outbox, ONE lock and ONE in-flight set across the
// two Next bundles. Next compiles the routes and instrumentation.ts into
// separate bundles, each with its own copy of every module variable. Here each
// "bundle" is fiat-session-service + settlement-listener loaded afresh after
// vi.resetModules(); the scripted chain enters through a globalThis hook that
// buildBackend reads (never setFiatDepsForTest, whose module-level override
// only one copy would see).
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { HOST, MODEL, ScriptedChain } from './helpers/scripted-chain';
import { CLIENT, USER } from './helpers/vault-harness';

type ServiceModule = typeof import('../src/lib/fiat-session-service');
type ListenerModule = typeof import('../src/lib/settlement-listener');

const g = globalThis as typeof globalThis & { __fiatBackend?: unknown; __fiatVaultForTest?: unknown };
const ENV = {
  FIAT_ALLOWED_HOSTS: HOST,
  FIAT_BACKEND_AUTH_PRIVATE_KEY: `0x${'11'.repeat(32)}`,
  FIAT_RECLAIM_STRANDED: '1',
};
let dir: string;

async function loadBundle(): Promise<{ service: ServiceModule; listener: ListenerModule }> {
  vi.resetModules();
  return {
    service: await import('../src/lib/fiat-session-service'),
    listener: await import('../src/lib/settlement-listener'),
  };
}

// Real timers here: the journals are real files in a temp dir, and real I/O
// does not progress under a fake clock. Receipt polling is 20 ms.
const POLL = 20;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'fiat-bundles-'));
  process.env.FIAT_DATA_DIR = dir;
  Object.assign(process.env, ENV);
  g.__fiatBackend = undefined;
});
afterEach(() => {
  for (const k of [...Object.keys(ENV), 'FIAT_DATA_DIR']) delete process.env[k];
  g.__fiatBackend = undefined;
  g.__fiatVaultForTest = undefined;
  rmSync(dir, { recursive: true, force: true });
});

describe('one outbox across the route and instrumentation bundles', () => {
  it('a sweep (instrumentation bundle) and an open (route bundle) sign consecutive nonces through ONE outbox, lock and in-flight set', async () => {
    // mutation: keep the outbox (or lock, or in-flight set) in a module-level
    // variable → each bundle has its own copy → a sweep and an open sign the
    // same nonce behind a lagging RPC.
    const chain = new ScriptedChain();
    chain.pendingOverride = 0; // a lagging RPC: it never sees our own mempool
    chain.sessions.set(77n, 0);
    g.__fiatVaultForTest = { ...chain.vault(), onAlarm: () => {}, pollMs: POLL };

    const routes = await loadBundle();
    const instr = await loadBundle();
    expect(routes.service.getFiatDeps).not.toBe(instr.service.getFiatDeps); // two module copies

    const depsA = await routes.service.getFiatDeps();
    const depsB = await instr.service.getFiatDeps();
    expect(depsA.outbox).toBeDefined();
    expect(depsB.outbox).toBe(depsA.outbox);
    expect(depsB.outbox!.inFlight).toBe(depsA.outbox!.inFlight);

    // The instrumentation bundle builds its reclaimer at boot, before any open.
    const reclaimer = await instr.listener.makeProductionReclaimer();
    expect(reclaimer).toBeDefined();

    const token = await depsA.credentials.issue(USER);
    await depsA.ledger.purchase(USER, 5_000_000n, 'evt');
    const service = await routes.service.getFiatSessionService();
    const open = service.open({ credential: token, host: HOST, modelId: MODEL, depositMicro: 500_000n, clientAddress: CLIENT, idempotencyKey: 'k1' });
    await vi.waitFor(() => expect(chain.broadcasts).toHaveLength(1));
    const sweep = reclaimer!.trigger(77n);
    await vi.waitFor(() => expect(chain.broadcasts).toHaveLength(2));

    expect(chain.broadcasts.map((b) => `${b.decoded.kind}@${b.nonce}`)).toEqual(['create@0', 'sweep@1']);
    expect(chain.replaced).toEqual([]);
    chain.mineAll();
    expect(await open).toMatchObject({ status: 'ok' });
    await sweep;
  });

  it('R5 in the instrumentation bundle never touches a live open of the route bundle (one shared in-flight set)', async () => {
    // mutation: a module-level in-flight set → R5 reads its own empty copy and
    // binds the hold / completes the key while the request is live.
    const chain = new ScriptedChain();
    chain.autoMine = true;
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    let gated = false;
    chain.onReceipt = async () => {
      if (!gated) {
        gated = true;
        await gate;
      }
    };
    g.__fiatVaultForTest = { ...chain.vault(), onAlarm: () => {}, pollMs: POLL };
    const routes = await loadBundle();
    const instr = await loadBundle();
    const depsA = await routes.service.getFiatDeps();
    const token = await depsA.credentials.issue(USER);
    await depsA.ledger.purchase(USER, 5_000_000n, 'evt');
    const service = await routes.service.getFiatSessionService();
    const open = service.open({ credential: token, host: HOST, modelId: MODEL, depositMicro: 500_000n, clientAddress: CLIENT, idempotencyKey: 'k1' });
    await vi.waitFor(() => expect(gated).toBe(true)); // mined; the open's own read is stalled

    const depsB = await instr.service.getFiatDeps();
    await depsB.outbox!.resolve(); // the create mined
    await instr.listener.runProductionVaultReconcile(() => {});
    expect(depsB.ledger.hold('h0')?.state).toBe('held');
    expect((await depsB.idempotency!.lookup(USER, 'k1'))?.state).toBe('pending');
    release();
    expect(await open).toMatchObject({ status: 'ok' });
  });

  it("buildBackend wires the idempotency journal's alarms to the backend's alarm sink, not console.error", async () => {
    // mutation: open the IdempotencyStore without the sink → the corrupt-line
    // alarm goes to console.error only and never reaches the sink.
    const { writeFileSync } = await import('node:fs');
    const good = JSON.stringify({ t: 'reserve', userId: USER, key: 'k2', fp: 'fp', at: 1, ca: CLIENT });
    writeFileSync(join(dir, 'idempotency.jsonl'), `{"t":"reserve","userId":"0xb5\n${good}\n`);
    const alarms: string[] = [];
    g.__fiatVaultForTest = { ...new ScriptedChain().vault(), onAlarm: (m: string) => alarms.push(m), pollMs: POLL };
    const deps = await (await loadBundle()).service.getFiatDeps();
    expect(alarms.filter((m) => /idempotency journal line 1/.test(m))).toHaveLength(1);
    expect((await deps.idempotency!.lookup(USER, 'k2'))?.state).toBe('pending');
  });
});

