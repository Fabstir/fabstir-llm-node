// GP1b — the card exchange rule's three records (plan section 4), append-only
// JSON lines under FIAT_DATA_DIR/fx/ (the backed-up volume, beside the ledger):
//   candles.jsonl  every final Kraken candle fetched; a later line for the same
//                  candle replaces the held one on load (R1)
//   ecb.jsonl      every ECB entry seen, and `{fetchedAt}` for every successful
//                  fetch of the file (R7)
//   served.jsonl   `{at, fxMicro|null}` on every change of what is served; the
//                  record R14(b) is judged against
// Each append is written and fsynced before it is relied on. A torn line (a
// write cut short) is skipped on load, and the next append starts a new line
// so it can never be glued to the fragment.
import { mkdir, open, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import type { Candle, EcbEntry, EcbHeld, ServedLine } from './fx-rule';

export type FxFileName = 'candles.jsonl' | 'ecb.jsonl' | 'served.jsonl';

/** The disk seam: tests run the store over memory. */
export interface FxFiles {
  /** The whole file, or undefined when it does not exist. */
  read(name: FxFileName): Promise<string | undefined>;
  /** Append and fsync before resolving. */
  append(name: FxFileName, text: string): Promise<void>;
}

export function diskFxFiles(dir: string): FxFiles {
  let made: Promise<unknown> | undefined;
  return {
    async read(name) {
      try {
        return await readFile(join(dir, name), 'utf8');
      } catch (e) {
        if ((e as NodeJS.ErrnoException).code === 'ENOENT') return undefined;
        throw e;
      }
    },
    async append(name, text) {
      made ??= mkdir(dir, { recursive: true });
      await made;
      const fh = await open(join(dir, name), 'a');
      try {
        await fh.writeFile(text, 'utf8');
        await fh.sync();
      } finally {
        await fh.close();
      }
    },
  };
}

export interface FxHeld {
  candles: Candle[];
  ecb: { entries: EcbEntry[]; fetchedAt: number[] };
  served: ServedLine[];
  /** Lines that did not parse (a torn write), per file. */
  skipped: Record<FxFileName, number>;
}

const isCandle = (v: unknown): v is Candle => {
  const c = v as Candle;
  return !!c && Number.isSafeInteger(c.time) && typeof c.close === 'string' && Number.isSafeInteger(c.count) && c.count >= 0;
};
const isEntry = (v: unknown): v is EcbEntry => {
  const e = v as EcbEntry;
  return !!e && typeof e.date === 'string' && typeof e.usd === 'string' && typeof e.gbp === 'string';
};
const isServed = (v: unknown): v is ServedLine => {
  const s = v as ServedLine;
  return !!s && typeof s.at === 'number' && Number.isFinite(s.at) && (s.fxMicro === null || Number.isSafeInteger(s.fxMicro));
};

function lines(text: string | undefined, accept: (v: unknown) => boolean): { values: unknown[]; skipped: number } {
  const values: unknown[] = [];
  let skipped = 0;
  for (const raw of (text ?? '').split('\n')) {
    if (raw.trim() === '') continue;
    let v: unknown;
    try {
      v = JSON.parse(raw);
    } catch {
      skipped += 1;
      continue;
    }
    if (accept(v)) values.push(v);
    else skipped += 1;
  }
  return { values, skipped };
}

export class FxStore {
  /** Files whose last byte is not a newline (a torn write): the next append
   *  begins with one. */
  private torn = new Set<FxFileName>();

  constructor(private readonly files: FxFiles) {}

  async load(): Promise<FxHeld> {
    const [c, e, s] = await Promise.all([
      this.files.read('candles.jsonl'),
      this.files.read('ecb.jsonl'),
      this.files.read('served.jsonl'),
    ]);
    for (const [name, text] of [['candles.jsonl', c], ['ecb.jsonl', e], ['served.jsonl', s]] as const) {
      if (text && !text.endsWith('\n')) this.torn.add(name);
    }
    const candles = lines(c, isCandle);
    const ecb = lines(e, (v) => isEntry(v) || (typeof (v as { fetchedAt?: unknown })?.fetchedAt === 'number'));
    const served = lines(s, isServed);
    // R1: a later line for the same candle replaces the held one.
    const byTime = new Map<number, Candle>();
    for (const v of candles.values as Candle[]) byTime.set(v.time, { time: v.time, close: v.close, count: v.count });
    const entries: EcbEntry[] = [];
    const fetchedAt: number[] = [];
    for (const v of ecb.values) {
      if (isEntry(v)) entries.push({ date: v.date, usd: v.usd, gbp: v.gbp });
      else fetchedAt.push((v as { fetchedAt: number }).fetchedAt);
    }
    return {
      candles: [...byTime.values()].sort((a, b) => a.time - b.time),
      ecb: { entries, fetchedAt },
      served: (served.values as ServedLine[]).map((l) => ({ at: l.at, fxMicro: l.fxMicro })),
      skipped: { 'candles.jsonl': candles.skipped, 'ecb.jsonl': ecb.skipped, 'served.jsonl': served.skipped },
    };
  }

  private async write(name: FxFileName, values: readonly unknown[]): Promise<void> {
    if (values.length === 0) return;
    const body = values.map((v) => JSON.stringify(v)).join('\n') + '\n';
    try {
      await this.files.append(name, (this.torn.has(name) ? '\n' : '') + body);
    } catch (e) {
      // Part of the line may be on disk (ENOSPC, EIO): start the next append on
      // a fresh line so it is not glued to the fragment.
      this.torn.add(name);
      throw e;
    }
    this.torn.delete(name);
  }

  appendCandles(candles: readonly Candle[]): Promise<void> {
    return this.write('candles.jsonl', candles.map((c) => ({ time: c.time, close: c.close, count: c.count })));
  }

  /** New or changed entries, then the instant of this successful fetch. */
  appendEcbFetch(entries: readonly EcbEntry[], fetchedAt: number): Promise<void> {
    return this.write('ecb.jsonl', [...entries.map((e) => ({ date: e.date, usd: e.usd, gbp: e.gbp })), { fetchedAt }]);
  }

  appendServed(line: ServedLine): Promise<void> {
    return this.write('served.jsonl', [{ at: line.at, fxMicro: line.fxMicro }]);
  }
}

export type { EcbHeld };
