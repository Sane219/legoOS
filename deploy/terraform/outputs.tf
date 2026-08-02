output "vpc_id" {
  description = "VPC ID."
  value       = module.vpc.vpc_id
}

output "cluster_name" {
  description = "EKS cluster name. Used with `aws eks update-kubeconfig`."
  value       = module.eks.cluster_name
}

output "cluster_endpoint" {
  description = "EKS API server endpoint."
  value       = module.eks.cluster_endpoint
}

output "cluster_certificate_authority_data" {
  description = "Base64-encoded cluster CA certificate."
  value       = module.eks.cluster_certificate_authority_data
  sensitive   = true
}

output "oidc_provider_arn" {
  description = "OIDC provider ARN for the EKS cluster, for wiring further IRSA roles."
  value       = module.eks.oidc_provider_arn
}

output "rds_endpoint" {
  description = "RDS Postgres connection endpoint (host:port)."
  value       = aws_db_instance.postgres.endpoint
}

output "redis_endpoint" {
  description = "ElastiCache Redis connection endpoint."
  value       = aws_elasticache_cluster.redis.cache_nodes[0].address
}

output "ecr_repository_urls" {
  description = "ECR repository URLs, keyed by service name (api, worker, web)."
  value       = { for name, repo in aws_ecr_repository.services : name => repo.repository_url }
}

output "api_irsa_role_arn" {
  description = "IAM role ARN for the api/worker service account to assume via IRSA."
  value       = aws_iam_role.api_irsa.arn
}
