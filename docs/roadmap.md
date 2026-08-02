# Roadmap

The full phased build plan for legoOS, as a checklist. Phases are sequential in intent but items
within a phase can be reordered as needed. Check items off as they're completed, and keep this
file up to date — see [CLAUDE.md](../CLAUDE.md) rules.

## Phase 1: Foundation

- [x] Set up the Rust workspace (Cargo workspace with `api`, `executor`, `worker` crates)
- [x] Set up the Next.js frontend app with Tailwind CSS configured
- [x] Write the initial `docker-compose.yml` (Postgres, Redis, Qdrant, api, frontend)
- [x] Design and migrate the initial Postgres schema (users, workspaces, sessions)
- [x] Implement user signup/login with password hashing and session/JWT auth
- [x] Implement workspace creation and membership
- [x] Build a basic authenticated frontend shell (login, dashboard layout, nav)
- [x] Define the workflow/node/edge data model in Postgres
- [x] Build DAG executor v1: linear + branching node execution, no queue yet (in-process)
- [x] Build a minimal React Flow canvas that can create and save a simple workflow
- [x] Set up GitHub Actions CI: `cargo build`, `cargo test`, `cargo clippy -- -D warnings`
- [x] Set up GitHub Actions CI: frontend lint, typecheck, and test
- [x] Write developer setup docs so a fresh clone can run `docker compose up` successfully

## Phase 2: AI + Security Core

- [x] Add an LLM provider abstraction supporting at least one cloud provider and one local runtime
- [x] Implement the agent node type: prompt template + model selection + tool list
- [x] Introduce the queue (Redis) and move node execution from in-process to worker processes
- [x] Implement trace event publishing from workers and a WebSocket channel on the API
- [x] Build the live execution trace UI (step-by-step, real-time status per node)
- [x] Implement MCP client support in workers (connect to and call tools on an MCP server)
- [x] Build the MCP connection management UI (add/configure an MCP server per workspace)
- [x] Implement the approval-gate node type (pause execution, wait for human decision)
- [x] Build the approval UI (inbox of pending approvals, approve/reject actions)
- [x] Implement per-workspace permission checks on agent/workflow/MCP configuration

## Phase 3: Knowledge + Intelligence

- [x] Implement document upload and storage for knowledge bases
- [x] Implement document chunking and embedding generation pipeline
- [x] Integrate Qdrant for embedding storage and similarity search
- [x] Implement the RAG node type (retrieve relevant chunks, inject into agent context)
- [ ] Design and implement the long-term memory data model (per-agent, per-workspace)
- [ ] Implement memory write (persist relevant facts/results after a run)
- [ ] Implement memory retrieval (surface relevant memory into a new run's context)
- [ ] Implement workflow scheduling (cron-style triggers, stored and executed on time)
- [ ] Build the scheduling UI (create/edit/pause a scheduled trigger for a workflow)
- [ ] Implement basic evaluation scoring for agent outputs (rule-based and/or LLM-judge)
- [ ] Implement cost tracking per execution (tokens and estimated $ per LLM call)
- [ ] Build the evaluation/cost dashboard (trends over time, per-agent and per-workflow)

## Phase 4: Infra Maturity

- [ ] Instrument API, executor, and workers with Prometheus metrics
- [ ] Build Grafana dashboards for execution throughput, worker health, and queue depth
- [ ] Add structured logging and distributed tracing across API/executor/worker
- [ ] Evaluate and migrate the queue from Redis to NATS for stronger delivery guarantees
- [ ] Write Kubernetes manifests (or Helm chart) for all services
- [ ] Write Terraform for provisioning the target Kubernetes cluster and cloud resources
- [ ] Set up a staging environment deployed via CI/CD
- [ ] Load-test the executor/worker pipeline and document throughput limits

## Phase 5: Product Polish

- [ ] Implement teams and role-based access control (RBAC) within a workspace
- [ ] Add first-party third-party app integrations beyond raw MCP (GitHub, Slack, Gmail, Notion)
- [ ] Polish onboarding flow for a brand-new user/workspace
- [ ] Harden the end-to-end demo workflow (from signup to a working scheduled RAG-backed agent)
- [ ] Write user-facing documentation and a public demo/screencast
- [ ] Perform a security pass (auth, secrets handling, MCP credential storage, RBAC edge cases)
- [ ] Tag and cut the v1 release
