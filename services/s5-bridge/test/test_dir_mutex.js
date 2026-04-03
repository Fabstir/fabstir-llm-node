// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
import { test } from 'node:test';
import assert from 'node:assert';
import { parentDir, acquireDirectoryLock, activeLockCount } from '../src/dir_mutex.js';

test('parentDir extracts immediate parent', () => {
  assert.strictEqual(parentDir('home/proofs/file.bin'), 'home/proofs');
  assert.strictEqual(parentDir('home/file.bin'), 'home');
  assert.strictEqual(parentDir('file.bin'), '/');
  assert.strictEqual(parentDir('a/b/c/d.txt'), 'a/b/c');
});

test('serializes writes to same directory', async () => {
  const order = [];
  const delay = (ms) => new Promise((r) => setTimeout(r, ms));

  const a = acquireDirectoryLock('same-dir').then(async (release) => {
    try {
      order.push('a:start');
      await delay(50);
      order.push('a:end');
    } finally {
      release();
    }
  });

  const b = acquireDirectoryLock('same-dir').then(async (release) => {
    try {
      order.push('b:start');
      await delay(50);
      order.push('b:end');
    } finally {
      release();
    }
  });

  await Promise.all([a, b]);
  assert.deepStrictEqual(order, ['a:start', 'a:end', 'b:start', 'b:end']);
});

test('allows parallel writes to different directories', async () => {
  const order = [];
  const delay = (ms) => new Promise((r) => setTimeout(r, ms));

  const a = acquireDirectoryLock('dir-a').then(async (release) => {
    try {
      order.push('a:start');
      await delay(50);
      order.push('a:end');
    } finally {
      release();
    }
  });

  const b = acquireDirectoryLock('dir-b').then(async (release) => {
    try {
      order.push('b:start');
      await delay(50);
      order.push('b:end');
    } finally {
      release();
    }
  });

  await Promise.all([a, b]);
  // Both should start before either ends
  const aStart = order.indexOf('a:start');
  const bStart = order.indexOf('b:start');
  const aEnd = order.indexOf('a:end');
  assert.ok(bStart < aEnd, `b:start (${bStart}) should come before a:end (${aEnd})`);
  assert.ok(aStart < aEnd, 'sanity');
});

test('cleans up after queue drains', async () => {
  const release = await acquireDirectoryLock('cleanup-key');
  assert.strictEqual(activeLockCount() >= 1, true);
  release();
  // Allow microtask to settle
  await new Promise((r) => setTimeout(r, 10));
  assert.strictEqual(activeLockCount(), 0);
});

test('error in locked operation releases lock', async () => {
  // First caller throws
  try {
    const release = await acquireDirectoryLock('error-key');
    try {
      throw new Error('boom');
    } finally {
      release();
    }
  } catch { /* expected */ }

  // Second caller should still acquire (no deadlock)
  const release2 = await acquireDirectoryLock('error-key');
  release2();
  assert.ok(true, 'second acquire succeeded — no deadlock');
});
