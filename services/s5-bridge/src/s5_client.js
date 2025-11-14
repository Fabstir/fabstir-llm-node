// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Enhanced S5.js Client Initialization
 *
 * Manages S5 P2P client lifecycle, identity, and portal registration
 */

import { S5 } from '@julesl23/s5js';
import { bridgeConfig } from './config.js';

let s5Instance = null;
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

      // Step 2: Recover identity from seed phrase
      if (bridgeConfig.seedPhrase) {
        console.log('🔐 Recovering identity from seed phrase...');
        await s5.recoverIdentityFromSeedPhrase(bridgeConfig.seedPhrase);
        console.log('✅ Identity recovered');
      } else {
        throw new Error('Seed phrase is required but not configured');
      }

      // Step 3: Register with S5 portal
      console.log(`🌐 Registering with S5 portal: ${bridgeConfig.portalUrl}`);
      await s5.registerOnNewPortal(bridgeConfig.portalUrl);
      console.log('✅ Portal registration complete');

      // Step 4: Initialize filesystem
      console.log('📁 Initializing S5 filesystem...');
      await s5.fs.ensureIdentityInitialized();
      console.log('✅ Filesystem initialized');

      s5Instance = s5;
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
    // Clear instance reference
    s5Instance = null;
    initializationPromise = null;
    console.log('✅ S5 client shutdown complete');
  }
}
