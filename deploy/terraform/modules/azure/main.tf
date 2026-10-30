# EKOS on Azure (RFC 0232): identity, Key Vault, backups in Blob Storage, and one of three
# computes (vm.tf, container.tf, kubernetes.tf).

data "azurerm_client_config" "current" {}

resource "random_string" "suffix" {
  length  = 6
  special = false
  upper   = false
}

locals {
  rg_name = var.resource_group_name != "" ? var.resource_group_name : "${var.name}-rg"
  suffix  = random_string.suffix.result
  tags    = merge({ app = "ekos", "managed-by" = "terraform" }, var.tags)

  public_url = {
    vm         = "https://${local.vm_host}"
    container  = "https://${var.name}.${try(azurerm_container_app_environment.this[0].default_domain, "")}"
    kubernetes = var.domain != "" ? "https://${var.domain}" : "http://127.0.0.1:8000"
  }[var.compute]

  backup_url = var.backup_cron == "" ? "" : "https://${azurerm_storage_account.backups.name}.blob.core.windows.net/${azurerm_storage_container.backups.name}/ekos/"

  # Key Vault names allow letters, digits and dashes.
  console_kv_names  = { for n in module.common.console_secret_names : n => "${var.name}-${lower(replace(n, "_", "-"))}" }
  git_sync_kv_names = { for n in module.common.git_sync_secret_names : n => "${var.name}-${lower(replace(n, "_", "-"))}" }
}

resource "azurerm_resource_group" "this" {
  name     = local.rg_name
  location = var.location
  tags     = local.tags
}

module "common" {
  source = "../common"

  public_url         = local.public_url
  oidc               = var.oidc
  oidc_client_secret = var.oidc_client_secret
  owners             = var.owners
  # A network share (Container Apps) can briefly have two revisions; Kubernetes uses Recreate
  # but a second pod can still be started by hand. A VM's local disk has one console.
  instance_lease = var.compute != "vm"
  backup_url     = local.backup_url
  backup_cron    = var.backup_cron
  # object_store's managed-identity client picks the user-assigned identity by client id.
  env = merge(
    { AZURE_CLIENT_ID = azurerm_user_assigned_identity.ekos.client_id },
    var.env,
  )
  secrets           = var.secrets
  git_repos         = var.git_repos
  git_token         = var.git_token
  git_username      = var.git_username
  git_sync_interval = var.git_sync_interval
}

# ── identity ─────────────────────────────────────────────────────────────────
# One user-assigned identity for the console (and its git sync): reads its own secrets, writes
# backups. Nothing else.

resource "azurerm_user_assigned_identity" "ekos" {
  name                = "${var.name}-identity"
  location            = azurerm_resource_group.this.location
  resource_group_name = azurerm_resource_group.this.name
  tags                = local.tags
}

# ── Key Vault ────────────────────────────────────────────────────────────────

resource "azurerm_key_vault" "this" {
  name                       = substr("${var.name}-kv-${local.suffix}", 0, 24)
  location                   = azurerm_resource_group.this.location
  resource_group_name        = azurerm_resource_group.this.name
  tenant_id                  = data.azurerm_client_config.current.tenant_id
  sku_name                   = "standard"
  rbac_authorization_enabled = true
  soft_delete_retention_days = 7
  purge_protection_enabled   = false
  tags                       = local.tags
}

# Whoever runs Terraform writes the secrets; the identity only reads them.
resource "azurerm_role_assignment" "deployer_secrets" {
  scope                = azurerm_key_vault.this.id
  role_definition_name = "Key Vault Secrets Officer"
  principal_id         = data.azurerm_client_config.current.object_id
}

resource "azurerm_role_assignment" "ekos_secrets" {
  scope                = azurerm_key_vault.this.id
  role_definition_name = "Key Vault Secrets User"
  principal_id         = azurerm_user_assigned_identity.ekos.principal_id
  principal_type       = "ServicePrincipal"
}

# Role assignments take a while to reach Key Vault's data plane.
resource "time_sleep" "rbac" {
  depends_on      = [azurerm_role_assignment.deployer_secrets, azurerm_role_assignment.ekos_secrets]
  create_duration = "60s"
}

resource "azurerm_key_vault_secret" "console" {
  for_each     = local.console_kv_names
  name         = each.value
  value        = module.common.console_secrets[each.key]
  key_vault_id = azurerm_key_vault.this.id
  content_type = "env:${each.key}"
  depends_on   = [time_sleep.rbac]
}

resource "azurerm_key_vault_secret" "git_sync" {
  for_each     = local.git_sync_kv_names
  name         = each.value
  value        = module.common.git_sync_secrets[each.key]
  key_vault_id = azurerm_key_vault.this.id
  content_type = "env:${each.key}"
  depends_on   = [time_sleep.rbac]
}

# ── backups ──────────────────────────────────────────────────────────────────

resource "azurerm_storage_account" "backups" {
  name                            = substr("${replace(var.name, "-", "")}bk${local.suffix}", 0, 24)
  location                        = azurerm_resource_group.this.location
  resource_group_name             = azurerm_resource_group.this.name
  account_tier                    = "Standard"
  account_replication_type        = "GRS"
  account_kind                    = "StorageV2"
  min_tls_version                 = "TLS1_2"
  allow_nested_items_to_be_public = false
  # Identity only: no account keys, no SAS.
  shared_access_key_enabled       = false
  default_to_oauth_authentication = true
  tags                            = local.tags
}

resource "azurerm_storage_container" "backups" {
  name                  = "backups"
  storage_account_id    = azurerm_storage_account.backups.id
  container_access_type = "private"
}

resource "azurerm_storage_management_policy" "backups" {
  storage_account_id = azurerm_storage_account.backups.id
  rule {
    name    = "expire-ekos-backups"
    enabled = true
    filters {
      blob_types   = ["blockBlob"]
      prefix_match = ["${azurerm_storage_container.backups.name}/ekos/"]
    }
    actions {
      base_blob {
        delete_after_days_since_creation_greater_than = var.backup_retention_days
      }
    }
  }
}

resource "azurerm_role_assignment" "ekos_backups" {
  scope                = azurerm_storage_container.backups.id
  role_definition_name = "Storage Blob Data Contributor"
  principal_id         = azurerm_user_assigned_identity.ekos.principal_id
  principal_type       = "ServicePrincipal"
}

# ── network ──────────────────────────────────────────────────────────────────

resource "azurerm_virtual_network" "this" {
  name                = "${var.name}-vnet"
  location            = azurerm_resource_group.this.location
  resource_group_name = azurerm_resource_group.this.name
  address_space       = [var.address_space]
  tags                = local.tags
}

resource "azurerm_subnet" "main" {
  name                 = "${var.name}-main"
  resource_group_name  = azurerm_resource_group.this.name
  virtual_network_name = azurerm_virtual_network.this.name
  address_prefixes     = [cidrsubnet(var.address_space, 4, 0)]

  # Container Apps' environment needs its own delegated subnet; Azure Files NFS is reached
  # through the storage service endpoint.
  dynamic "delegation" {
    for_each = var.compute == "container" ? [1] : []
    content {
      name = "container-apps"
      service_delegation {
        name    = "Microsoft.App/environments"
        actions = ["Microsoft.Network/virtualNetworks/subnets/join/action"]
      }
    }
  }

  service_endpoint {
    service = "Microsoft.Storage"
  }
}
