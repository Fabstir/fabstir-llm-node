// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
/**
 * Pin uploads to our own S5 portal.
 *
 * s5.js uploads a blob to the identity's portal accounts in order and stops at
 * the first that succeeds. An identity can also hold accounts on public
 * portals (host2's held s5.vup.cx first), and then every upload lands there,
 * outside our control. This drops every account whose host is not
 * `S5_PORTAL_URL`'s host from the live `accountConfigs`, so single-shot and
 * tus uploads can only reach our portal. It is the helper's `pinUploadPortal`
 * rule. In memory only: `accounts.json` is untouched, so an older build reads
 * the identity exactly as before.
 *
 * Account ids are `<host>:<id>` (e.g. `s5.platformlessai.ai:LnwIK4sjRbmiz6fk`).
 * If no account is on our host (a registration fault), nothing is dropped and
 * the caller logs a warning, as the helper does, so the fault shows in the
 * boot log instead of the bridge losing every upload account.
 */

/**
 * @param {Record<string, unknown> | undefined} accountConfigs the live map (mutated)
 * @param {string} portalUrl S5_PORTAL_URL
 * @returns {{ pinned: boolean, host: string, before: number, after: number, dropped: string[] }}
 */
export function pinUploadPortal(accountConfigs, portalUrl) {
  const host = new URL(portalUrl).host;
  const keys = Object.keys(accountConfigs || {});
  const ours = keys.filter((k) => k.startsWith(`${host}:`));
  if (!accountConfigs || ours.length === 0) {
    return { pinned: false, host, before: keys.length, after: keys.length, dropped: [] };
  }
  const dropped = keys.filter((k) => !k.startsWith(`${host}:`));
  for (const k of dropped) delete accountConfigs[k];
  return { pinned: true, host, before: keys.length, after: ours.length, dropped };
}
