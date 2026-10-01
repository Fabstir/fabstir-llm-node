// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * The upload path retries a TRANSIENT directory failure, and nothing else.
 *
 * Regression guard for the 2026-09-28 rehearsal: pathToCID() right after
 * fs.put() read the just-rewritten directory as a retryable 404, the bridge
 * answered 500 on the first attempt, and the node ended the training run.
 */
import { test } from 'node:test';
import assert from 'node:assert';
import { readFileSync } from 'node:fs';
import { S5DirectoryLoadError } from '@julesl23/s5js';
import { withDirectoryRetry } from '../src/s5_retry.js';

const transient = () =>
  new S5DirectoryLoadError('Directory blob is temporarily unavailable (404)', {
    path: 'home/training/job_1168',
    publicKey: 'ed01aabb',
    retryable: true,
  });

const noWait = async () => {};

test('a retryable directory failure is retried until the call succeeds', async () => {
  let calls = 0;
  const delays = [];
  const result = await withDirectoryRetry(
    async () => {
      calls++;
      if (calls < 3) throw transient();
      return 'cid';
    },
    { wait: noWait, onRetry: ({ delayMs }) => delays.push(delayMs) },
  );
  assert.strictEqual(result, 'cid');
  assert.strictEqual(calls, 3, 'one blip must not fail the upload');
  assert.deepStrictEqual(delays, [2000, 4000]);
});

test('a structural directory failure is NOT retried (it needs repair)', async () => {
  let calls = 0;
  await assert.rejects(
    withDirectoryRetry(
      async () => {
        calls++;
        throw new S5DirectoryLoadError('directory is unreadable', {
          path: 'home/training/job_1168',
          publicKey: 'ed01aabb',
          retryable: false,
        });
      },
      { wait: noWait },
    ),
    (e) => e.retryable === false,
  );
  assert.strictEqual(calls, 1);
});

test('any other error is rethrown at once', async () => {
  let calls = 0;
  await assert.rejects(
    withDirectoryRetry(
      async () => {
        calls++;
        throw new Error('portal refused the upload');
      },
      { wait: noWait },
    ),
    /portal refused/,
  );
  assert.strictEqual(calls, 1);
});

test('a directory that never recovers still fails, after the bounded attempts', async () => {
  let calls = 0;
  await assert.rejects(
    withDirectoryRetry(
      async () => {
        calls++;
        throw transient();
      },
      { attempts: 4, wait: noWait },
    ),
    (e) => e.retryable === true,
  );
  assert.strictEqual(calls, 4);
});

test('slow attempts stop at the overall deadline, before the node gives up', async () => {
  // Each attempt's own S5 call takes 100 s on a fake clock. Sleeps alone are
  // 56 s at most, but with the calls counted eight attempts would run for
  // about 13 minutes, long after the node's 300 s upload timeout.
  let clock = 0;
  let calls = 0;
  await assert.rejects(
    withDirectoryRetry(
      async () => {
        calls++;
        clock += 100_000;
        throw transient();
      },
      { deadlineMs: 240_000, now: () => clock, wait: async (ms) => { clock += ms; } },
    ),
    (e) => e.retryable === true,
  );
  // 100 s, sleep 2, 100 s, sleep 4, 100 s = 306 s: a third sleep would pass 240 s.
  assert.strictEqual(calls, 3);
  assert.ok(clock <= 310_000, `gave up at ${clock} ms`);
});

test('the default deadline sits under the node upload timeout (300 s)', async () => {
  const { DIR_RETRY_DEADLINE_MS } = await import('../src/s5_retry.js');
  assert.ok(DIR_RETRY_DEADLINE_MS > 0 && DIR_RETRY_DEADLINE_MS < 300_000);
});

test('the PUT route wraps BOTH directory reads in the retry', () => {
  // The route needs a live S5 client, so guard its wiring at the source: a
  // bare call here is exactly the 1.5.0 defect.
  const routes = readFileSync(new URL('../src/routes.js', import.meta.url), 'utf8');
  assert.match(routes, /withDirectoryRetry\(\(\) => s5\.fs\.put\(path,/);
  assert.match(routes, /withDirectoryRetry\(\(\) => advanced\.pathToCID\(path\)/);
  assert.doesNotMatch(routes, /await s5\.fs\.put\(path,/);
  assert.doesNotMatch(routes, /await advanced\.pathToCID\(path\)/);
});
