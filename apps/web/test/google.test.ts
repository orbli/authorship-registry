import { describe, expect, it, beforeAll, afterAll } from 'vitest';
import { createApp } from '../src/app.js';
import { loadConfig } from '../src/config.js';
import { openDb, type Db } from '../src/db/index.js';
import { migrate } from '../src/db/schema.js';
import { createMailer } from '../src/mail.js';
import { GOOGLE_TOKEN_URL, decodeJwtPayload, type FetchLike } from '../src/google.js';

const cfg = loadConfig({ DB_DIALECT: 'pglite', MAIL_DRIVER: 'console', BASE_URL: 'http://test.local', ADMIN_EMAIL: 'admin@test.local', GOOGLE_CLIENT_ID: 'cid.apps.googleusercontent.com', GOOGLE_CLIENT_SECRET: 'shh' } as NodeJS.ProcessEnv);

/** A fake Google token endpoint: the "code" is the email it will vouch for. */
const idToken = (claims: Record<string, unknown>) => ['e30', Buffer.from(JSON.stringify(claims)).toString('base64url'), 'sig'].join('.');
const fakeGoogle: FetchLike = async (url, init) => {
  expect(url).toBe(GOOGLE_TOKEN_URL);
  const body = new URLSearchParams(init!.body!);
  expect(body.get('client_secret')).toBe('shh');
  expect(body.get('code_verifier')).toBeTruthy();
  const code = body.get('code')!;
  if (code === 'bad') return { ok: false, status: 400, json: async () => ({ error: 'invalid_grant' }) };
  const [email, flag] = code.split('|');
  const claims = { iss: 'https://accounts.google.com', aud: flag === 'wrongaud' ? 'other' : cfg.google!.clientId, exp: Math.floor(Date.now() / 1000) + 300, email, email_verified: flag !== 'unverified', sub: '123' };
  return { ok: true, status: 200, json: async () => ({ id_token: idToken(claims) }) };
};

let db: Db;
let app: ReturnType<typeof createApp>;
beforeAll(async () => {
  db = await openDb(cfg);
  await migrate(db, { adminEmail: cfg.adminEmail });
  app = createApp({ db, mailer: createMailer(cfg), cfg, fetchImpl: fakeGoogle });
});
afterAll(() => db.close());

/** Drive /auth/google, then hit the callback with the given code and the state Google would echo. */
async function callback(code: string, opts: { next?: string; tamperState?: boolean } = {}) {
  const start = await app.request(`/auth/google${opts.next ? `?next=${encodeURIComponent(opts.next)}` : ''}`);
  expect(start.status).toBe(302);
  const loc = new URL(start.headers.get('location')!);
  expect(loc.origin + loc.pathname).toBe('https://accounts.google.com/o/oauth2/v2/auth');
  expect(loc.searchParams.get('redirect_uri')).toBe('http://test.local/auth/google/callback');
  expect(loc.searchParams.get('code_challenge_method')).toBe('S256');
  const cookie = start.headers.get('set-cookie')!.split(';')[0];
  const state = opts.tamperState ? 'nope' : loc.searchParams.get('state')!;
  return app.request(`/auth/google/callback?code=${encodeURIComponent(code)}&state=${state}`, { headers: { cookie } });
}

describe('Google sign-in', () => {
  it('shows the button only when configured', async () => {
    expect(await (await app.request('/login')).text()).toContain('Sign in with Google');
    const plain = createApp({ db, mailer: createMailer(cfg), cfg: { ...cfg, google: undefined } });
    expect(await (await plain.request('/login')).text()).not.toContain('Sign in with Google');
    expect((await plain.request('/auth/google')).status).toBe(404);
  });

  it('signs in a staff account whose email Google verified', async () => {
    const r = await callback('admin@test.local|ok', { next: '/admin/users' });
    expect(r.status).toBe(302);
    expect(r.headers.get('location')).toBe('/admin/users');
    // the callback also clears the goauth cookie, so pick the session cookie out of several
    const sid = r.headers.getSetCookie().find((x) => x.startsWith('sid='))!.split(';')[0];
    expect(sid).toMatch(/^sid=/);
    expect((await app.request('/admin/users', { headers: { cookie: sid } })).status).toBe(200);
    const rows = await db.query<{ action: string; detail: string }>(`SELECT action, detail FROM audit_log WHERE action = 'login'`);
    expect(rows.at(-1)?.detail).toBe('google');
  });

  it('refuses a Google account that is not staff, and logs it', async () => {
    const r = await callback('stranger@gmail.com|ok');
    expect(r.status).toBe(302);
    expect(r.headers.get('location')).toBe('/login?denied=staff');
    expect(r.headers.get('set-cookie') ?? '').not.toMatch(/sid=/);
    const rows = await db.query<{ detail: string }>(`SELECT detail FROM audit_log WHERE action = 'login_denied'`);
    expect(rows.map((x) => x.detail)).toContain('google: stranger@gmail.com');
    expect(await (await app.request('/login?denied=staff')).text()).toContain('not a registered staff account');
  });

  it('refuses unverified emails, wrong audience, bad codes and tampered state', async () => {
    for (const code of ['admin@test.local|unverified', 'admin@test.local|wrongaud', 'bad']) {
      const r = await callback(code);
      expect(r.headers.get('location'), code).toBe('/login?denied=google');
      expect(r.headers.get('set-cookie') ?? '').not.toMatch(/sid=/);
    }
    expect((await callback('admin@test.local|ok', { tamperState: true })).headers.get('location')).toBe('/login?denied=google');
  });

  it('decodes a JWT payload', () => {
    expect(decodeJwtPayload(idToken({ a: 1 }))).toEqual({ a: 1 });
  });
});
