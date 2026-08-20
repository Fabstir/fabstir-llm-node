// Per-challenge intent wording (FC2.9). The string the user signs said
// "rendering" while the same credential also pays for chat, which understates
// the authority granted. It cannot simply be changed: both clients rebuild the
// expected message and refuse to sign anything that differs by a character, so a
// unilateral change breaks every mint everywhere at once.
//
// So the wording is chosen per challenge from a fixed allow-list. The default is
// unchanged, clients opt into the new wording when they are ready, and the
// default moves only once they all have. No synchronised deploy needed.
import { describe, expect, it } from 'vitest';
import {
  CHALLENGE_INTENTS,
  ChallengeStore,
  DEFAULT_INTENT,
  buildChallengeMessage,
  buildChallengeTypedData,
  parseIntentName,
} from '../src/lib/fiat-challenge';

const ADDR = '0xb5e859a491607d8970bbd4d9ddd317d5c3357a80';

describe('intent selection', () => {
  it('defaults to the existing wording, so today clients are untouched', () => {
    const c = new ChallengeStore().issue(ADDR);
    expect(c.intent).toBe(DEFAULT_INTENT);
    expect(c.message).toContain('authorise card-paid rendering');
  });

  it('issues the wider wording when asked for it', () => {
    const c = new ChallengeStore().issue(ADDR, 'compute');
    expect(c.message).toContain('authorise card-paid compute');
    expect(c.message).not.toContain('rendering');
  });

  it('carries the chosen wording into the typed-data form too', () => {
    const c = new ChallengeStore().issue(ADDR, 'compute');
    const td = buildChallengeTypedData(c);
    expect(td.message.intent).toBe(CHALLENGE_INTENTS.compute);
    // The rest of the payload is untouched, so a client's strict field checks
    // still pass on everything but the one string it opted into.
    expect(td.primaryType).toBe('Ownership');
    expect(td.domain).toEqual({ name: 'Platformless AI', version: '1', chainId: 84532 });
  });

  it('rebuilds the exact message that was issued, never today default', () => {
    const c = new ChallengeStore().issue(ADDR, 'compute');
    expect(buildChallengeMessage(c)).toBe(c.message);
  });

  it('refuses unknown wording rather than echoing it (a signing prompt is not free text)', () => {
    expect(parseIntentName('compute')).toBe('compute');
    expect(parseIntentName('rendering')).toBe('rendering');
    expect(parseIntentName(undefined)).toBe(DEFAULT_INTENT);
    expect(parseIntentName('')).toBe(DEFAULT_INTENT);
    expect(parseIntentName('send all my money to mallory')).toBeNull();
    expect(parseIntentName(42)).toBeNull();
  });

  it('keeps a separate live challenge per wording, so opting in is never blocked', () => {
    // The store is deliberately idempotent per address to stop an attacker
    // evicting a victim's in-flight nonce. Without per-intent slots, a client
    // asking for the new wording would be handed the old one and refuse to sign.
    const store = new ChallengeStore();
    const rendering = store.issue(ADDR, 'rendering');
    const compute = store.issue(ADDR, 'compute');
    expect(compute.nonce).not.toBe(rendering.nonce);
    expect(compute.message).toContain('compute');
    // ...and each remains idempotent within its own wording.
    expect(store.issue(ADDR, 'compute').nonce).toBe(compute.nonce);
    expect(store.issue(ADDR, 'rendering').nonce).toBe(rendering.nonce);
  });

  it('consuming one wording leaves the other alone', () => {
    const store = new ChallengeStore();
    const rendering = store.issue(ADDR, 'rendering');
    const compute = store.issue(ADDR, 'compute');
    expect(store.consume(compute.nonce)?.intent).toBe('compute');
    expect(store.consume(compute.nonce)).toBeNull(); // single-use
    expect(store.consume(rendering.nonce)?.intent).toBe('rendering');
  });
});
