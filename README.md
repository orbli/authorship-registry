# authorship-registry

Confidential registry where verified reporters file concerns about undisclosed
AI-generated authorship, a review team vets them, and journal editors can search
accepted records before a publication decision.

## Shape

- Rust. A Cargo workspace; each crate under `apps/` is one container image and
  may hold several binaries. Shared code goes in further workspace crates.
- `apps/web`: the whole service today. axum HTTP server (Kubernetes, local dev)
  and a Lambda Function URL handler, same code, two binaries.
- Pages are HTML files in `apps/web/templates/` (Askama, Jinja syntax: `{{ x }}`,
  `{% if %}`, `{% for %}`, `{% extends "layout.html" %}`), compiled into the
  binary and type-checked at build time, so a typo in a template fails
  `cargo build`. The stylesheet is `apps/web/static/app.css`, served at
  `/static/app.css` with a content hash in the URL. Rust only supplies data
  (`src/views/`); there is no JavaScript.
- Database: Amazon Aurora DSQL (prod), vanilla Postgres (homelab demo and tests,
  embedded during `cargo test`). All SQL stays within the DSQL-compatible
  subset: UUID keys, no foreign keys, no triggers, no extensions. Fuzzy name
  matching happens in the app.
- `/` lists accepted records and searches them (fuzzy author name, DOI, title);
  `/report` is the intake form. Accepted records show the subject, paper and the
  review team's summary; the reporter is never shown anywhere public.
- Roles: reporter (submits, sees nothing after), reviewer (sees full record
  incl. reporter identity), editor (staff account; searches are audit-logged
  when signed in). The listing is public in this build: to restrict it to
  editors, add a `require(&user, &[Role::Editor], ..)` check to the `/` and `/record/{id}` handlers.
- Every signed-in read is audit-logged.

## Layout

```
Cargo.toml           workspace: members = apps/*
apps/web/            crate authorship-registry-web: src/ (lib + bins web, lambda), tests/
apps/web/templates/  HTML templates (Askama); layout.html is the shared shell
apps/web/static/     stylesheet, embedded at build time
Dockerfile           builds any app: docker build --build-arg APP=web .
.github/workflows/   fmt + clippy + tests, then one image per app, built natively
                     on amd64 and arm64 runners, to ghcr.io/orbli/authorship-registry/<app>
deploy/k8s/          Kubernetes demo manifests (fake data only)
infra/               AWS CDK (TypeScript; AWS has no Rust CDK): Lambda + DSQL + CloudFront
```

## Day to day

```
cargo test                      # unit + end-to-end flow (embedded PostgreSQL, downloaded once)
DATABASE_URL=... cargo run      # console mailer by default, http://localhost:3000
cargo run -- migrate
cargo clippy --workspace --all-targets
cargo fmt --all
```

Adding a second container: create `apps/<name>` as crate
`authorship-registry-<name>` with a binary named `<name>`, add `<name>` to the
two matrices in `.github/workflows/image.yml`, add a Deployment in
`deploy/k8s/`. The package on GHCR inherits the repo's visibility.

Operations, environment variables and the AWS deploy: `infra/README.md`.
