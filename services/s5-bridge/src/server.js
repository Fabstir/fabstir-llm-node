#!/usr/bin/env node
// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Enhanced S5.js Bridge Service - Main Server
 *
 * HTTP server that exposes Enhanced S5.js P2P storage via REST API
 */

import Fastify from 'fastify';
import cors from '@fastify/cors';
import { bridgeConfig, validateConfig, printConfigSummary } from './config.js';
import { initializeS5Client, shutdownS5Client } from './s5_client.js';
import { registerRoutes } from './routes.js';

/**
 * Create and configure Fastify server
 */
async function createServer() {
  const fastify = Fastify({
    logger: {
      level: bridgeConfig.logLevel,
      transport: bridgeConfig.prettyLogs
        ? {
            target: 'pino-pretty',
            options: {
              colorize: true,
              translateTime: 'HH:MM:ss',
              ignore: 'pid,hostname',
            },
          }
        : undefined,
    },
    bodyLimit: bridgeConfig.maxContentLength,
    requestTimeout: bridgeConfig.requestTimeout,
  });

  // Enable CORS (localhost only for security)
  await fastify.register(cors, {
    origin: ['http://localhost:*', 'http://127.0.0.1:*'],
    methods: ['GET', 'PUT', 'DELETE', 'HEAD', 'OPTIONS'],
  });

  // Add binary content parser for file uploads
  fastify.addContentTypeParser(
    'application/octet-stream',
    { parseAs: 'buffer' },
    (req, body, done) => {
      done(null, body);
    }
  );

  // Register API routes
  await registerRoutes(fastify);

  return fastify;
}

/**
 * Start the bridge service
 */
async function start() {
  try {
    // Print banner
    console.log('╔════════════════════════════════════════╗');
    console.log('║  Enhanced S5.js Bridge Service v1.2.0  ║');
    console.log('║  P2P Storage Bridge for Fabstir Node  ║');
    console.log('╚════════════════════════════════════════╝');
    console.log('');

    // Validate configuration
    console.log('🔧 Validating configuration...');
    validateConfig();
    printConfigSummary();
    console.log('');

    // Initialize S5 client
    console.log('🌐 Initializing Enhanced S5.js client...');
    await initializeS5Client();
    console.log('');

    // Create HTTP server
    console.log('🚀 Starting HTTP server...');
    const fastify = await createServer();

    // Start listening
    await fastify.listen({
      port: bridgeConfig.port,
      host: bridgeConfig.host,
    });

    console.log('');
    console.log('✅ Bridge service is ready!');
    console.log(`📡 HTTP API: http://${bridgeConfig.host}:${bridgeConfig.port}`);
    console.log('');
    console.log('Available endpoints:');
    console.log(`   GET    /health              - Health check`);
    console.log(`   GET    /s5/fs/{path}        - Download file`);
    console.log(`   PUT    /s5/fs/{path}        - Upload file`);
    console.log(`   DELETE /s5/fs/{path}        - Delete file`);
    console.log(`   GET    /s5/fs/{path}/       - List directory`);
    console.log('');

    // Graceful shutdown
    const signals = ['SIGINT', 'SIGTERM'];
    signals.forEach((signal) => {
      process.on(signal, async () => {
        console.log(`\n\n🛑 Received ${signal}, shutting down gracefully...`);
        await fastify.close();
        await shutdownS5Client();
        console.log('👋 Goodbye!');
        process.exit(0);
      });
    });
  } catch (error) {
    console.error('❌ Fatal error during startup:', error);
    process.exit(1);
  }
}

// Handle uncaught errors
process.on('unhandledRejection', (reason, promise) => {
  console.error('❌ Unhandled Rejection at:', promise, 'reason:', reason);
  process.exit(1);
});

process.on('uncaughtException', (error) => {
  console.error('❌ Uncaught Exception:', error);
  process.exit(1);
});

// Start the server
start();
