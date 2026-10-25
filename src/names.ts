/**
 * Name normalisation and fuzzy matching done in the application, not the
 * database. Aurora DSQL has no pg_trgm, and at the scale of this registry
 * (thousands of records) scoring candidates in memory is instant anyway.
 */

const CJK = /[぀-ヿ㐀-䶿一-鿿豈-﫿]/;

/** Lowercase, strip diacritics and punctuation, sort tokens so word order is irrelevant. */
export function nameKey(input: string): string {
  const base = input
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s]/gu, ' ')
    .replace(/\s+/g, ' ')
    .trim();
  if (CJK.test(base)) return base.replace(/\s/g, '');
  return base.split(' ').filter(Boolean).sort().join(' ');
}

export function trigrams(s: string): Set<string> {
  const padded = `  ${s} `;
  const out = new Set<string>();
  for (let i = 0; i + 3 <= padded.length; i++) out.add(padded.slice(i, i + 3));
  return out;
}

/** Jaccard similarity over trigrams, same idea as pg_trgm's similarity(). */
export function trigramSimilarity(a: string, b: string): number {
  if (!a || !b) return 0;
  if (a === b) return 1;
  const ta = trigrams(a);
  const tb = trigrams(b);
  let inter = 0;
  for (const t of ta) if (tb.has(t)) inter++;
  const union = ta.size + tb.size - inter;
  return union === 0 ? 0 : inter / union;
}

/**
 * Fraction of query tokens that match a candidate token exactly or as a prefix.
 * Catches "K. M. Wong" vs "Wong Ka Ming" where trigram similarity is poor.
 */
export function tokenMatch(queryKey: string, candidateKey: string): number {
  const q = queryKey.split(' ').filter(Boolean);
  const c = candidateKey.split(' ').filter(Boolean);
  if (q.length === 0 || c.length === 0) return 0;
  let hits = 0;
  for (const qt of q) {
    if (c.some((ct) => ct === qt || (qt.length === 1 ? ct.startsWith(qt) : ct.startsWith(qt) || qt.startsWith(ct)))) hits++;
  }
  return hits / q.length;
}

export function nameScore(query: string, candidateKey: string): number {
  const qk = nameKey(query);
  if (!qk) return 0;
  if (qk === candidateKey) return 1;
  if (CJK.test(qk) || CJK.test(candidateKey)) {
    return candidateKey.includes(qk) || qk.includes(candidateKey) ? 0.9 : trigramSimilarity(qk, candidateKey);
  }
  return Math.max(trigramSimilarity(qk, candidateKey), tokenMatch(qk, candidateKey) * 0.85);
}

export function looksLikeDoi(q: string): boolean {
  return /\b10\.\d{4,9}\/\S+/i.test(q.trim());
}

export function normalizeDoi(q: string): string {
  return q.trim().toLowerCase().replace(/^https?:\/\/(dx\.)?doi\.org\//, '').replace(/^doi:\s*/, '');
}
