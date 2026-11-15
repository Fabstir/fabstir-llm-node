import { describe, it, expect } from 'vitest';
import {
  M_BPS, DAY_START_UTC_HOUR, EFFECT_DELAY_S, CANDLE_S, GUARD_BPS, GUARD_MAX_GAP_DAYS, ALERT_STALE_S, DEAD_S, PREV_DAY_MAX,
  FETCH_MINUTES, RETRY_EVERY_S, RETRY_TIMES, CEILING_MICRO, FLOOR_MICRO, WINDOW_BEFORE_S, WINDOW_AFTER_S, SESSION_S,
  MAX_SESSION_S, ECB_FETCH_MINUTE,
  parseDecimal, isFinal, candleHundredths, ecbRefMicro, guard, rateDayStart, rateInForce, freshness, served, pinMicroOf,
  servedMax, sessionLifetimeOk,
  type Candle, type EcbEntry, type EcbHeld, type ServedLine,
} from '../src/lib/fx-rule';

// Pure module — no clock, no I/O (DC-2). Every SPEC-CARD-FX-RULE v1.3 vector runs here.

const at = (iso: string): number => Date.parse(iso) / 1000;
const DAY = 86_400;
const D = at('2026-11-14T08:00:00Z'); // the rate day under test starts here
const candle = (time: number, close: string, count = 14): Candle => ({ time, close, count });
const ECB13: EcbEntry = { date: '2026-11-13', usd: '1.1052', gbp: '0.83559' }; // refMicro 1322658
const dateOf = (t: number): string => new Date(t * 1000).toISOString().slice(0, 10);
/** The 13 Nov figures on every date from 20 days before D to the day after, fetched at 00:01 on 14 and 15 Nov. */
const ECB_DAILY: EcbHeld = {
  entries: Array.from({ length: 22 }, (_, i) => ({ ...ECB13, date: dateOf(D - (20 - i) * DAY) })),
  fetchedAt: [at('2026-11-14T00:01:00Z'), at('2026-11-15T00:01:00Z')],
};
const rate = (t: number, candles: Candle[], ecb: EcbHeld = ECB_DAILY) => rateInForce(candles, ecb, t).fxMicro;

describe('the rule\'s numbers (DC-3)', () => {
  it('are the SPEC\'s constants', () => {
    expect({ M_BPS, DAY_START_UTC_HOUR, EFFECT_DELAY_S, CANDLE_S, GUARD_BPS, GUARD_MAX_GAP_DAYS, ALERT_STALE_S, DEAD_S, PREV_DAY_MAX,
      FETCH_MINUTES, RETRY_EVERY_S, RETRY_TIMES, CEILING_MICRO, FLOOR_MICRO, WINDOW_BEFORE_S, WINDOW_AFTER_S, SESSION_S,
      MAX_SESSION_S, ECB_FETCH_MINUTE }).toEqual({ M_BPS: 50, DAY_START_UTC_HOUR: 8, EFFECT_DELAY_S: 180, CANDLE_S: 900,
      GUARD_BPS: 300, GUARD_MAX_GAP_DAYS: 5, ALERT_STALE_S: 3600, DEAD_S: 7200, PREV_DAY_MAX: 7, FETCH_MINUTES: [3, 18, 33, 48],
      RETRY_EVERY_S: 60, RETRY_TIMES: 10, CEILING_MICRO: 1_400_000, FLOOR_MICRO: 1_000_000, WINDOW_BEFORE_S: 300,
      WINDOW_AFTER_S: 60, SESSION_S: 1860, MAX_SESSION_S: 1920, ECB_FETCH_MINUTE: 1 });
  });
});

describe('R2 — a candle\'s rate', () => {
  it.each([
    ['0.7571', 4, 131, 130], ['0.75710000', 8, 131, 130], ['0.7600', 4, 130, 130], ['0.7692', 4, 129, 128],
    ['0.75', 2, 132, 132], ['0.71428', 5, 139, 138], ['0.7107', 4, 140, 139], ['1.0050', 4, 99, 98],
  ])('close %s (d = %i) → %i hundredths at M = 50, %i at M = 100', (close, d, at50, at100) => {
    expect(parseDecimal(close)?.d).toBe(d);
    expect(candleHundredths(close)).toBe(at50);
    expect(candleHundredths(close, 100)).toBe(at100);
  });

  it('parses exact digits: "0.7571" → C 7571, "0.75710000" → C 75710000', () => {
    expect(parseDecimal('0.7571')).toEqual({ c: 7571n, d: 4 });
    expect(parseDecimal('0.75710000')).toEqual({ c: 75_710_000n, d: 8 });
  });

  it.each(['1e-1', '-0.75', '', '0', '0.0000', ' 0.75', '.75', '0.', '0.7571000000001', '0.75\n', '0,75'])(
    'refuses %j by the grammar', (close) => {
      expect(parseDecimal(close)).toBeUndefined();
      expect(candleHundredths(close)).toBeUndefined();
    });

  it('1.0050 → 0.99: below the floor, so no rate (R8)', () => {
    expect(served({ inForce: candleHundredths('1.0050')! * 10_000, freshness: 'fresh' })).toEqual({ kind: 'none', reason: 'floor' });
  });
});

