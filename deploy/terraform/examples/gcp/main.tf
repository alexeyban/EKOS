# EKOS on Google Cloud — a complete root module (RFC 0232).
#
#   cp terraform.tfvars.example terraform.tfvars   # edit
#   gcloud auth application-default login
#   terraform init && terraform apply
#
# `compute` picks vm, container or kubernetes; everything else stays the same.

terraform {
  required_version = ">= 1.9"
  required_providers {
    google = {
      source  = "hashicorp/google"
      version = "~> 8.9"
    }
    helm = {
      source  = "hashicorp/helm"
      version = "~> 3.0"
    }
  }
}

provider "google" {
  project = var.project_id
  region  = var.region
}

data "google_client_config" "current" {}

# Used only with compute = "kubernetes"; unconfigured otherwise.
provider "helm" {
  kubernetes = {
    host                   = try(module.ekos.kubernetes.host, null)
    cluster_ca_certificate = try(module.ekos.kubernetes.cluster_ca_certificate, null)
    token                  = data.google_client_config.current.access_token
  }
}

module "ekos" {
  source = "../../modules/gcp"

  name               = var.name
  project_id         = var.project_id
  region             = var.region
  compute            = var.compute
  image              = var.image
  domain             = var.domain
  admin_cidrs        = var.admin_cidrs
  oidc               = var.oidc
  oidc_client_secret = var.oidc_client_secret
  owners             = var.owners
  secrets            = var.secrets
  git_repos          = var.git_repos
  git_token          = var.git_token
}

variable "project_id" { type = string }
variable "region" {
  type    = string
  default = "europe-west3"
}
variable "name" {
  type    = string
  default = "ekos"
}
variable "compute" {
  type    = string
  default = "vm"
}
variable "image" {
  type    = string
  default = "ghcr.io/alexeyban/ekos:1.1.0"
}
variable "domain" {
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
output "service_account" { value = module.ekos.service_account }
output "static_tokens" {
  value     = module.ekos.static_tokens
  sensitive = true
}
