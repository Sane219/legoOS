# Architecture Decision Records

A log of significant architectural decisions, why they were made, and what alternatives were
considered. Format: one entry per decision, numbered sequentially, never renumbered or deleted
(superseded decisions get a new entry that references the old one).

---

## ADR-001: Rust + Axum for the Core Orchestration Engine

**Status:** Accepted

**Context:** The core orchestration engine (API, DAG executor, workers) needed a language and web
framework. The realistic alternatives were Node.js (TypeScript) and Python, both of which have
mature, widely-used AI/agent ecosystems and would have let the same person write backend and
frontend in one language (Node) or lean on the deepest existing AI library ecosystem (Python).

**Decision:** Build the core orchestration engine in Rust, using Axum as the web framework.

**Rationale:**

- **Async performance** — the executor and workers are I/O-bound (LLM calls, MCP tool calls,
  queue/DB round-trips) and need to handle many concurrent, mostly-idle tasks cheaply. Tokio's
  async runtime is well-suited to this profile, with lower memory overhead per concurrent task
  than Node's event loop or Python's asyncio at meaningful scale.
- **Memory safety** — a scheduler/worker system with shared execution state is exactly the kind of
  code where data races are easy to introduce accidentally. Rust's ownership model rules out that
  class of bug at compile time rather than relying on runtime discipline.
- **Alignment with the emerging Rust-native AI agent ecosystem** — projects like Rig, AutoAgents,
  and swarms-rs are building agent primitives natively in Rust. Building the core engine in Rust
  keeps the door open to adopting or interoperating with that ecosystem, rather than working
  against it from Node or Python.
- Axum specifically was chosen over Actix-web/Rocket for its tight, idiomatic integration with
  Tokio and Tower middleware, keeping the async stack coherent end to end.

**Consequences:** The frontend and backend are in different languages, so there's no shared type
definitions between them without extra tooling (e.g. generating TypeScript types from Rust). The
Rust AI/LLM ecosystem is younger and less battle-tested than Python's, so some LLM-provider or
tooling integrations may need to be written in-house rather than pulled off the shelf.
