// GP1b — the card exchange-rate feed (plan section 4, SPEC-CARD-FX-RULE v1.3).
// One instance per process, on globalThis (instrumentation and every route
// bundle share it, A5). It holds the three records of `card-fx-store.ts`,
// fetches Kraken at :03/:18/:33/:48 (R11, with up to 10 retries a minute apart
// when the candle that closed 3 minutes earlier is not final yet) and the ECB
// file at boot and at :01 past every hour (R7), and answers ONE question:
// `servedNow(t)`, the rate served at instant t. When that answer differs from
// the last `served.jsonl` line, the new line is fsynced BEFORE the answer is
// returned, so every rate a buyer is shown is on record for R14(b).
//
// The rule itself is `fx-rule.ts`, v2's module copied unchanged: every number
// comes from its constants, none from the environment.
import { join } from 'node:path';
import {
  CEILING_MICRO,
  ECB_FETCH_MINUTE,
  EFFECT_DELAY_S,
  CANDLE_S,
  FETCH_MINUTES,
  RETRY_EVERY_S,
  RETRY_TIMES,
  WINDOW_AFTER_S,
  WINDOW_BEFORE_S,
  freshness,
  pinMicroOf,
  rateInForce,
  served,
  servedMax,
  type Candle,
  type EcbEntry,
  type ServedLine,
} from './fx-rule';
import { FxStore, diskFxFiles } from './card-fx-store';
import { fetchEcb, fetchKraken, type EcbResult, type Fetcher, type KrakenResult } from './card-fx-sources';

const DAY_S = 86_400;
const HOUR_S = 3600;
/** The ECB file must have been fetched today by this time of day, UTC. */
export const ECB_LATE_S = 2 * HOUR_S;
/** The tick that records changes no request triggered (a candle taking effect). */
export const TICK_S = 60;
/** How often the driver looks at the schedule. */
const DRIVER_MS = 5_000;

/** What is served at an instant: a rate, or none and why. */
export type FeedAnswer =
  | { kind: 'rate'; fxMicro: number; source: 'rule' | 'pin'; capped: boolean }
  | { kind: 'none'; reason: 'no-rate' | 'floor' | 'dead' | 'misconfigured' };

// ---- settings ---------------------------------------------------------------

export type CardFxSettings =
  | { ok: true; pin?: number; ntfyUrl?: string }
  | { ok: false; problem: string; ntfyUrl?: string };

/** R8/R10 settings. `FIAT_CARD_FX_MAX_USD_PER_GBP` must read exactly `1.40`
 *  (the rule's ceiling, kept only to be read and checked); the optional pin
 *  must be two decimals from 1.00 to 1.40. Anything else is `ok: false`: the
 *  feed then serves no rate and raises an urgent alert, and the rest of the
 *  service runs as before (A5). */
export function cardFxSettingsFromEnv(env: Record<string, string | undefined> = process.env): CardFxSettings {
  const ntfyUrl = env.FIAT_FX_URGENT_NTFY_URL ? env.FIAT_FX_URGENT_NTFY_URL : undefined;
  const extra = ntfyUrl ? { ntfyUrl } : {};
  const max = env.FIAT_CARD_FX_MAX_USD_PER_GBP;
  if (max !== (CEILING_MICRO / 1_000_000).toFixed(2)) {
    return { ok: false, problem: `FIAT_CARD_FX_MAX_USD_PER_GBP must be 1.40 (the rule's ceiling), got ${max === undefined ? 'nothing' : JSON.stringify(max)}`, ...extra };
  }
  const rawPin = env.FIAT_CARD_FX_PIN_USD_PER_GBP;
  if (rawPin === undefined || rawPin === '') return { ok: true, ...extra };
  const pin = pinMicroOf(rawPin);
  if (pin === undefined) {
    return { ok: false, problem: `FIAT_CARD_FX_PIN_USD_PER_GBP must be two decimals from 1.00 to 1.40, got ${JSON.stringify(rawPin)}`, ...extra };
  }
  return { ok: true, pin, ...extra };
}

// ---- alerts (R16, A16) --------------------------------------------------------

export interface FxAlerts {
  /** A notice to the review channel: a `[fiat-fx] ALERT:` journal line. */
  alert(message: string): void;
  /** Pages: a `[fiat-fx] URGENT:` line and, when configured, a push. */
  urgent(message: string): void;
  log(message: string): void;
}

export function productionFxAlerts(ntfyUrl?: string, poster: typeof fetch = fetch): FxAlerts {
  return {
    alert: (m) => console.error(`[fiat-fx] ALERT: ${m}`),
    urgent: (m) => {
      console.error(`[fiat-fx] URGENT: ${m}`);
      if (ntfyUrl) {
        // Never throws and never waits: the journal line is the record.
        poster(ntfyUrl, { method: 'POST', body: `[fiat-fx] URGENT: ${m}`, signal: AbortSignal.timeout(10_000) }).catch((e) =>
          console.error(`[fiat-fx] ALERT: the urgent push failed (${e instanceof Error ? e.message : String(e)})`)
        );
      }
    },
    log: (m) => console.log(`[fiat-fx] ${m}`),
  };
}

