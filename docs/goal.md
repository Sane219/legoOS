# Goal

## Purpose

aios exists to make agentic AI work operable, not just possible. Anyone can wire an LLM to a
prompt and call it an agent. Making that agent reliable — observable when it runs, safe when it
acts, connected to real data and real tools, and cheap enough to run continuously — is a much
harder and much less solved problem. aios is a self-hosted platform that treats an AI agent as a
piece of infrastructure: something you design visually, version, test, monitor, and trust, the
same way you'd treat a service in production rather than a one-off script.

Concretely, aios lets you:

- Define agents with their own prompts, tools, and model choice
- Compose those agents into multi-step workflows using a visual, DAG-based builder
- Connect agents to the outside world through MCP servers and third-party app integrations
- Ground agents in your own data via RAG-backed knowledge bases
- Give agents memory that persists across runs, not just within a single conversation
- Run models locally or in the cloud, and switch between them per-agent or per-step
- Schedule workflows to run on a timer or in response to external events
- Watch every execution happen live, step by step, with full tracing
- Track quality (evaluation) and spend (cost) over time
- Insert human approval gates before an agent is allowed to take a risky action
- Collaborate on all of the above as a team, with shared workspaces and permissions

## Target Users

- **Individual builders / hobbyists** who want a self-hosted alternative to stitching together
  ChatGPT, Zapier, and a vector DB by hand, and who want to own their data and their agent logic.
- **Small technical teams** who need to automate real workflows (support triage, data pipelines,
  internal tooling) with LLM steps in the loop, and need to trust what the agent did after the
  fact — not just hope it worked.
- **Developers learning agentic AI architecture** who want a reference implementation of a DAG
  executor, MCP integration, RAG pipeline, and agent memory system, built in the open.
- **Self-hosting / privacy-conscious users** who don't want their agent workflows, documents, or
  conversation history living in someone else's SaaS product.

aios is explicitly *not* trying to compete with hosted, consumer-facing chat products on
day-one polish. It's trying to be the tool a technical user reaches for when they want an agent
platform they control end to end.

## What "Done" Looks Like at v1

v1 is reached when a user can, entirely self-hosted:

1. Sign in, create a workspace, and define at least one agent with a custom prompt and tool set.
2. Build a multi-step workflow visually, connecting agent nodes, MCP tool calls, and conditional
   branches into a DAG, and execute it.
3. Connect a real MCP server (e.g. GitHub or Slack) and have an agent call it as a tool mid-workflow.
4. Upload a set of documents, have them chunked and embedded, and have an agent answer questions
   grounded in that knowledge base (RAG).
5. Have an agent recall relevant context from a previous run without it being re-supplied in the
   prompt (long-term memory).
6. Schedule a workflow to run on a cron schedule without manual triggering.
7. Watch a workflow execute in real time via a live trace UI, and inspect a past execution's full
   step-by-step history after the fact.
8. See basic evaluation and cost metrics for a workflow's runs over time.
9. Require human approval before a specific node executes, and have the workflow pause and resume
   correctly around that gate.
10. Invite a second team member into the workspace with a restricted role, and have permissions
    enforced.

Everything in [docs/roadmap.md](roadmap.md) exists to reach this list. Anything not required to
reach it is explicitly deferred to a later phase.
