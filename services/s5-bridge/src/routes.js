// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * HTTP API Routes for Enhanced S5.js Bridge
 *
 * Provides REST API for S5 filesystem operations
 */

import { getS5Client, getS5Status } from './s5_client.js';

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

      const data = await s5.fs.get(path);

      // Return raw bytes
      reply
        .header('Content-Type', 'application/octet-stream')
        .header('X-S5-Path', path)
        .send(Buffer.from(data));
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
    if (!s5) {
      return reply.code(503).send({
        error: 'S5 client not initialized',
      });
    }

    // Extract path from URL
    const path = request.url.replace('/s5/fs/', '');

    try {
      // Get raw body bytes
      const data = request.body;

      if (!data || data.length === 0) {
        return reply.code(400).send({
          error: 'Request body is empty',
        });
      }

      fastify.log.info({ path, size: data.length }, 'Uploading file to S5');

      await s5.fs.put(path, new Uint8Array(data));

      reply.code(201).send({
        success: true,
        path,
        size: data.length,
      });
    } catch (error) {
      fastify.log.error({ path, error: error.message }, 'Failed to upload file');
      reply.code(500).send({
        error: 'Upload failed',
        path,
        message: error.message,
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

  // Root endpoint
  fastify.get('/', async (request, reply) => {
    reply.send({
      service: 'Enhanced S5.js Bridge',
      version: '1.0.0',
      endpoints: {
        health: 'GET /health',
        download: 'GET /s5/fs/{path}',
        upload: 'PUT /s5/fs/{path}',
        delete: 'DELETE /s5/fs/{path}',
        list: 'GET /s5/fs/{path}/',
      },
    });
  });
}
