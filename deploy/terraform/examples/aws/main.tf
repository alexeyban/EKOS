# EKOS on AWS — a complete root module (RFC 0232).
#
#   cp terraform.tfvars.example terraform.tfvars   # edit
#   aws sso login   # or any other credentials
#   terraform init && terraform apply
#
# `compute` picks vm, container or kubernetes; everything else stays the same.

terraform {
  required_version = ">= 1.9"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.70"
    }
    helm = {
      source  = "hashicorp/helm"
      version = "~> 3.0"
    }
  }
}

provider "aws" {
  region = var.region
}

# Used only with compute = "kubernetes"; unconfigured otherwise. The token comes from the AWS CLI
# at the moment it is needed (the cluster does not exist yet when the plan is made).
provider "helm" {
  kubernetes = {
    host                   = try(module.ekos.kubernetes.host, null)
    cluster_ca_certificate = try(module.ekos.kubernetes.cluster_ca_certificate, null)
    exec = {
      api_version = "client.authentication.k8s.io/v1beta1"
      command     = "aws"
      args        = ["eks", "get-token", "--cluster-name", var.name, "--region", var.region]
    }
  }
}

module "ekos" {
  source = "../../modules/aws"

  name               = var.name
  region             = var.region
  compute            = var.compute
  image              = var.image
  domain             = var.domain
  certificate_arn    = var.certificate_arn
  admin_cidrs        = var.admin_cidrs
  oidc               = var.oidc
  oidc_client_secret = var.oidc_client_secret
  owners             = var.owners
  secrets            = var.secrets
  git_repos          = var.git_repos
  git_token          = var.git_token
}

variable "region" {
  type    = string
  default = "eu-central-1"
}
variable "certificate_arn" {
  type    = string
  default = ""
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
  default = "ghcr.io/alexeyban/ekos:latest"
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
output "load_balancer_dns" { value = module.ekos.load_balancer_dns }
output "vm_instance_id" { value = module.ekos.vm_instance_id }
output "static_tokens" {
  value     = module.ekos.static_tokens
  sensitive = true
}
