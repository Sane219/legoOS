# Dev Pipeline

How to run legoOS locally, how CI works, where tests live, and how to verify a feature actually
works before calling it done.

## Local Dev Workflow

The whole stack — API, executor, workers, frontend, Postgres, Redis, Qdrant — runs via Docker
Compose:

```bash
docker compose up
```

This is the source of truth for "does this run." If a feature only works when run outside
Compose, it isn't done.

For faster iteration on a single service, you can run that service natively against the rest of
the stack in Compose:

```bash
docker compose up postgres redis qdrant   # infra only
cargo run -p api                          # run the API natively, hot-reloadable
```

```bash
cd frontend
npm run dev                               # run the frontend natively
```

Environment variables are read from `.env` (copy `.env.example` and fill in values: database URL,
Redis/NATS URL, Qdrant URL, LLM provider API keys). Never commit a real `.env` file.

## Continuous Integration

CI runs via GitHub Actions on every push and every pull request. The pipeline:

1. **Backend** — `cargo build`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`
2. **Frontend** — `npm ci`, lint, typecheck, `npm run test`
3. (Later phases) — Docker image build check, integration tests against a real Postgres/Redis in
   the CI runner

A push that fails any of these steps should not be merged. CI is the enforcement mechanism for the
conventions in [CLAUDE.md](../CLAUDE.md).

## Where Tests Live

- **Rust** — unit tests live alongside the code they test (`#[cfg(test)] mod tests` in the same
  file), following standard Rust convention. Integration tests that need multiple crates or a real
  database live under each crate's `tests/` directory.
- **Frontend** — component and unit tests live alongside the component (`*.test.tsx` next to the
  component file).
- **DAG executor node types** — every new node type must have at least one test exercising its
  execution logic (success case and at least one failure/edge case). See [CLAUDE.md](../CLAUDE.md).

As the project matures past Phase 1, this section will expand with end-to-end test locations once
they exist (see [roadmap.md](roadmap.md) Phase 5).

## "How to Verify a Feature Works" Checklist Template

Copy this into a PR description or commit message when finishing a roadmap item, and fill it in.
This is deliberately generic — the specifics change per feature, but the shape doesn't.

```markdown
### Verification: <feature name>

- [ ] Ran `docker compose up` from a clean state and the feature was reachable
- [ ] Exercised the golden path manually (describe the exact steps taken)
- [ ] Exercised at least one edge case / failure path (describe it)
- [ ] Added/updated an automated test covering the above
- [ ] `cargo test` / `npm run test` passes locally
- [ ] `cargo clippy -- -D warnings` passes with no new warnings
- [ ] Updated `docs/roadmap.md` to check off the corresponding item
- [ ] Updated other docs if the change affects architecture, tech stack, or setup steps
```

This template will fill out differently for a backend node type versus a frontend screen versus
an infra change — the point is that every completed roadmap item has some evidence attached to it
beyond "it compiled."

## Step 1 Verification Log

Verification for the project skeleton + auth deliverable (`apps/api`, `docker-compose.yml`,
`apps/api/Dockerfile`, `.github/workflows/ci.yml`).

**Environment note:** the sandbox this was built in has no working `docker` CLI (WSL2 distro
without Docker Desktop's WSL integration enabled), so `docker compose up --build` itself could not
be executed here. To still verify the application logic end-to-end, a local PostgreSQL 16 server
was extracted from the Ubuntu `postgresql-16` `.deb` package (no root required — `dpkg-deb -x`)
and run on a non-default port. `docker-compose.yml` and the `Dockerfile` were reviewed by hand for
correctness (service names, health check, env vars, build context, migration-embedding). Anyone
with a working Docker install can run `docker compose up --build` directly — the app code itself
does not know or care whether Postgres is reached via Compose or any other host.

1. **`cargo build`** — clean build, 0 errors.
2. **`cargo fmt -- --check`** — clean after one `cargo fmt` pass; no diffs on recheck.
3. **`cargo clippy --all-targets --all -- -D warnings`** — `cargo clippy: No issues found`.
4. **`cargo test --all`** (against the locally-extracted Postgres) — 6/6 passed:
   ```
   running 6 tests
   test me_without_token_returns_401 ... ok
   test me_with_valid_token_succeeds ... ok
   test register_succeeds ... ok
   test login_succeeds ... ok
   test login_wrong_password_fails ... ok
   test register_duplicate_email_fails ... ok

   test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.63s
   ```
5. **`sqlx migrate run`** (via `sqlx-cli`, installed with `--features postgres,rustls`) — applied
   `0001_create_users` cleanly against a fresh database.
6. **Ran the compiled server directly** (`cargo run --bin api`) against the local Postgres and
   exercised every endpoint with `curl`:
   - `GET /health` → `200 {"status":"ok"}`
   - `POST /api/auth/register` (new email) → `200` with a JWT
   - `POST /api/auth/login` (same credentials) → `200` with a JWT
   - `GET /api/auth/me` with `Authorization: Bearer <token>` → `200` with the user's id/email/
     created_at
   - `GET /api/auth/me` with no header → `401 {"error":"missing or invalid authorization token"}`
7. **Structured request logging** — confirmed `tower_http::trace::TraceLayer` logs
   method/uri/status/latency at the default `RUST_LOG=info` level (its default span level is
   `DEBUG`, which would otherwise silently drop these logs — `TraceLayer` is explicitly configured
   with `DefaultMakeSpan`/`DefaultOnResponse` at `Level::INFO` in `main.rs` to fix this):
   ```
   INFO request{method=GET uri=/health version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=0 ms status=200
   INFO request{method=POST uri=/api/auth/login version=HTTP/1.1}: tower_http::trace::on_response: finished processing request latency=542 ms status=200
   ```
8. **`.github/workflows/ci.yml`** — parsed successfully with PyYAML (the `on:` key reads back as
   the boolean `True` under PyYAML's YAML 1.1 rules, which is expected and harmless — GitHub's own
   parser treats `on:` as the literal trigger key). Confirmed live: pushing this commit triggered
   run [`30153552094`](https://github.com/Sane219/legoOS/actions/runs/30153552094) on
   `Sane219/legoOS`, which passed end to end in 2m44s — checkout, Rust toolchain + clippy/rustfmt
   install, a real `postgres:16-alpine` service container, `sqlx-cli` install, migrations, `cargo
   fmt -- --check`, `cargo clippy --all-targets --all -- -D warnings`, and `cargo test --all` all
   green. This is also the first real Docker-backed confirmation that the Postgres service
   container + migration step work, since the local sandbox this was built in has no usable Docker
   CLI (see the environment note above).
