// The card exchange rule, SPEC-CARD-FX-RULE v1.3 R1–R10 + R14(b)/(c) (IMPLEMENTATION-CARD-FX-FEED DC-2). NO imports:
// Platformless AI copies this file unchanged. No I/O and no clock: every instant comes in as UTC seconds, and every
// money figure is an integer worked out in bigint. fxMicro = US dollars per pound × 10^6, always a whole cent here.

// ── The rule's numbers (DC-3). Never env: changing one changes the SPEC, so a new SPEC version comes first.
export const M_BPS = 50, DAY_START_UTC_HOUR = 8, EFFECT_DELAY_S = 180, CANDLE_S = 900;                    // R2, R3, R4, R1
export const GUARD_BPS = 300, GUARD_MAX_GAP_DAYS = 5, ECB_FETCH_MINUTE = 1;                                // R7
export const PREV_DAY_MAX = 7, ALERT_STALE_S = 3600, DEAD_S = 7200;                                        // R6, R9
export const FETCH_MINUTES: readonly number[] = [3, 18, 33, 48], RETRY_EVERY_S = 60, RETRY_TIMES = 10;     // R11
export const CEILING_MICRO = 1_400_000, FLOOR_MICRO = 1_000_000;                                           // R8
export const WINDOW_BEFORE_S = 300, WINDOW_AFTER_S = 60, MAX_SESSION_S = 1920, SESSION_S = 1860;           // R14(b), R14(c), R15

/** A held FINAL Kraken candle (R1): `time` its OPEN time; `close` the raw index-4 string; `count` index 7. */
export interface Candle { time: number; close: string; count: number }
/** One ECB date (R7): `date` 'YYYY-MM-DD'; `usd` and `gbp` the raw per-euro strings. */
export interface EcbEntry { date: string; usd: string; gbp: string }
/** Every ECB entry seen (a later entry for a date wins) and the instant of every successful fetch of the file. */
export interface EcbHeld { entries: readonly EcbEntry[]; fetchedAt: readonly number[] }
/** R7 for one candle. 'fail' also covers a close R2's grammar refuses; 'unjudged' = no successful ECB fetch
 *  since 00:00 UTC of its date yet: not usable now, judged once one succeeds. 'skipped' is usable, with an alert. */
export type Verdict = 'pass' | 'fail' | 'skipped' | 'unjudged';
/** R5/R6 at an instant. `fixUsable` false = R6 applied. The lists name candle times (refused → cardFxGuard,
 *  skipped → cardFxEcb) over every rate day the answer looked at. */
export interface InForce { fxMicro: number | undefined; fixUsable: boolean; refused: number[]; skipped: number[]; unjudged: number[] }
export type Freshness = 'fresh' | 'stale' | 'dead';
export type Served = { kind: 'rate'; fxMicro: number; source: 'rule' | 'pin'; capped: boolean } | { kind: 'none'; reason: 'no-rate' | 'floor' | 'dead' };
/** One served.jsonl line (DC-4): from `at` (UTC seconds, may be fractional) the product served `fxMicro`, or nothing. */
export interface ServedLine { at: number; fxMicro: number | null }

const DAY_S = 86_400, BPS = 10_000n;
const mod = (a: number, b: number): number => ((a % b) + b) % b;
const midnight = (t: number): number => t - mod(t, DAY_S);

const DECIMAL = /^[0-9]+(\.[0-9]{1,12})?$/;
/** R2's exact decimal: `c` its digits as an integer, `d` its places. Undefined unless the grammar matches and it is > 0. */
export function parseDecimal(s: string): { c: bigint; d: number } | undefined {
  if (typeof s !== 'string' || !DECIMAL.test(s)) return undefined;
  const [whole, frac = ''] = s.split('.');
  const c = BigInt(whole + frac);
  return c > 0n ? { c, d: frac.length } : undefined;
}

/** R1: a candle is final if and only if its open time is at or before `result.last`. */
export const isFinal = (time: number, last: number): boolean => time <= last;

