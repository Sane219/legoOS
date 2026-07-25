# CLAUDE.md

## Tech Stack

- Backend: Rust, Axum, Tokio
- Frontend: Next.js, React, Tailwind CSS
- Data: PostgreSQL, Redis, Qdrant

## Coding Conventions

- Rust: use `anyhow` for application errors and `thiserror` for library/typed errors; prefer
  explicit `Result` types over panics or `unwrap()` outside of tests
- Rust: format with `cargo fmt`, lint with `cargo clippy`
- Frontend: TypeScript strict mode is on; never use `any`

## Commands

- `docker compose up` — run the full stack locally
- `cargo test` — run backend tests
- `npm run test` — run frontend tests
- `cargo clippy -- -D warnings` — must pass clean before every commit

## Rules

- Never hardcode secrets or API keys; use environment variables
- Always add a test for new node types in the DAG executor
- Always update `docs/roadmap.md`'s checklist when a roadmap item is completed
