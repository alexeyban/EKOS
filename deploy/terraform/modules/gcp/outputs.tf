output "url" {
  description = "Where the console is. Kubernetes without a domain: through `kubectl port-forward`."
  value       = local.public_url
}

output "compute" {
  value = var.compute
}

output "service_account" {
  value = google_service_account.ekos.email
}

output "secrets" {
  description = "Secret Manager secret ids, by the variable each becomes."
  value       = local.all_secret_ids
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
  value = try(google_compute_address.vm[0].address, null)
}

output "kubernetes" {
  description = "GKE connection for the helm provider (compute = kubernetes); add a token from google_client_config."
  value = local.gke == 1 ? {
    host                   = "https://${google_container_cluster.this[0].endpoint}"
    cluster_ca_certificate = base64decode(google_container_cluster.this[0].master_auth[0].cluster_ca_certificate)
  } : null
  sensitive = true
}

output "ingress_ip" {
  description = "Kubernetes with a domain (public): the load balancer's address. Point the domain's A record here; the managed certificate is issued once it resolves."
  value       = try(google_compute_global_address.ingress[0].address, null)
}
