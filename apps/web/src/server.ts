import { serve } from '@hono/node-server';
import { boot } from './boot.js';

// Plain HTTP server: used on Kubernetes, locally, and by Lambda Web Functions.
const { cfg, app } = await boot();
serve({ fetch: app.fetch, port: cfg.port }, (info) => {
  console.log(`authorship-registry listening on http://localhost:${info.port} (db=${cfg.dbDialect}, mail=${cfg.mailDriver})`);
});
