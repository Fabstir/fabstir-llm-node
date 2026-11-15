// GP1b — the two sources of the card exchange rule (SPEC-CARD-FX-RULE v1.3 R1,
// R7): Kraken's 15-minute USDC/GBP candles and the ECB's 90-day reference
// file. Fetchers and parsers only; what the numbers mean is `fx-rule.ts`.
// The fetcher is injected, so no test touches the network.
import type { Candle, EcbEntry } from './fx-rule';

/** R1: the fixed URL, no account and no key. */
export const KRAKEN_URL = 'https://api.kraken.com/0/public/OHLC?pair=USDCGBP&interval=15';
/** R7: the fixed 90-day file. */
export const ECB_URL = 'https://www.ecb.europa.eu/stats/eurofxref/eurofxref-hist-90d.xml';
export const FETCH_TIMEOUT_MS = 10_000;
/** The 90-day XML is about 100 KB; Kraken's 720 candles are smaller. */
export const FETCH_MAX_BYTES = 1_000_000;
const MAX_REDIRECTS = 3;

export type Fetcher = (url: string, init: { signal: AbortSignal; redirect: 'manual' }) => Promise<Response>;

export type KrakenResult = { ok: true; last: number; candles: Candle[] } | { ok: false; reason: string };
export type EcbResult = { ok: true; entries: EcbEntry[] } | { ok: false; reason: string };

/**
 * R1. `result` holds `last` and exactly one other key, the candle array (its
 * spelling is not assumed). Only final candles (`time <= last`) are kept. A
 * non-empty `error`, or no candle array, is a failed fetch. A row that is not
 * shaped `[time, …, close (4), …, count (7)]` is dropped; the close itself is
 * kept as the raw string and judged by the rule (R2), never parsed here.
 */
export function parseKraken(json: unknown): KrakenResult {
  const j = json as { error?: unknown; result?: unknown } | null;
  if (!j || typeof j !== 'object') return { ok: false, reason: 'not a JSON object' };
  if (Array.isArray(j.error) && j.error.length > 0) return { ok: false, reason: `error: ${j.error.map(String).join('; ')}` };
  const result = j.result as Record<string, unknown> | undefined;
  if (!result || typeof result !== 'object') return { ok: false, reason: 'no result' };
  const last = result.last;
  if (typeof last !== 'number' || !Number.isSafeInteger(last)) return { ok: false, reason: 'no integer result.last' };
  const others = Object.keys(result).filter((k) => k !== 'last');
  if (others.length !== 1) return { ok: false, reason: `result has ${others.length} keys besides last, not one` };
  const rows = result[others[0]!];
  if (!Array.isArray(rows)) return { ok: false, reason: `result.${others[0]} is not an array` };
  const candles: Candle[] = [];
  for (const row of rows) {
    if (!Array.isArray(row) || row.length < 8) continue;
    const [time, , , , close, , , count] = row as unknown[];
    if (typeof time !== 'number' || !Number.isSafeInteger(time)) continue;
    if (typeof close !== 'string') continue;
    if (typeof count !== 'number' || !Number.isSafeInteger(count) || count < 0) continue;
    if (time > last) continue; // R1: not final (the open candle)
    candles.push({ time, close, count });
  }
  return { ok: true, last, candles };
}

const attrsOf = (tag: string): Record<string, string> => {
  const out: Record<string, string> = {};
  for (const m of tag.matchAll(/([A-Za-z_][\w-]*)\s*=\s*(['"])(.*?)\2/g)) out[m[1]!] = m[3]!;
  return out;
};

/**
 * R7. Every `<Cube time=…>` date with both a `USD` and a `GBP` rate, quotes
 * `'` or `"`. A file that yields no such date is a failed fetch. The rates
 * stay raw strings (the rule parses them as exact decimals).
 */
export function parseEcb(xml: string): EcbResult {
  if (typeof xml !== 'string') return { ok: false, reason: 'not text' };
  const entries: EcbEntry[] = [];
  // A date's Cube holds self-closing currency Cubes, so the first </Cube> after it ends the date.
  for (const day of xml.matchAll(/<Cube\b([^>]*\btime\s*=\s*(['"])[^'"]*\2[^>]*)>([\s\S]*?)<\/Cube>/g)) {
    const date = attrsOf(day[1]!).time;
    if (!date || !/^\d{4}-\d{2}-\d{2}$/.test(date)) continue;
    const rates: Record<string, string> = {};
    for (const c of day[3]!.matchAll(/<Cube\b([^>]*)\/?>/g)) {
      const a = attrsOf(c[1]!);
      if (a.currency && a.rate !== undefined) rates[a.currency] = a.rate;
    }
    if (rates.USD !== undefined && rates.GBP !== undefined) entries.push({ date, usd: rates.USD, gbp: rates.GBP });
  }
  return entries.length > 0 ? { ok: true, entries } : { ok: false, reason: 'no date with both a USD and a GBP rate' };
}

/** GET `url` with a 10 s timeout and a 1 MB limit, following redirects only
 *  on the same host. Throws on anything else. */
export async function boundedGet(fetcher: Fetcher, url: string): Promise<string> {
  const signal = AbortSignal.timeout(FETCH_TIMEOUT_MS);
  const host = new URL(url).host;
  let at = url;
  for (let hop = 0; hop <= MAX_REDIRECTS; hop++) {
    const res = await fetcher(at, { signal, redirect: 'manual' });
    if (res.status >= 300 && res.status < 400) {
      const location = res.headers.get('location');
      if (!location) throw new Error(`${res.status} without a location`);
      const next = new URL(location, at);
      if (next.host !== host || next.protocol !== 'https:') throw new Error(`redirect to another host refused: ${next.host}`);
      at = next.toString();
      continue;
    }
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return readCapped(res, FETCH_MAX_BYTES);
  }
  throw new Error(`more than ${MAX_REDIRECTS} redirects`);
}

async function readCapped(res: Response, max: number): Promise<string> {
  const declared = Number(res.headers.get('content-length'));
  if (Number.isFinite(declared) && declared > max) throw new Error(`body of ${declared} bytes is over ${max}`);
  if (!res.body) return '';
  const reader = res.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > max) {
      await reader.cancel().catch(() => {});
      throw new Error(`body over ${max} bytes`);
    }
    chunks.push(value);
  }
  return Buffer.concat(chunks).toString('utf8');
}

export async function fetchKraken(fetcher: Fetcher): Promise<KrakenResult> {
  try {
    const text = await boundedGet(fetcher, KRAKEN_URL);
    let json: unknown;
    try {
      json = JSON.parse(text);
    } catch {
      return { ok: false, reason: 'body is not JSON' };
    }
    return parseKraken(json);
  } catch (e) {
    return { ok: false, reason: e instanceof Error ? e.message : String(e) };
  }
}

export async function fetchEcb(fetcher: Fetcher): Promise<EcbResult> {
  try {
    return parseEcb(await boundedGet(fetcher, ECB_URL));
  } catch (e) {
    return { ok: false, reason: e instanceof Error ? e.message : String(e) };
  }
}
