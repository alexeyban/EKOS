output "url" {
  description = "Where the console is. Kubernetes without a domain: through `kubectl port-forward`."
  value       = local.public_url
}

output "compute" {
  value = var.compute
}

output "runtime_role_arn" {
  value = aws_iam_role.runtime.arn
}

output "secrets" {
  description = "Secrets Manager secret names, by the variable each becomes."
  value       = local.all_secret_names
}

output "backup_url" {
  description = "Where scheduled backups go; restore with `ekos home restore --from <url>`."
  value       = local.backup_url
}

output "production" {
  value = module.common.production
}

output "static_tokens" {
  description = "Read and write sign-in tokens, only without OIDC."
  value       = module.common.static_tokens
  sensitive   = true
}

output "vm_public_ip" {
  value = try(aws_eip.vm[0].public_ip, null)
}

output "vm_instance_id" {
  description = "For `aws ssm start-session --target <id>`."
  value       = try(aws_instance.vm[0].id, null)
}

output "load_balancer_dns" {
  description = "Container: point `domain` (CNAME) here."
  value       = try(aws_lb.this[0].dns_name, null)
}

output "kubernetes" {
  description = "EKS connection for the helm provider (compute = kubernetes); add a token from aws_eks_cluster_auth."
  value = local.eks == 1 ? {
    name                   = aws_eks_cluster.this[0].name
    host                   = aws_eks_cluster.this[0].endpoint
    cluster_ca_certificate = base64decode(aws_eks_cluster.this[0].certificate_authority[0].data)
  } : null
}
