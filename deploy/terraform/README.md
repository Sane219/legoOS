# legoOS Terraform (Phase 4 infra)

Provisions the cloud infrastructure the `deploy/helm/legoos` chart deploys onto. This directory
does not touch Kubernetes resources — the handoff point is: Terraform builds a cluster with
`kubectl`/`helm` access, then the Helm chart takes it from there.

## Cloud choice: AWS / EKS

AWS was picked because it's the most common choice for a project like this to showcase, and EKS
is the standard managed-Kubernetes option there, with mature, well-maintained upstream Terraform
modules (`terraform-aws-modules/vpc/aws`, `terraform-aws-modules/eks/aws`). GCP (GKE) or Azure
(AKS) would be equally valid — porting this would mean swapping the VPC/EKS modules for
`terraform-google-modules/network`+GKE or the `Azure/aks` module equivalents, and RDS/ElastiCache
for Cloud SQL/Memorystore or Azure Database/Cache. No multi-cloud abstraction is built here; pick
one and commit to it.

## What this provisions

- **VPC** (`terraform-aws-modules/vpc/aws` ~> 5.13): public + private subnets across 3 AZs, one
  NAT gateway for non-production environments (one per AZ for `production`), route tables.
- **EKS** (`terraform-aws-modules/eks/aws` ~> 20.31): cluster + one managed node group
  (2-3x `t3.medium` by default — sized for a small app, not for load). IRSA (IAM Roles for
  Service Accounts) is enabled via the cluster's OIDC provider.
- **RDS Postgres**: single instance, single-AZ, `db.t4g.micro` by default. This is the production
  upgrade path away from the in-cluster Postgres StatefulSet the Helm chart runs for local/dev —
  point the app's `DATABASE_URL` at this instead once it exists. Bump `multi_az = true` in
  `main.tf` as the production-hardening step; not done by default to keep cost down.
- **ElastiCache Redis**: single node, `cache.t4g.micro` by default. Same reasoning as RDS — the
  production upgrade path from the in-cluster Redis the Helm chart runs for local/dev.
- **Qdrant**: intentionally **not** provisioned here. There's no first-party Terraform provider
  for Qdrant Cloud. Self-hosting on the EKS cluster (what the Helm chart already does) is the
  pragmatic default for this project's stage — revisit Qdrant Cloud only if usage grows enough to
  justify its cost.
- **ECR**: one repository per service (`legoos/api`, `legoos/worker`, `legoos/web`) with image
  scanning on push. This is what the (separate, not-yet-built) staging CI/CD pipeline will push
  images to.
- **IAM / IRSA**: the EKS module's OIDC provider plus one example role (`api_irsa`) scoped to let
  the `legoos-api` service account read secrets under `legoos/<env>/*` in Secrets Manager. Add
  further roles the same way only when a pod actually needs another AWS permission — no
  speculative roles are pre-built here.
- **Remote state**: S3 backend + DynamoDB lock table, configured in `backend.tf`.

## Prerequisites

1. An AWS account and credentials (`aws configure` or equivalent env vars/SSO) with permission to
   create VPCs, EKS clusters, RDS/ElastiCache instances, ECR repos, and IAM roles.
2. Terraform >= 1.7.0 and the AWS CLI installed locally.
3. **Bootstrap the state backend** (one-time, out of band — Terraform can't create the bucket it
   stores its own state in):
   ```bash
   aws s3api create-bucket --bucket legoos-tfstate-<your-suffix> --region us-east-1
   aws s3api put-bucket-versioning --bucket legoos-tfstate-<your-suffix> \
     --versioning-configuration Status=Enabled
   aws dynamodb create-table --table-name legoos-tfstate-lock \
     --attribute-definitions AttributeName=LockID,AttributeType=S \
     --key-schema AttributeName=LockID,KeyType=HASH \
     --billing-mode PAY_PER_REQUEST
   ```
   Then update the bucket name in `backend.tf` (bucket names are globally unique, so the
   placeholder `legoos-tfstate-CHANGEME` will not work as-is).
4. Copy `terraform.tfvars.example` to `terraform.tfvars` and fill in real values. Set
   `rds_password` via `TF_VAR_rds_password` (or a secrets manager) rather than in the file.

## Usage

```bash
terraform init
terraform plan
terraform apply
```

Then hand off to the Helm chart:

```bash
aws eks update-kubeconfig --name <cluster_name output> --region us-east-1
helm install legoos deploy/helm/legoos -n legoos --create-namespace
```

Point the chart's Postgres/Redis config at the `rds_endpoint`/`redis_endpoint` outputs instead of
the chart's own in-cluster Postgres/Redis when running against this infrastructure.

## Estimated cost (rough — verify against current AWS pricing before trusting this)

For the staging-sized defaults (single NAT gateway, 2x t3.medium nodes, db.t4g.micro,
cache.t4g.micro), a very rough US-East-1 ballpark is **$250-350/month**, dominated by the EKS
control plane (~$73/mo flat), the 2 worker nodes, and the single NAT gateway's hourly + data
processing charges. This is a napkin estimate, not a quote — run it through the
[AWS Pricing Calculator](https://calculator.aws) with current rates before committing budget.

## Honest status: unverified

This was authored **without a `terraform` binary or an AWS account available to validate
against** — there is no sandbox in this environment to run `terraform init`/`validate`/`plan`
against. The HCL was written carefully by hand against the documented module interfaces
(`terraform-aws-modules/vpc/aws` ~> 5.13, `terraform-aws-modules/eks/aws` ~> 20.31) and standard
AWS resource schemas, but:

- `terraform validate` has never been run against this configuration.
- No `terraform plan` has been generated — resource arguments, module output names, and IAM
  policy JSON have not been checked against a real provider schema.
- No cost, quota, or region-availability issues have been checked against a real account.

**Run `terraform validate` and a real `terraform plan`/`apply` in a disposable sandbox AWS account
before trusting this anywhere near production billing or a real cluster.**
