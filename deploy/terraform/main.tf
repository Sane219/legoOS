locals {
  name = "${var.project}-${var.environment}"
}

provider "aws" {
  region = var.aws_region
  default_tags {
    tags = var.tags
  }
}

data "aws_availability_zones" "available" {
  state = "available"
}

# ---------------------------------------------------------------------------
# VPC — public + private subnets across 2-3 AZs, NAT gateway(s), route tables.
# Using the upstream module rather than hand-rolling this; it's the de facto
# standard for AWS VPCs in Terraform.
# ---------------------------------------------------------------------------
module "vpc" {
  source  = "terraform-aws-modules/vpc/aws"
  version = "~> 5.13"

  name = local.name
  cidr = var.vpc_cidr
  azs  = var.azs

  # /24s carved out of the /16, one public + one private per AZ.
  private_subnets = [for i, az in var.azs : cidrsubnet(var.vpc_cidr, 8, i)]
  public_subnets  = [for i, az in var.azs : cidrsubnet(var.vpc_cidr, 8, i + 100)]

  enable_nat_gateway = true
  single_nat_gateway  = var.environment != "production" # one NAT for staging keeps cost down; one per AZ in prod
  enable_dns_hostnames = true
  enable_dns_support   = true

  # Required tags for EKS to discover subnets for load balancers.
  public_subnet_tags = {
    "kubernetes.io/role/elb"                     = "1"
    "kubernetes.io/cluster/${local.name}"        = "shared"
  }
  private_subnet_tags = {
    "kubernetes.io/role/internal-elb"            = "1"
    "kubernetes.io/cluster/${local.name}"        = "shared"
  }

  tags = var.tags
}

# ---------------------------------------------------------------------------
# EKS cluster + one managed node group sized for a small app.
# Using the upstream module rather than hand-rolling control plane / node
# group / auth wiring.
# ---------------------------------------------------------------------------
module "eks" {
  source  = "terraform-aws-modules/eks/aws"
  version = "~> 20.31"

  cluster_name    = local.name
  cluster_version = var.eks_cluster_version

  vpc_id     = module.vpc.vpc_id
  subnet_ids = module.vpc.private_subnets

  cluster_endpoint_public_access = true

  # Needed so we can wire IRSA roles for pods below.
  enable_irsa = true

  eks_managed_node_groups = {
    default = {
      instance_types = [var.eks_node_instance_type]
      min_size       = var.eks_node_min_size
      max_size       = var.eks_node_max_size
      desired_size   = var.eks_node_desired_size
      capacity_type  = "ON_DEMAND"
    }
  }

  tags = var.tags
}

# ---------------------------------------------------------------------------
# RDS Postgres — production upgrade path from the in-cluster Postgres
# StatefulSet the Helm chart (deploy/helm/legoos) runs for local/dev.
# Single-AZ for this project's stage; set multi_az = true as the production
# hardening step once this needs real availability guarantees.
# ---------------------------------------------------------------------------
resource "aws_db_subnet_group" "postgres" {
  name       = "${local.name}-postgres"
  subnet_ids = module.vpc.private_subnets
  tags       = var.tags
}

