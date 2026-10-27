import { describe, expect, it } from 'vitest';
import { originAllowed } from '../src/origin.js';

describe('originAllowed', () => {
  const secret = 'abc123secret';
  it('accepts the exact header', () => expect(originAllowed({ 'x-origin-verify': secret }, secret)).toBe(true));
  it('rejects a missing header', () => expect(originAllowed({}, secret)).toBe(false));
  it('rejects undefined headers', () => expect(originAllowed(undefined, secret)).toBe(false));
  it('rejects a wrong value of equal length', () => expect(originAllowed({ 'x-origin-verify': 'abc123secreX' }, secret)).toBe(false));
  it('rejects a prefix', () => expect(originAllowed({ 'x-origin-verify': 'abc123' }, secret)).toBe(false));
});
