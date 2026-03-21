// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * HTTP API Routes for Enhanced S5.js Bridge
 *
 * Provides REST API for S5 filesystem operations
 */

import { getS5Client, getS5Status, getAdvancedClient } from './s5_client.js';
import { BlobIdentifier } from '@julesl23/s5js/dist/src/identifier/blob.js';
import { MULTIHASH_BLAKE3 } from '@julesl23/s5js/dist/src/constants.js';

/**
 * Register all routes with Fastify server
 *
 * @param {import('fastify').FastifyInstance} fastify
 */
export async function registerRoutes(fastify) {
  // Health check endpoint
  fastify.get('/health', async (request, reply) => {
    const status = await getS5Status();

    reply.code(status.connected ? 200 : 503).send({
      status: status.connected ? 'healthy' : 'unhealthy',
      service: 's5-bridge',
      timestamp: new Date().toISOString(),
      ...status,
    });
  });

  // GET /s5/fs/{path} - Download file from S5
  fastify.get('/s5/fs/*', async (request, reply) => {
    const s5 = getS5Client();
    if (!s5) {
      return reply.code(503).send({
        error: 'S5 client not initialized',
      });
    }

    // Extract path from URL (everything after /s5/fs/)
    const path = request.url.replace('/s5/fs/', '');

    try {
      fastify.log.info({ path }, 'Downloading file from S5');

      const result = await s5.fs.get(path);

      fastify.log.debug({ resultType: typeof result, resultConstructor: result?.constructor?.name }, 'Got result from s5.fs.get()');

      // Handle different return types from s5.fs.get()
      let data;
      if (result instanceof Uint8Array) {
        data = Buffer.from(result);
      } else if (Buffer.isBuffer(result)) {
        data = result;
      } else if (result && result.data) {
        // If result is an object with .data property
        data = Buffer.from(result.data);
      } else if (typeof result === 'string') {
        data = Buffer.from(result);
      } else if (ArrayBuffer.isView(result)) {
        data = Buffer.from(result.buffer, result.byteOffset, result.byteLength);
      } else {
        // Last resort - try to convert to buffer
        fastify.log.warn({ result }, 'Unexpected result type from s5.fs.get()');
        data = Buffer.from(JSON.stringify(result));
      }

      // Return raw bytes
      reply
        .header('Content-Type', 'application/octet-stream')
        .header('X-S5-Path', path)
        .send(data);
    } catch (error) {
      fastify.log.error({ path, error: error.message }, 'Failed to download file');
      reply.code(404).send({
        error: 'File not found or download failed',
        path,
        message: error.message,
      });
    }
  });

  // PUT /s5/fs/{path} - Upload file to S5
  fastify.put('/s5/fs/*', async (request, reply) => {
    const s5 = getS5Client();
    const advanced = getAdvancedClient();

    fastify.log.info('📤 [S5-UPLOAD] PUT request received');

    if (!s5) {
      fastify.log.error('📤 [S5-UPLOAD] ❌ S5 client not initialized');
      return reply.code(503).send({
        error: 'S5 client not initialized',
      });
    }

    // CRITICAL: Verify portal accounts are configured for network uploads
    // Without portal accounts, content is stored locally but NOT uploaded to S5 network
    const hasIdentity = !!s5.apiWithIdentity;
    const accountConfigs = s5.apiWithIdentity?.accountConfigs || {};
    const accountCount = Object.keys(accountConfigs).length;
    const accountIds = Object.keys(accountConfigs);

    fastify.log.info({
      hasIdentity,
      accountCount,
      accountIds,
    }, '📤 [S5-UPLOAD] S5 client state check');

    if (accountCount === 0) {
      fastify.log.error('📤 [S5-UPLOAD] 🚨 NO PORTAL ACCOUNTS - uploads will NOT reach S5 network!');
      return reply.code(503).send({
        error: 'S5 portal not configured',
        message: 'No portal accounts available. Content would be stored locally only, not on S5 network. Configure S5_SEED_PHRASE and restart the bridge.',
        debug: { hasIdentity, accountCount },
      });
    }

    // Extract path from URL
    const path = request.url.replace('/s5/fs/', '');
    const requestId = `req-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;

    try {
      // Get raw body bytes
      const data = request.body;

      if (!data || data.length === 0) {
        fastify.log.warn({ requestId, path }, '📤 [S5-UPLOAD] ❌ Empty request body');
        return reply.code(400).send({
          error: 'Request body is empty',
        });
      }

      fastify.log.info({
        requestId,
        path,
        size: data.length,
        portalAccounts: accountCount,
        portalIds: accountIds,
      }, '📤 [S5-UPLOAD] Starting upload to S5 network');

      // Store the file - this uploads blob AND updates directory structure
      const uploadStartTime = Date.now();
      fastify.log.debug({ requestId, path }, '📤 [S5-UPLOAD] Calling s5.fs.put()...');

      await s5.fs.put(path, new Uint8Array(data));

      const uploadDuration = Date.now() - uploadStartTime;
      fastify.log.info({
        requestId,
        path,
        uploadDurationMs: uploadDuration
      }, '📤 [S5-UPLOAD] ✅ s5.fs.put() completed');

      // Get the CID using Advanced API with BlobIdentifier format
      // BlobIdentifier format (~59 chars) includes file size and is REQUIRED by S5 portals
      // Raw hash format (53 chars) from pathToCID() is rejected by portals
      let cid = null;
      let rawHashHex = null;

      if (advanced) {
        try {
          fastify.log.debug({ requestId, path }, '📤 [S5-UPLOAD] Getting CID via Advanced API...');

          // pathToCID() returns raw 32-byte BLAKE3 hash
          const rawHash = await advanced.pathToCID(path);
          rawHashHex = Buffer.from(rawHash).toString('hex');

          // Construct 33-byte hash with BLAKE3 multihash prefix (0x1e)
          const hashWithPrefix = new Uint8Array(33);
          hashWithPrefix[0] = MULTIHASH_BLAKE3;  // 0x1e
          hashWithPrefix.set(rawHash, 1);

          // Create BlobIdentifier with hash and file size
          const blobId = new BlobIdentifier(hashWithPrefix, data.length);
          cid = blobId.toBase32();  // Returns ~59 char CID (base32 with 'b' prefix)

          fastify.log.info({
            requestId,
            path,
            cid,
            cidLength: cid.length,
            rawHashHex,
            size: data.length,
          }, '📤 [S5-UPLOAD] ✅ BlobIdentifier CID generated');
        } catch (cidError) {
          fastify.log.error({
            requestId,
            path,
            error: cidError.message,
            stack: cidError.stack,
          }, '📤 [S5-UPLOAD] ❌ Failed to get CID from Advanced API');
        }
      } else {
        fastify.log.warn({ requestId, path }, '📤 [S5-UPLOAD] ⚠️ Advanced API not available');
      }

      // Verify CID was generated
      if (!cid) {
        fastify.log.error({ requestId, path }, '📤 [S5-UPLOAD] ❌ Upload succeeded but no CID generated');
        return reply.code(500).send({
          error: 'Upload incomplete',
          message: 'File stored but CID generation failed. Content may not be retrievable by CID.',
          path,
          debug: { requestId, hasAdvancedApi: !!advanced },
        });
      }

      const totalDuration = Date.now() - uploadStartTime;
      fastify.log.info({
        requestId,
        path,
        cid,
        cidLength: cid.length,
        size: data.length,
        totalDurationMs: totalDuration,
        portalAccount: accountIds[0],
      }, '📤 [S5-UPLOAD] ✅ UPLOAD COMPLETE - Content stored on S5 network');

      reply.code(201).send({
        success: true,
        path,
        size: data.length,
        cid,  // Return the S5 CID in proper format
        networkUploaded: true,  // Flag to confirm blob was uploaded to network
        debug: {
          requestId,
          uploadDurationMs: totalDuration,
          portalAccount: accountIds[0],
          rawHashHex,
        },
      });
    } catch (error) {
      fastify.log.error({
        requestId,
        path,
        error: error.message,
        stack: error.stack,
        errorType: error.constructor.name,
      }, '📤 [S5-UPLOAD] ❌ UPLOAD FAILED');

      reply.code(500).send({
        error: 'Upload failed',
        path,
        message: error.message,
        debug: { requestId, errorType: error.constructor.name },
      });
    }
  });

  // DELETE /s5/fs/{path} - Delete file from S5
  fastify.delete('/s5/fs/*', async (request, reply) => {
    const s5 = getS5Client();
    if (!s5) {
      return reply.code(503).send({
        error: 'S5 client not initialized',
      });
    }

    // Extract path from URL
    const path = request.url.replace('/s5/fs/', '');

    try {
      fastify.log.info({ path }, 'Deleting file from S5');

      await s5.fs.delete(path);

      reply.code(204).send();
    } catch (error) {
      fastify.log.error({ path, error: error.message }, 'Failed to delete file');
      reply.code(500).send({
        error: 'Delete failed',
        path,
        message: error.message,
      });
    }
  });

  // NOTE: Directory listing route (/s5/fs/*/) disabled
  // Wildcard pattern /s5/fs/*/ is invalid in Fastify (wildcard must be last character)
  // TODO: Implement directory listing with query parameter instead (e.g., /s5/fs/*?list=true)

  // =========================================================================
  // S5 Portal Compatibility Routes (v8.26.1+)
  //
  // These routes implement the standard S5 portal API surface so that external
  // services (e.g. the transcoder sidecar) can use this bridge as PORTAL_URL.
  // The transcoder expects:
  //   - POST /s5/upload/tus (TUS upload protocol)
  //   - GET  /s5/blob/{cid} (download by CID)
  // =========================================================================

  // GET /s5/blob/{cid} - Download blob by CID (S5 portal compatibility)
  // The transcoder downloads source videos via this endpoint.
  // Uses S5.js P2P download (downloadByCID) — no portal auth required.
  fastify.get('/s5/blob/:cid', async (request, reply) => {
    const s5 = getS5Client();
    if (!s5) {
      return reply.code(503).send({ error: 'S5 client not initialized' });
    }

    const { cid } = request.params;
    try {
      fastify.log.info({ cid }, '📥 [S5-BLOB] Downloading blob by CID via P2P');

      // Use the authenticated S5.js client to download via P2P network
      // This uses the seed-phrase identity and discovers signed URLs automatically
      const result = await s5.apiWithIdentity.downloadByCID(cid);

      let data;
      if (result instanceof Uint8Array) {
        data = Buffer.from(result);
      } else if (Buffer.isBuffer(result)) {
        data = result;
      } else if (result && result.data) {
        data = Buffer.from(result.data);
      } else {
        data = Buffer.from(result);
      }

      fastify.log.info({ cid, size: data.length }, '📥 [S5-BLOB] ✅ Blob downloaded via P2P');

      reply
        .header('Content-Type', 'application/octet-stream')
        .header('Content-Length', data.length)
        .send(data);
    } catch (error) {
      fastify.log.error({ cid, error: error.message }, '📥 [S5-BLOB] ❌ Download failed');
      reply.code(502).send({ error: 'Blob download failed', cid, message: error.message });
    }
  });

  // GET /api/locations/:hash - S5 portal locations API (encrypted download compatibility)
  // The transcoder calls this for encrypted files to get download URLs for blob chunks.
  // We proxy to the upstream S5 portal which knows the actual chunk locations.
  fastify.get('/api/locations/:hash', async (request, reply) => {
    const s5 = getS5Client();
    if (!s5) {
      return reply.code(503).send({ error: 'S5 client not initialized' });
    }

    const { hash } = request.params;
    const types = request.query.types || '5,3';

    try {
      fastify.log.info({ hash, types }, '📥 [S5-LOCATIONS] Looking up locations');

      // Return a locations response pointing to our own /s5/blob/ endpoint.
      // The transcoder will GET each URL in parts[] to download chunks.
      // Since we serve /s5/blob/ via P2P (downloadByCID), this creates a
      // self-referencing loop that works without external portal auth.
      //
      // For encrypted files, the CID in the hash is the encrypted blob's hash.
      // The transcoder downloads all parts, concatenates, then decrypts locally.
      const selfUrl = `http://${request.headers.host || 'localhost:5522'}`;
      const blobUrl = `${selfUrl}/s5/download/${hash}`;

      fastify.log.info({ hash, blobUrl }, '📥 [S5-LOCATIONS] ✅ Returning self-referencing location');

      reply.send({
        locations: [
          { parts: [blobUrl] }
        ]
      });
    } catch (error) {
      fastify.log.error({ hash, error: error.message }, '📥 [S5-LOCATIONS] ❌ Lookup failed');
      reply.code(502).send({ error: 'Locations lookup failed', message: error.message });
    }
  });

  // GET /s5/download/:hash - Download blob by raw base64url hash (encrypted download support)
  // The transcoder calls /api/locations/:hash which returns a URL pointing here.
  // The :hash param is a base64url-encoded byte array (type prefix + hash).
  fastify.get('/s5/download/:hash', async (request, reply) => {
    const s5 = getS5Client();
    if (!s5) {
      return reply.code(503).send({ error: 'S5 client not initialized' });
    }

    const { hash } = request.params;
    try {
      fastify.log.info({ hash }, '📥 [S5-DOWNLOAD] Downloading blob by raw hash');

      const hashBytes = Buffer.from(hash, 'base64url');
      const result = await s5.apiWithIdentity.downloadBlobAsBytes(hashBytes);
      const data = Buffer.from(result);

      fastify.log.info({ hash, size: data.length }, '📥 [S5-DOWNLOAD] ✅ Blob downloaded');

      reply
        .header('Content-Type', 'application/octet-stream')
        .header('Content-Length', data.length)
        .send(data);
    } catch (error) {
      fastify.log.error({ hash, error: error.message }, '📥 [S5-DOWNLOAD] ❌ Download failed');
      reply.code(502).send({ error: 'Blob download failed', hash, message: error.message });
    }
  });

  // In-memory TUS upload store (maps upload ID → { data, offset, size, path })
  const tusUploads = new Map();

  // POST /s5/upload/tus - TUS: Create upload (S5 portal compatibility)
  // The transcoder creates a TUS upload, then PATCHes data in chunks.
  fastify.post('/s5/upload/tus', async (request, reply) => {
    const s5 = getS5Client();
    if (!s5) {
      return reply.code(503).send({ error: 'S5 client not initialized' });
    }

    const uploadLength = parseInt(request.headers['upload-length'] || '0', 10);
    const uploadId = `tus-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;

    fastify.log.info({ uploadId, uploadLength }, '📤 [S5-TUS] Creating upload');

    tusUploads.set(uploadId, {
      data: Buffer.alloc(uploadLength),
      offset: 0,
      size: uploadLength,
    });

    reply
      .code(201)
      .header('Tus-Resumable', '1.0.0')
      .header('Upload-Offset', '0')
      .header('Location', `http://${request.headers.host || 'localhost:5522'}/s5/upload/tus/${uploadId}`)
      .send();
  });

  // PATCH /s5/upload/tus/:id - TUS: Upload chunk
  fastify.patch('/s5/upload/tus/:id', async (request, reply) => {
    const s5 = getS5Client();
    const advanced = getAdvancedClient();
    if (!s5) {
      return reply.code(503).send({ error: 'S5 client not initialized' });
    }

    const { id } = request.params;
    const upload = tusUploads.get(id);
    if (!upload) {
      return reply.code(404).send({ error: 'Upload not found' });
    }

    const offset = parseInt(request.headers['upload-offset'] || '0', 10);
    const chunk = request.body;

    if (!chunk || chunk.length === 0) {
      return reply.code(400).send({ error: 'Empty chunk' });
    }

    // Copy chunk into buffer at offset
    chunk.copy(upload.data, offset);
    upload.offset = offset + chunk.length;

    fastify.log.info({ id, offset, chunkSize: chunk.length, newOffset: upload.offset, totalSize: upload.size }, '📤 [S5-TUS] Chunk received');

    // If upload is complete, store to S5
    if (upload.offset >= upload.size) {
      try {
        const path = `tus-uploads/${id}`;
        fastify.log.info({ id, path, size: upload.data.length }, '📤 [S5-TUS] Upload complete, storing to S5');

        await s5.fs.put(path, new Uint8Array(upload.data));

        // Generate CID
        let cid = null;
        if (advanced) {
          const rawHash = await advanced.pathToCID(path);
          const hashWithPrefix = new Uint8Array(33);
          hashWithPrefix[0] = MULTIHASH_BLAKE3;
          hashWithPrefix.set(rawHash, 1);
          const blobId = new BlobIdentifier(hashWithPrefix, upload.data.length);
          cid = blobId.toBase32();
        }

        tusUploads.delete(id);
        fastify.log.info({ id, cid }, '📤 [S5-TUS] ✅ Stored to S5');

        reply
          .code(204)
          .header('Tus-Resumable', '1.0.0')
          .header('Upload-Offset', String(upload.offset))
          .header('X-S5-CID', cid || '')
          .send();
      } catch (error) {
        tusUploads.delete(id);
        fastify.log.error({ id, error: error.message }, '📤 [S5-TUS] ❌ S5 store failed');
        reply.code(500).send({ error: 'Upload storage failed', message: error.message });
      }
    } else {
      // More chunks expected
      reply
        .code(204)
        .header('Tus-Resumable', '1.0.0')
        .header('Upload-Offset', String(upload.offset))
        .send();
    }
  });

  // HEAD /s5/upload/tus/:id - TUS: Check upload status
  fastify.head('/s5/upload/tus/:id', async (request, reply) => {
    const { id } = request.params;
    const upload = tusUploads.get(id);
    if (!upload) {
      return reply.code(404).send();
    }

    reply
      .code(200)
      .header('Tus-Resumable', '1.0.0')
      .header('Upload-Offset', String(upload.offset))
      .header('Upload-Length', String(upload.size))
      .send();
  });

  // Root endpoint
  fastify.get('/', async (request, reply) => {
    reply.send({
      service: 'Enhanced S5.js Bridge',
      version: '1.2.0',
      endpoints: {
        health: 'GET /health',
        download: 'GET /s5/fs/{path}',
        upload: 'PUT /s5/fs/{path}',
        delete: 'DELETE /s5/fs/{path}',
        blobDownload: 'GET /s5/blob/{cid}',
        blobDownloadByHash: 'GET /s5/download/{hash}',
        tusUpload: 'POST /s5/upload/tus',
      },
    });
  });
}
