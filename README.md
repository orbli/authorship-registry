# authorship-registry

Confidential registry where verified reporters file concerns about undisclosed
AI-generated authorship, a review team vets them, and journal editors can search
accepted records before a publication decision.

## Shape

- One stateless container (HTTP server), one database.
- Database: Amazon Aurora DSQL (prod), vanilla Postgres (local / homelab demo).
  All SQL stays within the DSQL-compatible subset: UUID keys, no foreign keys,
  no triggers, no extensions. Fuzzy name matching happens in the app.
- Region: ap-east-1 (Hong Kong). Data does not leave Hong Kong.
- Roles: reporter (submits, sees nothing after), reviewer (sees full record
  incl. reporter identity), editor (sees accepted records, reporter stripped).
- Every editor read is audit-logged.

## Deploy targets

- `deploy/k8s/` – homelab / any Kubernetes (demo, fake data only)
- `deploy/aws/` – Lambda (container) + DSQL + SES, scale to zero

## Status

Scaffold only. See issues for the build plan.
