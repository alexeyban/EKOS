# compute = "kubernetes": AKS + the EKOS Helm chart (RFC 0232). One pod with the console and the
# git sync over one ReadWriteOnce managed disk; secrets come from Key Vault through the AKS
# Secrets Store CSI add-on, authenticated by workload identity (no keys in the cluster).
#
# The helm provider is configured by the caller from this module's `kubernetes` output (see
# examples/azure); this module only declares the release.

locals {
  aks = var.compute == "kubernetes" ? 1 : 0
  # registry/repository[:tag] — a registry port is not a tag.
  image_parts      = regex("^(.*?)(?::([^:/]+))?$", var.image)
  k8s_service_acct = "ekos"
}

resource "azurerm_kubernetes_cluster" "this" {
  count                     = local.aks
  name                      = "${var.name}-aks"
  location                  = azurerm_resource_group.this.location
  resource_group_name       = azurerm_resource_group.this.name
  dns_prefix                = "${var.name}-${local.suffix}"
  oidc_issuer_enabled       = true
  workload_identity_enabled = true
  tags                      = local.tags

  default_node_pool {
    name           = "system"
    vm_size        = var.aks_node_size
    node_count     = 1
    vnet_subnet_id = azurerm_subnet.main.id
  }

  node_provisioning_profile {
    mode = "Manual"
  }

  identity {
    type = "SystemAssigned"
  }

  network_profile {
    network_plugin      = "azure"
    network_plugin_mode = "overlay"
  }

  key_vault_secrets_provider {
    secret_rotation_enabled = true
  }
}

resource "azurerm_federated_identity_credential" "ekos" {
  count                     = local.aks
  name                      = "${var.name}-aks"
  user_assigned_identity_id = azurerm_user_assigned_identity.ekos.id
  audience                  = ["api://AzureADTokenExchange"]
  issuer                    = azurerm_kubernetes_cluster.this[0].oidc_issuer_url
  subject                   = "system:serviceaccount:${var.kubernetes_namespace}:${local.k8s_service_acct}"
}

locals {
  # The Azure provider's SecretProviderClass parameters: each Key Vault secret is mounted as a
  # file named after the variable it becomes.
  aks_csi = {
    for group, names in { console = local.console_kv_names, gitSync = local.git_sync_kv_names } : group => {
      enabled                     = length(names) > 0
      provider                    = "azure"
      parameters = {
        usePodIdentity = "false"
        clientID       = azurerm_user_assigned_identity.ekos.client_id
        keyvaultName   = azurerm_key_vault.this.name
        tenantId       = data.azurerm_client_config.current.tenant_id
        objects = yamlencode({
          array = [for env_name, kv_name in names : yamlencode({
            objectName  = kv_name
            objectType  = "secret"
            objectAlias = env_name
          })]
        })
      }
    }
  }
}

resource "helm_release" "ekos" {
  count            = local.aks
  name             = "ekos"
  chart            = "${path.module}/../../../helm/ekos"
  namespace        = var.kubernetes_namespace
  create_namespace = true
  timeout          = 900

  values = concat([yamlencode({
    image = {
      repository = local.image_parts[0]
      tag        = coalesce(local.image_parts[1], "latest")
    }
    # The chart sets the lease and supervision itself.
    env = { for k, v in module.common.console_env : k => v if !contains(["EKOS_CONSOLE_INSTANCE_LEASE", "EKOS_CONSOLE_SUPERVISED"], k) }
    csi = local.aks_csi
    serviceAccount = {
      create      = true
      name        = local.k8s_service_acct
      annotations = { "azure.workload.identity/client-id" = azurerm_user_assigned_identity.ekos.client_id }
    }
    podLabels = { "azure.workload.identity/use" = "true" }
    persistence = {
      storageClass = "managed-csi-premium"
      size         = "${var.home_size_gb}Gi"
    }
    gitSync = {
      enabled         = module.common.git_sync_enabled
      repos           = var.git_repos
      intervalSeconds = var.git_sync_interval
      username        = var.git_username
    }
    ingress = {
      enabled = var.domain != ""
      host    = var.domain
    }
  })], var.helm_values)

  lifecycle {
    # A domain makes the console's address (and the OIDC redirect) https://<domain>; what serves
    # it — an ingress class, a certificate — differs per cluster and is the operator's to give.
    precondition {
      condition     = var.domain == "" || length(var.helm_values) > 0
      error_message = "compute = \"kubernetes\" with a domain needs helm_values with the ingress class and TLS (ingress.className, ingress.tlsSecretName or the class's certificate annotations); see docs/deployment.md."
    }
  }

  depends_on = [
    azurerm_federated_identity_credential.ekos,
    azurerm_key_vault_secret.console,
    azurerm_key_vault_secret.git_sync,
    azurerm_role_assignment.ekos_backups,
  ]
}