const hundredthsOf = (p: { c: bigint; d: number }, marginBps: number): bigint => ((BPS - BigInt(marginBps)) * 10n ** BigInt(p.d)) / (p.c * 100n);
/** R2: floor((10000 − M) × 10^d / (C × 100)); the rate is this ÷ 100. Undefined for a refused close. */
export function candleHundredths(close: string, marginBps = M_BPS): number | undefined {
  if (!Number.isInteger(marginBps) || marginBps < 0 || marginBps >= 10_000) throw new RangeError(`marginBps must be an integer in [0, 10000): got ${marginBps}`);
  const p = parseDecimal(close);
  return p && Number(hundredthsOf(p, marginBps));
}
function refOf(usd: string, gbp: string): bigint | undefined {
  const u = parseDecimal(usd), g = parseDecimal(gbp);
  return u && g ? (u.c * 10n ** BigInt(g.d + 6)) / (g.c * 10n ** BigInt(u.d)) : undefined;
}
/** R7: floor(USD × 10^6 / GBP), both per euro as exact decimals. */
export const ecbRefMicro = (usd: string, gbp: string): number | undefined => { const r = refOf(usd, gbp); return r === undefined ? r : Number(r); };

const DATE = /^(\d{4})-(\d{2})-(\d{2})$/;
function dayNumber(date: string): number | undefined {
  const m = DATE.exec(date);
  if (!m) return undefined;
  const ms = Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  return new Date(ms).toISOString().slice(0, 10) === date ? ms / 86_400_000 : undefined;
}
interface Judge { refs: Map<number, bigint>; lastFetch: number }
function judgeOf(ecb: EcbHeld): Judge {
  const refs = new Map<number, bigint>();
  for (const e of ecb.entries) {
    const day = dayNumber(e.date), ref = refOf(e.usd, e.gbp);
    if (day !== undefined && ref !== undefined) refs.set(day, ref); // an unreadable entry counts as absent
  }
  return { refs, lastFetch: ecb.fetchedAt.reduce((a, f) => (f > a ? f : a), -Infinity) };
}
function verdictOf(candle: Candle, j: Judge): Verdict {
  const p = parseDecimal(candle.close);
  if (!p) return 'fail';
  const start = midnight(candle.time);
  if (j.lastFetch < start) return 'unjudged';
  for (let back = 1; back <= GUARD_MAX_GAP_DAYS; back++) { // the latest ECB date strictly before the candle's date
    const ref = j.refs.get(start / DAY_S - back);
    if (ref === undefined) continue;
    const x = p.c * ref, diff = 10n ** BigInt(p.d + 6) - x;
    return (diff < 0n ? -diff : diff) * BPS <= BigInt(GUARD_BPS) * x ? 'pass' : 'fail';
  }
  return 'skipped';
}
/** R7 for one candle against what the build holds of the ECB file. */
export const guard = (candle: Candle, ecb: EcbHeld): Verdict => verdictOf(candle, judgeOf(ecb));

/** R5: the latest 08:00:00 UTC at or before t − 180 s. */
export function rateDayStart(t: number): number {
  const u = Math.floor(t) - EFFECT_DELAY_S - DAY_START_UTC_HOUR * 3600;
  return u - mod(u, DAY_S) + DAY_START_UTC_HOUR * 3600;
}

