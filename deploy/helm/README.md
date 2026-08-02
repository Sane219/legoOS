# legoOS Helm chart

Chart lives at `deploy/helm/legoos/`. Deploys api, worker, web, bundled postgres/redis/qdrant,
and optional prometheus/grafana.

## Prerequisites

- A running Kubernetes cluster (any — kind/minikube for local, or a real cluster) with a
  default `StorageClass` for the bundled postgres/redis/qdrant/prometheus PVCs.
- `helm` v3 and `kubectl` on your machine, pointed at that cluster.
- Images for `api`, `worker`, `web` built and pushed somewhere the cluster can pull from
  (the chart does not build images — see `apps/*/Dockerfile`).

## Install

```bash
helm install legoos deploy/helm/legoos -f my-values.yaml
```

## Values you must override before a real install

- `api.image.repository` / `.tag`, `worker.image.*`, `web.image.*` — the default `latest`
  tags and `legoos/*` repo names are placeholders.
- `secrets.jwtSecret`, `secrets.mcpCredentialKey` (64 hex chars — `openssl rand -hex 32`),
  `secrets.anthropicApiKey`, `secrets.voyageApiKey` — the defaults match docker-compose's dev
  values and must not be used anywhere reachable. Prefer setting `secrets.existingSecret` to
  a Secret you manage outside Helm (sealed-secrets, external-secrets, etc.) over passing
  plaintext values through `-f`/`--set`.
- `ingress.enabled` (+ `ingress.className`, `ingress.api.host`, `ingress.web.host`,
  `ingress.tls`) if you want external access instead of `kubectl port-forward`.
- `postgres.auth.password` — the bundled Postgres StatefulSet uses a plaintext password from
  values today; wire it through a Secret before using this for anything real.

## Learning-project shortcuts, noted for the record

- `postgres`, `redis`, `qdrant` are bundled single-replica StatefulSets with PVCs — the
  simplest self-contained option. The production upgrade path is managed services (RDS,
  ElastiCache, Qdrant Cloud): point `global.postgresHost` / `global.redisHost` /
  `global.qdrantHost` at them and drop the corresponding StatefulSet via `-f` overrides (or
  fork the templates) once you make that move.
- `prometheus`/`grafana` are single-replica Deployments with PVCs, not HA — fine for a
  learning/demo cluster, not for production monitoring.

## Staging via CI/CD

`.github/workflows/deploy-staging.yml` builds and pushes `api`/`worker`/`web` images to GHCR
(`ghcr.io/<owner>/legoos-<service>:<sha>`) on every successful `main` CI run (or manually via
`workflow_dispatch`) — that part needs nothing beyond the repo's own `GITHUB_TOKEN` and
always runs. The `deploy` job then runs `helm upgrade --install` with `-f values-staging.yaml`
and `--set *.image.tag=<sha>`, but only if a `STAGING_KUBECONFIG` repo secret (base64-encoded
kubeconfig) is set — until a real staging cluster exists (`deploy/terraform`, then this
chart), that secret is absent and the job logs why and stops rather than failing loudly or
pretending to deploy. Add the secret once a cluster is up to make it real.

## Not verified — read before trusting this chart

This chart was authored **without access to a real Kubernetes cluster or the `helm` CLI**
(neither exists in the sandbox it was written in). What *was* checked:

- `Chart.yaml` and `values.yaml` parse as valid YAML (`python3 -c "import yaml; yaml.safe_load(...)"`).
- Every template file was hand-inspected for balanced `{{ }}` blocks, consistent indentation,
  and correct use of the `legoos.*` helpers.

What was **not** checked, because the tools to check it don't exist here:

- `helm lint deploy/helm/legoos` has not been run.
- `helm template deploy/helm/legoos` has not been run — no template has actually been
  rendered end-to-end, so subtle Go-template bugs (bad indent levels inside `tpl`/`indent`
  calls, wrong `dict` keys) could still be hiding.
- Nothing has been applied to a real or fake cluster (no `kubectl apply --dry-run=server`
  either).

**Run `helm lint` and `helm template` (and ideally a real `helm install` against a throwaway
kind cluster) before trusting this chart in CI or production.**
