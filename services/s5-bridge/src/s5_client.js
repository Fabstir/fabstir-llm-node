// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Enhanced S5.js Client Initialization (v1.2.0)
 *
 * Manages S5 P2P client lifecycle, identity, and portal registration
 * Uses high-level S5.js APIs for signing and credential storage
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

import { S5, CHALLENGE_TYPE_REGISTER } from '@julesl23/s5js';
import { FS5Advanced } from '@julesl23/s5js/advanced';
import { ethers } from 'ethers';
import { bridgeConfig } from './config.js';

let s5Instance = null;
let advancedInstance = null;
let initializationPromise = null;

/**
 * Try to login to portal (for returning hosts with existing accounts)
 *
 * This attempts login using existing stored account credentials.
 * The S5.js library handles login automatically when setting up accounts.
 *
 * @param {S5} s5 - S5 instance with identity
 * @param {string} portalUrl - Portal URL
 * @returns {Promise<boolean>} true if login successful
 */
async function tryPortalLogin(s5, portalUrl) {
  try {
    console.log(`🔑 Checking for existing portal account: ${portalUrl}`);

    // Check if we already have an account configured for this portal
    const uri = new URL(portalUrl);
    const accountConfigs = s5.apiWithIdentity?.accountConfigs || {};

    for (const id of Object.keys(accountConfigs)) {
      if (id.startsWith(`${uri.host}:`)) {
        console.log('✅ Existing account found for this portal');
        return true;
      }
    }

    console.log('   No existing account for this portal');
    return false;
  } catch (error) {
    console.log(`   Login check error: ${error.message}`);
    return false;
  }
}

/**
 * Register with S5 portal via on-chain verification
 *
 * Flow:
 * 1. Generate purpose-specific seed for this portal
 * 2. Get S5 public key using high-level API
 * 3. Sign message with ETH key for on-chain verification
 * 4. Portal verifies ETH signature + checks NodeRegistry on-chain
 * 5. Portal returns S5 challenge
 * 6. Sign challenge with S5 key using high-level API
 * 7. Portal returns authToken
 * 8. Store credentials using high-level API
 *
 * @param {S5} s5 - S5 instance with identity
 * @param {string} portalUrl - Portal URL
 * @param {string} hostPrivateKey - Host's ETH private key
 * @returns {Promise<boolean>} true if registration successful
 */