// ---- the schedule (R7, R11) ----------------------------------------------------

/** The next Kraken slot strictly after `t`: :03, :18, :33 or :48, UTC. */
export function nextKrakenSlot(t: number): number {
  const hour = Math.floor(t / HOUR_S) * HOUR_S;
  for (let h = hour; ; h += HOUR_S) {
    for (const m of FETCH_MINUTES) if (h + m * 60 > t) return h + m * 60;
  }
}

/** The next ECB fetch strictly after `t`: :01 past an hour, UTC. */
export function nextEcbSlot(t: number): number {
  const hour = Math.floor(t / HOUR_S) * HOUR_S + ECB_FETCH_MINUTE * 60;
  return hour > t ? hour : hour + HOUR_S;
}

/** The candle that closed `EFFECT_DELAY_S` before a slot: its open time. */
export const candleBeforeSlot = (slot: number): number => slot - EFFECT_DELAY_S - CANDLE_S;

// ---- the feed ----------------------------------------------------------------

export interface CardFxDeps {
  store: FxStore;
  fetchKraken: () => Promise<KrakenResult>;
  fetchEcb: () => Promise<EcbResult>;
  alerts: FxAlerts;
  settings: CardFxSettings;
  /** UTC seconds (may be fractional). */
  now?: () => number;
}

interface Retry {
  candle: number;
  left: number;
  at: number;
}

export class CardFxFeed {
  private candles = new Map<number, Candle>();
  private ecbEntries = new Map<string, EcbEntry>();
  private ecbFetchedAt: number[] = [];
  private lines: ServedLine[] = [];
  private chain: Promise<unknown> = Promise.resolve();
  private active = new Set<string>();
  private nextKrakenAt = 0;
  private nextEcbAt = 0;
  private nextTickAt = 0;
  private retry: Retry | undefined;
  private driver: ReturnType<typeof setInterval> | undefined;
  private ticking = false;

  private constructor(private readonly d: CardFxDeps) {}

  private nowS(): number {
    return (this.d.now ?? (() => Date.now() / 1000))();
  }

  /** Re-read the store (R6's recompute after a restart), then fetch both
   *  sources once and record what is served. */
  static async open(d: CardFxDeps, opts: { fetchAtBoot?: boolean } = {}): Promise<CardFxFeed> {
    const feed = new CardFxFeed(d);
    const held = await d.store.load();
    for (const c of held.candles) feed.candles.set(c.time, c);
    for (const e of held.ecb.entries) feed.ecbEntries.set(e.date, e);
    feed.ecbFetchedAt = [...held.ecb.fetchedAt];
    feed.lines = held.served;
    for (const [name, n] of Object.entries(held.skipped)) {
      if (n > 0) d.alerts.alert(`${name}: ${n} unreadable line(s) skipped on load (a torn write)`);
    }
    const first = feed.lines.find((l) => l.fxMicro !== null);
    d.alerts.log(
      first
        ? `first served under the rule at ${new Date(first.at * 1000).toISOString()} (the first line of served.jsonl with a rate); ${feed.lines.length} served line(s) held`
        : 'no rate has been served under the rule yet (served.jsonl holds none)'
    );
    if (!d.settings.ok) d.alerts.urgent(`card exchange rate misconfigured, no rate is served: ${d.settings.problem}`);
    const t = feed.nowS();
    feed.nextKrakenAt = nextKrakenSlot(t);
    feed.nextEcbAt = nextEcbSlot(t);
    feed.nextTickAt = t + TICK_S;
    if (opts.fetchAtBoot !== false) {
      await feed.fetchEcbNow(t);
      await feed.fetchKrakenNow();
    }
    await feed.servedNow(t);
    return feed;
  }

  private serial<T>(f: () => Promise<T>): Promise<T> {
    const run = this.chain.then(f, f);
    this.chain = run.catch(() => undefined);
    return run;
  }

  /** The held records, for tests and the README's checks. */
  heldCandles(): Candle[] {
    return [...this.candles.values()].sort((a, b) => a.time - b.time);
  }
  servedLines(): readonly ServedLine[] {
    return this.lines;
  }

  async fetchKrakenNow(): Promise<boolean> {
    const r = await this.d.fetchKraken();
    if (!r.ok) {
      this.d.alerts.log(`Kraken fetch failed: ${r.reason}`);
      return false;
    }
    await this.serial(async () => {
      const fresh = r.candles.filter((c) => {
        const held = this.candles.get(c.time);
        return !held || held.close !== c.close || held.count !== c.count;
      });
      await this.d.store.appendCandles(fresh);
      for (const c of fresh) this.candles.set(c.time, c); // R1: later values replace held ones
    });
    return true;
  }

