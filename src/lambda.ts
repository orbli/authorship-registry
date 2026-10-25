import { handle, type LambdaEvent, type LambdaContext } from 'hono/aws-lambda';
import { boot } from './boot.js';

// Event Function entry (Function URL). Boot once per execution environment.
let ready: Promise<ReturnType<typeof handle>> | undefined;

export const handler = async (event: LambdaEvent, context: LambdaContext) => {
  ready ??= boot().then(({ app }) => handle(app));
  return (await ready)(event, context);
};
