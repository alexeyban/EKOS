output "url" {
  description = "Where the console is. Kubernetes without a domain: through `kubectl port-forward`."
  value       = local.public_url
}

output "compute" {
  value = var.compute
}

output "resource_group" {
  value = azurerm_resource_group.this.name
}

output "key_vault" {
  value = { name = azurerm_key_vault.this.name, uri = azurerm_key_vault.this.vault_uri }
}

output "identity" {
  value = { client_id = azurerm_user_assigned_identity.ekos.client_id, principal_id = azurerm_user_assigned_identity.ekos.principal_id }
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
  value = try(azurerm_public_ip.vm[0].ip_address, null)
}

output "kubernetes" {
  description = "AKS connection for the helm provider (compute = kubernetes)."
  value = local.aks == 1 ? {
    host                   = azurerm_kubernetes_cluster.this[0].kube_config[0].host
    client_certificate     = base64decode(azurerm_kubernetes_cluster.this[0].kube_config[0].client_certificate)
    client_key             = base64decode(azurerm_kubernetes_cluster.this[0].kube_config[0].client_key)
    cluster_ca_certificate = base64decode(azurerm_kubernetes_cluster.this[0].kube_config[0].cluster_ca_certificate)
  } : null
  sensitive = true
}
