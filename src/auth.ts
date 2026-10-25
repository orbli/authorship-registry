import { createHash, randomBytes, randomUUID } from 'node:crypto';
import type { Context, MiddlewareHandler } from 'hono';
import { getCookie } from 'hono/cookie';
import { one, type Db } from './db/index.js';

export type Role = 'admin' | 'reviewer' | 'editor';
export const ROLES: Role[] = ['admin', 'reviewer', 'editor'];

export interface User {
  id: string;
  email: string;
  name: string;
  role: Role;
  disabled: boolean;
}

export type TokenPurpose = 'login' | 'verify_report';

export const SESSION_COOKIE = 'sid';

export function randomToken(): string {
  return randomBytes(32).toString('base64url');
}

export function hashToken(raw: string): string {
  return createHash('sha256').update(raw).digest('hex');
}

/** Create a single-use token; returns the raw token to put in a link. Only the hash is stored. */
export async function issueToken(db: Db, p: { purpose: TokenPurpose; email: string; reportId?: string; ttlMinutes: number }): Promise<string> {
  const raw = randomToken();
  await db.query(
    `INSERT INTO tokens (id, token_hash, purpose, email, report_id, expires_at, used_at) VALUES ($1, $2, $3, $4, $5, $6, NULL)`,
    [randomUUID(), hashToken(raw), p.purpose, p.email, p.reportId ?? null, new Date(Date.now() + p.ttlMinutes * 60_000)],
  );
  return raw;
}

export interface TokenRow {
  id: string;
  email: string;
  purpose: TokenPurpose;
  report_id: string | null;
}

/** Validate and burn a token. Returns undefined when unknown, expired, wrong purpose or already used. */
export async function consumeToken(db: Db, raw: string, purpose: TokenPurpose): Promise<TokenRow | undefined> {
  const row = await one<TokenRow & { expires_at: Date; used_at: Date | null }>(db,     `SELECT id, email, purpose, report_id, expires_at, used_at FROM tokens WHERE token_hash = $1`,
    [hashToken(raw)],
  );
  if (!row || row.purpose !== purpose || row.used_at || new Date(row.expires_at) < new Date()) return undefined;
  // Guard against a double click: only the first UPDATE wins.
  const updated = await db.query(`UPDATE tokens SET used_at = $1 WHERE id = $2 AND used_at IS NULL RETURNING id`, [new Date(), row.id]);
  return updated.length ? row : undefined;
}

export async function createSession(db: Db, userId: string, ttlHours: number): Promise<string> {
  const raw = randomToken();
  await db.query(`INSERT INTO sessions (id, token_hash, user_id, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)`, [
    randomUUID(), hashToken(raw), userId, new Date(Date.now() + ttlHours * 3_600_000), new Date(),
  ]);
  return raw;
}

export async function destroySession(db: Db, raw: string): Promise<void> {
  await db.query(`DELETE FROM sessions WHERE token_hash = $1`, [hashToken(raw)]);
}

export async function userFromSession(db: Db, raw: string | undefined): Promise<User | undefined> {
  if (!raw) return undefined;
  const s = await one<{ user_id: string; expires_at: Date }>(db, `SELECT user_id, expires_at FROM sessions WHERE token_hash = $1`, [hashToken(raw)]);
  if (!s || new Date(s.expires_at) < new Date()) return undefined;
  const u = await one<User>(db, `SELECT id, email, name, role, disabled FROM users WHERE id = $1`, [s.user_id]);
  return u && !u.disabled ? u : undefined;
}

export type AuthEnv = { Variables: { user?: User; db: Db } };

/** Loads the current user (if any) into c.var.user. Never rejects. */
export const loadUser: MiddlewareHandler<AuthEnv> = async (c, next) => {
  c.set('user', await userFromSession(c.var.db, getCookie(c, SESSION_COOKIE)));
  await next();
};

/** Rejects with 401/403 unless the user holds one of the roles. Admins pass everything. */
export function requireRole(...roles: Role[]): MiddlewareHandler<AuthEnv> {
  return async (c, next) => {
    const u = c.var.user;
    if (!u) return c.redirect(`/login?next=${encodeURIComponent(c.req.path)}`);
    if (u.role !== 'admin' && !roles.includes(u.role)) return c.text('Forbidden', 403);
    await next();
  };
}

export function currentUser(c: Context<AuthEnv>): User | undefined {
  return c.var.user;
}
