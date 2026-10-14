// D20 item 1 (BM1.2c) — power cuts. The REAL JsonlLedgerStore and outbox
// journal (built by the same `fiatJournalStores` as buildBackend) run over an
// in-memory node:fs/promises that records writes and FileHandle.sync() and, on
// a simulated cut, drops every line written since that file's last sync. A
// stand-in store would bypass the very fsync these tests guard.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('node:fs/promises', async () => {
  const m = await import('./helpers/fake-fs');
  return { ...m.fakeFsModule, default: m.fakeFsModule };
});

import { fakeDisk } from './helpers/fake-fs';
import { fiatJournalStores } from '../src/lib/fiat-session-service';
import { JsonlLedgerStore } from '../src/lib/ledger';
import { CLIENT, USER, makeHarness } from './helpers/vault-harness';

const MIN = 60_000;
const DATA = '/srv/fiat-data';

beforeEach(() => {
  fakeDisk.reset();
  vi.useFakeTimers({ now: new Date('2026-10-08T00:00:00Z') });
});
afterEach(() => {
  vi.useRealTimers();
});

describe('power cuts (real journals over a recording filesystem)', () => {
  it('the outbox append is fsynced before its broadcast: a cut at the broadcast, with the chain mining it, keeps the entry', async () => {
    // mutation: no fsync on the outbox journal → the entry is lost while its
    // transaction mines (its nonce reused, its session invisible).
    const h = await makeHarness({ stores: fiatJournalStores(DATA) });
    h.chain.onBroadcast = (_raw, d) => {
      if (d.kind !== 'create') return;
      h.chain.mineAll();
      fakeDisk.powerCut();
      h.crash();
    };
    h.open({ idempotencyKey: 'k1' }).catch(() => undefined); // the dead process's open may throw
    await vi.advanceTimersByTimeAsync(0);
    const h2 = await h.reboot({ stores: fiatJournalStores(DATA) });
    expect(h2.outbox.all()).toEqual([expect.objectContaining({ kind: 'create', nonce: 0, link: 'h0' })]);
    // The next send knows nonce 0 is taken even if the RPC lags.
    h2.chain.pendingOverride = 0;
    void h2.open({ idempotencyKey: 'k2' });
    await vi.advanceTimersByTimeAsync(0);
    expect(h2.chain.broadcastsOf('create').map((b) => b.nonce)).toEqual([0, 1]);
  });

  it('a cut after the hold append and before the outbox entry: at boot no entry, nothing mined, and the hold released after 10 min with its key freed', async () => {
    // mutation: release at once → a live open's hold (and key) released.
    const h = await makeHarness({ stores: fiatJournalStores(DATA) });
    h.chain.onEstimate = (d) => {
      if (d.kind !== 'create') return;
      fakeDisk.powerCut();
      h.crash();
      throw new Error('power lost');
    };
    h.open({ idempotencyKey: 'k1' }).catch(() => undefined); // the dead process's open may throw
    await vi.advanceTimersByTimeAsync(0);
    const h2 = await h.reboot({ stores: fiatJournalStores(DATA) });
    h2.chain.onEstimate = undefined;
    expect(h2.outbox.all()).toHaveLength(0);
    expect(h2.chain.broadcasts).toHaveLength(0);
    expect(h2.ledger.hold('h0')?.state).toBe('held');
    expect((await h2.idempotency.lookup(USER, 'k1'))?.state).toBe('pending');
    await vi.advanceTimersByTimeAsync(MIN);
    await h2.tick();
    expect(h2.ledger.hold('h0')?.state).toBe('held');
    await vi.advanceTimersByTimeAsync(10 * MIN);
    await h2.tick();
    expect(h2.ledger.hold('h0')?.state).toBe('released');
    expect(await h2.idempotency.lookup(USER, 'k1')).toBeNull();
    expect(h2.ledger.availableMicro(USER)).toBe(10_000_000n);
  });

  it('a cut after the fsynced outbox append and broadcast, with the chain mining it: at boot the hold is found and BOUND and its key completed', async () => {
    // mutation: no ledger fsync → the hold line is lost, leaving an unlinked
    // create (an orphan nobody is credited for).
    const h = await makeHarness({ stores: fiatJournalStores(DATA) });
    h.chain.onBroadcast = (_raw, d) => {
      if (d.kind !== 'create') return;
      h.chain.mineAll();
      fakeDisk.powerCut();
      h.crash();
    };
    h.open({ idempotencyKey: 'k1' }).catch(() => undefined); // the dead process's open may throw
    await vi.advanceTimersByTimeAsync(0);
    const h2 = await h.reboot({ stores: fiatJournalStores(DATA) });
    await h2.tick();
    expect(h2.ledger.hold('h0')?.state).toBe('bound');
    expect((await h2.idempotency.lookup(USER, 'k1'))?.state).toBe('done');
    expect(h2.ledger.openVaultOrphans()).toHaveLength(0);
    expect(h2.ledger.availableMicro(USER)).toBe(9_500_000n);
  });

  it('a torn outbox tail (a cut mid-append) never swallows the next fsynced line: the create appended after it survives the next boot', async () => {
    // mutation: no tail repair on open → the next send line is glued onto the
    // torn fragment, the merged line is dropped as "torn" at the next boot: a
    // broadcast create is lost (its hold released, its key freed, no orphan).
    const h = await makeHarness({ stores: fiatJournalStores(DATA) });
    fakeDisk.tearTail(`${DATA}/vault-outbox.jsonl`, '{"t":"send","kind":"approve","nonce":0,"ha');
    const h2 = await h.reboot({ stores: fiatJournalStores(DATA) });
    expect(fakeDisk.syncedText(`${DATA}/vault-outbox.jsonl`)).toBe(''); // repaired, durably
    h2.open({ idempotencyKey: 'k1' }).catch(() => undefined);
    await vi.advanceTimersByTimeAsync(0);
    expect(h2.chain.broadcastsOf('create')).toHaveLength(1);
    fakeDisk.powerCut();
    h2.crash();
    const h3 = await h2.reboot({ stores: fiatJournalStores(DATA) });
    expect(h3.outbox.all()).toEqual([expect.objectContaining({ kind: 'create', nonce: 0, link: 'h0' })]);
    await vi.advanceTimersByTimeAsync(11 * MIN);
    await h3.tick();
    // The create is unmined but OWNED: the hold stays held and the key pending.
    expect(h3.ledger.hold('h0')?.state).toBe('held');
    expect((await h3.idempotency.lookup(USER, 'k1'))?.state).toBe('pending');
  });

  it("a torn idempotency tail never swallows the next `reserve`: the key is still pending after the next boot", async () => {
    // mutation: no tail repair on open → the reserve line is merged into the
    // torn fragment and skipped at boot: the key is free while its hold lives.
    const h = await makeHarness({ stores: fiatJournalStores(DATA) });
    fakeDisk.tearTail(`${DATA}/idempotency.jsonl`, '{"t":"reserve","userId":"0xb5e8');
    const h2 = await h.reboot({ stores: fiatJournalStores(DATA) });
    await h2.idempotency.reserve(USER, 'k9', 'fp', CLIENT);
    fakeDisk.powerCut();
    h2.crash();
    const h3 = await h2.reboot({ stores: fiatJournalStores(DATA) });
    expect(await h3.idempotency.lookup(USER, 'k9')).toEqual({ state: 'pending', fingerprint: 'fp' });
  });

  it('concurrent appends after a failed partial append: the second repair never truncates the first line away', async () => {
    // mutation: no per-store append queue → both see the torn tail, A repairs
    // and syncs its line, B's stale repair truncates it: A's line is lost.
    const path = `${DATA}/vault-outbox.jsonl`;
    const store = new JsonlLedgerStore(path);
    await store.load();
    await store.append('{"n":0}');
    fakeDisk.failNextAppend = '{"n":"to';
    await expect(store.append('{"n":"torn"}')).rejects.toThrow(/EIO/);
    let truncates = 0;
    let aDone!: Promise<void>;
    fakeDisk.beforeOp = (op) => {
      // B's repair (the second truncate) waits until A's line is written and synced.
      if (op === 'truncate' && ++truncates === 2) return aDone;
    };
    aDone = store.append('{"n":"A"}');
    const b = store.append('{"n":"B"}');
    await Promise.all([aDone, b]);
    fakeDisk.beforeOp = undefined;
    expect(fakeDisk.syncedText(path)).toBe('{"n":0}\n{"n":"A"}\n{"n":"B"}\n');
    expect(await new JsonlLedgerStore(path).load()).toEqual(['{"n":0}', '{"n":"A"}', '{"n":"B"}']);
  });

  it('a line can never be glued onto another append still in progress', async () => {
    // mutation: no per-store append queue → B lands between A's two halves:
    // two corrupt lines, both lost at the next boot.
    const path = `${DATA}/idempotency.jsonl`;
    const store = new JsonlLedgerStore(path);
    await store.load();
    fakeDisk.chunkedAppends = true;
    let bDone!: Promise<void>;
    let first = true;
    fakeDisk.beforeOp = (op) => {
      if (op !== 'append-rest' || !first) return;
      first = false;
      // A is half-written: give B every chance to write now (bounded, so a
      // queued B cannot deadlock A).
      return Promise.race([bDone, new Promise<void>((r) => setTimeout(r, 1_000))]);
    };
    const a = store.append('{"t":"reserve","key":"A"}');
    bDone = store.append('{"t":"reserve","key":"B"}');
    await vi.advanceTimersByTimeAsync(1_000);
    await Promise.all([a, bDone]);
    fakeDisk.chunkedAppends = false;
    fakeDisk.beforeOp = undefined;
    expect(await new JsonlLedgerStore(path).load()).toEqual(['{"t":"reserve","key":"A"}', '{"t":"reserve","key":"B"}']);
  });

  it('the append queue stays alive after a rejected append', async () => {
    const path = `${DATA}/ledger.jsonl`;
    const store = new JsonlLedgerStore(path);
    await store.load();
    fakeDisk.failNextAppend = '{"x';
    await expect(store.append('{"x":1}')).rejects.toThrow(/EIO/);
    await store.append('{"x":2}');
    expect(await new JsonlLedgerStore(path).load()).toEqual(['{"x":2}']);
  });
});

