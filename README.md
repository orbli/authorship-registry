# authorship-registry

Confidential registry where verified reporters file concerns about undisclosed
AI-generated authorship, a review team vets them, and journal editors can search
accepted records before a publication decision.

## Shape

- Monorepo. Each folder under `apps/` is one container image; `packages/` holds
  code shared between apps (empty so far). npm workspaces, one lockfile.
- `apps/web`: the whole service today. Hono HTTP server (Kubernetes, local dev)
  and a Lambda Function URL handler, same code.
- Database: Amazon Aurora DSQL (prod), vanilla Postgres (homelab demo), PGlite
  (tests, `npm run dev`). All SQL stays within the DSQL-compatible subset: UUID
  keys, no foreign keys, no triggers, no extensions. Fuzzy name matching happens
  in the app.
- `/` lists accepted records and searches them (fuzzy author name, DOI, title);
  `/report` is the intake form. Accepted records show the subject, paper and the
  review team's summary; the reporter is never shown anywhere public.
- Roles: reporter (submits, sees nothing after), reviewer (sees full record
  incl. reporter identity), editor (staff account; searches are audit-logged
  when signed in). The listing is public in this build: to restrict it to
  editors, add `requireRole('editor')` to the `/` and `/record/:id` routes.
- Every signed-in read is audit-logged.

## Layout

```
apps/web/            web app + Lambda handler (src/, test/, package.json)
packages/            shared workspaces (none yet)
Dockerfile           builds any app: docker build --build-arg APP=web .
.github/workflows/   typecheck + tests, then one image per app to
                     ghcr.io/orbli/authorship-registry/<app>
deploy/k8s/          Kubernetes demo manifests (fake data only)
infra/               AWS CDK: Lambda + DSQL + CloudFront, region eu-north-1
```

## Day to day

```
npm ci
npm run dev            # PGlite + console mailer, http://localhost:3000
npm test               # 24 tests incl. the full report -> email -> review flow
npm run typecheck
npm run build          # every app
```

Adding a second container: create `apps/<name>` with its own `package.json`
(`build` must emit `dist/server.js`), add `<name>` to the matrix in
`.github/workflows/image.yml`, add a Deployment in `deploy/k8s/`. The first push
creates a new, private GHCR package; flip it to public or give the cluster a
pull secret.

Operations, environment variables and the AWS deploy: `infra/README.md`.
