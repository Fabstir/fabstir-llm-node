// GP1b — the card exchange-rate feed: sources, store, feed (plan section 4).
// No network: the fetchers and the clock are injected; the disk is memory,
// except one test of the real append-and-fsync files in a temp directory.
import { describe, it, expect } from 'vitest';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { parseKraken, parseEcb, boundedGet, KRAKEN_URL, ECB_URL, type Fetcher } from '../src/lib/card-fx-sources';
import { FxStore, diskFxFiles, type FxFiles, type FxFileName } from '../src/lib/card-fx-store';
import {
  CardFxFeed, cardFxSettingsFromEnv, nextKrakenSlot, nextEcbSlot, candleBeforeSlot,
  type CardFxSettings, type FxAlerts,
} from '../src/lib/card-fx';
import type { Candle, EcbEntry } from '../src/lib/fx-rule';

const at = (iso: string): number => Date.parse(iso) / 1000;
const DAY = 86_400;
const D = at('2026-11-14T08:00:00Z');
const c = (time: number, close: string, count = 14): Candle => ({ time, close, count });
const ECB13 = { usd: '1.1052', gbp: '0.83559' };
const dateOf = (t: number): string => new Date(t * 1000).toISOString().slice(0, 10);
const ecbDays = (from: number, n: number, rates = ECB13): EcbEntry[] =>
  Array.from({ length: n }, (_, i) => ({ date: dateOf(from + i * DAY), ...rates }));

// ---- sources -----------------------------------------------------------------

describe('parseKraken (R1)', () => {
  const row = (time: number, close: string, count = 5) => [time, '0.75', '0.76', '0.74', close, '0.75', '100.0', count];
  it('finds the candle array under any key, keeps only final candles (time <= last)', () => {
    const r = parseKraken({ error: [], result: { USDCGBP_ODD_NAME: [row(100, '0.7571'), row(1000, '0.7600'), row(1900, '0.7700')], last: 1000 } });
    expect(r).toEqual({ ok: true, last: 1000, candles: [c(100, '0.7571', 5), c(1000, '0.7600', 5)] });
  });
  it('a non-empty error is a failed fetch', () => {
    expect(parseKraken({ error: ['EQuery:Unknown asset pair'], result: { X: [], last: 1 } }).ok).toBe(false);
  });
  it('no candle array, or two keys besides last, is a failed fetch', () => {
    expect(parseKraken({ error: [], result: { last: 1 } }).ok).toBe(false);
    expect(parseKraken({ error: [], result: { A: [], B: [], last: 1 } }).ok).toBe(false);
    expect(parseKraken({ error: [], result: { A: 'x', last: 1 } }).ok).toBe(false);
    expect(parseKraken('nope').ok).toBe(false);
  });
  it('keeps the raw close string (the rule judges it) and drops misshapen rows', () => {
    const r = parseKraken({ error: [], result: { P: [row(100, '1e-1'), [100, 'x'], row(200, '0.75', -1 as never)], last: 1000 } });
    expect(r).toEqual({ ok: true, last: 1000, candles: [c(100, '1e-1', 5)] });
  });
});

describe('parseEcb (R7)', () => {
  const xml = (q: string) =>
    `<?xml version="1.0"?><gesmes:Envelope><Cube>` +
    `<Cube time=${q}2026-11-13${q}><Cube currency=${q}USD${q} rate=${q}1.1052${q}/><Cube currency=${q}JPY${q} rate=${q}170.1${q}/><Cube currency=${q}GBP${q} rate=${q}0.83559${q}/></Cube>` +
    `<Cube time=${q}2026-11-12${q}><Cube currency=${q}USD${q} rate=${q}1.1000${q}/></Cube>` +
    `</Cube></gesmes:Envelope>`;
  it('reads single and double quotes; a date without both USD and GBP is left out', () => {
    for (const q of ["'", '"']) {
      expect(parseEcb(xml(q))).toEqual({ ok: true, entries: [{ date: '2026-11-13', usd: '1.1052', gbp: '0.83559' }] });
    }
  });
  it('an empty or unreadable file is a failed fetch', () => {
    expect(parseEcb('').ok).toBe(false);
    expect(parseEcb('<html>maintenance</html>').ok).toBe(false);
  });
});