describe('R1 — finality', () => {
  it('is time <= last only: time == last is final, the next candle is not', () => {
    expect(isFinal(D, D)).toBe(true);
    expect(isFinal(D - CANDLE_S, D)).toBe(true);
    expect(isFinal(D + CANDLE_S, D)).toBe(false);
    expect(isFinal(D + 1, D)).toBe(false);
  });

  it('a revised close replaces the held one: the later candle for the same time wins', () => {
    const fix = D - CANDLE_S;
    expect(rate(D + 180, [candle(fix, '0.7571'), candle(fix, '0.75')])).toBe(1_320_000);
    expect(rate(D + 180, [candle(fix, '0.75'), candle(fix, '0.7571')])).toBe(1_310_000);
  });
});

describe('R7 — the ECB check', () => {
  const noon = at('2026-11-14T12:00:00Z');
  const held = (entries: EcbEntry[], fetchedAt = [at('2026-11-14T00:01:00Z')]): EcbHeld => ({ entries, fetchedAt });

  it('refMicro for 13 Nov 2026 is 1322658', () => {
    expect(ecbRefMicro('1.1052', '0.83559')).toBe(1_322_658);
    expect(ecbRefMicro('1.1052', '0')).toBeUndefined();
  });

  it.each([['0.7571', 'pass'], ['0.7341', 'pass'], ['0.7340', 'fail'], ['0.7794', 'pass'], ['0.7795', 'fail']])(
    'close %s → %s', (close, verdict) => {
      expect(guard(candle(noon, close), held([ECB13]))).toBe(verdict);
    });

  it('judges against the latest ECB date strictly BEFORE the candle\'s UTC date, never its own date', () => {
    const own: EcbEntry = { date: '2026-11-14', usd: '1', gbp: '1' }; // 1.00 would fail 0.7571
    expect(guard(candle(noon, '0.7571'), held([ECB13, own]))).toBe('pass');
    expect(guard(candle(at('2026-11-14T00:00:00Z'), '0.7571'), held([own]))).toBe('skipped');
  });

  it('a 5-day gap still judges; a 6-day gap skips the check (flagged as skipped, not failed)', () => {
    expect(guard(candle(noon, '0.7571'), held([{ ...ECB13, date: '2026-11-09' }]))).toBe('pass');
    expect(guard(candle(noon, '0.7340'), held([{ ...ECB13, date: '2026-11-09' }]))).toBe('fail');
    expect(guard(candle(noon, '0.7571'), held([{ ...ECB13, date: '2026-11-08' }]))).toBe('skipped');
  });

  it('no successful fetch since 00:00 UTC of the candle\'s date → not judged yet; one at 00:00:00 judges it', () => {
    expect(guard(candle(noon, '0.7571'), held([ECB13], [at('2026-11-13T23:59:59Z')]))).toBe('unjudged');
    expect(guard(candle(noon, '0.7571'), held([ECB13], [at('2026-11-13T23:59:59Z'), at('2026-11-14T00:00:00Z')]))).toBe('pass');
  });

  it('an unjudged fix candle is not usable (R6), and becomes usable after the fetch', () => {
    const candles = [candle(D - DAY - CANDLE_S, '0.75'), candle(D - CANDLE_S, '0.7571')];
    const early = { entries: ECB_DAILY.entries, fetchedAt: [at('2026-11-13T00:01:00Z')] };
    expect(rateInForce(candles, early, D + 180)).toMatchObject({ fxMicro: 1_320_000, fixUsable: false, unjudged: [D - CANDLE_S] });
    expect(rateInForce(candles, ECB_DAILY, D + 180)).toMatchObject({ fxMicro: 1_310_000, fixUsable: true, unjudged: [] });
  });

  it('a failed or ungrammatical candle is refused and named; a skipped one is used and named', () => {
    const r = rateInForce([candle(D - DAY - CANDLE_S, '0.75'), candle(D - CANDLE_S, '0.7340'), candle(D, 'bad'), candle(D + CANDLE_S, '0.7600')],
      ECB_DAILY, D + 2 * CANDLE_S + 180);
    expect(r).toMatchObject({ fxMicro: 1_300_000, fixUsable: false, refused: [D - CANDLE_S, D], skipped: [] });
    const s = rateInForce([candle(D - CANDLE_S, '0.7571')], { entries: [], fetchedAt: ECB_DAILY.fetchedAt }, D + 180);
    expect(s).toMatchObject({ fxMicro: 1_310_000, fixUsable: true, skipped: [D - CANDLE_S] });
  });
});

