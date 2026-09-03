// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * The rule that stops a degraded S5 read being recorded as absence.
 *
 * Regression guard for the incident where a transient directory failure was
 * served as 404 and the indexer wrote "this platform has no content" down as
 * fact. 404 must mean absent, and nothing else.
 */
import { test } from 'node:test';
import assert from 'node:assert';
import { S5DirectoryLoadError } from '@julesl23/s5js';
import { classifyS5ReadError } from '../src/s5_read_errors.js';

const PATH = 'home/fabstir/operators/acme/catalogue.json';

test('a retryable directory failure is 503, never 404', () => {
  const err = new S5DirectoryLoadError('registry 404 for known directory', {
    path: 'home/fabstir/operators',
    publicKey: 'ed01aabb',
    retryable: true,
  });
  const r = classifyS5ReadError(err, PATH);

  assert.strictEqual(r.status, 503, 'a transient failure reported as 404 gets recorded as absence');
  assert.strictEqual(r.headers['Retry-After'], '2');
  assert.strictEqual(r.body.retryable, true);
});

test('a structural directory failure is 502, and a retry will not help', () => {
  const err = new S5DirectoryLoadError('directory is unreadable', {
    path: 'home/fabstir/operators',
    publicKey: 'ed01aabb',
    retryable: false,
  });
  const r = classifyS5ReadError(err, PATH);

  assert.strictEqual(r.status, 502);
  assert.strictEqual(r.body.retryable, false);
  assert.strictEqual(r.headers['Retry-After'], undefined, 'do not invite a retry that cannot succeed');
});

test('the failing directory is reported separately from the requested path', () => {
  // repairDirectory() takes the directory that failed, which is an ancestor of
  // the path asked for. Losing that attribution makes the error unactionable.
  const err = new S5DirectoryLoadError('boom', {
    path: 'home/fabstir/operators',
    publicKey: 'ed01aabb',
    retryable: true,
  });
  const r = classifyS5ReadError(err, PATH);

  assert.strictEqual(r.body.path, PATH);
  assert.strictEqual(r.body.failedPath, 'home/fabstir/operators');
  assert.strictEqual(r.body.publicKey, 'ed01aabb');
});

test('an ordinary error stays 404 — genuine absence is still absence', () => {
  const r = classifyS5ReadError(new Error('no such entry'), PATH);
  assert.strictEqual(r.status, 404);
  assert.strictEqual(r.body.message, 'no such entry');
});

test('a non-Error throw does not crash the classifier', () => {
  const r = classifyS5ReadError('kaboom', PATH);
  assert.strictEqual(r.status, 404);
  assert.strictEqual(r.body.message, 'kaboom');
});
