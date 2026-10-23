import { describe, expect, test } from 'bun:test';
import { isMac } from './platform';

describe('isMac', () => {
  test('recognises macOS from client hints and the legacy platform field', () => {
    expect(isMac({ userAgentData: { platform: 'macOS' } })).toBe(true);
    expect(isMac({ platform: 'MacIntel', maxTouchPoints: 0 })).toBe(true);
  });

  test('treats Windows, Linux, Android and iPhone as not a Mac', () => {
    expect(isMac({ userAgentData: { platform: 'Windows' }, platform: 'Win32' })).toBe(false);
    expect(isMac({ platform: 'Linux x86_64' })).toBe(false);
    expect(isMac({ userAgentData: { platform: 'Android' } })).toBe(false);
    expect(isMac({ platform: 'iPhone', maxTouchPoints: 5 })).toBe(false);
  });

  test('an iPad asking for the desktop site is not a Mac', () => {
    expect(isMac({ platform: 'MacIntel', maxTouchPoints: 5 })).toBe(false);
  });

  test('falls back to the user agent when no platform is reported', () => {
    expect(isMac({ userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' })).toBe(true);
    expect(isMac({ userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' })).toBe(false);
    expect(isMac({})).toBe(false);
  });
});
