/**
 * "Sign in with Google" for staff, as a second door next to the magic link.
 *
 * Plain OpenID Connect authorization-code flow with PKCE, no library: the app
 * sends the browser to Google, Google sends back a one-time code, the server
 * swaps it for an ID token over TLS and reads the verified email out of it.
 * That email is then looked up in the users table exactly like a magic-link
 * login; an address that is not a staff account is refused. Google never
 * creates accounts here.
 *
 * The ID token's signature is not re-verified: it arrives straight from
 * Google's token endpoint on a TLS connection we opened, which Google's own
 * docs list as sufficient. Issuer, audience, expiry and email_verified are
 * still checked.
 */
import { createHash, randomBytes } from 'node:crypto';
import type { Config } from './config.js';

export const GOOGLE_AUTH_URL = 'https://accounts.google.com/o/oauth2/v2/auth';
export const GOOGLE_TOKEN_URL = 'https://oauth2.googleapis.com/token';
const ISSUERS = new Set(['accounts.google.com', 'https://accounts.google.com']);

export type FetchLike = (input: string, init?: { method?: string; headers?: Record<string, string>; body?: string }) => Promise<{ ok: boolean; status: number; json(): Promise<unknown> }>;

export const googleEnabled = (cfg: Config) => Boolean(cfg.google?.clientId);

const b64url = (b: Buffer) => b.toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

/** Fresh state + PKCE pair for one sign-in attempt; keep them in a short-lived cookie. */
export function beginGoogleLogin() {
  const verifier = b64url(randomBytes(32));
  return { state: b64url(randomBytes(16)), verifier, challenge: b64url(createHash('sha256').update(verifier).digest()) };
}

export function googleAuthUrl(p: { clientId: string; redirectUri: string; state: string; challenge: string }): string {
  const q = new URLSearchParams({
    client_id: p.clientId,
    redirect_uri: p.redirectUri,
    response_type: 'code',
    scope: 'openid email',
    state: p.state,
    code_challenge: p.challenge,
    code_challenge_method: 'S256',
    prompt: 'select_account',
  });
  return `${GOOGLE_AUTH_URL}?${q}`;
}

let cachedSecret: string | undefined;
/** Client secret from env, or once from the SSM SecureString named by GOOGLE_CLIENT_SECRET_PARAM. */
async function clientSecret(cfg: Config): Promise<string> {
  if (cfg.google?.clientSecret) return cfg.google.clientSecret;
  if (!cachedSecret && cfg.google?.clientSecretParam) {
    const { SSMClient, GetParameterCommand } = await import('@aws-sdk/client-ssm');
    const r = await new SSMClient({ region: cfg.awsRegion }).send(new GetParameterCommand({ Name: cfg.google.clientSecretParam, WithDecryption: true }));
    cachedSecret = r.Parameter?.Value;
  }
  if (!cachedSecret) throw new Error('Google client secret not configured');
  return cachedSecret;
}

export interface GoogleIdentity { email: string; sub: string }

/** Swap the callback code for an ID token and return the verified email, or throw. */
export async function finishGoogleLogin(cfg: Config, p: { code: string; verifier: string; redirectUri: string }, fetchImpl: FetchLike = fetch as unknown as FetchLike): Promise<GoogleIdentity> {
  const clientId = cfg.google!.clientId;
  const body = new URLSearchParams({
    code: p.code,
    client_id: clientId,
    client_secret: await clientSecret(cfg),
    redirect_uri: p.redirectUri,
    grant_type: 'authorization_code',
    code_verifier: p.verifier,
  });
  const res = await fetchImpl(GOOGLE_TOKEN_URL, { method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded' }, body: body.toString() });
  const tok = (await res.json()) as { id_token?: string; error?: string; error_description?: string };
  if (!res.ok || !tok.id_token) throw new Error(`Google token exchange failed: ${tok.error ?? res.status} ${tok.error_description ?? ''}`.trim());
  const claims = decodeJwtPayload(tok.id_token) as { iss?: string; aud?: string; exp?: number; email?: string; email_verified?: boolean; sub?: string };
  if (!claims.iss || !ISSUERS.has(claims.iss)) throw new Error('Google ID token: bad issuer');
  if (claims.aud !== clientId) throw new Error('Google ID token: bad audience');
  if (!claims.exp || claims.exp * 1000 < Date.now()) throw new Error('Google ID token: expired');
  if (!claims.email || claims.email_verified !== true) throw new Error('Google ID token: email not verified');
  return { email: claims.email.toLowerCase(), sub: String(claims.sub ?? '') };
}

export function decodeJwtPayload(jwt: string): unknown {
  const parts = jwt.split('.');
  if (parts.length !== 3) throw new Error('not a JWT');
  return JSON.parse(Buffer.from(parts[1].replace(/-/g, '+').replace(/_/g, '/'), 'base64').toString('utf8'));
}
