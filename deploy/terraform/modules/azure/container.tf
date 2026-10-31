# compute = "container": Azure Container Apps (RFC 0232). One app, one replica, two containers:
# the console and the git sync. The EKOS home and the sources are Azure Files NFS shares (SQLite
# needs POSIX locks; SMB does not give them reliably), reached through the environment's
# subnet. Secrets are Key Vault references resolved by the app's identity. Revisions can overlap
# for a moment; the console's instance lease keeps one writer.

locals {
  ca = var.compute == "container" ? 1 : 0
}

resource "azurerm_log_analytics_workspace" "ca" {
  count               = local.ca
  name                = "${var.name}-logs"
  location            = azurerm_resource_group.this.location
  resource_group_name = azurerm_resource_group.this.name
  sku                 = "PerGB2018"
  retention_in_days   = 30
  tags                = local.tags
}

resource "azurerm_container_app_environment" "this" {
  count               = local.ca
  name                = "${var.name}-env"
  location            = azurerm_resource_group.this.location
  resource_group_name = azurerm_resource_group.this.name
  # azurerm 4.x refuses a workspace id without the destination stated (seen on the first apply).
  logs_destination           = "log-analytics"
  log_analytics_workspace_id = azurerm_log_analytics_workspace.ca[0].id
  infrastructure_subnet_id   = azurerm_subnet.main.id
  # Private: the environment's load balancer has an address in the subnet only.
  internal_load_balancer_enabled = var.exposure == "private"
  tags                           = local.tags

  workload_profile {
    name                  = "Consumption"
    workload_profile_type = "Consumption"
  }
}

# An internal environment's default domain resolves only through a private DNS zone: every app
# name under it points at the environment's internal address, for whatever is in the VNet (or
# peered with it, or resolving through it).
resource "azurerm_private_dns_zone" "ca" {
  count               = local.ca == 1 && var.exposure == "private" ? 1 : 0
  name                = azurerm_container_app_environment.this[0].default_domain
  resource_group_name = azurerm_resource_group.this.name
  tags                = local.tags
}

resource "azurerm_private_dns_zone_virtual_network_link" "ca" {
  count                = length(azurerm_private_dns_zone.ca)
  name                 = "${var.name}-ca"
  private_dns_zone_id  = azurerm_private_dns_zone.ca[0].id
  virtual_network_id   = azurerm_virtual_network.this.id
  registration_enabled = false
  tags                 = local.tags
}

resource "azurerm_private_dns_a_record" "ca" {
  count               = length(azurerm_private_dns_zone.ca)
  name                = "*"
  private_dns_zone_id = azurerm_private_dns_zone.ca[0].id
  ttl                 = 300
  records             = [azurerm_container_app_environment.this[0].static_ip_address]
}

# Premium FileStorage is the only account kind with NFS shares. NFS has no encryption in
# transit, so HTTPS-only is off and the account is closed to everything but the subnet.
resource "azurerm_storage_account" "files" {
  count                      = local.ca
  name                       = substr("${replace(var.name, "-", "")}fs${local.suffix}", 0, 24)
  location                   = azurerm_resource_group.this.location
  resource_group_name        = azurerm_resource_group.this.name
  account_tier               = "Premium"
  account_kind               = "FileStorage"
  account_replication_type   = "LRS"
  https_traffic_only_enabled = false
  min_tls_version            = "TLS1_2"
  tags                       = local.tags

  network_rules {
    default_action             = "Deny"
    virtual_network_subnet_ids = [azurerm_subnet.main.id]
    bypass                     = ["AzureServices"]
  }
}

resource "azurerm_storage_share" "home" {
  count              = local.ca
  name               = "home"
  storage_account_id = azurerm_storage_account.files[0].id
  enabled_protocol   = "NFS"
  # Premium shares are 100 GiB at least.
  quota = max(100, var.home_size_gb)
}

resource "azurerm_storage_share" "sources" {
  count              = local.ca
  name               = "sources"
  storage_account_id = azurerm_storage_account.files[0].id
  enabled_protocol   = "NFS"
  quota              = 100
}

locals {
  nfs_server = local.ca == 1 ? "${azurerm_storage_account.files[0].name}.file.core.windows.net" : ""
  # One environment storage per (share, mode): the console mounts the sources read-only, the git
  # sync read-write.
  ca_storages = local.ca == 1 ? {
    home       = { share = "/${azurerm_storage_account.files[0].name}/home", mode = "ReadWrite" }
    sources-rw = { share = "/${azurerm_storage_account.files[0].name}/sources", mode = "ReadWrite" }
    sources-ro = { share = "/${azurerm_storage_account.files[0].name}/sources", mode = "ReadOnly" }
  } : {}
  ca_secret_name = { for n in concat(module.common.console_secret_names, module.common.git_sync_secret_names) : n => lower(replace(n, "_", "-")) }
}

