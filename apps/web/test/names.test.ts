import { describe, expect, it } from 'vitest';
import { looksLikeDoi, nameKey, nameScore, normalizeDoi } from '../src/names.js';

describe('nameKey', () => {
  it('ignores case, punctuation, diacritics and word order', () => {
    expect(nameKey('Wong, Ka-Ming')).toBe('ka ming wong');
    expect(nameKey('ka ming WONG')).toBe('ka ming wong');
    expect(nameKey('José Núñez')).toBe('jose nunez');
  });
  it('keeps CJK names whole', () => {
    expect(nameKey('黃 家明')).toBe('黃家明');
  });
});

describe('nameScore', () => {
  const key = nameKey('Wong Ka Ming');
  it('exact after normalisation is 1', () => expect(nameScore('ka-ming wong', key)).toBe(1));
  it('initials match tokens by prefix', () => expect(nameScore('K. M. Wong', key)).toBeGreaterThan(0.5));
  it('typos still score', () => expect(nameScore('Wong Ka Mng', key)).toBeGreaterThan(0.4));
  it('unrelated names score low', () => expect(nameScore('Chan Tai Man', key)).toBeLessThan(0.35));
  it('CJK substring', () => expect(nameScore('家明', nameKey('黃家明'))).toBeGreaterThan(0.8));
});

describe('doi', () => {
  it('detects and normalises', () => {
    expect(looksLikeDoi('https://doi.org/10.1000/XYZ.123')).toBe(true);
    expect(looksLikeDoi('Wong')).toBe(false);
    expect(normalizeDoi('https://doi.org/10.1000/XYZ.123')).toBe('10.1000/xyz.123');
    expect(normalizeDoi('doi: 10.1000/XYZ')).toBe('10.1000/xyz');
  });
});
