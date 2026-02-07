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

import { S5 } from '@julesl23/s5js';
import { signChallenge, CHALLENGE_TYPE_REGISTER } from '@julesl23/s5js/dist/src/account/sign_challenge.js';
import { base64UrlNoPaddingEncode, base64UrlNoPaddingDecode } from '@julesl23/s5js/dist/src/util/base64.js';
import { FS5Advanced } from '@julesl23/s5js/advanced';
import { ethers } from 'ethers';
import { bridgeConfig } from './config.js';

let s5Instance = null;
let advancedInstance = null;
let initializationPromise = null;

/**
 * Try to login to portal using stored account seed
 *
 * When the S5.js library's built-in login fails (e.g. portal returns non-standard
 * response), this function manually performs the login flow and handles various
 * response formats.
 *
 * @param {S5} s5 - S5 instance with identity
 * @param {string} portalUrl - Portal URL
 * @param {string} accountId - Account ID from accounts.json
 * @param {object} accountEntry - Account entry from accounts.json
 * @returns {Promise<string|null>} Auth token if successful, null otherwise
 */
async function tryManualLogin(s5, portalUrl, accountId, accountEntry) {
  try {
    const seed = base64UrlNoPaddingDecode(accountEntry.seed);
    const identity = s5.identity;
    const portalAccountsSeed = identity.portalAccountSeed;
    const portalAccountKey = await s5.node.crypto.hashBlake3(
      new Uint8Array([...portalAccountsSeed, ...seed])
    );
    const keyPair = await s5.node.crypto.newKeyPairEd25519(portalAccountKey);
    const publicKey = base64UrlNoPaddingEncode(keyPair.publicKey);

    console.log(`🔄 Manual login attempt for ${accountId}`);

    // Step 1: GET challenge
    const loginGetRes = await fetch(`${portalUrl}/s5/account/login?pubKey=${publicKey}`);
    if (!loginGetRes.ok) {
      console.log(`   Login GET failed: HTTP ${loginGetRes.status}`);
      return null;
    }
    const loginGetData = await loginGetRes.json();
    const challengeBase64 = loginGetData.challenge;
    if (!challengeBase64) {
      console.log('   No challenge in login response');
      return null;
    }

    // Step 2: Sign challenge
    const { CHALLENGE_TYPE_LOGIN } = await import('@julesl23/s5js/dist/src/account/sign_challenge.js');
    const uri = new URL(portalUrl);
    const challengeBytes = base64UrlNoPaddingDecode(challengeBase64);
    const challengeResult = await signChallenge(keyPair, challengeBytes, CHALLENGE_TYPE_LOGIN, uri.host, s5.node.crypto);

    // Step 3: POST signed challenge
    const loginPostRes = await fetch(`${portalUrl}/s5/account/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        pubKey: publicKey,
        response: base64UrlNoPaddingEncode(challengeResult.response),
        signature: base64UrlNoPaddingEncode(challengeResult.signature),
        label: 's5-bridge',
      }),
    });
    if (!loginPostRes.ok) {
      console.log(`   Login POST failed: HTTP ${loginPostRes.status}`);
      return null;
    }

    const loginData = await loginPostRes.json();
    // Handle various response formats
    const authToken = loginData.authToken || loginData.token || loginData.auth_token;
    if (typeof authToken === 'string' && authToken.length > 0) {
      console.log('✅ Manual login successful');
      return authToken;
    }

    console.log('   Login response has no auth token:', JSON.stringify(loginData).slice(0, 200));
    return null;
  } catch (error) {
    console.log(`   Manual login failed: ${error.message}`);
    return null;
  }
}

/**
 * Restore auth token for a portal account
 *
 * Updates the authStore and accountConfigs with the given auth token.
 *
 * @param {S5} s5 - S5 instance with identity
 * @param {string} portalUrl - Portal URL
 * @param {string} accountId - Account ID
 * @param {string} authToken - Auth token to store
 */
async function restoreAuthToken(s5, portalUrl, accountId, authToken) {
  const uri = new URL(portalUrl);

  // Update authStore
  const authTokenKey = new TextEncoder().encode(`identity_main_account_${accountId}_auth_token`);
  await s5.authStore.put(authTokenKey, new TextEncoder().encode(authToken));

  // Re-create portal config with correct auth token
  const { S5Portal } = await import('@julesl23/s5js/dist/src/account/portal.js');
  const portalConfig = new S5Portal(
    uri.protocol.replace(':', ''),
    uri.hostname + (uri.port ? `:${uri.port}` : ''),
    { 'Authorization': `Bearer ${authToken}` }
  );
  s5.apiWithIdentity.accountConfigs[accountId] = portalConfig;

  // Also update accounts.json entry for future restarts
  if (s5.apiWithIdentity?.accounts?.['accounts']?.[accountId]) {
    s5.apiWithIdentity.accounts['accounts'][accountId].authToken = authToken;
    try {
      await s5.apiWithIdentity.saveStorageServices();
      console.log('   ✅ Auth token persisted for future restarts');
    } catch (e) {
      console.warn('   ⚠️  Could not persist auth token:', e.message);
    }
  }
}

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
    const accounts = s5.apiWithIdentity?.accounts?.['accounts'] || {};

    for (const id of Object.keys(accountConfigs)) {
      if (!id.startsWith(`${uri.host}:`)) continue;

      console.log(`🔑 Found account: ${id}`);

      // Check if the auth token in the portal config is valid
      // After restart, setupAccount may fail to login and create a config with empty Bearer token
      const portalConfig = accountConfigs[id];
      const authHeader = portalConfig?.headers?.['Authorization'] || '';
      const currentToken = authHeader.replace('Bearer ', '').trim();

      if (currentToken) {
        console.log('✅ Existing account found with valid auth token');
        return true;
      }

      // Auth token is empty - try to restore from accounts.json (persisted in S5 hidden DB)
      console.log('⚠️  Auth token empty after restart - attempting to restore from stored credentials');
      const accountEntry = accounts[id];
      const storedToken = accountEntry?.authToken;

      if (storedToken) {
        console.log('🔄 Restoring auth token from stored credentials...');
        await restoreAuthToken(s5, portalUrl, id, storedToken);
        console.log('✅ Auth token restored successfully');
        return true;
      }

      // No stored auth token - try manual login using the account seed
      if (accountEntry?.seed) {
        const manualToken = await tryManualLogin(s5, portalUrl, id, accountEntry);
        if (manualToken) {
          await restoreAuthToken(s5, portalUrl, id, manualToken);
          return true;
        }
      }

      console.log('❌ Could not restore auth - will attempt re-registration');
      return false;
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
    // Uses the same derivation as S5APIWithIdentity.registerAccount()
    const accountSeed = s5.node.crypto.generateSecureRandomBytes(32);
    const portalAccountsSeed = identity.portalAccountSeed;
    const portalAccountKey = await s5.node.crypto.hashBlake3(
      new Uint8Array([...portalAccountsSeed, ...accountSeed])
    );
    const keyPair = await s5.node.crypto.newKeyPairEd25519(portalAccountKey);
    const s5PubKey = base64UrlNoPaddingEncode(keyPair.publicKey);
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
        // Try to login with existing account credentials
        const accounts = s5.apiWithIdentity?.accounts?.['accounts'] || {};
        for (const [id, entry] of Object.entries(accounts)) {
          if (!id.startsWith(`${new URL(portalUrl).host}:`)) continue;
          if (entry?.seed) {
            const token = await tryManualLogin(s5, portalUrl, id, entry);
            if (token) {
              await restoreAuthToken(s5, portalUrl, id, token);
              return true;
            }
          }
        }
        // Could not get auth token but account exists - uploads may fail
        console.warn('⚠️  Account exists but could not obtain auth token');
        return true;
      } else {
        console.error(`❌ Portal challenge request failed: ${errorMsg}`);
      }
      return false;
    }

    const { challenge: challengeBase64 } = await challengeRes.json();
    console.log('   ✅ On-chain verification passed, got S5 challenge');

    // Step 2: Sign challenge using S5.js signChallenge (same as standard registration)
    const uri = new URL(portalUrl);
    const challengeBytes = base64UrlNoPaddingDecode(challengeBase64);
    console.log(`   Challenge: ${challengeBytes.length} bytes`);
    console.log(`   Portal host for blake3: "${uri.host}"`);
    console.log(`   Key pair pubKey length: ${keyPair.publicKey.length}`);
    const challengeResult = await signChallenge(
      keyPair, challengeBytes, CHALLENGE_TYPE_REGISTER, uri.host, s5.node.crypto
    );
    console.log(`   Response: ${challengeResult.response.length} bytes, Signature: ${challengeResult.signature.length} bytes`);

    // Encode response and signature as base64url
    const responseBase64 = base64UrlNoPaddingEncode(challengeResult.response);
    const signature = base64UrlNoPaddingEncode(challengeResult.signature);

    // Step 3: Complete registration
    // s5Signature = Ed25519 signature of response (for S5 registration)
    // signature = ETH signature of message (for NodeRegistry verification)
    const completeBody = {
      pubKey: s5PubKey,
      challenge: challengeBase64,
      response: responseBase64,
      s5Signature: signature,
      ethAddress: ethWallet.address,
      signature: ethSignature,
      message,
    };
    console.log(`   pubKey: ${s5PubKey.length} chars, response: ${responseBase64.length} chars, s5Signature: ${signature.length} chars`);
    const registerRes = await fetch(`${portalUrl}/s5/account/register-host/complete`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(completeBody),
    });

    if (!registerRes.ok) {
      const errorText = await registerRes.text().catch(() => '');
      let errorMsg;
      try {
        const errorJson = JSON.parse(errorText);
        errorMsg = errorJson.error || `HTTP ${registerRes.status}`;
      } catch {
        errorMsg = errorText || `HTTP ${registerRes.status}`;
      }

      if (errorMsg.includes('already has an account')) {
        console.log('✅ Account already registered on portal (at complete step)');
        // Try manual login with existing account credentials
        const accounts2 = s5.apiWithIdentity?.accounts?.['accounts'] || {};
        for (const [id2, entry2] of Object.entries(accounts2)) {
          if (!id2.startsWith(`${new URL(portalUrl).host}:`)) continue;
          if (entry2?.seed) {
            const token2 = await tryManualLogin(s5, portalUrl, id2, entry2);
            if (token2) {
              await restoreAuthToken(s5, portalUrl, id2, token2);
              return true;
            }
          }
        }
        return true;
      }

      console.error(`❌ Portal registration failed (${registerRes.status}): ${errorMsg}`);
      return false;
    }

    const result = await registerRes.json();
    const authToken = result.authToken;

    if (!authToken) {
      console.error('❌ No auth token in registration response');
      return false;
    }

    console.log('✅ Portal registration complete (on-chain verified)');

    // Step 4: Store credentials using same pattern as S5APIWithIdentity.registerAccount()
    const seedBase64 = base64UrlNoPaddingEncode(accountSeed);
    const id = `${uri.host}:${base64UrlNoPaddingEncode(accountSeed.slice(0, 12))}`;

    const accounts = s5.apiWithIdentity?.accounts || {
      'accounts': {},
      'active': [],
      'uploadOrder': { 'default': [] }
    };

    accounts['accounts'][id] = {
      'url': `${uri.protocol}//${uri.host}`,
      'seed': seedBase64,
      'authToken': authToken,
      'createdAt': new Date().toISOString(),
    };
    accounts['active'].push(id);
    if (!accounts['uploadOrder']) accounts['uploadOrder'] = { 'default': [] };
    accounts['uploadOrder']['default'].push(id);

    if (s5.apiWithIdentity) {
      s5.apiWithIdentity.accounts = accounts;
    }

    // Store auth token
    const authTokenKey = new TextEncoder().encode(`identity_main_account_${id}_auth_token`);
    await s5.authStore.put(authTokenKey, new TextEncoder().encode(authToken));

    // Setup account and persist to hidden DB
    try {
      await s5.apiWithIdentity.setupAccount(id);
      await s5.apiWithIdentity.saveStorageServices();
      console.log(`   ✅ Credentials stored (account: ${id})`);
    } catch (e) {
      console.warn('⚠️  Could not persist account to hidden DB:', e.message);
      console.log('   Attempting manual config fallback...');

      // Fallback: Create portal config manually if setupAccount fails
      const { S5Portal } = await import('@julesl23/s5js/dist/src/account/portal.js');
      const portalConfig = new S5Portal(
        uri.protocol.replace(':', ''),
        uri.hostname + (uri.port ? `:${uri.port}` : ''),
        { 'Authorization': `Bearer ${authToken}` }
      );
      s5.apiWithIdentity.accountConfigs[id] = portalConfig;
      console.log(`   ✅ Manual config set (account: ${id})`);
    }

    return true;

  } catch (error) {
    if (error.message && error.message.includes('already has an account')) {
      console.log('✅ Account already registered on portal (caught)');
      // Try manual login with existing account credentials
      const accounts3 = s5.apiWithIdentity?.accounts?.['accounts'] || {};
      for (const [id3, entry3] of Object.entries(accounts3)) {
        if (!id3.startsWith(`${new URL(portalUrl).host}:`)) continue;
        if (entry3?.seed) {
          const token3 = await tryManualLogin(s5, portalUrl, id3, entry3);
          if (token3) {
            await restoreAuthToken(s5, portalUrl, id3, token3);
            return true;
          }
        }
      }
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

      // Step 3: Register with S5 portal
      let accountReady = false;
      if (bridgeConfig.portalUrl) {
        // Try login first (for returning hosts)
        accountReady = await tryPortalLogin(s5, bridgeConfig.portalUrl);

        if (!accountReady) {
          if (bridgeConfig.hostPrivateKey) {
            // On-chain verified registration (for portals that require NodeRegistry verification)
            accountReady = await tryOnChainRegistration(s5, bridgeConfig.portalUrl, bridgeConfig.hostPrivateKey);
          } else {
            // Standard S5 registration (for open portals without on-chain requirement)
            try {
              console.log(`🌐 Registering with portal: ${bridgeConfig.portalUrl}`);
              await s5.registerOnNewPortal(bridgeConfig.portalUrl);
              console.log('✅ Portal registration complete (standard flow)');
              accountReady = true;
            } catch (regError) {
              const msg = regError.message || '';
              if (msg.includes('already has an account')) {
                console.log('✅ Account already registered on portal');
                accountReady = true;
              } else {
                console.warn(`⚠️  Registration failed: ${msg}`);
                console.log('📥 Bridge will operate in read-only mode (downloads only)');
              }
            }
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