  async fetchEcbNow(t = this.nowS()): Promise<boolean> {
    const r = await this.d.fetchEcb();
    if (!r.ok) {
      this.d.alerts.log(`ECB fetch failed: ${r.reason}`);
      return false;
    }
    await this.serial(async () => {
      const fresh = r.entries.filter((e) => {
        const held = this.ecbEntries.get(e.date);
        return !held || held.usd !== e.usd || held.gbp !== e.gbp;
      });
      await this.d.store.appendEcbFetch(fresh, t);
      for (const e of fresh) this.ecbEntries.set(e.date, e);
      this.ecbFetchedAt.push(t);
    });
    return true;
  }

  private evaluate(t: number): { answer: FeedAnswer; conditions: Map<string, { urgent: boolean; message: string }> } {
    const conditions = new Map<string, { urgent: boolean; message: string }>();
    if (!this.d.settings.ok) {
      conditions.set('misconfigured', { urgent: true, message: `no card rate is served: ${this.d.settings.problem}` });
      return { answer: { kind: 'none', reason: 'misconfigured' }, conditions };
    }
    const candles = [...this.candles.values()];
    const ecb = { entries: [...this.ecbEntries.values()], fetchedAt: this.ecbFetchedAt };
    const pin = this.d.settings.pin;
    const inForce = rateInForce(candles, ecb, t);
    const fresh = freshness(candles, t);
    const s = served({ inForce: inForce.fxMicro, freshness: fresh, ...(pin !== undefined ? { pin } : {}) });
    const day = new Date(Math.floor(t / DAY_S) * DAY_S * 1000).toISOString().slice(0, 10);
    if (pin !== undefined) {
      // R10: once a day while set (the key changes with the date).
      conditions.set(`pin:${day}`, { urgent: false, message: `the card rate is pinned at ${(pin / 1e6).toFixed(2)}; unset FIAT_CARD_FX_PIN_USD_PER_GBP when the feed is healthy` });
    } else {
      if (fresh === 'stale') conditions.set('stale', { urgent: false, message: 'no traded Kraken candle for over 1 hour (R9): the rate goes at 2 hours' });
      for (const time of inForce.refused) {
        conditions.set(`guard:${time}`, { urgent: false, message: `candle ${new Date(time * 1000).toISOString()} refused by the ECB check (R7)` });
      }
      for (const time of inForce.skipped) {
        conditions.set(`ecb-skip:${time}`, { urgent: false, message: `candle ${new Date(time * 1000).toISOString()} used without the ECB check: no ECB reference within 5 days (R7)` });
      }
      const midnight = Math.floor(t / DAY_S) * DAY_S;
      const lastEcb = this.ecbFetchedAt.reduce((a, f) => (f > a ? f : a), -Infinity);
      if (t - midnight >= ECB_LATE_S && lastEcb < midnight) {
        conditions.set(`ecb-late:${day}`, { urgent: false, message: `the ECB file has not been fetched since 00:00 UTC (R7): today's candles are not judged, so the rate holds and cannot fall; a pin is the remedy` });
      }
      if (s.kind === 'rate' && s.capped) conditions.set('ceiling', { urgent: false, message: `the rate in force is above the 1.40 ceiling and is cut to it (R8): buyers pay more than the market` });
    }
    if (s.kind === 'none') {
      const why = { dead: 'no traded Kraken candle for over 2 hours (R9)', floor: 'the rate in force is below the 1.00 floor (R8)', 'no-rate': 'no rate in force (R6: no usable fix within 7 days)' }[s.reason];
      conditions.set(`none:${s.reason}`, { urgent: true, message: `no card rate is served, card top-ups are refused: ${why}` });
    }
    return { answer: s, conditions };
  }

  private raise(conditions: Map<string, { urgent: boolean; message: string }>): void {
    for (const [key, c] of conditions) {
      if (this.active.has(key)) continue;
      this.active.add(key);
      (c.urgent ? this.d.alerts.urgent : this.d.alerts.alert)(c.message);
    }
    for (const key of [...this.active]) {
      if (conditions.has(key)) continue;
      this.active.delete(key);
      this.d.alerts.log(`cleared: ${key}`);
    }
  }