describe('boundedGet', () => {
  const res = (body: string, init: ResponseInit = {}) => new Response(body, init);
  it('follows a redirect on the same host only', async () => {
    const seen: string[] = [];
    const same: Fetcher = async (url) => {
      seen.push(url);
      return seen.length === 1 ? res('', { status: 302, headers: { location: '/0/public/OHLC?x=1' } }) : res('ok');
    };
    expect(await boundedGet(same, KRAKEN_URL)).toBe('ok');
    const other: Fetcher = async () => res('', { status: 302, headers: { location: 'https://evil.example/x' } });
    await expect(boundedGet(other, ECB_URL)).rejects.toThrow(/another host/);
  });
  it('refuses a body over 1 MB and a non-2xx answer', async () => {
    await expect(boundedGet(async () => res('x'.repeat(1_000_001)), ECB_URL)).rejects.toThrow(/over/);
    await expect(boundedGet(async () => res('no', { status: 500 }), ECB_URL)).rejects.toThrow(/500/);
  });
  it('passes a timeout signal and manual redirects', async () => {
    let init: Parameters<Fetcher>[1] | undefined;
    await boundedGet(async (_u, i) => ((init = i), res('ok')), ECB_URL);
    expect(init?.redirect).toBe('manual');
    expect(init?.signal).toBeInstanceOf(AbortSignal);
  });
});

// ---- store -------------------------------------------------------------------

function memFiles(initial: Partial<Record<FxFileName, string>> = {}) {
  const data: Record<string, string> = { ...initial };
  const files: FxFiles & { data: Record<string, string>; failNext?: Error; gate?: Promise<void> } = {
    data,
    async read(name) {
      return data[name];
    },
    async append(name, text) {
      if (files.gate) await files.gate;
      if (files.failNext) {
        const e = files.failNext;
        files.failNext = undefined;
        throw e;
      }
      data[name] = (data[name] ?? '') + text;
    },
  };
  return files;
}

