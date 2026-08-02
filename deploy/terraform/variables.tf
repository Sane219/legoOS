variable "project" {
  description = "Short project name used as a prefix for resource names/tags."
  type        = string
  default     = "legoos"
}

variable "environment" {
  description = "Environment name (e.g. staging, production). Used in resource naming/tags."
  type        = string
  default     = "staging"
}

variable "aws_region" {
  description = "AWS region to provision into."
  type        = string
  default     = "us-east-1"
}

variable "vpc_cidr" {
  description = "CIDR block for the VPC."
  type        = string
  default     = "10.42.0.0/16"
}

variable "azs" {
  description = "Availability zones to spread subnets across (2-3 for HA)."
  type        = list(string)
  default     = ["us-east-1a", "us-east-1b", "us-east-1c"]
}

variable "eks_cluster_version" {
  description = "Kubernetes version for the EKS control plane."
  type        = string
  default     = "1.30"
}

variable "eks_node_instance_type" {
  description = "Instance type for the managed node group. t3.medium is enough for a small app; bump or add a second node group if load-test results (separate roadmap item) show it's needed."
  type        = string
  default     = "t3.medium"
}

variable "eks_node_desired_size" {
  description = "Desired node count in the managed node group."
  type        = number
  default     = 2
}

variable "eks_node_min_size" {
  description = "Minimum node count."
  type        = number
  default     = 2
}

variable "eks_node_max_size" {
  description = "Maximum node count. Kept close to desired for now — no autoscaling policy is wired up yet, see README."
  type        = number
  default     = 3
}

variable "rds_instance_class" {
  description = "Instance class for the RDS Postgres instance."
  type        = string
  default     = "db.t4g.micro"
}

variable "rds_engine_version" {
  description = "Postgres engine version for RDS."
  type        = string
  default     = "16.4"
}

variable "rds_allocated_storage_gb" {
  description = "Allocated storage (GB) for RDS."
  type        = number
  default     = 20
}

variable "rds_database_name" {
  description = "Default database name created on the RDS instance."
  type        = string
  default     = "legoos"
}

variable "rds_username" {
  description = "Master username for the RDS instance."
  type        = string
  default     = "legoos"
}

variable "rds_password" {
  description = "Master password for the RDS instance. Pass via TF_VAR_rds_password or a secrets manager — never commit a real value."
  type        = string
  sensitive   = true
}

variable "redis_node_type" {
  description = "Node type for the ElastiCache Redis instance."
  type        = string
  default     = "cache.t4g.micro"
}

variable "redis_engine_version" {
  description = "Redis engine version for ElastiCache."
  type        = string
  default     = "7.1"
}

variable "ecr_repositories" {
  description = "Names of ECR repositories to create, one per deployable service."
  type        = list(string)
  default     = ["api", "worker", "web"]
}

variable "tags" {
  description = "Common tags applied to all resources."
  type        = map(string)
  default = {
    Project   = "legoos"
    ManagedBy = "terraform"
  }
}
