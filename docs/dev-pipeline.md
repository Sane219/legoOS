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
