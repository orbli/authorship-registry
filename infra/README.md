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

- One stateless Node.js 22 HTTP server (Hono, server-rendered HTML), one database.
- All SQL stays inside the Aurora DSQL subset so the same code runs on DSQL,
  vanilla Postgres and embedded PGlite: UUID keys generated in the app, no
  foreign keys, no triggers, no extensions, one statement per query, indexes
  created `ASYNC` on DSQL. Fuzzy name matching happens in the app (`src/names.ts`).
- Entry points: `src/server.ts` (plain HTTP server for Kubernetes, local dev and
  Lambda Web Functions), `src/lambda.ts` (Lambda Function URL handler).

## Run locally

```
npm install
npm run dev            # embedded Postgres, emails printed to the terminal
ADMIN_EMAIL=you@example.org npm run dev   # seeds you as admin so you can sign in
npm test               # 19 tests: unit + full end-to-end flow
```

## Configuration (environment)

| Variable | Meaning |
|---|---|
| `DB_DIALECT` | `pglite` (dev), `postgres`, or `dsql`. Inferred from the two below if unset. |
| `DATABASE_URL` | Postgres URL, for `postgres` |
| `DSQL_ENDPOINT` | `<id>.dsql.<region>.on.aws`, for `dsql` (IAM auth, no password) |
| `MAIL_DRIVER` | `console` or `ses` |
| `SENDER_EMAIL` | Verified SES sender |
| `BASE_URL` | Public origin for links in emails. Unset on Lambda = use the request origin. |
| `ADMIN_EMAIL` | Seeded as admin on first migration |
| `MIGRATE_ON_START` | default `true`; or run `npm run migrate` separately |

## Deploy: AWS, scale to zero (`infra/`)

Lambda (arm64, zip, no Docker needed) + Aurora DSQL + SES, via CDK.

```
cd infra && npm install
npx cdk bootstrap                       # once per account/region
npx cdk deploy -c senderEmail=registry@your-domain.org -c adminEmail=you@your-domain.org
```

Then: click the SES verification email sent to the sender address, and request
SES production access in the console (new accounts are sandboxed and can only
mail verified addresses). The stack outputs the public URL.

Idle cost is zero: DSQL and Lambda both bill nothing when unused and both have
always-free tiers that cover a registry of this size. Region is `us-east-1`;
if the hosting institution needs data residency, DSQL also runs in Hong Kong
(`ap-east-1`, opt-in region).

**Account note:** an AWS account on the post-2025 *free plan* cannot create
Lambda, DSQL, SES or CloudFormation resources (an AWS-managed SCP denies
them). Upgrade to the paid plan first; the free credits carry over.

## Deploy: Kubernetes demo (`deploy/k8s/`)

For a homelab or any cluster. Fake data only. Image is built multi-arch
(amd64 + arm64) by GitHub Actions to `ghcr.io/orbli/authorship-registry`.

```
kubectl apply -k deploy/k8s     # after creating the two secrets named in kustomization.yaml
```

## Not yet done / decisions for the owning institution

- Rate limiting and bot protection on the public form (CloudFront + WAF, or a captcha).
- Retention policy, right of reply for reported authors, and who may delete.
- ORCID sign-in for reporters and ORCID iDs for reported authors, which would
  make identity exact instead of fuzzy.
- Custom domain in front of the Function URL.
