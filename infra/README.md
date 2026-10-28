# authorship-registry

Confidential registry where verified reporters file concerns about undisclosed
AI-generated authorship, a human review team vets them, and journal editors can
search accepted records before a publication decision.

## How it works

1. **Report.** Anyone fills in the public form with their real name, email and
   affiliation, the author and paper concerned, and what they observed. Nothing
   is filed until they click a single-use confirmation link sent to that email.
2. **Review.** Reviewers see the full report including the reporter's identity,
   and accept it with an editor-facing summary, or reject it. Every view is logged.
3. **Search.** Editors search accepted records by author name (fuzzy, word-order
   independent, so "Wong Ka Ming" also finds "K. M. Wong"), DOI or title. The
   reporter is never shown. Every search and record view is logged.

Roles: `admin` (everything, manages staff), `reviewer`, `editor`. Staff sign in
with a magic link emailed to a pre-registered address; there are no passwords.

## Shape

- One stateless Rust HTTP server (axum, server-rendered HTML via maud), one database.
- All SQL stays inside the Aurora DSQL subset so the same code runs on DSQL and
  vanilla Postgres: UUID keys generated in the app, no foreign keys, no
  triggers, no extensions, one statement per query, indexes created `ASYNC` on
  DSQL. Fuzzy name matching happens in the app (`apps/web/src/names.rs`).
- Two binaries from the same crate (`apps/web`): `web` (plain HTTP server for
  Kubernetes and local dev, plus `migrate` and `mail-test` subcommands) and
  `lambda` (Lambda Function URL handler via `lambda_http`).

## Run locally

Needs a Rust toolchain (https://rustup.rs) and a PostgreSQL to point at.

```
DATABASE_URL=postgres://user:pass@localhost/registry MAIL_DRIVER=console ADMIN_EMAIL=you@example.org cargo run
cargo test             # unit tests + end-to-end flow against an embedded PostgreSQL (downloaded once to ~/.theseus)
TEST_DATABASE_URL=postgres://... cargo test   # or run the tests against your own server
cargo run -- migrate   # apply migrations and seed the admin, then exit
cargo run -- mail-test you@example.org
```

## Configuration (environment)

| Variable | Meaning |
|---|---|
| `DB_DIALECT` | `postgres` or `dsql`. Inferred from the two below if unset. |
| `DATABASE_URL` | Postgres URL, for `postgres` |
| `DSQL_ENDPOINT` | `<id>.dsql.<region>.on.aws`, for `dsql` (IAM auth, no password) |
| `MAIL_DRIVER` | `console`, `smtp` or `ses` |
| `SENDER_EMAIL` | Verified SES sender |
| `BASE_URL` | Public origin for links in emails. Unset on Lambda = use the request origin. |
| `ADMIN_EMAIL` | Seeded as admin on first migration |
| `GOOGLE_CLIENT_ID` | Enables "Sign in with Google" for staff (see below). Unset = button hidden. |
| `GOOGLE_CLIENT_SECRET` / `GOOGLE_CLIENT_SECRET_PARAM` | The OAuth client secret, inline or as an SSM SecureString name |
| `MIGRATE_ON_START` | default `true`; or run `web migrate` separately |
| `RUST_LOG` | log filter, default `info` |

## Deploy: AWS, scale to zero (`infra/`)

Lambda (arm64, zip) + Aurora DSQL + SES, via CDK. The function is the `lambda`
binary, built by [cargo-lambda](https://www.cargo-lambda.info) (`pip install
cargo-lambda`, or let the construct fall back to Docker) on the
`provided.al2023` runtime.

```
cd infra && npm install
npx cdk bootstrap                       # once per account/region
npx cdk deploy -c senderEmail=registry@your-domain.org -c adminEmail=you@your-domain.org
```

Then: click the SES verification email sent to the sender address, and request
SES production access in the console (new accounts are sandboxed and can only
mail verified addresses). The stack outputs the public URL.

Idle cost is zero: DSQL and Lambda both bill nothing when unused and both have
always-free tiers that cover a registry of this size. Region is `eu-north-1` (this organization's home region; its SCP denies regional services elsewhere);
if the hosting institution needs data residency, DSQL also runs in Hong Kong
(`ap-east-1`, opt-in region).

**Account note:** if every Lambda/DSQL/CloudFormation call fails with `explicit deny in a
service control policy: arn:aws:organizations::<mgmt-account>:policy/...`, the account is a
member of an AWS Organization whose SCP blocks those services. The management account ID is
in that ARN (`aws organizations describe-organization` shows its email). Only the management
account can detach the SCP or move the account out of the restricted OU; the account's own
plan (free vs paid, `aws freetier get-account-plan-state`) has nothing to do with it.

## Deploy: Kubernetes demo (`deploy/k8s/`)

For a homelab or any cluster. Fake data only. The image is built natively on
amd64 and arm64 runners by GitHub Actions and merged into one multi-arch tag,
`ghcr.io/orbli/authorship-registry/web`. The runtime image is Debian slim plus
one binary; no interpreter.

```
kubectl apply -k deploy/k8s     # after creating the two secrets named in kustomization.yaml
```

## Not yet done / decisions for the owning institution

- Rate limiting and bot protection on the public form (CloudFront + WAF, or a captcha).
- Retention policy, right of reply for reported authors, and who may delete.
- ORCID sign-in for reporters and ORCID iDs for reported authors, which would
  make identity exact instead of fuzzy.
- Custom domain in front of the Function URL.

## Staff "Sign in with Google"

A second door next to the emailed link. Google only proves which email address
the person controls; the app then looks that address up in `users` exactly as a
magic-link login does. Anyone whose address is not a staff account is refused
(`/login?denied=staff`, audit row `login_denied`). No account is ever created
from a Google sign-in.

Getting the credential from Google Cloud (one time, about five minutes):

1. https://console.cloud.google.com → pick or create a project (any project; it
   is just a container for the OAuth client).
2. **APIs & Services → OAuth consent screen** (newer consoles call it
   *Google Auth Platform → Branding / Audience*). User type **External**, app
   name "Authorship Registry", your support email, no extra scopes (only
   `openid` and `email` are requested, which need no verification).
   Under *Audience* either add each staff address as a **test user** while the
   app is in Testing, or click **Publish app** so any Google account can reach
   the consent screen (the staff check still happens in this app).
3. **APIs & Services → Credentials → Create credentials → OAuth client ID**,
   type **Web application**. Authorized redirect URIs, one per deployment:
   `https://registry-lab.orbb.li/auth/google/callback`,
   `https://ada.orbb.li/auth/google/callback`,
   `http://localhost:3000/auth/google/callback`.
   No "authorized JavaScript origins" are needed (server-side flow).
4. Copy the **Client ID** (public, ends in `.apps.googleusercontent.com`) and
   the **Client secret** (keep out of git).

Wire it in:

- Kubernetes: add `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` keys to the
  `registry-app` Secret and restart; the Deployment reads both as optional.
- AWS: `aws ssm put-parameter --name /authorship-registry/google-client-secret --type SecureString --value "$SECRET"`
  then `cdk deploy ... -c googleClientId=<client id>`.
- Local: `GOOGLE_CLIENT_ID=... GOOGLE_CLIENT_SECRET=... npm run dev`.