resource "azurerm_container_app_environment_storage" "this" {
  for_each                     = local.ca_storages
  name                         = each.key
  container_app_environment_id = azurerm_container_app_environment.this[0].id
  share_name                   = each.value.share
  access_mode                  = each.value.mode
  nfs_server_url               = local.nfs_server
  depends_on                   = [azurerm_storage_share.home, azurerm_storage_share.sources]
}

resource "azurerm_container_app" "ekos" {
  count                        = local.ca
  name                         = var.name
  container_app_environment_id = azurerm_container_app_environment.this[0].id
  resource_group_name          = azurerm_resource_group.this.name
  revision_mode                = "Single"
  workload_profile_name        = "Consumption"
  tags                         = local.tags

  identity {
    type         = "UserAssigned"
    identity_ids = [azurerm_user_assigned_identity.ekos.id]
  }

  dynamic "registry" {
    for_each = local.acr_server == "" ? [] : [local.acr_server]
    content {
      server   = registry.value
      identity = azurerm_user_assigned_identity.ekos.id
    }
  }

  # Iterated over the names, not the secret resources: those carry the (sensitive) values.
  dynamic "secret" {
    for_each = local.ca_secret_name
    content {
      name                = secret.value
      key_vault_secret_id = contains(keys(local.console_kv_names), secret.key) ? azurerm_key_vault_secret.console[secret.key].versionless_id : azurerm_key_vault_secret.git_sync[secret.key].versionless_id
      identity            = azurerm_user_assigned_identity.ekos.id
    }
  }

  ingress {
    external_enabled = true
    target_port      = 8000
    transport        = "http"
    traffic_weight {
      latest_revision = true
      percentage      = 100
    }
  }

  template {
    min_replicas                     = 1
    max_replicas                     = 1
    termination_grace_period_seconds = 60

    # NFS shares start owned by root; the image runs as uid 1000.
    init_container {
      name    = "own-volumes"
      image   = "docker.io/library/busybox:1.37"
      command = ["sh", "-c", "chown 1000:1000 /mnt/home /mnt/sources"]
      cpu     = 0.25
      memory  = "0.5Gi"
      volume_mounts {
        name = "home"
        path = "/mnt/home"
      }
      volume_mounts {
        name = "sources-rw"
        path = "/mnt/sources"
      }
    }

    container {
      name   = "ekos"
      image  = var.image
      cpu    = var.container_cpu
      memory = var.container_memory

      dynamic "env" {
        for_each = module.common.console_env
        content {
          name  = env.key
          value = env.value
        }
      }
      dynamic "env" {
        for_each = toset(module.common.console_secret_names)
        content {
          name        = env.key
          secret_name = local.ca_secret_name[env.key]
        }
      }

      volume_mounts {
        name = "home"
        path = "/var/lib/ekos"
      }
      volume_mounts {
        name = "sources-ro"
        path = "/sources"
      }

      # Container Apps allows 10 failures at most (60 is refused at apply), so the start allowance
      # (10 min, room for a first-start restore) comes from the interval.
      startup_probe {
        transport               = "HTTP"
        port                    = 8000
        path                    = "/api/health"
        interval_seconds        = 60
        failure_count_threshold = 10
      }
      liveness_probe {
        transport        = "HTTP"
        port             = 8000
        path             = "/api/health"
        interval_seconds = 30
        timeout          = 5
      }
    }

    dynamic "container" {
      for_each = module.common.git_sync_enabled ? [1] : []
      content {
        name    = "git-sync"
        image   = var.image
        command = ["/usr/local/bin/ekos-entrypoint", "ekos-git-sync"]
        cpu     = 0.25
        memory  = "0.5Gi"

        dynamic "env" {
          for_each = merge(module.common.git_sync_env, { EKOS_GIT_SYNC_DEST = "/sources" })
          content {
            name  = env.key
            value = env.value
          }
        }
        dynamic "env" {
          for_each = toset(module.common.git_sync_secret_names)
          content {
            name        = env.key
            secret_name = local.ca_secret_name[env.key]
          }
        }
        volume_mounts {
          name = "sources-rw"
          path = "/sources"
        }
      }
    }

    dynamic "volume" {
      for_each = azurerm_container_app_environment_storage.this
      content {
        name         = volume.key
        storage_name = volume.value.name
        storage_type = "NfsAzureFile"
      }
    }
  }

  depends_on = [time_sleep.rbac, azurerm_role_assignment.ekos_backups, azurerm_role_assignment.ekos_acr_pull]
}
