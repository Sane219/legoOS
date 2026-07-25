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

## Step 2 Verification Log

Verification for the frontend skeleton (`apps/web`: Next.js + Tailwind, a login/register/dashboard
shell wired to the Step 1 API, `apps/web/Dockerfile`, frontend CI).

1. **`npm run lint`** (`next lint` via ESLint) — clean. One real issue surfaced and was fixed
   along the way: the newer `react-hooks/set-state-in-effect` rule flagged `Nav` calling
   `setState` synchronously inside a `useEffect` to read the auth token from `localStorage`.
   Rather than suppress it, `lib/auth.ts` now exposes a `useAuthToken()` hook built on React's
   `useSyncExternalStore` — the correct primitive for syncing component state to an external store
   like `localStorage`, which also picks up token changes from other tabs via the `storage` event.
2. **`npm run typecheck`** (`tsc --noEmit`, `strict: true`) — clean, no `any` anywhere.
3. **`npm run test`** (Vitest + Testing Library) — 4/4 passed, covering `Nav`'s authed/unauthed
   states and `AuthForm`'s success and error paths.
4. **`npm run build`** (`next build`, `output: "standalone"`) — succeeds; verified the standalone
   output actually contains `server.js` and `.next/static`, which is what `apps/web/Dockerfile`
   copies into the runtime image.
5. **Real browser end-to-end run** — per this repo's rule to exercise UI changes in an actual
   browser, not just unit tests: `next start` was run against a locally-running copy of the Step 1
   API (same local-Postgres setup as the Step 1 log, since Docker still isn't available in this
   sandbox — see the environment note above), driven with Playwright/Chromium. All of the
   following passed against the real running app:
   - `/` while logged out redirects to `/login`
   - registering a new user redirects to `/dashboard` and renders that user's email/id/joined date
   - the nav shows "Dashboard"/"Log out" once authed
   - logging out redirects to `/login`, and `/dashboard` then redirects back to `/login`
   - logging back in with the same credentials reaches `/dashboard` again
   - logging in with a wrong password shows "invalid credentials" and stays on `/login`

   This run caught a real bug that no unit test would have: the API had no CORS headers, so the
   browser silently blocked every cross-origin `fetch` from `localhost:3000` to `localhost:8080`
   with a preflight failure (curl and the Rust integration tests don't enforce CORS, so Step 1's
   verification never exercised this path). Fixed in `apps/api/src/main.rs` by adding
   `tower_http::cors::CorsLayer::permissive()` — marked with a `ponytail:` comment since it's
   deliberately wide open for local/dev use (auth here is a bearer token the frontend JS attaches
   explicitly, not a cookie, so this isn't a CSRF hole yet) and needs scoping to known origins
   during the Phase 5 security pass.
6. **`.github/workflows/ci.yml`** — added a `frontend` job (checkout, Node 20, `npm ci`, lint,
   typecheck, test, build). Confirmed live: pushing this commit triggered run
   [`30154453882`](https://github.com/Sane219/legoOS/actions/runs/30154453882), where both the
   `frontend` job (40s) and `backend` job (1m4s) passed.

## Step 3 Verification Log

Verification for workspaces & teams (migration `0002_create_workspaces`, `apps/api/src/workspaces.rs`,
`POST/GET /api/workspaces`, `GET /api/workspaces/{id}`, `GET/POST /api/workspaces/{id}/members`).

Scope note: the roadmap item "Design and migrate the initial Postgres schema (users, workspaces,
sessions)" is left unchecked — `workspaces` and `workspace_members` are migrated, but a `sessions`
table was deliberately not added. Auth here is stateless JWT with no server-side session state to
back (see [decisions.md](decisions.md) / `apps/api/src/jwt.rs`), so a `sessions` table would have
no consumer yet; add one if/when refresh tokens or server-side revocation are needed.

1. **`sqlx migrate run`** — applied `0002_create_workspaces` cleanly on top of `0001_create_users`.
2. **`cargo fmt -- --check`** / **`cargo clippy --all-targets --all -- -D warnings`** — clean.
3. **`cargo test --all`** — 14/14 passed (6 existing auth tests + 8 new workspace tests: create,
   empty-name validation, list-is-scoped-to-membership, 404-for-non-members, add-member-as-owner
   plus it showing up in the member list, 403-when-not-owner, 409-on-duplicate-membership, and
   400-on-unknown-email).
4. **Live curl run against the real server** (same local-Postgres setup as prior steps, migrated
   with `sqlx migrate run`) covering the full golden path and every error path:
   - register two users, create a workspace as the first (owner) — `200`
   - list workspaces as the owner — `200`, shows the one workspace
   - get the workspace by id as the owner — `200`
   - add the second user as a member — `200`, `role: "member"`
   - list members — `200`, shows both the owner and the new member
   - the new member (not an owner) tries to add someone else — `403`
   - adding the same member again — `409`
   - adding an unknown email — `400`
   - a third, unrelated user requesting the workspace by id — `404` (not merely 403, so workspace
     existence isn't leaked to non-members)
