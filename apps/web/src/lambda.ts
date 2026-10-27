import { handle, type LambdaEvent, type LambdaContext } from 'hono/aws-lambda';
import { boot } from './boot.js';
import { originAllowed } from './origin.js';

// Event Function entry (Function URL). Boot once per execution environment.
let ready: Promise<ReturnType<typeof handle>> | undefined;

export const handler = async (event: LambdaEvent, context: LambdaContext) => {
  // When CloudFront fronts the function (custom domain), only accept requests it relayed.
  const secret = process.env.ORIGIN_SECRET;
  if (secret && !originAllowed((event as { headers?: Record<string, string | undefined> }).headers, secret)) {
    return { statusCode: 403, headers: { 'content-type': 'text/plain' }, body: 'Forbidden: use the public hostname.' };
  }
  ready ??= boot().then(({ app }) => handle(app));
  return (await ready)(event, context);
};
