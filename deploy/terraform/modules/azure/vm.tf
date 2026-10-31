# compute = "vm": one Ubuntu VM running the image with Docker Compose (RFC 0232). The EKOS home
# and the checked-out sources live on a managed data disk that outlives the VM; secrets are read
# from Key Vault with the VM's identity at every start; Caddy serves HTTPS.

locals {
  vm      = var.compute == "vm" ? 1 : 0
  vm_host = var.domain != "" ? var.domain : try(azurerm_public_ip.vm[0].fqdn, "")

  vm_secrets = concat(
    [for n, kv in local.console_kv_names : { group = "console", name = n, ref = kv }],
    [for n, kv in local.git_sync_kv_names : { group = "git-sync", name = n, ref = kv }],
  )
}

resource "azurerm_public_ip" "vm" {
  count               = local.vm
  name                = "${var.name}-ip"
  location            = azurerm_resource_group.this.location
  resource_group_name = azurerm_resource_group.this.name
  allocation_method   = "Static"
  sku                 = "Standard"
  domain_name_label   = "${var.name}-${local.suffix}"
  tags                = local.tags
}

resource "azurerm_network_security_group" "vm" {
  count               = local.vm
  name                = "${var.name}-nsg"
  location            = azurerm_resource_group.this.location
  resource_group_name = azurerm_resource_group.this.name
  tags                = local.tags

  security_rule {
    name                       = "https"
    priority                   = 100
    direction                  = "Inbound"
    access                     = "Allow"
    protocol                   = "Tcp"
    source_port_range          = "*"
    destination_port_ranges    = ["80", "443"]
    source_address_prefixes    = var.allowed_cidrs
    destination_address_prefix = "*"
  }

  dynamic "security_rule" {
    for_each = length(var.admin_cidrs) > 0 ? [1] : []
    content {
      name                       = "ssh"
      priority                   = 110
      direction                  = "Inbound"
      access                     = "Allow"
      protocol                   = "Tcp"
      source_port_range          = "*"
      destination_port_range     = "22"
      source_address_prefixes    = var.admin_cidrs
      destination_address_prefix = "*"
    }
  }
}

resource "azurerm_network_interface" "vm" {
  count               = local.vm
  name                = "${var.name}-nic"
  location            = azurerm_resource_group.this.location
  resource_group_name = azurerm_resource_group.this.name
  tags                = local.tags

  ip_configuration {
    name                          = "primary"
    subnet_id                     = azurerm_subnet.main.id
    private_ip_address_allocation = "Dynamic"
    public_ip_address_id          = azurerm_public_ip.vm[0].id
  }
}

resource "azurerm_network_interface_security_group_association" "vm" {
  count                     = local.vm
  network_interface_id      = azurerm_network_interface.vm[0].id
  network_security_group_id = azurerm_network_security_group.vm[0].id
}

resource "azurerm_managed_disk" "home" {
  count                = local.vm
  name                 = "${var.name}-home"
  location             = azurerm_resource_group.this.location
  resource_group_name  = azurerm_resource_group.this.name
  storage_account_type = "Premium_LRS"
  create_option        = "Empty"
  disk_size_gb         = var.home_size_gb
  tags                 = local.tags
}

resource "azurerm_linux_virtual_machine" "vm" {
  count                 = local.vm
  name                  = "${var.name}-vm"
  location              = azurerm_resource_group.this.location
  resource_group_name   = azurerm_resource_group.this.name
  size                  = var.vm_size
  admin_username        = "ekosadmin"
  network_interface_ids = [azurerm_network_interface.vm[0].id]
  tags                  = local.tags

  admin_ssh_key {
    username   = "ekosadmin"
    public_key = var.ssh_public_key
  }

  identity {
    type         = "UserAssigned"
    identity_ids = [azurerm_user_assigned_identity.ekos.id]
  }

  os_disk {
    caching              = "ReadWrite"
    storage_account_type = "Premium_LRS"
    disk_size_gb         = 64
  }

  source_image_reference {
    publisher = "Canonical"
    offer     = "ubuntu-24_04-lts"
    sku       = "server"
    version   = "latest"
  }

  custom_data = base64encode(templatefile("${path.module}/../../templates/cloud-init.yaml.tftpl", {
    compose   = file("${path.module}/../../templates/docker-compose.vm.yml")
    caddyfile = file("${path.module}/../../templates/Caddyfile")
    compose_env = join("\n", [
      "EKOS_IMAGE=${var.image}",
      "EKOS_DOMAIN=${local.vm_host}",
      "COMPOSE_PROFILES=${join(",", concat(["tls"], module.common.git_sync_enabled ? ["git-sync"] : []))}",
    ])
    console_env  = join("\n", [for k, v in module.common.console_env : "${k}=${v}"])
    git_sync_env = join("\n", [for k, v in module.common.git_sync_env : "${k}=${replace(v, "\n", ",")}"])
    prepare_disk = templatefile("${path.module}/../../templates/prepare-disk.sh.tftpl", {
      device = "/dev/disk/azure/scsi1/lun0"
    })
    fetch_secrets = templatefile("${path.module}/../../templates/fetch-secrets.sh.tftpl", {
      cloud           = "azure"
      secrets         = local.vm_secrets
      azure_client_id = azurerm_user_assigned_identity.ekos.client_id
      azure_vault_uri = azurerm_key_vault.this.vault_uri
      azure_registry  = local.acr_server
      aws_region      = ""
    })
  }))

  lifecycle {
    precondition {
      condition     = var.ssh_public_key != ""
      error_message = "compute = \"vm\" on Azure needs ssh_public_key."
    }
  }

  depends_on = [
    azurerm_key_vault_secret.console,
    azurerm_key_vault_secret.git_sync,
    azurerm_role_assignment.ekos_backups,
    azurerm_role_assignment.ekos_acr_pull,
  ]
}

resource "azurerm_virtual_machine_data_disk_attachment" "home" {
  count              = local.vm
  managed_disk_id    = azurerm_managed_disk.home[0].id
  virtual_machine_id = azurerm_linux_virtual_machine.vm[0].id
  lun                = 0
  caching            = "ReadOnly"
}
