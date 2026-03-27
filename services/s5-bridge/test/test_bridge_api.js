// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Enhanced S5.js Bridge API Tests
 *
 * Tests HTTP endpoints for S5 filesystem operations
 */

import { test, describe, before, after } from 'node:test';
import assert from 'node:assert';

/**
 * Note: These are integration tests that require:
 * 1. S5_SEED_PHRASE environment variable set
 * 2. Bridge service running on localhost:5522
 * 3. Network connectivity to S5 peers and portal
 *
 * To run tests:
 *   export S5_SEED_PHRASE="your twelve word seed phrase here"
 *   npm start  # In one terminal
 *   npm test   # In another terminal
 */

const BRIDGE_URL = process.env.BRIDGE_URL || 'http://localhost:5522';
const TEST_PATH = 'home/test-bridge/test-file.txt';
const TEST_DATA = Buffer.from('Hello from bridge test!');

describe('Enhanced S5.js Bridge API', () => {
  before(async () => {
    // Wait for bridge to be ready
    let ready = false;
    let attempts = 0;
    const maxAttempts = 10;

    while (!ready && attempts < maxAttempts) {
      try {
        const response = await fetch(`${BRIDGE_URL}/health`);
        if (response.ok) {
          ready = true;
        }
      } catch (error) {
        attempts++;
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
    }

    if (!ready) {
      throw new Error('Bridge service not ready after 10 seconds');
    }
  });

  test('GET /health - should return healthy status', async () => {
    const response = await fetch(`${BRIDGE_URL}/health`);
    assert.strictEqual(response.status, 200);

    const data = await response.json();
    assert.strictEqual(data.status, 'healthy');
    assert.strictEqual(data.initialized, true);
    assert.strictEqual(data.connected, true);
  });

  test('GET / - should return service info', async () => {
    const response = await fetch(`${BRIDGE_URL}/`);
    assert.strictEqual(response.status, 200);

    const data = await response.json();
    assert.strictEqual(data.service, 'Enhanced S5.js Bridge');
    assert.ok(data.endpoints);
  });

  test('PUT /s5/fs/{path} - should upload file', async () => {
    const response = await fetch(`${BRIDGE_URL}/s5/fs/${TEST_PATH}`, {
      method: 'PUT',
      body: TEST_DATA,
      headers: {
        'Content-Type': 'application/octet-stream',
      },
    });

    assert.strictEqual(response.status, 201);

    const data = await response.json();
    assert.strictEqual(data.success, true);
    assert.strictEqual(data.path, TEST_PATH);
    assert.strictEqual(data.size, TEST_DATA.length);
  });

  test('GET /s5/fs/{path} - should download file', async () => {
    // Upload first
    await fetch(`${BRIDGE_URL}/s5/fs/${TEST_PATH}`, {
      method: 'PUT',
      body: TEST_DATA,
    });

    // Download
    const response = await fetch(`${BRIDGE_URL}/s5/fs/${TEST_PATH}`);
    assert.strictEqual(response.status, 200);

    const data = await response.arrayBuffer();
    assert.deepStrictEqual(Buffer.from(data), TEST_DATA);
  });

  test('GET /s5/fs/{path}/ - should list directory', async () => {
    const dirPath = 'home/test-bridge/';
    const response = await fetch(`${BRIDGE_URL}/s5/fs/${dirPath}`);

    // May be 200 with entries or empty array, depending on S5.js implementation
    assert.ok(response.status === 200 || response.status === 404);

    if (response.status === 200) {
      const data = await response.json();
      assert.strictEqual(data.path, dirPath);
      assert.ok(Array.isArray(data.entries));
    }
  });

  test('DELETE /s5/fs/{path} - should delete file', async () => {
    // Upload first
    await fetch(`${BRIDGE_URL}/s5/fs/${TEST_PATH}`, {
      method: 'PUT',
      body: TEST_DATA,
    });

    // Delete
    const response = await fetch(`${BRIDGE_URL}/s5/fs/${TEST_PATH}`, {
      method: 'DELETE',
    });

    assert.strictEqual(response.status, 204);
  });

  test('GET /s5/fs/{path} - should return 404 for non-existent file', async () => {
    const response = await fetch(
      `${BRIDGE_URL}/s5/fs/non-existent-file-${Date.now()}.txt`
    );

    assert.strictEqual(response.status, 404);

    const data = await response.json();
    assert.ok(data.error);
  });

  test('PUT /s5/fs/{path} - should reject empty body', async () => {
    const response = await fetch(`${BRIDGE_URL}/s5/fs/${TEST_PATH}`, {
      method: 'PUT',
      body: Buffer.alloc(0),
    });

    assert.strictEqual(response.status, 400);

    const data = await response.json();
    assert.ok(data.error);
  });

  test('POST+PATCH /s5/upload/tus — TUS upload completes with 204', async () => {
    const payload = Buffer.from('TUS upload test data');

    // POST to create TUS upload
    const createRes = await fetch(`${BRIDGE_URL}/s5/upload/tus`, {
      method: 'POST',
      headers: {
        'Upload-Length': String(payload.length),
        'Tus-Resumable': '1.0.0',
      },
    });
    assert.strictEqual(createRes.status, 201);
    const location = createRes.headers.get('location');
    assert.ok(location, 'Location header must be present');

    const uploadId = location.split('/').pop();

    // PATCH to send the data
    const patchRes = await fetch(`${BRIDGE_URL}/s5/upload/tus/${uploadId}`, {
      method: 'PATCH',
      headers: {
        'Content-Type': 'application/offset+octet-stream',
        'Upload-Offset': '0',
        'Tus-Resumable': '1.0.0',
      },
      body: payload,
    });
    assert.strictEqual(patchRes.status, 204, 'PATCH should return 204 (not 500)');
  });

  after(async () => {
    // Cleanup: Try to delete test file
    try {
      await fetch(`${BRIDGE_URL}/s5/fs/${TEST_PATH}`, {
        method: 'DELETE',
      });
    } catch (error) {
      // Ignore cleanup errors
    }
  });
});