resource "aws_security_group" "postgres" {
  name        = "${local.name}-postgres"
  description = "Allow Postgres access from the EKS node security group only"
  vpc_id      = module.vpc.vpc_id

  ingress {
    from_port       = 5432
    to_port         = 5432
    protocol        = "tcp"
    security_groups = [module.eks.node_security_group_id]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = var.tags
}

resource "aws_db_instance" "postgres" {
  identifier     = "${local.name}-postgres"
  engine         = "postgres"
  engine_version = var.rds_engine_version

  instance_class    = var.rds_instance_class
  allocated_storage = var.rds_allocated_storage_gb
  storage_encrypted = true

  db_name  = var.rds_database_name
  username = var.rds_username
  password = var.rds_password

  db_subnet_group_name   = aws_db_subnet_group.postgres.name
  vpc_security_group_ids = [aws_security_group.postgres.id]

  multi_az            = false # bump to true for production HA
  publicly_accessible = false
  skip_final_snapshot = var.environment != "production"

  backup_retention_period = var.environment == "production" ? 7 : 1

  tags = var.tags
}

# ---------------------------------------------------------------------------
# ElastiCache Redis — production upgrade path from the in-cluster Redis the
# Helm chart runs for local/dev. Single node for this project's stage.
# ---------------------------------------------------------------------------
resource "aws_elasticache_subnet_group" "redis" {
  name       = "${local.name}-redis"
  subnet_ids = module.vpc.private_subnets
  tags       = var.tags
}

resource "aws_security_group" "redis" {
  name        = "${local.name}-redis"
  description = "Allow Redis access from the EKS node security group only"
  vpc_id      = module.vpc.vpc_id

  ingress {
    from_port       = 6379
    to_port         = 6379
    protocol        = "tcp"
    security_groups = [module.eks.node_security_group_id]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = var.tags
}

resource "aws_elasticache_cluster" "redis" {
  cluster_id           = "${local.name}-redis"
  engine               = "redis"
  engine_version       = var.redis_engine_version
  node_type            = var.redis_node_type
  num_cache_nodes      = 1
  port                 = 6379
  subnet_group_name    = aws_elasticache_subnet_group.redis.name
  security_group_ids   = [aws_security_group.redis.id]

  tags = var.tags
}

# Qdrant is intentionally NOT provisioned here — there's no first-party
# Terraform provider for Qdrant Cloud. Self-hosting on the EKS cluster
# (what deploy/helm/legoos already does) is the pragmatic default; revisit
# Qdrant Cloud only if usage grows enough to justify its cost. See README.

# ---------------------------------------------------------------------------
# ECR — one repository per deployable service. This is what the (separate,
# not-yet-built) staging CI/CD pipeline will push images to.
# ---------------------------------------------------------------------------
resource "aws_ecr_repository" "services" {
  for_each = toset(var.ecr_repositories)

  name                 = "${var.project}/${each.value}"
  image_tag_mutability = "IMMUTABLE"

  image_scanning_configuration {
    scan_on_push = true
  }

  tags = var.tags
}

# ---------------------------------------------------------------------------
# IRSA wiring: the EKS module already creates the cluster's OIDC provider
# (enable_irsa = true above). Below is one example least-privilege role —
# for the api/worker service account to read app secrets from Secrets
# Manager — showing the pattern. Add more roles the same way only once a
# pod actually needs another AWS permission; don't pre-build roles for
# permissions nothing uses yet.
# ---------------------------------------------------------------------------
data "aws_iam_policy_document" "api_irsa_assume" {
  statement {
    effect  = "Allow"
    actions = ["sts:AssumeRoleWithWebIdentity"]

    principals {
      type        = "Federated"
      identifiers = [module.eks.oidc_provider_arn]
    }

    condition {
      test     = "StringEquals"
      variable = "${module.eks.oidc_provider}:sub"
      values   = ["system:serviceaccount:legoos:legoos-api"]
    }

    condition {
      test     = "StringEquals"
      variable = "${module.eks.oidc_provider}:aud"
      values   = ["sts.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "api_irsa" {
  name               = "${local.name}-api-irsa"
  assume_role_policy = data.aws_iam_policy_document.api_irsa_assume.json
  tags               = var.tags
}

data "aws_iam_policy_document" "api_secrets_read" {
  statement {
    effect  = "Allow"
    actions = ["secretsmanager:GetSecretValue"]
    resources = [
      "arn:aws:secretsmanager:${var.aws_region}:*:secret:${var.project}/${var.environment}/*"
    ]
  }
}

resource "aws_iam_role_policy" "api_secrets_read" {
  name   = "${local.name}-api-secrets-read"
  role   = aws_iam_role.api_irsa.id
  policy = data.aws_iam_policy_document.api_secrets_read.json
}
