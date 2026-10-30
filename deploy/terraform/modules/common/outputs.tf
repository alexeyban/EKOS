output "console_env" {
  value = local.console_env
}

output "console_secrets" {
  value     = local.console_secrets
  sensitive = true
}

output "console_secret_names" {
  value = local.console_secret_names
}

output "git_sync_enabled" {
  value = local.git_sync_enabled
}

output "git_sync_env" {
  value = local.git_sync_env
}

output "git_sync_secrets" {
  value     = local.git_sync_secrets
  sensitive = true
}

output "git_sync_secret_names" {
  value = local.git_sync_secret_names
}

output "production" {
  value = local.production
}

output "static_tokens" {
  description = "Present only without OIDC."
  value = var.oidc.issuer == "" ? {
    read  = random_password.read_token[0].result
    write = random_password.write_token[0].result
  } : null
  sensitive = true
}
