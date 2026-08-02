# ADR-0001: Stay on Redis Streams, don't migrate to NATS JetStream

## Status

Accepted (2026-08-02).

## Context

Roadmap item: "Evaluate and migrate the queue from Redis to NATS for stronger delivery
guarantees." The `queue`/`worker` crates already implement, on top of Redis Streams:

- A consumer group (`XGROUP`/`XREADGROUP`) so multiple worker replicas compete for the
  same stream without double-processing an entry.
- `XACK` on success.
- `XPENDING` + `XCLAIM` to reclaim jobs a crashed worker read but never acked, past a
  visibility timeout (`reclaim_stuck`).
- A `MAX_DELIVERIES` cutoff that routes a job to a dead-letter stream instead of retrying
  forever.
- `workflow_schedules` cron triggers using Postgres `FOR UPDATE SKIP LOCKED` so multiple
  worker replicas split due schedules without double-firing.

So at-least-once delivery, crash recovery, and horizontal worker scaling are already solved
today, by hand, on top of Redis's primitives.

## Options considered

**A. Migrate to NATS JetStream.** JetStream gives durable consumers with three
acknowledgment verbs — ack, nak (redeliver, optionally with backoff), and term (give up) —
plus an `AckWait`-based redelivery timeout, which is the same shape as our hand-rolled
`XPENDING`/`XCLAIM`/`MAX_DELIVERIES` logic but built into the broker instead of the app.
JetStream also supports message-ID deduplication for stronger effectively-once semantics.

**B. Stay on Redis Streams**, keep the current hand-rolled reclaim logic.

## What NATS would concretely add

- Ack/nak/term as first-class verbs instead of `XPENDING`+`XCLAIM`+a manual deliveries
  counter — less code, not more capability we're missing today.
- Built-in message-ID dedup for effectively-once delivery. We're at-least-once today (a
  redelivered job could double-run if a worker crashes after side effects but before
  `XACK`) — this is a real gap, but it's one dedup could close on *either* broker (an
  idempotency key per execution_id would work against Redis too).
- Materially higher producer/consumer throughput at scale (~820k vs ~480k msg/s in
  published 2026 benchmarks) — not a constraint we're anywhere near; workflow executions
  are nowhere close to 10k/s.
- Lower operational burden for consumer-group semantics in a Kubernetes-native deployment,
  per current (2026) guidance favoring NATS for k8s microservice communication.

Redis Streams, per the same sources, has a real *known* gap we haven't hit yet: unbounded
stream growth without `MAXLEN`/`XTRIM`. Checking `apps/worker/src/lib.rs`, our `XADD` calls
(job stream and dead-letter stream) don't set `MAXLEN` — this is a real, currently-open
issue, independent of NATS. Worth its own follow-up ticket.

## Migration cost (if we chose A)

- Rewrite `apps/queue` (new client crate, `RunJob` publish/subscribe semantics) and
  `worker`'s whole consume loop (`ensure_group`, `read_new`, `reclaim_stuck`,
  `run_due_schedules`'s enqueue step, `process_entry`'s ack).
  Every test currently exercising a job through Redis would need rewriting to a NATS
  test double or a NATS CI service: `apps/api/tests/schedules.rs`,
  `apps/worker/tests/schedules.rs`, `apps/worker/tests/resolve_mcp_connections.rs`,
  `apps/api/tests/trace.rs`, `apps/api/tests/workflows.rs`, `apps/api/tests/approvals.rs`
  (all of these call `run_execution_inline`/hit Redis directly).
- Cutover plan for in-flight jobs during deploy (drain the Redis stream or dual-write).
- A NATS service container in CI — low-risk to add (same pattern as the existing
  postgres/redis/qdrant services), but still a new moving part to maintain.
- Redis is also still used elsewhere in this codebase (trace event pub/sub in
  `apps/api/src/trace.rs`) — migrating just the job queue leaves Redis in the stack
  regardless, so "remove Redis" isn't a side benefit of this migration.

## Decision

**Stay on Redis Streams.** The concrete capability gap JetStream would close — built-in
ack/nak/term instead of hand-rolled `XCLAIM` — is already implemented and tested here. The
throughput/latency case for JetStream doesn't apply at legoOS's current or foreseeable
scale (workflow executions, not high-frequency event streaming). The migration cost (rewrite
+ re-verify five-plus test files + new CI service + cutover plan) is not justified by a
guarantee we don't currently need and Redis isn't going away from the stack anyway (trace
pub/sub still uses it).

## Consequences

- No migration work undertaken now; `apps/queue`/`worker` unchanged.
- Two real, independent follow-ups surfaced by this evaluation, tracked separately from the
  NATS question:
  1. Add `MAXLEN` (approximate, `~`) to both `XADD` calls in `apps/worker/src/lib.rs` so
     the job and dead-letter streams don't grow unbounded.
  2. Consider an idempotency key (e.g. skip re-running a node whose result already exists
     for that `execution_id`) to close the at-least-once double-processing gap — this
     would help regardless of broker choice.

## Revisit trigger

Revisit this decision if any of the following becomes true:
- Workflow execution volume approaches Redis Streams' practical throughput ceiling
  (~10k+ jobs/sec sustained) — nowhere close today.
- We need JetStream-specific features Redis Streams can't do at all: multi-datacenter
  stream replication/mirroring, or exactly-once semantics that a simple idempotency key
  can't approximate.
- The team is already standardizing on NATS for service-to-service messaging elsewhere
  (e.g. replacing the trace pub/sub channel too), making a single migration worth doing
  once for both use cases instead of just the job queue in isolation.

## Sources

- [NATS vs. Kafka vs. Redis Streams for Java Microservices](https://www.javacodegeeks.com/2026/03/nats-vs-kafka-vs-redis-streams-for-java-microservices-when-simpler-actually-wins.html)
- [Real-Time Event Streaming: Kafka vs Redis Streams vs NATS in 2026](https://dev.to/young_gao/real-time-event-streaming-kafka-vs-redis-streams-vs-nats-in-2026-34o1)
- [Reliable Message Delivery in NATS JetStream: Acks, Retries, Dead Letters, and Replay — Synadia](https://www.synadia.com/blog/jetstream-reliable-delivery-dlq-replay)
- [Consumers | NATS Docs](https://docs.nats.io/nats-concepts/jetstream/consumers)
- [How to Use Redis Streams for Durable Message Queues](https://how2.sh/posts/how-to-use-redis-streams-for-real-time-messaging/)
