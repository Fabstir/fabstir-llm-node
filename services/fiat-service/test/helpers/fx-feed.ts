// GP tests: card exchange-rate feeds without a network or a disk.
import { CardFxFeed, type CardFxSettings } from '../../src/lib/card-fx';
import { FxStore, type FxFiles } from '../../src/lib/card-fx-store';
import { servedMax, type ServedLine } from '../../src/lib/fx-rule';

export function memoryFxFiles(): FxFiles & { data: Record<string, string> } {
  const data: Record<string, string> = {};
  return {
    data,
    async read(name) {
      return data[name];
    },
    async append(name, text) {
      data[name] = (data[name] ?? '') + text;
    },
  };
}

/** A feed over memory that serves `pin` (or nothing, when misconfigured). */
export async function testFeed(o: { pin?: number; settings?: CardFxSettings } = {}) {
  const files = memoryFxFiles();
  const urgent: string[] = [];
  const feed = await CardFxFeed.open(
    {
      store: new FxStore(files),
      fetchKraken: async () => ({ ok: false, reason: 'offline in tests' }),
      fetchEcb: async () => ({ ok: false, reason: 'offline in tests' }),
      alerts: { alert: () => {}, urgent: (m) => urgent.push(m), log: () => {} },
      settings: o.settings ?? { ok: true, pin: o.pin ?? 1_330_000 },
    },
    { fetchAtBoot: false }
  );
  return { feed, files, urgent };
}

/** The webhook's R14 seam over fixed served lines. */
export function servedFx(lines: ServedLine[]) {
  const urgents: string[] = [];
  return {
    urgents,
    lines,
    windowMax: (createdS: number) => servedMax(lines, createdS - 300, createdS + 60),
    urgent: (m: string) => void urgents.push(m),
  };
}

/** Served 1.33 since long before any fixture's `created`. */
export const ALWAYS_133: ServedLine[] = [{ at: 0, fxMicro: 1_330_000 }];