describe('R3/R4 — rateDayStart', () => {
  it.each([['07:59:59', D - DAY], ['08:00:00', D], ['08:02:59', D], ['08:03:00', D]])(
    't − 180 s at %s UTC → the day that starts at the expected 08:00', (hms, start) => {
      expect(rateDayStart(at(`2026-11-14T${hms}Z`) + EFFECT_DELAY_S)).toBe(start);
    });

  it('the fix candle opens at 07:45 and takes effect at 08:03', () => {
    expect(new Date((rateDayStart(D + 180) - CANDLE_S) * 1000).toISOString()).toBe('2026-11-14T07:45:00.000Z');
  });
});

describe('R5 — the rate in force, the SPEC day table', () => {
  const day = [
    candle(D - DAY - CANDLE_S, '0.75'), // the previous day: 1.32
    candle(D - CANDLE_S, '0.7571'), candle(D, '0.7600'), candle(D + CANDLE_S, '0.7560'), candle(D + DAY - CANDLE_S, '0.7560'),
  ];
  it.each([
    ['08:03 → 1.31 (the fix alone)', D + 180, 1_310_000, 1_320_000],
    ['08:18 → 1.30', D + 1080, 1_300_000, 1_310_000],
    ['08:33 → 1.30 (the day\'s lowest holds)', D + 1980, 1_300_000, 1_300_000],
    ['next day 08:03 → 1.31 (the one daily rise)', D + DAY + 180, 1_310_000, 1_300_000],
  ])('%s, and 1 s before it', (_, t, now, before) => {
    expect(rate(t, day)).toBe(now);
    expect(rate(t - 1, day)).toBe(before);
  });
});

describe('R6 — a candle not yet seen', () => {
  const prev = candle(D - DAY - CANDLE_S, '0.75'); // the previous day ended at 1.32
  it('the SPEC example: 1.32 until today\'s candles arrive; 08:00 first → 1.30; then the fix → still 1.30', () => {
    expect(rateInForce([prev], ECB_DAILY, D + 180)).toMatchObject({ fxMicro: 1_320_000, fixUsable: false });
    expect(rate(D + 1080, [prev])).toBe(1_320_000);
    expect(rate(D + 1080, [prev, candle(D, '0.7600')])).toBe(1_300_000);
    expect(rate(D + 1080, [prev, candle(D, '0.7600'), candle(D - CANDLE_S, '0.7571')])).toBe(1_300_000);
  });

  it('the lower of the two: today\'s candle above the previous day does not raise it', () => {
    expect(rate(D + 1080, [prev, candle(D, '0.7400')])).toBe(1_320_000); // 1.34, within 3% of the ECB
  });

  it('goes back exactly 7 rate days → a rate; 8 → none, even with candles today', () => {
    const fixOf = (back: number) => candle(D - back * DAY - CANDLE_S, '0.7571');
    expect(rate(D + 1080, [fixOf(7)])).toBe(1_310_000);
    expect(rate(D + 1080, [fixOf(7), candle(D, '0.7600')])).toBe(1_300_000);
    expect(rate(D + 1080, [fixOf(8)])).toBeUndefined();
    expect(rate(D + 1080, [fixOf(8), candle(D, '0.7600')])).toBeUndefined();
    expect(rate(D + 1080, [])).toBeUndefined();
  });
});

describe('R9 — freshness', () => {
  const T = D; // closes at D + 900
  it.each([[3600, 'fresh'], [3601, 'stale'], [7200, 'stale'], [7201, 'dead']] as const)(
    '%i s after the close of the newest traded candle → %s', (age, want) => {
      expect(freshness([candle(T - CANDLE_S, '0.7571'), candle(T, '0.7571')], T + CANDLE_S + age)).toBe(want);
    });

  it('a run of count 0 candles does not refresh it; no traded candle is dead', () => {
    const halted = [candle(T, '0.7571'), ...[1, 2, 3, 4, 5, 6, 7, 8, 9].map((k) => candle(T + k * CANDLE_S, '0.7571', 0))];
    expect(freshness(halted, T + CANDLE_S + 7201)).toBe('dead');
    expect(freshness([candle(T, '0.7571', 0)], T + CANDLE_S)).toBe('dead');
    expect(freshness([], T)).toBe('dead');
  });
});

