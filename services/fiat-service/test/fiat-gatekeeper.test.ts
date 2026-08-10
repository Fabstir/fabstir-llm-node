// FC1.1 — the gatekeeper is the security boundary (Decision 3): pure policy
// over a ledger snapshot. Refusal order is deterministic and every refusal is
// a stable machine-readable reason. Config comes from server env; the DEFAULT
// allow-list is EMPTY, so an unconfigured deployment refuses every fiat open.
import { afterEach, describe, expect, it } from 'vitest';
import {
  gatekeeperConfigFromEnv,
  makeGatekeeper,
  type GatekeeperConfig,
  type LedgerView,
} from '../src/lib/gatekeeper';

const HOST = '0xAbCd000000000000000000000000000000000001';

const CONFIG: GatekeeperConfig = {
  allowedHosts: [HOST],
  maxDepositPerSessionMicro: 2_000_000n,
  maxDailySpendMicro: 10_000_000n,
  maxOpensPerMinute: 3,
};

function view(overrides: Partial<LedgerView> = {}): LedgerView {
  return {
    availableMicro: 5_000_000n,
    spentInWindowMicro: 0n,
    opensInWindow: 0,
    ...overrides,
  };
}

describe('gatekeeper decisions', () => {
  const gate = makeGatekeeper(CONFIG);

  it('allows a clean request', () => {
    expect(gate(view(), { host: HOST, depositMicro: 500_000n })).toEqual({ allow: true });
  });

  it('checks the allow-list before anything else (no balance probing off-list)', () => {
    const decision = gate(view({ availableMicro: 0n }), { host: '0x9999999999999999999999999999999999999999', depositMicro: 500_000n });
    expect(decision).toEqual({ allow: false, reason: 'HOST_NOT_ALLOWED' });
  });

  it('compares hosts case-insensitively', () => {
    expect(gate(view(), { host: HOST.toUpperCase().replace('0X', '0x'), depositMicro: 500_000n })).toEqual({
      allow: true,
    });
  });

  it('refuses non-positive and over-cap deposits', () => {
    expect(gate(view(), { host: HOST, depositMicro: 0n })).toEqual({ allow: false, reason: 'INVALID_DEPOSIT' });
    expect(gate(view(), { host: HOST, depositMicro: -1n })).toEqual({ allow: false, reason: 'INVALID_DEPOSIT' });
    expect(gate(view(), { host: HOST, depositMicro: 2_000_001n })).toEqual({
      allow: false,
      reason: 'DEPOSIT_OVER_CAP',
    });
  });

  it('refuses over-balance', () => {
    expect(gate(view({ availableMicro: 499_999n }), { host: HOST, depositMicro: 500_000n })).toEqual({
      allow: false,
      reason: 'INSUFFICIENT_BALANCE',
    });
  });

  it('refuses when the daily window would be exceeded, counting the requested deposit', () => {
    expect(
      gate(view({ spentInWindowMicro: 9_600_000n }), { host: HOST, depositMicro: 500_000n })
    ).toEqual({ allow: false, reason: 'DAILY_CAP_EXCEEDED' });
    expect(
      gate(view({ spentInWindowMicro: 9_500_000n }), { host: HOST, depositMicro: 500_000n })
    ).toEqual({ allow: true });
  });

  it('refuses when the per-minute open count is reached', () => {
    expect(gate(view({ opensInWindow: 3 }), { host: HOST, depositMicro: 500_000n })).toEqual({
      allow: false,
      reason: 'RATE_LIMITED',
    });
  });
});

describe('gatekeeperConfigFromEnv', () => {
  afterEach(() => {
    delete process.env.FIAT_ALLOWED_HOSTS;
    delete process.env.FIAT_MAX_SESSION_DEPOSIT_MICRO;
    delete process.env.FIAT_MAX_DAILY_SPEND_MICRO;
    delete process.env.FIAT_MAX_OPENS_PER_MINUTE;
  });

  it('parses the comma-separated allow-list and numeric caps', () => {
    process.env.FIAT_ALLOWED_HOSTS = `${HOST}, 0xdead000000000000000000000000000000000002`;
    process.env.FIAT_MAX_SESSION_DEPOSIT_MICRO = '1500000';
    process.env.FIAT_MAX_DAILY_SPEND_MICRO = '5000000';
    process.env.FIAT_MAX_OPENS_PER_MINUTE = '5';
    const config = gatekeeperConfigFromEnv();
    expect(config.allowedHosts).toEqual([HOST, '0xdead000000000000000000000000000000000002']);
    expect(config.maxDepositPerSessionMicro).toBe(1_500_000n);
    expect(config.maxDailySpendMicro).toBe(5_000_000n);
    expect(config.maxOpensPerMinute).toBe(5);
  });

  it('defaults to an EMPTY allow-list (unconfigured deployments refuse every open)', () => {
    const config = gatekeeperConfigFromEnv();
    expect(config.allowedHosts).toEqual([]);
    const gate = makeGatekeeper(config);
    expect(gate(view(), { host: HOST, depositMicro: 500_000n })).toEqual({
      allow: false,
      reason: 'HOST_NOT_ALLOWED',
    });
  });

  it('rejects malformed cap values instead of silently defaulting them', () => {
    process.env.FIAT_MAX_SESSION_DEPOSIT_MICRO = 'lots';
    expect(() => gatekeeperConfigFromEnv()).toThrow(/FIAT_MAX_SESSION_DEPOSIT_MICRO/);
  });
});
