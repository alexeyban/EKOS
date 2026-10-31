# Plans of the Azure module with mocked providers (RFC 0232): no account, no network. They check
# the module's own logic — which resources each compute creates, what reaches the console's
# environment, the private exposure, the preconditions — not what Azure would accept, which only
# an apply shows (docs/deployment.md §10).

mock_provider "azurerm" {
  override_during = plan
  mock_resource "azurerm_private_dns_zone" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.Network/privateDnsZones/happy-sea-1234.westeurope.azurecontainerapps.io" }
  }
  mock_resource "azurerm_log_analytics_workspace" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.OperationalInsights/workspaces/ekos-logs" }
  }
  mock_resource "azurerm_storage_container" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.Storage/storageAccounts/ekosbk/blobServices/default/containers/backups" }
  }
  mock_resource "azurerm_storage_account" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.Storage/storageAccounts/ekosbk" }
  }
  mock_resource "azurerm_linux_virtual_machine" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.Compute/virtualMachines/ekos-vm" }
  }
  mock_resource "azurerm_managed_disk" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.Compute/disks/ekos-home" }
  }
  mock_resource "azurerm_public_ip" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.Network/publicIPAddresses/ekos-vm" }
  }
  mock_resource "azurerm_network_interface" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.Network/networkInterfaces/ekos-vm" }
  }
  mock_resource "azurerm_network_security_group" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.Network/networkSecurityGroups/ekos-vm" }
  }
  mock_resource "azurerm_subnet" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.Network/virtualNetworks/ekos-vnet/subnets/ekos-main" }
  }
  mock_resource "azurerm_virtual_network" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.Network/virtualNetworks/ekos-vnet" }
  }
  mock_resource "azurerm_key_vault" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.KeyVault/vaults/ekos-kv" }
  }
  mock_resource "azurerm_resource_group" {
    defaults = { id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg" }
  }
  mock_data "azurerm_client_config" {
    defaults = {
      tenant_id = "00000000-0000-0000-0000-000000000001"
      object_id = "00000000-0000-0000-0000-000000000002"
    }
  }
  mock_resource "azurerm_user_assigned_identity" {
    defaults = {
      id           = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/ekos-identity"
      client_id    = "00000000-0000-0000-0000-0000000000c1"
      principal_id = "00000000-0000-0000-0000-0000000000p1"
    }
  }
  mock_resource "azurerm_container_app_environment" {
    defaults = {
      id                = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.App/managedEnvironments/ekos-env"
      default_domain    = "happy-sea-1234.westeurope.azurecontainerapps.io"
      static_ip_address = "10.42.0.250"
    }
  }
  mock_resource "azurerm_kubernetes_cluster" {
    defaults = {
      id              = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-rg/providers/Microsoft.ContainerService/managedClusters/ekos-aks"
      oidc_issuer_url = "https://westeurope.oic.prod-aks.azure.com/t/c/"
      # base64 of "mock"
      kube_config = [{
        host                   = "https://ekos-aks.hcp.westeurope.azmk8s.io:443"
        client_certificate     = "bW9jaw=="
        client_key             = "bW9jaw=="
        cluster_ca_certificate = "bW9jaw=="
      }]
    }
  }
}
mock_provider "helm" {
  override_during = plan
}
mock_provider "time" {
  override_during = plan
}
mock_provider "random" {
  override_during = plan
  mock_resource "random_string" {
    defaults = { result = "a1b2c3" }
  }
  mock_resource "random_password" {
    defaults = { result = "generated-for-the-plan-only" }
  }
}

variables {
  name           = "ekos"
  location       = "westeurope"
  ssh_public_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILMjyL+FJKwbVCQZEcPgal8cL26Cn1cIG2iqSJwDEepw ekos-plan-test"
}

run "vm_by_default" {
  command = plan

  assert {
    condition     = length(azurerm_linux_virtual_machine.vm) == 1 && length(azurerm_container_app.ekos) == 0 && length(azurerm_kubernetes_cluster.this) == 0
    error_message = "compute = vm creates the VM only."
  }
  assert {
    condition     = output.url == "https://${azurerm_public_ip.vm[0].fqdn}" || startswith(output.url, "https://")
    error_message = "the VM's address is HTTPS."
  }
  # One console on a local disk: no lease.
  assert {
    condition     = module.common.console_env["EKOS_CONSOLE_INSTANCE_LEASE"] == "false"
    error_message = "a VM does not need the instance lease."
  }
}

run "backups_reach_the_console_and_no_static_tokens_with_oidc" {
  command = plan
  variables {
    oidc   = { issuer = "https://login.microsoftonline.com/t/v2.0", client_id = "app" }
    owners = "alice@example.com"
  }

  assert {
    condition     = module.common.console_env["EKOS_CONSOLE_BACKUP_URL"] == output.backup_url && endswith(output.backup_url, "/backups/ekos/")
    error_message = "the console backs up to the module's container."
  }
  assert {
    condition     = module.common.console_env["EKOS_CONSOLE_PRODUCTION"] == "true"
    error_message = "OIDC and owners make a production console."
  }
  assert {
    condition     = !contains(module.common.console_secret_names, "EKOS_CONSOLE_CONSOLE_TOKEN")
    error_message = "with OIDC there are no static tokens."
  }
  assert {
    condition     = module.common.console_env["EKOS_CONSOLE_OIDC_REDIRECT_URI"] == "${output.url}/api/auth/callback"
    error_message = "the OIDC redirect is the console's own address."
  }
}

