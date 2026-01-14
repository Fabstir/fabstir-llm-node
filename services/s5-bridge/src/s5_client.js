// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Enhanced S5.js Client Initialization
 *
 * Manages S5 P2P client lifecycle, identity, and portal registration
 */

// Polyfill browser APIs for Node.js environment
import 'fake-indexeddb/auto';
import { WebSocket } from 'ws';
import { TextEncoder, TextDecoder } from 'node:util';

// Make browser APIs global for S5.js
global.WebSocket = WebSocket;
global.TextEncoder = TextEncoder;
global.TextDecoder = TextDecoder;
// Note: global.crypto already exists in Node.js 20+

import { S5 } from '@julesl23/s5js';
import { FS5Advanced } from '@julesl23/s5js/advanced';
import { bridgeConfig } from './config.js';

let s5Instance = null;
let advancedInstance = null;
let initializationPromise = null;

/**
 * Initialize Enhanced S5.js client with P2P connectivity
 *
 * This function:
 * 1. Creates S5 instance with initial P2P peers
 * 2. Recovers identity from seed phrase
 * 3. Registers with S5 portal
 * 4. Initializes filesystem
 *
 * @returns {Promise<S5>} Initialized S5 client instance
 * @throws {Error} if initialization fails
 */
export async function initializeS5Client() {
  // Return existing instance if already initialized
  if (s5Instance) {
    return s5Instance;
  }

  // If initialization is in progress, wait for it
  if (initializationPromise) {
    return initializationPromise;
  }

  console.log('🚀 Initializing Enhanced S5.js client...');

  initializationPromise = (async () => {
    try {
      // Step 1: Create S5 instance with P2P peers
      console.log(`📡 Connecting to ${bridgeConfig.initialPeers.length} P2P peer(s)...`);
      const s5 = await S5.create({
        initialPeers: bridgeConfig.initialPeers,
      });

      console.log('✅ S5 instance created');

      // Step 2: Recover identity from seed phrase (required even for read-only operations)
      if (bridgeConfig.seedPhrase) {
        console.log('🔐 Recovering identity from seed phrase...');
        await s5.recoverIdentityFromSeedPhrase(bridgeConfig.seedPhrase);
        console.log('✅ Identity recovered');
      } else {
        console.warn('⚠️  No seed phrase configured - filesystem operations will fail');
      }

      // Step 3: Register with S5 portal (check if account exists)
      let accountReady = false;
      if (bridgeConfig.portalUrl && bridgeConfig.registerWithPortal !== false) {
        try {
          console.log(`🌐 Attempting portal registration: ${bridgeConfig.portalUrl}`);
          await s5.registerOnNewPortal(bridgeConfig.portalUrl);
          console.log('✅ Portal registration complete (new account)');
          accountReady = true;
        } catch (error) {
          // "User already has an account" means the account is registered - this is SUCCESS!
          if (error.message && error.message.includes('already has an account')) {
            console.log('✅ Account already registered on portal (using existing account)');
            accountReady = true;
          } else {
            console.warn('⚠️  Portal registration failed');
            console.warn(`   Error: ${error.message}`);
            console.log('📥 Bridge will operate in read-only mode (downloads only)');
          }
        }
      }

      // Step 4: Initialize filesystem if account is ready
      if (accountReady) {
        console.log('🔧 Initializing filesystem for read/write operations...');
        try {
          await s5.fs.ensureIdentityInitialized();
          console.log('✅ Filesystem initialized - uploads and downloads ready');
        } catch (fsError) {
          console.warn('⚠️  Filesystem initialization failed:', fsError.message);
          console.log('📥 Bridge will operate in read-only mode');
        }
      } else {
        console.log('📥 Skipping filesystem initialization (read-only mode)');
        console.log('✅ Bridge ready for downloading existing S5 content');
      }

      // CRITICAL: Verify portal accounts are configured for uploads
      const accountCount = Object.keys(s5.apiWithIdentity?.accountConfigs || {}).length;
      if (accountCount > 0) {
        const accountIds = Object.keys(s5.apiWithIdentity.accountConfigs);
        console.log(`✅ Portal accounts configured: ${accountCount}`);
        accountIds.forEach(id => console.log(`   - ${id}`));
        console.log('🌐 Uploads will be stored on S5 network');
      } else {
        console.error('🚨 NO PORTAL ACCOUNTS CONFIGURED!');
        console.error('   Uploads will be stored LOCALLY ONLY, NOT on S5 network.');
        console.error('   Content will NOT be downloadable by CID.');
        console.error('   Check S5_SEED_PHRASE and portal registration.');
      }

      s5Instance = s5;

      // Initialize Advanced API for CID operations
      try {
        advancedInstance = new FS5Advanced(s5.fs);
        console.log('✅ Advanced CID API initialized');
      } catch (advError) {
        console.warn('⚠️  Failed to initialize Advanced API:', advError.message);
      }

      console.log('🎉 Enhanced S5.js client fully initialized');

      return s5;
    } catch (error) {
      console.error('❌ Failed to initialize S5 client:', error);
      initializationPromise = null; // Reset so retry is possible
      throw error;
    }
  })();

  return initializationPromise;
}

/**
 * Get the initialized S5 client instance
 *
 * @returns {S5 | null} S5 client or null if not initialized
 */
export function getS5Client() {
  return s5Instance;
}

/**
 * Get the FS5Advanced client for CID operations
 *
 * @returns {FS5Advanced | null} Advanced client or null if not initialized
 */
export function getAdvancedClient() {
  return advancedInstance;
}

/**
 * Check if S5 client is initialized
 *
 * @returns {boolean} true if client is ready
 */
export function isS5Initialized() {
  return s5Instance !== null;
}

/**
 * Get P2P connectivity status
 *
 * @returns {Object} Status information
 */
export async function getS5Status() {
  if (!s5Instance) {
    return {
      initialized: false,
      connected: false,
      peerCount: 0,
      error: 'S5 client not initialized',
    };
  }

  try {
    // Note: Enhanced S5.js may not expose peer count directly
    // This is a placeholder for status information
    return {
      initialized: true,
      connected: true,
      peerCount: bridgeConfig.initialPeers.length,
      portal: bridgeConfig.portalUrl,
    };
  } catch (error) {
    return {
      initialized: true,
      connected: false,
      peerCount: 0,
      error: error.message,
    };
  }
}

/**
 * Shutdown S5 client gracefully
 */
export async function shutdownS5Client() {
  if (s5Instance) {
    console.log('🛑 Shutting down S5 client...');
    // Note: Enhanced S5.js may not have explicit shutdown
    // Clear instance references
    s5Instance = null;
    advancedInstance = null;
    initializationPromise = null;
    console.log('✅ S5 client shutdown complete');
  }
}
