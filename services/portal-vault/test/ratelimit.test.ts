// R7: requests per client address per minute, with an injected clock.
import { describe, expect, it } from 'vitest';
import { RateLimiter, clientKey } from '../src/ratelimit.js';

function make(perMinute: number, maxClients = 1000) {
  const clock = { t: 1_800_000_000_000 };
  return { limiter: new RateLimiter({ perMinute, now: () => clock.t, maxClients }), clock };
}

describe('RateLimiter', () => {
  it('allows the configured number per minute and refuses the next', () => {
    const { limiter } = make(3);
    for (let i = 0; i < 3; i++) expect(limiter.check('1.2.3.4').allowed).toBe(true);
    const refused = limiter.check('1.2.3.4');
    expect(refused.allowed).toBe(false);
    expect(refused.retryAfterSeconds).toBeGreaterThanOrEqual(1);
    expect(refused.retryAfterSeconds).toBeLessThanOrEqual(60);
  });

  it('counts each client separately', () => {
    const { limiter } = make(1);
    expect(limiter.check('1.2.3.4').allowed).toBe(true);
    expect(limiter.check('1.2.3.4').allowed).toBe(false);
    expect(limiter.check('5.6.7.8').allowed).toBe(true);
  });

  it('opens again when the minute has passed, and not before', () => {
    const { limiter, clock } = make(1);
    expect(limiter.check('c').allowed).toBe(true);
    clock.t += 59_999;
    expect(limiter.check('c').allowed).toBe(false);
    clock.t += 1;
    expect(limiter.check('c').allowed).toBe(true);
  });

  it('reports the seconds left in the window', () => {
    const { limiter, clock } = make(1);
    limiter.check('c');
    clock.t += 45_000;
    expect(limiter.check('c').retryAfterSeconds).toBe(15);
  });

  it('forgets clients whose window has passed', () => {
    const { limiter, clock } = make(1);
    for (let i = 0; i < 50; i++) limiter.check(`client-${i}`);
    expect(limiter.size()).toBe(50);
    clock.t += 60_000;
    limiter.check('fresh');
    limiter.sweep(); // on a timer in the running service, never on a call
    expect(limiter.size()).toBe(1);
  });

  it('bounds its memory: past the client cap new clients are refused, known ones still counted', () => {
    const { limiter, clock } = make(5, 2);
    expect(limiter.check('a').allowed).toBe(true);
    expect(limiter.check('b').allowed).toBe(true);
    expect(limiter.check('c').allowed).toBe(false);
    expect(limiter.check('a').allowed).toBe(true);
    expect(limiter.size()).toBe(2);
    clock.t += 60_000;
    expect(limiter.check('c').allowed).toBe(true);
  });

  it('refuses a nonsense limit', () => {
    expect(() => new RateLimiter({ perMinute: 0, now: Date.now })).toThrow();
    expect(() => new RateLimiter({ perMinute: 1.5, now: Date.now })).toThrow();
  });
});

describe('clientKey (R7 v0.2)', () => {
  it('leaves an IPv4 address unchanged', () => {
    expect(clientKey('203.0.113.5')).toBe('203.0.113.5');
    expect(clientKey('127.0.0.1')).toBe('127.0.0.1');
  });

  it('keys an IPv6 client on its /64', () => {
    expect(clientKey('2001:db8:1:2:3:4:5:6')).toBe('2001:db8:1:2::/64');
    expect(clientKey('2001:db8:1:2::1')).toBe('2001:db8:1:2::/64');
    expect(clientKey('2001:db8:1:2:ffff:ffff:ffff:ffff')).toBe('2001:db8:1:2::/64');
    expect(clientKey('2001:DB8:0001:0002::9')).toBe('2001:db8:1:2::/64');
    expect(clientKey('2001:db8::1')).toBe('2001:db8:0:0::/64');
    expect(clientKey('::1')).toBe('0:0:0:0::/64');
    expect(clientKey('fe80::1%eth0')).toBe('fe80:0:0:0::/64');
    expect(clientKey('2001:db8:1:3::1')).not.toBe(clientKey('2001:db8:1:2::1'));
  });

  it('treats IPv4-mapped IPv6 as IPv4, in both spellings', () => {
    expect(clientKey('::ffff:203.0.113.5')).toBe('203.0.113.5');
    expect(clientKey('::ffff:cb00:7105')).toBe('203.0.113.5');
    expect(clientKey('0:0:0:0:0:ffff:cb00:7105')).toBe('203.0.113.5');
  });

  it('gives one fixed key to anything that is not an address', () => {
    expect(clientKey('unknown')).toBe('unknown');
    expect(clientKey('not an ip')).toBe('unknown');
    expect(clientKey('')).toBe('unknown');
  });

  it('one limiter bucket for a whole /64', () => {
    const { limiter } = make(2);
    expect(limiter.check(clientKey('2001:db8:1:2::1')).allowed).toBe(true);
    expect(limiter.check(clientKey('2001:db8:1:2::2')).allowed).toBe(true);
    expect(limiter.check(clientKey('2001:db8:1:2:aaaa::3')).allowed).toBe(false);
    expect(limiter.check(clientKey('2001:db8:1:3::1')).allowed).toBe(true);
  });
});

describe('RateLimiter sweeping (R7 v0.2)', () => {
  it('never does a full sweep on a call: a full table of live clients costs a constant per check', () => {
    const { limiter } = make(60, 5000);
    for (let i = 0; i < 5000; i++) limiter.check(`c${i}`);
    expect(limiter.size()).toBe(5000);
    const before = limiter.examined;
    let refused = 0;
    for (let i = 0; i < 500; i++) if (!limiter.check(`new-${i}`).allowed) refused++;
    expect(refused).toBe(500);
    expect(limiter.examined - before).toBeLessThanOrEqual(500 * 2);
  });

  it('sweep() releases every expired client and reports how many', () => {
    const { limiter, clock } = make(1);
    for (let i = 0; i < 30; i++) limiter.check(`c${i}`);
    clock.t += 30_000;
    for (let i = 30; i < 40; i++) limiter.check(`c${i}`);
    clock.t += 30_000;
    expect(limiter.sweep()).toBe(30);
    expect(limiter.size()).toBe(10);
  });

  it('sweeps on a timer that does not keep the process alive', async () => {
    const limiter = new RateLimiter({ perMinute: 1, now: () => clock.t });
    const clock = { t: 0 };
    limiter.check('a');
    clock.t = 60_000;
    const timer = limiter.startSweeping(5);
    expect(timer.hasRef()).toBe(false);
    await new Promise((r) => setTimeout(r, 40));
    clearInterval(timer);
    expect(limiter.size()).toBe(0);
  });
});
