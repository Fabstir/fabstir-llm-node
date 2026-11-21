// Rules about the source tree itself.
//   - viem is imported in exactly one source module, src/signature.ts.
//   - no web framework; node:http only.
//   - the source never writes to the console outside the one logger in server.ts.
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const srcDir = join(root, 'src');
const sources = readdirSync(srcDir, { recursive: true })
  .map(String)
  .filter((f) => f.endsWith('.ts'))
  .map((f) => ({ file: f, text: readFileSync(join(srcDir, f), 'utf8') }));

/** Module specifiers a file imports. Comments are dropped first, so prose such
 *  as: indistinguishable from "x", is not read as an import. */
const importsOf = (source: string): string[] =>
  [...source
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '')
    .matchAll(/(?:from\s+|import\s*\(\s*|require\s*\(\s*|^\s*import\s+)['"]([^'"]+)['"]/gm)].map((m) => m[1]!);

describe('source rules', () => {
  it('finds the source modules', () => {
    expect(sources.map((s) => s.file).sort()).toEqual(
      ['challenge.ts', 'config.ts', 'handlers.ts', 'message.ts', 'ratelimit.ts', 'server.ts', 'signature.ts', 'store-s3.ts', 'store.ts', 'token.ts'].sort()
    );
  });

  it('imports viem in exactly one module: src/signature.ts', () => {
    const users = sources.filter((s) => importsOf(s.text).some((i) => i === 'viem' || i.startsWith('viem/'))).map((s) => s.file);
    expect(users).toEqual(['signature.ts']);
  });

  it('imports the S3 client in exactly one module: src/store-s3.ts', () => {
    const users = sources.filter((s) => importsOf(s.text).some((i) => i === 'minio' || i.startsWith('minio/'))).map((s) => s.file);
    expect(users).toEqual(['store-s3.ts']);
  });

  it('imports nothing but viem, minio, node built-ins and its own modules', () => {
    for (const s of sources) {
      for (const i of importsOf(s.text)) {
        const ok = i.startsWith('node:') || i.startsWith('./') || i === 'viem' || i.startsWith('viem/') || i === 'minio';
        expect(ok, `${s.file} imports ${i}`).toBe(true);
      }
    }
  });

  it('declares only viem and minio as runtime dependencies, pinned exactly', () => {
    const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
    expect(Object.keys(pkg.dependencies).sort()).toEqual(['minio', 'viem']);
    for (const v of [...Object.values(pkg.dependencies), ...Object.values(pkg.devDependencies)]) {
      expect(String(v)).toMatch(/^\d+\.\d+\.\d+$/);
    }
  });

  it('writes to the console nowhere but server.ts', () => {
    for (const s of sources) {
      if (s.file === 'server.ts') continue;
      expect(/\bconsole\.|process\.std(out|err)/.test(s.text), `${s.file} writes output`).toBe(false);
    }
  });
});