const heldByTime = (candles: readonly Candle[]): Map<number, Candle> => new Map(candles.map((c) => [c.time, c])); // later wins (R1)
/** R5/R6 at instant `t`, over the held final candles. No rate: R6 found no usable fix within {@link PREV_DAY_MAX} days. */
export function rateInForce(candles: readonly Candle[], ecb: EcbHeld, t: number): InForce {
  const held = heldByTime(candles), j = judgeOf(ecb);
  const out: InForce = { fxMicro: undefined, fixUsable: false, refused: [], skipped: [], unjudged: [] };
  const lower = (a: bigint | undefined, b: bigint): bigint => (a === undefined || b < a ? b : a);
  const dayRate = (start: number, upTo: number, back: number): bigint | undefined => {
    let min: bigint | undefined, fix = false;
    for (let s = start - CANDLE_S; s <= upTo; s += CANDLE_S) {
      const c = held.get(s);
      if (!c) continue;
      const v = verdictOf(c, j);
      if (v !== 'pass' && v !== 'skipped') { (v === 'fail' ? out.refused : out.unjudged).push(s); continue; }
      if (v === 'skipped') out.skipped.push(s);
      min = lower(min, hundredthsOf(parseDecimal(c.close)!, M_BPS));
      fix ||= s === start - CANDLE_S;
    }
    if (back === 0) out.fixUsable = fix;
    if (fix) return min;
    if (back === PREV_DAY_MAX) return undefined;
    // The previous day's rate: R5/R6 at 07:59:59 UTC, the last second of that day.
    const prev = dayRate(start - DAY_S, start - 1 - EFFECT_DELAY_S - CANDLE_S, back + 1);
    return prev === undefined || min === undefined ? prev : lower(prev, min);
  };
  const r = dayRate(rateDayStart(t), Math.floor(t) - EFFECT_DELAY_S - CANDLE_S, 0);
  out.fxMicro = r === undefined ? undefined : Number(r * 10_000n); // above 2^53 only far over the ceiling, which cuts it
  return out;
}

/** R9: from the close of the newest held final candle with count > 0. None held is dead. */
export function freshness(candles: readonly Candle[], t: number): Freshness {
  let newest = -Infinity;
  for (const c of heldByTime(candles).values()) if (c.count > 0 && c.time > newest) newest = c.time;
  const age = t - (newest + CANDLE_S);
  return age > DEAD_S ? 'dead' : age > ALERT_STALE_S ? 'stale' : 'fresh';
}

const pinOk = (micro: number): boolean => Number.isSafeInteger(micro) && micro % 10_000 === 0 && micro >= FLOOR_MICRO && micro <= CEILING_MICRO;
/** R8–R10: what the product serves. A set pin replaces R5–R9; `pin` must come from {@link pinMicroOf}. */
export function served(x: { inForce: number | undefined; freshness: Freshness; pin?: number }): Served {
  if (x.pin !== undefined) {
    if (!pinOk(x.pin)) throw new RangeError(`pin must be a whole cent from ${FLOOR_MICRO} to ${CEILING_MICRO} fxMicro: got ${x.pin}`);
    return { kind: 'rate', fxMicro: x.pin, source: 'pin', capped: false };
  }
  if (x.freshness === 'dead') return { kind: 'none', reason: 'dead' };
  if (x.inForce === undefined) return { kind: 'none', reason: 'no-rate' };
  if (x.inForce < FLOOR_MICRO) return { kind: 'none', reason: 'floor' };
  return { kind: 'rate', fxMicro: Math.min(x.inForce, CEILING_MICRO), source: 'rule', capped: x.inForce > CEILING_MICRO };
}
/** R10: a pin as fxMicro: exactly two decimals, 1.00 to the ceiling; undefined for anything else. */
export function pinMicroOf(s: string): number | undefined {
  if (typeof s !== 'string' || !/^[0-9]\.[0-9]{2}$/.test(s)) return undefined;
  const micro = BigInt(s.replace('.', '')) * 10_000n;
  return micro >= BigInt(FLOOR_MICRO) && micro <= BigInt(CEILING_MICRO) ? Number(micro) : undefined;
}

/** R14(b): the highest rate served over [from, to]: the line in force at `from`, plus every line in (from, to]. */
export function servedMax(lines: readonly ServedLine[], from: number, to: number): number | undefined {
  let atFrom: ServedLine | undefined, max: number | undefined;
  const add = (fx: number | null): void => { if (fx !== null && (max === undefined || fx > max)) max = fx; };
  for (const l of lines) {
    if (l.at <= from) { if (!atFrom || l.at >= atFrom.at) atFrom = l; } // a tie: the later line wins
    else if (l.at <= to) add(l.fxMicro);
  }
  if (atFrom) add(atFrom.fxMicro);
  return max;
}
/** R14(c): Stripe's whole-second `expires_at − created` is positive and at most {@link MAX_SESSION_S}. */
export const sessionLifetimeOk = (created: number, expires: number): boolean =>
  Number.isSafeInteger(created) && Number.isSafeInteger(expires) && expires > created && expires - created <= MAX_SESSION_S;
