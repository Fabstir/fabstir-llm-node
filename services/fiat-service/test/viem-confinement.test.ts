// Enforces the FC2 invariant (goal 4) automatically: `viem` may be imported from
// exactly ONE app-source file, src/lib/fiat-signature.ts. OP1.3a adds exactly one
// more, src/lib/owner-relayer.ts (the Add a passkey relayer: user operations,
// its own gas wallet), and nothing else. This is the "grep gate"
// made real, so viem can never quietly spread onto the auth-critical path (or
// bloat the client bundle) without a red test.
import { describe, expect, it } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

function walk(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (/\.(ts|tsx)$/.test(name)) out.push(p);
  }
  return out;
}

describe('viem confinement (goal 4)', () => {
  it('is imported only by src/lib/fiat-signature.ts and src/lib/owner-relayer.ts', () => {
    const files = [...walk('src'), ...walk('app')];
    const importers = files
      .filter((f) => /from ['"]viem/.test(readFileSync(f, 'utf8')))
      .map((f) => f.replace(/\\/g, '/'))
      .sort();
    expect(importers).toEqual(['src/lib/fiat-signature.ts', 'src/lib/owner-relayer.ts']);
  });
});