describe('FxStore', () => {
  it('skips a torn last line, and the next append starts a fresh line', async () => {
    const files = memFiles({ 'served.jsonl': '{"at":1,"fxMicro":1310000}\n{"at":2,"fxM' });
    const store = new FxStore(files);
    const held = await store.load();
    expect(held.served).toEqual([{ at: 1, fxMicro: 1310000 }]);
    expect(held.skipped['served.jsonl']).toBe(1);
    await store.appendServed({ at: 3, fxMicro: null });
    expect((await new FxStore(files).load()).served).toEqual([{ at: 1, fxMicro: 1310000 }, { at: 3, fxMicro: null }]);
  });
  it('an append that fails part-written starts the next one on a fresh line', async () => {
    const files = memFiles();
    const store = new FxStore(files);
    await store.appendServed({ at: 1, fxMicro: 1310000 });
    const inner = files.append.bind(files);
    let cut = true;
    files.append = async (name, text) => {
      if (cut) { cut = false; await inner(name, text.slice(0, 7)); throw new Error('ENOSPC'); }
      return inner(name, text);
    };
    await expect(store.appendServed({ at: 2, fxMicro: 1320000 })).rejects.toThrow('ENOSPC');
    await store.appendServed({ at: 3, fxMicro: 1300000 });
    expect((await new FxStore(files).load()).served).toEqual([{ at: 1, fxMicro: 1310000 }, { at: 3, fxMicro: 1300000 }]);
  });
  it('a later line for the same candle replaces the held one (R1)', async () => {
    const files = memFiles();
    const store = new FxStore(files);
    await store.appendCandles([c(100, '0.7571'), c(1000, '0.7600')]);
    await store.appendCandles([c(100, '0.7580', 3)]);
    expect((await new FxStore(files).load()).candles).toEqual([c(100, '0.7580', 3), c(1000, '0.7600')]);
  });
  it('keeps every ECB entry and the instant of every successful fetch', async () => {
    const files = memFiles();
    const store = new FxStore(files);
    await store.appendEcbFetch([{ date: '2026-11-13', ...ECB13 }], 500);
    await store.appendEcbFetch([], 4100);
    expect((await new FxStore(files).load()).ecb).toEqual({ entries: [{ date: '2026-11-13', ...ECB13 }], fetchedAt: [500, 4100] });
  });
  it('the disk files: appended, read back after a reopen', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'gp-fx-'));
    try {
      const store = new FxStore(diskFxFiles(join(dir, 'fx')));
      await store.appendServed({ at: 1, fxMicro: 1_320_000 });
      writeFileSync(join(dir, 'fx', 'served.jsonl'), readFileSync(join(dir, 'fx', 'served.jsonl'), 'utf8') + '{"at":2,');
      const again = new FxStore(diskFxFiles(join(dir, 'fx')));
      expect((await again.load()).served).toEqual([{ at: 1, fxMicro: 1_320_000 }]);
      await again.appendServed({ at: 3, fxMicro: 1_310_000 });
      expect((await new FxStore(diskFxFiles(join(dir, 'fx'))).load()).served).toEqual([
        { at: 1, fxMicro: 1_320_000 },
        { at: 3, fxMicro: 1_310_000 },
      ]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

// ---- the feed ----------------------------------------------------------------

interface Harness {
  feed: CardFxFeed;
  files: ReturnType<typeof memFiles>;
  market: Candle[];
  ecb: { entries: EcbEntry[]; ok: boolean };
  clock: { t: number };
  alerts: string[];
  urgent: string[];
  logs: string[];
  calls: { kraken: number; ecb: number };
}

async function harness(o: {
  t: number;
  market?: Candle[];
  ecbEntries?: EcbEntry[];
  ecbOk?: boolean;
  settings?: CardFxSettings;
  files?: ReturnType<typeof memFiles>;
  krakenOk?: boolean;
}): Promise<Harness> {
  const files = o.files ?? memFiles();
  const market = o.market ?? [];
  const ecb = { entries: o.ecbEntries ?? ecbDays(D - 20 * DAY, 22), ok: o.ecbOk ?? true };
  const clock = { t: o.t };
  const alerts: string[] = [], urgent: string[] = [], logs: string[] = [];
  const calls = { kraken: 0, ecb: 0 };
  const sink: FxAlerts = { alert: (m) => alerts.push(m), urgent: (m) => urgent.push(m), log: (m) => logs.push(m) };
  const feed = await CardFxFeed.open({
    store: new FxStore(files),
    fetchKraken: async () => {
      calls.kraken += 1;
      if (o.krakenOk === false) return { ok: false, reason: 'down' };
      return { ok: true, last: Math.max(0, ...market.map((m) => m.time)), candles: [...market] };
    },
    fetchEcb: async () => {
      calls.ecb += 1;
      return ecb.ok ? { ok: true, entries: [...ecb.entries] } : { ok: false, reason: 'down' };
    },
    alerts: sink,
    settings: o.settings ?? { ok: true },
    now: () => clock.t,
  });
  return { feed, files, market, ecb, clock, alerts, urgent, logs, calls };
}

const rateOf = async (feed: CardFxFeed, t: number) => {
  const a = await feed.servedNow(t);
  return a.kind === 'rate' ? a.fxMicro : a.reason;
};

describe('the feed: the R5 day table end to end', () => {
  it('1.31 at 08:03, 1.30 at 08:18, holds 1.30 at 08:33, 1.31 the next day at 08:03', async () => {
    const market = [c(D - 900, '0.7571'), c(D, '0.7600'), c(D + 900, '0.7560')];
    const h = await harness({ t: D + 180, market });
    expect(await rateOf(h.feed, D + 180)).toBe(1_310_000);
    expect(await rateOf(h.feed, D + 1080)).toBe(1_300_000);
    expect(await rateOf(h.feed, D + 1980)).toBe(1_300_000);
    // the next day's fix, plus fresh candles up to it
    for (let s = D + 1800; s <= D + DAY - 900; s += 900) h.market.push(c(s, s === D + DAY - 900 ? '0.7560' : '0.7600'));
    h.clock.t = D + DAY + 180;
    await h.feed.fetchEcbNow(D + DAY + 60);
    await h.feed.fetchKrakenNow();
    expect(await rateOf(h.feed, D + DAY + 180)).toBe(1_310_000);
    expect(h.feed.servedLines().map((l) => l.fxMicro)).toEqual([1_310_000, 1_300_000, 1_310_000]);
  });

  it('a restart from the store reaches the same rate with both sources down', async () => {
    const market = [c(D - 900, '0.7571'), c(D, '0.7600')];
    const h = await harness({ t: D + 1080, market });
    expect(await rateOf(h.feed, D + 1100)).toBe(1_300_000);
    const again = await harness({ t: D + 1200, files: h.files, krakenOk: false, ecbOk: false });
    expect(await rateOf(again.feed, D + 1200)).toBe(1_300_000);
    expect(again.feed.servedLines()).toEqual(h.feed.servedLines()); // no change, so no new line
    expect(again.logs.some((l) => l.includes('first served under the rule at'))).toBe(true);
  });
});

describe('the feed: R6, a missed fix candle', () => {
  it('serves the previous day (1.32), falls to 1.30 when 08:00 arrives, and stays 1.30 once the fix arrives', async () => {
    const prev = [c(D - DAY - 900, '0.75'), c(D - 1800, '0.75')]; // the 13 Nov fix (1.32) and a recent traded candle
    const ecbFetched = [{ date: '2026-11-12', ...ECB13 }, { date: '2026-11-13', ...ECB13 }];
    const h = await harness({ t: D + 180, market: [...prev], ecbEntries: ecbFetched });
    expect(await rateOf(h.feed, D + 180)).toBe(1_320_000);
    h.market.push(c(D, '0.7600'));
    await h.feed.fetchKrakenNow();
    expect(await rateOf(h.feed, D + 1080)).toBe(1_300_000);
    h.market.push(c(D - 900, '0.7571'));
    await h.feed.fetchKrakenNow();
    expect(await rateOf(h.feed, D + 1100)).toBe(1_300_000);
  });
});

describe('the feed: ECB not fetched today', () => {
  it('today\'s candles are not judged: the rate holds and cannot fall, with an alert', async () => {
    const market = [c(D - DAY - 900, '0.75'), c(D - 1800, '0.75')];
    const h = await harness({ t: D - DAY + 120, market, ecbEntries: ecbDays(D - 20 * DAY, 20) });
    // boot fetched the ECB file on 13 Nov; today it is down
    h.ecb.ok = false;
    h.market.push(c(D - 900, '0.7571'), c(D, '0.7600'));
    await h.feed.fetchEcbNow(D + 60);
    await h.feed.fetchKrakenNow();
    expect(await rateOf(h.feed, D + 1080)).toBe(1_320_000);
    expect(h.alerts.some((a) => a.includes('has not been fetched since 00:00 UTC'))).toBe(true);
    // the file comes back: the 08:00 candle is judged and the rate falls
    h.ecb.ok = true;
    await h.feed.fetchEcbNow(D + 1100);
    expect(await rateOf(h.feed, D + 1110)).toBe(1_300_000);
  });
});

describe('the feed: freshness, pin, ceiling, floor, settings', () => {
  const day = [c(D - 900, '0.7571'), c(D, '0.7600')];
  it('stale after 1 hour (alert, once), dead after 2 hours (no rate, urgent)', async () => {
    const h = await harness({ t: D + 1080, market: day });
    const newestClose = D + 900;
    expect(await rateOf(h.feed, newestClose + 3600)).toBe(1_300_000);
    expect(await rateOf(h.feed, newestClose + 3601)).toBe(1_300_000);
    await h.feed.servedNow(newestClose + 3700);
    expect(h.alerts.filter((a) => a.includes('over 1 hour'))).toHaveLength(1);
    expect(await rateOf(h.feed, newestClose + 7201)).toBe('dead');
    expect(h.urgent.filter((u) => u.includes('no card rate is served'))).toHaveLength(1);
    expect(h.feed.servedLines().at(-1)).toEqual({ at: newestClose + 7201, fxMicro: null });
  });
  it('a count-0 candle does not keep the rate alive', async () => {
    const h = await harness({ t: D + 1080, market: [...day, c(D + 900, '0.7600', 0), c(D + 1800, '0.7600', 0)] });
    expect(await rateOf(h.feed, D + 900 + 7201)).toBe('dead');
  });
  it('a pin replaces the rule, even with no candles, and alerts once a day', async () => {
    const h = await harness({ t: D, settings: { ok: true, pin: 1_350_000 } });
    expect(await rateOf(h.feed, D)).toBe(1_350_000);
    await h.feed.servedNow(D + 3600);
    expect(h.alerts.filter((a) => a.includes('pinned at 1.35'))).toHaveLength(1);
    await h.feed.servedNow(D + DAY);
    expect(h.alerts.filter((a) => a.includes('pinned at 1.35'))).toHaveLength(2);
  });
  it('a rate above 1.40 is cut to 1.40, with an alert', async () => {
    const ecb = ecbDays(D - 20 * DAY, 22, { usd: '1.17', gbp: '0.83' }); // ref 1.4096
    const h = await harness({ t: D + 180, market: [c(D - 900, '0.70')], ecbEntries: ecb });
    expect(await rateOf(h.feed, D + 180)).toBe(1_400_000);
    expect(h.alerts.some((a) => a.includes('ceiling'))).toBe(true);
  });
  it('a rate below 1.00 is no rate, urgent', async () => {
    const ecb = ecbDays(D - 20 * DAY, 22, { usd: '1.0', gbp: '1.0' });
    const h = await harness({ t: D + 180, market: [c(D - 900, '1.0050')], ecbEntries: ecb });
    expect(await rateOf(h.feed, D + 180)).toBe('floor');
    expect(h.urgent.some((u) => u.includes('floor'))).toBe(true);
  });
  it('a misconfigured feed serves nothing and pages, naming the setting', async () => {
    const settings = cardFxSettingsFromEnv({ FIAT_CARD_FX_MAX_USD_PER_GBP: '1.45' });
    const h = await harness({ t: D + 180, market: day, settings });
    expect(await rateOf(h.feed, D + 180)).toBe('misconfigured');
    expect(h.urgent.some((u) => u.includes('FIAT_CARD_FX_MAX_USD_PER_GBP'))).toBe(true);
  });
  it('the settings: 1.40 exactly, an optional well-formed pin', () => {
    expect(cardFxSettingsFromEnv({ FIAT_CARD_FX_MAX_USD_PER_GBP: '1.40' })).toEqual({ ok: true });
    for (const bad of [undefined, '1.4', '1.400', '1.39', ' 1.40']) {
      expect(cardFxSettingsFromEnv({ FIAT_CARD_FX_MAX_USD_PER_GBP: bad }).ok).toBe(false);
    }
    expect(cardFxSettingsFromEnv({ FIAT_CARD_FX_MAX_USD_PER_GBP: '1.40', FIAT_CARD_FX_PIN_USD_PER_GBP: '1.33' })).toEqual({ ok: true, pin: 1_330_000 });
    for (const bad of ['1.3', '1.41', '0.99', 'x']) {
      expect(cardFxSettingsFromEnv({ FIAT_CARD_FX_MAX_USD_PER_GBP: '1.40', FIAT_CARD_FX_PIN_USD_PER_GBP: bad }).ok).toBe(false);
    }
    expect(cardFxSettingsFromEnv({ FIAT_CARD_FX_MAX_USD_PER_GBP: '1.40', FIAT_FX_URGENT_NTFY_URL: 'https://ntfy.example/x' })).toEqual({
      ok: true,
      ntfyUrl: 'https://ntfy.example/x',
    });
  });
  it('an alert clears when its condition ends and is raised again if it recurs (A16)', async () => {
    const h = await harness({ t: D + 1080, market: day });
    await h.feed.servedNow(D + 900 + 3700);
    h.market.push(c(D + 900, '0.7600'));
    await h.feed.fetchKrakenNow();
    await h.feed.servedNow(D + 1800 + 100);
    expect(h.logs.some((l) => l === 'cleared: stale')).toBe(true);
    await h.feed.servedNow(D + 1800 + 900 + 3700);
    expect(h.alerts.filter((a) => a.includes('over 1 hour'))).toHaveLength(2);
  });
});

describe('the feed: a served change is on record before it is returned', () => {
  it('servedNow waits for the served.jsonl append', async () => {
    const h = await harness({ t: D + 180, market: [c(D - 900, '0.7571'), c(D, '0.7600')] });
    let open!: () => void;
    h.files.gate = new Promise((r) => (open = r));
    let answered = false;
    const p = h.feed.servedNow(D + 1080).then((a) => ((answered = true), a));
    await new Promise((r) => setTimeout(r, 20));
    expect(answered).toBe(false);
    open();
    expect(await p).toMatchObject({ kind: 'rate', fxMicro: 1_300_000 });
    expect(h.files.data['served.jsonl']).toContain('"fxMicro":1300000');
  });
  it('a failed append returns no rate (the caller refuses the sale)', async () => {
    const h = await harness({ t: D + 180, market: [c(D - 900, '0.7571'), c(D, '0.7600')] });
    h.files.failNext = new Error('EIO');
    await expect(h.feed.servedNow(D + 1080)).rejects.toThrow('EIO');
    expect(h.feed.servedLines().at(-1)?.fxMicro).toBe(1_310_000);
  });
  it('windowMax is R14(b) over the served lines', async () => {
    const h = await harness({ t: D + 180, market: [c(D - 900, '0.7571'), c(D, '0.7600')] });
    await h.feed.servedNow(D + 1080);
    expect(h.feed.windowMax(D + 1080 + 299)).toBe(1_310_000);
    expect(h.feed.windowMax(D + 1080 + 300)).toBe(1_300_000);
    expect(h.feed.windowMax(D + 180 - 61)).toBeUndefined();
  });
});

describe('the schedule (R7, R11)', () => {
  it('Kraken at :03, :18, :33, :48; the ECB file at :01', () => {
    expect(nextKrakenSlot(D)).toBe(D + 180);
    expect(nextKrakenSlot(D + 180)).toBe(D + 1080);
    expect(nextKrakenSlot(D + 2880 + 30)).toBe(D + 3600 + 180);
    expect(nextEcbSlot(D + 30)).toBe(D + 60);
    expect(nextEcbSlot(D + 60)).toBe(D + 3600 + 60);
    expect(candleBeforeSlot(D + 180)).toBe(D - 900);
  });
  it('the candle not final at the slot: retried every 60 s, at most 10 times', async () => {
    const h = await harness({ t: D + 150, market: [c(D - 1800, '0.7571')] });
    expect(h.calls.kraken).toBe(1); // boot
    for (let t = D + 180; t < D + 1080; t += 30) await h.feed.tick(t);
    expect(h.calls.kraken).toBe(1 + 1 + 10);
    expect(h.feed.pendingRetry()).toBeUndefined();
  });
  it('the retries stop as soon as the candle is final', async () => {
    const h = await harness({ t: D + 150, market: [c(D - 1800, '0.7571')] });
    await h.feed.tick(D + 180);
    await h.feed.tick(D + 240);
    await h.feed.tick(D + 300);
    h.market.push(c(D - 900, '0.7571'));
    await h.feed.tick(D + 360);
    for (let t = D + 390; t < D + 1080; t += 30) await h.feed.tick(t);
    expect(h.calls.kraken).toBe(1 + 1 + 3);
  });
  it('the ECB file is fetched at boot and at :01 past every hour; the served tick runs every 60 s', async () => {
    const h = await harness({ t: D + 30, market: [c(D - 900, '0.7571')] });
    expect(h.calls.ecb).toBe(1);
    await h.feed.tick(D + 60);
    await h.feed.tick(D + 3600);
    await h.feed.tick(D + 3660);
    expect(h.calls.ecb).toBe(3);
    // the 08:03 effect is recorded by the tick, no request needed
    await h.feed.tick(D + 180);
    expect(h.feed.servedLines().at(-1)).toMatchObject({ fxMicro: 1_310_000 });
  });
});