  /**
   * The rate served at `t` (R5–R10). If it differs from the last served line,
   * the new line is on disk before this resolves. `/packs`, `/purchase` and
   * the 60-second tick all call it.
   */
  servedNow(t = this.nowS()): Promise<FeedAnswer> {
    return this.serial(async () => {
      const { answer, conditions } = this.evaluate(t);
      this.raise(conditions);
      const fx = answer.kind === 'rate' ? answer.fxMicro : null;
      const last = this.lines[this.lines.length - 1];
      if (!last || last.fxMicro !== fx) {
        const line = { at: t, fxMicro: fx };
        await this.d.store.appendServed(line);
        this.lines.push(line);
        this.d.alerts.log(
          `served rate ${last ? (last.fxMicro === null ? 'none' : (last.fxMicro / 1e6).toFixed(2)) : '(first line)'} -> ` +
            `${fx === null ? `none (${answer.kind === 'none' ? answer.reason : ''})` : `${(fx / 1e6).toFixed(2)}${answer.kind === 'rate' && answer.source === 'pin' ? ' (pin)' : ''}`}`
        );
      }
      return answer;
    });
  }

  /** R14(b): the highest rate served from `created − 300` to `created + 60`;
   *  undefined when none was (an empty window refuses, A5). */
  windowMax(createdS: number): number | undefined {
    return servedMax(this.lines, createdS - WINDOW_BEFORE_S, createdS + WINDOW_AFTER_S);
  }

  urgent(message: string): void {
    this.d.alerts.urgent(message);
  }

  /**
   * One pass of the schedule at `t`: the Kraken slot and its retries (R11),
   * the ECB hour (R7), and the 60-second served tick. Never throws.
   */
  async tick(t = this.nowS()): Promise<void> {
    if (this.ticking) return;
    this.ticking = true;
    try {
      if (t >= this.nextKrakenAt) {
        const slot = this.nextKrakenAt;
        this.nextKrakenAt = nextKrakenSlot(t);
        await this.fetchKrakenNow();
        const candle = candleBeforeSlot(slot);
        this.retry = this.candles.has(candle) ? undefined : { candle, left: RETRY_TIMES, at: t + RETRY_EVERY_S };
      } else if (this.retry && t >= this.retry.at) {
        const r = this.retry;
        await this.fetchKrakenNow();
        r.left -= 1;
        this.retry = this.candles.has(r.candle) || r.left === 0 ? undefined : { ...r, at: t + RETRY_EVERY_S };
      }
      if (t >= this.nextEcbAt) {
        this.nextEcbAt = nextEcbSlot(t);
        await this.fetchEcbNow(t);
      }
      if (t >= this.nextTickAt) {
        this.nextTickAt = t + TICK_S;
        await this.servedNow(t);
      }
    } catch (e) {
      this.d.alerts.alert(`the feed's tick failed: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      this.ticking = false;
    }
  }

  /** Retries left for the candle of the last slot (tests). */
  pendingRetry(): Readonly<Retry> | undefined {
    return this.retry;
  }

  start(): void {
    if (this.driver) return;
    this.driver = setInterval(() => void this.tick(), DRIVER_MS);
    this.driver.unref?.();
  }

  stop(): void {
    if (this.driver) clearInterval(this.driver);
    this.driver = undefined;
  }
}

// ---- the one instance (A5) ------------------------------------------------------

const g = globalThis as typeof globalThis & { __fiatCardFx?: Promise<CardFxFeed> };

/** The process's feed, built and started on first use (instrumentation at
 *  boot; any route that needs it first). A failed build is forgotten so the
 *  next call tries again. */
export function getCardFx(): Promise<CardFxFeed> {
  if (!g.__fiatCardFx) {
    const settings = cardFxSettingsFromEnv();
    const dataDir = process.env.FIAT_DATA_DIR ?? './data/fiat';
    // no-store: a feed rebuilt inside a route handler would otherwise inherit
    // the request's context and Next's data cache could serve an old answer.
    const fetcher: Fetcher = (url, init) => fetch(url, { ...init, cache: 'no-store' });
    const build = CardFxFeed.open({
      store: new FxStore(diskFxFiles(join(dataDir, 'fx'))),
      fetchKraken: () => fetchKraken(fetcher),
      fetchEcb: () => fetchEcb(fetcher),
      alerts: productionFxAlerts(settings.ntfyUrl),
      settings,
    }).then((feed) => {
      feed.start();
      return feed;
    });
    g.__fiatCardFx = build;
    build.catch((e) => {
      if (g.__fiatCardFx === build) g.__fiatCardFx = undefined;
      console.error(`[fiat-fx] URGENT: the card exchange-rate feed did not start: ${e instanceof Error ? e.message : String(e)}`);
    });
  }
  return g.__fiatCardFx;
}

/** Tests: put a feed in place of the env-built one (undefined forgets it). */
export function setCardFxForTest(feed: CardFxFeed | undefined): void {
  if (!feed) {
    void g.__fiatCardFx?.then((f) => f.stop(), () => undefined);
    g.__fiatCardFx = undefined;
    return;
  }
  g.__fiatCardFx = Promise.resolve(feed);
}
