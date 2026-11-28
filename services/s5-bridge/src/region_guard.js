// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Storage region guard (SR1 D3).
 *
 * A host has exactly one storage region, named by the required S5_REGION
 * (no default). Its bridge may talk only to that region's portal and peer.
 * Two checks, both refusing to start:
 *
 * 1. regionErrors: before the S5 client is even created, S5_REGION must be in
 *    the table below, S5_PORTAL_URL's host must be the region's portal host,
 *    and every S5_INITIAL_PEERS entry's host must be the region's peer host
 *    (at least one peer).
 * 2. enforceUploadRegion: after login and before the filesystem is
 *    initialised, the upload pin must have taken and every account left in the
 *    live account list (`apiWithIdentity.accountConfigs`, what s5.js uploads
 *    to) must be on the region's portal host.
 *
 * `eu` is not configured yet (SR1 P7) and is refused like any unknown name.
 */

import { pinUploadPortal } from './upload_pin.js';

export const REGIONS = Object.freeze({
  global: Object.freeze({ portalHost: 's5.platformlessai.ai', peerHost: 's5.platformlessai.ai' }),
  uk: Object.freeze({ portalHost: 's5-uk.platformlessai.ai', peerHost: 's5-uk.platformlessai.ai' }),
});

function parseUrl(s) {
  try {
    return new URL(s);
  } catch {
    return null;
  }
}

/**
 * @param {{ region?: string, portalUrl?: string, initialPeers?: string[] }} cfg
 * @returns {string[]} every reason to refuse; empty when the configuration is in region
 */
export function regionErrors({ region, portalUrl, initialPeers } = {}) {
  const name = (region || '').trim();
  if (!name) {
    return ['S5_REGION is required (global or uk) and has no default'];
  }
  if (!Object.hasOwn(REGIONS, name)) {
    const hint = name === 'eu' ? ' (the eu region is not configured yet)' : '';
    return [`S5_REGION "${name}" is not a configured region${hint}; configured: ${Object.keys(REGIONS).join(', ')}`];
  }
  const want = REGIONS[name];
  const errors = [];

  const portal = parseUrl(portalUrl || '');
  if (!portal || !portal.hostname) {
    errors.push(`S5_PORTAL_URL "${portalUrl || ''}" cannot be parsed as a URL`);
  } else if (portal.hostname !== want.portalHost) {
    errors.push(`S5_PORTAL_URL host ${portal.hostname} is not the ${name} portal host ${want.portalHost}`);
  }

  const peers = (initialPeers || []).map((p) => (p || '').trim()).filter(Boolean);
  if (peers.length === 0) {
    errors.push(`S5_INITIAL_PEERS must name at least one peer on ${want.peerHost}`);
  }
  for (const p of peers) {
    const u = parseUrl(p);
    if (!u || !u.hostname) {
      errors.push(`S5_INITIAL_PEERS entry "${p}" cannot be parsed as a URL`);
    } else if (u.hostname !== want.peerHost) {
      errors.push(`S5_INITIAL_PEERS entry "${p}": host ${u.hostname} is not the ${name} peer host ${want.peerHost}`);
    }
  }
  return errors;
}

/**
 * @param {Record<string, unknown> | undefined} accountConfigs the live account list after the pin
 * @param {string} portalUrl S5_PORTAL_URL (already checked by regionErrors)
 * @param {{ pinned: boolean, host: string }} pin the result of pinUploadPortal
 * @returns {string[]}
 */
export function liveAccountErrors(accountConfigs, portalUrl, pin) {
  const host = new URL(portalUrl).host;
  const errors = [];
  if (!pin || !pin.pinned) {
    errors.push(`upload pin did not take: no portal account on ${host}`);
  }
  const off = Object.keys(accountConfigs || {}).filter((k) => !k.startsWith(`${host}:`));
  if (off.length > 0) {
    errors.push(`portal accounts outside ${host} in the live list: ${off.join(', ')}`);
  }
  return errors;
}

/**
 * Pin uploads to the region portal, then check the live list. Throws when the
 * bridge must not continue.
 * @param {{ accountConfigs?: Record<string, unknown> } | undefined} api s5.apiWithIdentity
 * @param {string} portalUrl
 */
export function enforceUploadRegion(api, portalUrl) {
  const pin = pinUploadPortal(api?.accountConfigs, portalUrl);
  const errors = liveAccountErrors(api?.accountConfigs, portalUrl, pin);
  if (errors.length > 0) {
    throw new Error(`Storage region guard refused to start:\n${errors.join('\n')}`);
  }
  return pin;
}