run "container_is_public_with_the_lease" {
  command = plan
  variables {
    compute = "container"
  }

  assert {
    condition     = length(azurerm_container_app.ekos) == 1 && length(azurerm_linux_virtual_machine.vm) == 0
    error_message = "compute = container creates the app only."
  }
  assert {
    condition     = azurerm_container_app_environment.this[0].internal_load_balancer_enabled == false && length(azurerm_private_dns_zone.ca) == 0
    error_message = "public by default."
  }
  assert {
    condition     = module.common.console_env["EKOS_CONSOLE_INSTANCE_LEASE"] == "true"
    error_message = "a network share needs the lease."
  }
  assert {
    condition     = output.url == "https://ekos.happy-sea-1234.westeurope.azurecontainerapps.io"
    error_message = "the app's own HTTPS name."
  }
  # Container Apps refuses more than 10 failures (the module once set 60).
  assert {
    condition     = one(azurerm_container_app.ekos[0].template[0].container[0].startup_probe).failure_count_threshold <= 10
    error_message = "a startup probe allows at most 10 failures."
  }
}

run "container_private_gets_an_internal_environment_and_its_dns_zone" {
  command = plan
  variables {
    compute  = "container"
    exposure = "private"
  }

  assert {
    condition     = azurerm_container_app_environment.this[0].internal_load_balancer_enabled == true
    error_message = "private makes the environment internal."
  }
  assert {
    condition     = azurerm_private_dns_zone.ca[0].name == "happy-sea-1234.westeurope.azurecontainerapps.io"
    error_message = "the zone is the environment's default domain."
  }
  assert {
    condition     = azurerm_private_dns_a_record.ca[0].name == "*" && contains(azurerm_private_dns_a_record.ca[0].records, "10.42.0.250")
    error_message = "every app name points at the environment's internal address."
  }
}

run "private_is_refused_on_a_vm" {
  command = plan
  variables {
    exposure = "private"
  }
  expect_failures = [var.exposure]
}

run "kubernetes_without_a_domain_is_port_forward_only" {
  command = plan
  variables {
    compute = "kubernetes"
  }

  assert {
    condition     = length(azurerm_kubernetes_cluster.this) == 1 && output.url == "http://127.0.0.1:8000"
    error_message = "no domain: port-forward."
  }
  assert {
    condition     = yamldecode(helm_release.ekos[0].values[0]).ingress.enabled == false
    error_message = "no ingress without a domain."
  }
  # The chart sets these itself.
  assert {
    condition     = !contains(keys(yamldecode(helm_release.ekos[0].values[0]).env), "EKOS_CONSOLE_INSTANCE_LEASE")
    error_message = "the chart owns the lease setting."
  }
}

# AKS has no ingress preset (app routing's NGINX is retired in November 2026), so a domain still
# needs the operator's ingress.
run "kubernetes_with_a_domain_needs_helm_values" {
  command = plan
  variables {
    compute = "kubernetes"
    domain  = "ekos.example.com"
  }
  expect_failures = [helm_release.ekos]
}

run "a_private_registry_is_pulled_with_the_deployments_identity" {
  command = plan
  variables {
    compute = "container"
    image   = "ekosdemo.azurecr.io/ekos:abc123"
    acr_id  = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-shared/providers/Microsoft.ContainerRegistry/registries/EkosDemo"
  }

  assert {
    condition     = azurerm_role_assignment.ekos_acr_pull[0].role_definition_name == "AcrPull" && azurerm_role_assignment.ekos_acr_pull[0].principal_id == azurerm_user_assigned_identity.ekos.principal_id
    error_message = "the identity may pull from the registry."
  }
  assert {
    condition     = one(azurerm_container_app.ekos[0].registry).server == "ekosdemo.azurecr.io"
    error_message = "Container Apps pulls through its registry identity (login server lower-cased)."
  }
}

run "a_vm_signs_in_to_the_private_registry_at_each_start" {
  command = plan
  variables {
    acr_id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/ekos-shared/providers/Microsoft.ContainerRegistry/registries/ekosdemo"
  }

  assert {
    condition     = strcontains(base64decode(azurerm_linux_virtual_machine.vm[0].custom_data), base64encode(templatefile("../../templates/fetch-secrets.sh.tftpl", { cloud = "azure", secrets = local.vm_secrets, azure_client_id = azurerm_user_assigned_identity.ekos.client_id, azure_vault_uri = azurerm_key_vault.this.vault_uri, azure_registry = "ekosdemo.azurecr.io", aws_region = "" })))
    error_message = "the boot script is the one rendered for this registry."
  }
  assert {
    condition     = strcontains(templatefile("../../templates/fetch-secrets.sh.tftpl", { cloud = "azure", secrets = local.vm_secrets, azure_client_id = "c", azure_vault_uri = "https://kv/", azure_registry = local.acr_server, aws_region = "" }), "docker login ekosdemo.azurecr.io")
    error_message = "the boot script signs in to the registry."
  }
  assert {
    condition     = !strcontains(templatefile("../../templates/fetch-secrets.sh.tftpl", { cloud = "azure", secrets = local.vm_secrets, azure_client_id = "c", azure_vault_uri = "https://kv/", azure_registry = "", aws_region = "" }), "docker login")
    error_message = "no registry: no sign-in."
  }
}

run "no_registry_no_pull_role" {
  command = plan

  assert {
    condition     = length(azurerm_role_assignment.ekos_acr_pull) == 0
    error_message = "a public image needs no role."
  }
}