describe('R8/R10 — served and the pin', () => {
  it('the pin wins over the rule, a dead feed and no rate', () => {
    for (const freshness of ['fresh', 'dead'] as const) {
      for (const inForce of [1_310_000, 1_500_000, 900_000, undefined]) {
        expect(served({ inForce, freshness, pin: 1_330_000 })).toEqual({ kind: 'rate', fxMicro: 1_330_000, source: 'pin', capped: false });
      }
    }
  });

  it('serves the rule, cuts it to 1.40 (flagged), refuses below 1.00, a dead feed and no rate', () => {
    expect(served({ inForce: 1_310_000, freshness: 'stale' })).toEqual({ kind: 'rate', fxMicro: 1_310_000, source: 'rule', capped: false });
    expect(served({ inForce: 1_400_000, freshness: 'fresh' })).toEqual({ kind: 'rate', fxMicro: 1_400_000, source: 'rule', capped: false });
    expect(served({ inForce: 1_410_000, freshness: 'fresh' })).toEqual({ kind: 'rate', fxMicro: 1_400_000, source: 'rule', capped: true });
    expect(served({ inForce: 1_000_000, freshness: 'fresh' })).toMatchObject({ kind: 'rate', fxMicro: 1_000_000 });
    expect(served({ inForce: 990_000, freshness: 'fresh' })).toEqual({ kind: 'none', reason: 'floor' });
    expect(served({ inForce: 1_310_000, freshness: 'dead' })).toEqual({ kind: 'none', reason: 'dead' });
    expect(served({ inForce: undefined, freshness: 'fresh' })).toEqual({ kind: 'none', reason: 'no-rate' });
  });

  it('refuses a pin pinMicroOf would refuse', () => {
    expect(() => served({ inForce: 1_310_000, freshness: 'fresh', pin: 1_410_000 })).toThrow(RangeError);
    expect(() => served({ inForce: 1_310_000, freshness: 'fresh', pin: 1_315_000 })).toThrow(RangeError);
  });

  it('pinMicroOf: exactly two decimals, 1.00 to 1.40', () => {
    expect(pinMicroOf('1.00')).toBe(1_000_000);
    expect(pinMicroOf('1.31')).toBe(1_310_000);
    expect(pinMicroOf('1.40')).toBe(1_400_000);
    for (const bad of ['0.99', '01.31', '1.41', '2.00', '1.3', '1.310', '1', '1.', ' 1.31', '1.31 ', '+1.31', '1e0', '1,31', '']) {
      expect(pinMicroOf(bad)).toBeUndefined();
    }
  });
});

describe('R14 — servedMax and sessionLifetimeOk', () => {
  const lines: ServedLine[] = [
    { at: 100, fxMicro: 1_310_000 }, { at: 200, fxMicro: 1_300_000 }, { at: 300, fxMicro: null }, { at: 400.4, fxMicro: 1_320_000 },
  ];
  it('takes the line in force at `from` plus every line in (from, to]', () => {
    expect(servedMax(lines, 150, 250)).toBe(1_310_000);
    expect(servedMax(lines, 200, 250)).toBe(1_300_000);
    expect(servedMax(lines, 250, 400.4)).toBe(1_320_000);
    expect(servedMax(lines, 250, 400)).toBe(1_300_000);
  });

  it('null lines add nothing; no line → none', () => {
    expect(servedMax(lines, 300, 400)).toBeUndefined();
    expect(servedMax(lines, 350, 399)).toBeUndefined();
    expect(servedMax(lines, 0, 99)).toBeUndefined();
    expect(servedMax([], 0, 1e12)).toBeUndefined();
  });

  it('lifetime 1920 s passes, 1921 s fails', () => {
    expect(sessionLifetimeOk(1_000, 1_000 + 1920)).toBe(true);
    expect(sessionLifetimeOk(1_000, 1_000 + SESSION_S)).toBe(true);
    expect(sessionLifetimeOk(1_000, 1_000 + 1921)).toBe(false);
    expect(sessionLifetimeOk(1_000, 1_000)).toBe(false);
    expect(sessionLifetimeOk(1_000, 1_000.5 + 1920)).toBe(false);
  });
});