async function tryOnChainRegistration(s5, portalUrl, hostPrivateKey) {
  try {
    console.log(`🌐 Registering with portal via on-chain verification: ${portalUrl}`);

    // Verify identity is available
    const identity = s5.identity;
    if (!identity) {
      console.error('❌ No S5 identity available - ensure seed phrase is configured');
      return false;
    }

    // Generate purpose-specific seed for this portal account
    const accountSeed = s5.node.crypto.generateSecureRandomBytes(32);
    const seedBase64 = Buffer.from(accountSeed).toString('base64')
      .replace(/\+/g, '-').replace(/\//g, '_').replace(/=/g, '');

    // Get S5 public key using high-level API
    const s5PubKey = await s5.getSigningPublicKey(seedBase64);
    console.log(`   S5 PubKey: ${s5PubKey.slice(0, 20)}...`);

    // Create ETH wallet for on-chain verification
    const ethWallet = new ethers.Wallet(hostPrivateKey);
    const message = `Register S5 account for ${ethWallet.address}`;
    const ethSignature = await ethWallet.signMessage(message);
    console.log(`   ETH Address: ${ethWallet.address}`);

    // Step 1: Request challenge from portal
    const challengeRes = await fetch(`${portalUrl}/s5/account/register-host`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        pubKey: s5PubKey,
        ethAddress: ethWallet.address,
        signature: ethSignature,
        message,
      }),
    });

    if (!challengeRes.ok) {
      const error = await challengeRes.json().catch(() => ({}));
      const errorMsg = error.error || `HTTP ${challengeRes.status}`;

      if (errorMsg.includes('not registered on NodeRegistry') || errorMsg.includes('not an active node')) {
        console.error('❌ Host not registered on NodeRegistry');
        console.error('   Register your host via CLI/TUI first, then restart the bridge');
      } else if (errorMsg.includes('Invalid signature')) {
        console.error('❌ ETH signature verification failed');
        console.error('   Check HOST_PRIVATE_KEY is correct');
      } else if (errorMsg.includes('already has an account')) {
        console.log('✅ Account already registered on portal');
        return true;
      } else {
        console.error(`❌ Portal challenge request failed: ${errorMsg}`);
      }
      return false;
    }

    const { challenge: challengeBase64 } = await challengeRes.json();
    console.log('   ✅ On-chain verification passed, got S5 challenge');

    // Step 2: Build challenge message and sign with high-level API
    const uri = new URL(portalUrl);
    const challengeBytes = Buffer.from(
      challengeBase64.replace(/-/g, '+').replace(/_/g, '/'),
      'base64'
    );
    const portalHostHash = await s5.node.crypto.hashBlake3(
      new TextEncoder().encode(uri.host)
    );
    const challengeMessage = new Uint8Array([
      CHALLENGE_TYPE_REGISTER,
      ...challengeBytes,
      ...portalHostHash,
    ]);

    // Sign using high-level API (returns base64url encoded signature)
    const signature = await s5.sign(challengeMessage, seedBase64);

    // Encode response as base64url
    const responseBase64 = Buffer.from(challengeMessage).toString('base64')
      .replace(/\+/g, '-').replace(/\//g, '_').replace(/=/g, '');

    // Step 3: Complete registration
    const registerRes = await fetch(`${portalUrl}/s5/account/register-host/complete`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        pubKey: s5PubKey,
        challenge: challengeBase64,
        response: responseBase64,
        signature: signature,
        ethAddress: ethWallet.address,
        ethSignature: ethSignature,
        message,
      }),
    });

    if (!registerRes.ok) {
      const error = await registerRes.json().catch(() => ({}));
      const errorMsg = error.error || `HTTP ${registerRes.status}`;

      if (errorMsg.includes('already has an account')) {
        console.log('✅ Account already registered on portal');
        return true;
      }

      console.error(`❌ Portal registration failed: ${errorMsg}`);
      return false;
    }

    const result = await registerRes.json();
    const authToken = result.authToken;

    if (!authToken) {
      console.error('❌ No auth token in registration response');
      return false;
    }

    console.log('✅ Portal registration complete (on-chain verified)');

    // Step 4: Store credentials using high-level API
    try {
      await s5.storePortalCredentials(portalUrl, seedBase64, authToken);
      console.log('   ✅ Credentials stored via S5.js API');
    } catch (storeError) {
      console.warn('⚠️  Could not store credentials via API:', storeError.message);
      console.log('   Attempting manual storage fallback...');

      // Fallback: Manual storage if high-level API fails
      const id = `${uri.host}:${seedBase64.slice(0, 16)}`;
      const accounts = s5.apiWithIdentity?.accounts || {
        'accounts': {},
        'active': [],
        'uploadOrder': { 'default': [] }
      };

      accounts['accounts'][id] = {
        'url': `${uri.protocol}//${uri.host}`,
        'seed': seedBase64,
        'createdAt': new Date().toISOString(),
      };
      accounts['active'].push(id);
      if (!accounts['uploadOrder']) accounts['uploadOrder'] = { 'default': [] };
      accounts['uploadOrder']['default'].push(id);

      // Store auth token
      const authTokenKey = new TextEncoder().encode(`identity_main_account_${id}_auth_token`);
      await s5.authStore.put(authTokenKey, new TextEncoder().encode(authToken));

      // Setup account config for uploads
      const portalConfig = {
        protocol: uri.protocol.replace(':', ''),
        host: uri.hostname + (uri.port ? `:${uri.port}` : ''),
        headers: { 'Authorization': `Bearer ${authToken}` },
        apiURL: (endpoint, params = {}) => {
          const base = `${uri.protocol}//${uri.hostname}${uri.port ? `:${uri.port}` : ''}/s5/${endpoint}`;
          const queryString = Object.keys(params).length > 0
            ? '?' + new URLSearchParams(params).toString()
            : '';
          return base + queryString;
        }
      };

      s5.apiWithIdentity.accountConfigs[id] = portalConfig;

      try {
        await s5.apiWithIdentity.saveStorageServices();
        console.log('   ✅ Credentials stored via fallback');
      } catch (e) {
        console.warn('⚠️  Could not persist account to hidden DB:', e.message);
      }

      console.log(`   Account ID: ${id}`);
    }

    return true;

  } catch (error) {
    if (error.message && error.message.includes('already has an account')) {
      console.log('✅ Account already registered on portal');
      return true;
    }

    console.error('❌ On-chain registration error:', error.message);
    console.log('📥 Bridge will operate in read-only mode');
    return false;
  }
}

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

      // Step 3: Register with S5 portal via on-chain verification
      let accountReady = false;
      if (bridgeConfig.portalUrl) {
        // Try login first (for returning hosts)
        accountReady = await tryPortalLogin(s5, bridgeConfig.portalUrl);

        // If login failed, try on-chain verified registration
        if (!accountReady && bridgeConfig.hostPrivateKey) {
          accountReady = await tryOnChainRegistration(s5, bridgeConfig.portalUrl, bridgeConfig.hostPrivateKey);
        } else if (!accountReady && !bridgeConfig.hostPrivateKey) {
          console.warn('⚠️  HOST_PRIVATE_KEY not set - cannot register with portal');
          console.log('📥 Bridge will operate in read-only mode (downloads only)');
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
