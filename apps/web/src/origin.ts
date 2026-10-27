import { timingSafeEqual } from 'node:crypto';

export const ORIGIN_HEADER = 'x-origin-verify';

/** True when the request carries the shared secret CloudFront adds on the way to the origin.
 *  Lets the Function URL stay AuthType NONE (browser form POSTs work) while refusing direct hits. */
export function originAllowed(headers: Record<string, string | undefined> | undefined, secret: string): boolean {
  const got = headers?.[ORIGIN_HEADER] ?? headers?.[ORIGIN_HEADER.toUpperCase()] ?? headers?.['X-Origin-Verify'];
  if (!got || got.length !== secret.length) return false;
  return timingSafeEqual(Buffer.from(got), Buffer.from(secret));
}
