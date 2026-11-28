// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Enhanced S5.js Bridge Service Configuration
 *
 * Environment variables for configuring P2P storage access
 */

import { config } from 'dotenv';
import { regionErrors } from './region_guard.js';

// Load environment variables
config();

export const bridgeConfig = {
  // HTTP Server Configuration
  port: parseInt(process.env.BRIDGE_PORT || '5522', 10),
  host: process.env.BRIDGE_HOST || 'localhost',

  // S5 Identity Configuration
  seedPhrase: process.env.S5_SEED_PHRASE || '',

  // Host Ethereum Configuration (for on-chain verified S5 registration)
  hostPrivateKey: process.env.HOST_PRIVATE_KEY || '',

  // Storage region (SR1 D3): required, no default. See region_guard.js.
  region: (process.env.S5_REGION || '').trim(),

  // S5 Network Configuration (Platformless AI Portal - Sia Storage). The
  // defaults are the Global portal and peer; any other region must set both,
  // and the region guard refuses a portal or peer outside S5_REGION.
  portalUrl: process.env.S5_PORTAL_URL || 'https://s5.platformlessai.ai',
  initialPeers: (process.env.S5_INITIAL_PEERS ||
    'wss://z2DcjTLqfj6PTMsDbFfgtuHtYmrKeibFTkvqY8QZeyR3YmE@s5.platformlessai.ai/s5/p2p'
  ).split(',').map(p => p.trim()).filter(Boolean),

  // Logging Configuration
  logLevel: process.env.LOG_LEVEL || 'info',
  prettyLogs: process.env.PRETTY_LOGS === 'true',

  // Performance Configuration
  requestTimeout: parseInt(process.env.REQUEST_TIMEOUT_MS || '30000', 10),
  maxContentLength: parseInt(process.env.MAX_CONTENT_LENGTH || '104857600', 10), // 100MB default
};

/**
 * Validate configuration on startup
 * @throws {Error} if configuration is invalid
 */
export function validateConfig() {
  const errors = [];

  if (!bridgeConfig.seedPhrase) {
    errors.push('S5_SEED_PHRASE is required but not set');
  }

  errors.push(...regionErrors(bridgeConfig));

  if (bridgeConfig.port < 1 || bridgeConfig.port > 65535) {
    errors.push(`BRIDGE_PORT must be between 1-65535, got: ${bridgeConfig.port}`);
  }

  if (errors.length > 0) {
    throw new Error(`Configuration validation failed:\n${errors.join('\n')}`);
  }
}

/**
 * Print configuration summary (without sensitive data)
 */
export function printConfigSummary() {
  console.log('📋 Bridge Configuration:');
  console.log(`   Host: ${bridgeConfig.host}`);
  console.log(`   Port: ${bridgeConfig.port}`);
  console.log(`   Region: ${bridgeConfig.region}`);
  console.log(`   Portal: ${bridgeConfig.portalUrl}`);
  console.log(`   Peers: ${bridgeConfig.initialPeers.length} configured`);
  console.log(`   Identity: ${bridgeConfig.seedPhrase ? '✅ Configured' : '❌ Missing'}`);
  console.log(`   Host Key: ${bridgeConfig.hostPrivateKey ? '✅ Configured' : '⚠️ Missing (on-chain registration disabled)'}`);
  console.log(`   Log Level: ${bridgeConfig.logLevel}`);
  console.log(`   Request Timeout: ${bridgeConfig.requestTimeout}ms`);
  console.log(`   Max Content: ${(bridgeConfig.maxContentLength / 1024 / 1024).toFixed(2)}MB`);
}
