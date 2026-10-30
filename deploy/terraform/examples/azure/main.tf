# EKOS on Azure — a complete root module (RFC 0232).
#
#   cp terraform.tfvars.example terraform.tfvars   # edit
#   az login
#   terraform init && terraform apply
#
# `compute` picks vm, container or kubernetes; everything else stays the same.

terraform {
  required_version = ">= 1.9"
  required_providers {
    azurerm = {
      source  = "hashicorp/azurerm"
      version = "~> 5.10"
    }
    helm = {
      source  = "hashicorp/helm"
      version = "~> 3.0"
    }
  }
}

provider "azurerm" {
  features {
    key_vault {
      purge_soft_delete_on_destroy = false
    }
  }
  subscription_id = var.subscription_id
  # The backups account has shared keys off; containers and policies are managed with Entra ID.
  storage_use_azuread = true
}

# Used only with compute = "kubernetes"; unconfigured otherwise.
provider "helm" {
  kubernetes = {
    host                   = try(module.ekos.kubernetes.host, null)
    client_certificate     = try(module.ekos.kubernetes.client_certificate, null)
    client_key             = try(module.ekos.kubernetes.client_key, null)
    cluster_ca_certificate = try(module.ekos.kubernetes.cluster_ca_certificate, null)
  }
}

module "ekos" {
  source = "../../modules/azure"

  name               = var.name
  location           = var.location
  compute            = var.compute
  image              = var.image
  domain             = var.domain
  ssh_public_key     = var.ssh_public_key
  admin_cidrs        = var.admin_cidrs
  oidc               = var.oidc
  oidc_client_secret = var.oidc_client_secret
  owners             = var.owners
  secrets            = var.secrets
  git_repos          = var.git_repos
  git_token          = var.git_token
}

variable "subscription_id" { type = string }
variable "name" {
  type    = string
  default = "ekos"
}
variable "location" {
  type    = string
  default = "westeurope"
}
variable "compute" {
  type    = string
  default = "vm"
}
variable "image" {
  type    = string
  default = "ghcr.io/alexeyban/ekos:latest"
}
variable "domain" {
  type    = string
  default = ""
}
variable "ssh_public_key" {
  type    = string
  default = ""
}
variable "admin_cidrs" {
  type    = list(string)
  default = []
}
variable "oidc" {
  type = object({
    issuer       = optional(string, "")
    client_id    = optional(string, "")
    scopes       = optional(string, "openid email profile")
    role_claim   = optional(string, "groups")
    write_values = optional(string, "")
  })
  default = {}
}
variable "oidc_client_secret" {
  type      = string
  default   = ""
  sensitive = true
}
variable "owners" {
  type    = string
  default = ""
}
variable "secrets" {
  type      = map(string)
  default   = {}
  sensitive = true
}
variable "git_repos" {
  type    = list(string)
  default = []
}
variable "git_token" {
  type      = string
  default   = ""
  sensitive = true
}

output "url" { value = module.ekos.url }
output "backup_url" { value = module.ekos.backup_url }
output "key_vault" { value = module.ekos.key_vault }
output "static_tokens" {
  value     = module.ekos.static_tokens
  sensitive = true
}
