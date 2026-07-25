# Tech Stack

Every major technology choice, and the reasoning behind it. See
[decisions.md](decisions.md) for the formal ADR log; this file covers the full stack in one
place.

## Backend: Rust

The core orchestration engine (API, DAG executor, workers) is written in Rust.

- **Async performance** — the executor and workers spend most of their time waiting on I/O (LLM
  calls, MCP tool calls, DB/queue round-trips). Tokio's async runtime handles high volumes of
  concurrent, mostly-idle tasks with a small memory footprint, which matters once many workflows
  are running concurrently.
- **Memory safety without a GC** — no garbage collector pauses affecting execution latency, and
  the borrow checker rules out a whole class of concurrency bugs (data races) that would otherwise
  be easy to introduce in a highly concurrent scheduler/worker system.
- **Alignment with the emerging Rust AI agent ecosystem** — projects like Rig, AutoAgents, and
  swarms-rs are building agent primitives natively in Rust. Building aios in Rust keeps the door
  open to adopting or interoperating with that ecosystem instead of working against it.
- **Single static binary deployment** — simplifies the Docker images and reduces runtime
  dependency surface compared to a Node or Python service.

## Axum

Axum is the HTTP framework for the API layer. Chosen over alternatives (Actix-web, Rocket) for its
tight integration with Tokio and Tower (middleware, timeouts, tracing), its minimal-magic
extractor-based API, and its active maintenance by the Tokio team — keeping the whole async stack
(Tokio, Axum, Tower, tracing) coherent and idiomatic.

## Frontend: Next.js + React + Tailwind CSS

- **Next.js / React** — the dominant choice for building a data-heavy, interactive SPA-style
  dashboard, with a large ecosystem and easy deployment story.
- **Tailwind CSS** — utility-first styling that keeps a one-person (or small-team) frontend
  consistent without hand-rolling a design system from scratch.
- **React Flow** — purpose-built for node/edge diagram editors, which is exactly what the visual
  workflow builder is. Handles pan/zoom, node dragging, edge routing, and custom node rendering
  out of the box, avoiding a large chunk of bespoke canvas/SVG work.

## PostgreSQL

The system of record for all durable, relational state: users, workspaces, permissions, agent and
workflow definitions, execution/task history, evaluation results, and cost data. Chosen over a
NoSQL store because workflow/execution data is inherently relational (workflows have nodes, nodes
have dependencies, executions have tasks, tasks belong to executions) and benefits from real
transactions, foreign keys, and mature tooling (migrations, backups, JSONB for flexible node
config where needed).

## Redis / NATS (Queue)

Decouples the DAG executor from workers so slow steps (LLM calls, MCP tool calls) never block
scheduling, and so workers can scale horizontally. Redis is the Phase 1–3 choice: it's simple to
run locally (already needed for caching-adjacent use cases), has mature client libraries, and is
enough for the throughput of an early-stage single-node deployment. NATS is the planned Phase 4
upgrade once the platform needs stronger delivery guarantees, multi-node fan-out, or higher
sustained throughput than a single Redis instance comfortably provides — see
[decisions.md](decisions.md).

## Qdrant (Vector Database)

Stores embeddings for RAG knowledge base documents and, later, long-term agent memory entries.
Chosen for being purpose-built for vector similarity search (as opposed to bolting a vector
extension onto Postgres), its straightforward Docker deployment, and a query API that's simple to
call from Rust workers via HTTP/gRPC.

## React Flow

Called out separately from "Frontend" above because it's the single biggest lever on how fast the
visual workflow builder can be built: it owns the entire canvas/graph-editing interaction model
(nodes, edges, drag-to-connect, minimap, zoom), leaving aios to focus on domain-specific node types
and execution overlays rather than reimplementing a diagramming library.

## Infra: Docker, Kubernetes, Terraform, Prometheus, Grafana, GitHub Actions

- **Docker** — the baseline for local dev (`docker compose up`) and deployment; every service
  (API, executor, workers, Postgres, Redis, Qdrant) runs as a container from day one so local and
  production environments stay close.
- **Kubernetes** — the Phase 4 target for running aios at scale, once a single Docker Compose host
  isn't enough (horizontal worker scaling, rolling deploys, self-healing).
- **Terraform** — infrastructure as code for provisioning the Kubernetes cluster and supporting
  cloud resources reproducibly, rather than hand-configuring infra.
- **Prometheus + Grafana** — metrics collection and dashboards for execution throughput, worker
  health, queue depth, and LLM cost — the observability layer that makes "is this system healthy"
  answerable at a glance.
- **GitHub Actions** — CI on every push: build, test, and lint for both the Rust backend and the
  Next.js frontend, catching regressions before they merge.
