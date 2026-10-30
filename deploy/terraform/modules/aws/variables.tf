# ── where and how ─────────────────────────────────────────────────────────────

variable "name" {
  description = "Prefix of every resource name."
  type        = string
  default     = "ekos"
  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{1,14}$", var.name))
    error_message = "name: 2-15 lowercase letters, digits or dashes, starting with a letter."
  }
}

variable "region" {
  description = "e.g. eu-central-1. The provider's region must be the same."
  type        = string
}

variable "compute" {
  description = "Where EKOS runs: `vm` (one EC2 instance, Docker, an EBS volume), `container` (ECS Fargate behind an ALB, over EFS) or `kubernetes` (EKS Auto Mode + the Helm chart)."
  type        = string
  default     = "vm"
  validation {
    condition     = contains(["vm", "container", "kubernetes"], var.compute)
    error_message = "compute must be vm, container or kubernetes."
  }
}

variable "image" {
  description = "The EKOS image."
  type        = string
  default     = "ghcr.io/alexeyban/ekos:latest"
}

variable "domain" {
  description = "Public DNS name for the console. VM: optional (default: <ip>.sslip.io; Caddy gets a certificate). Container: required, with certificate_arn; point it at the `load_balancer_dns` output. Kubernetes: the ingress host, empty for port-forward only."
  type        = string
  default     = ""
}

variable "certificate_arn" {
  description = "Container only: ACM certificate for `domain`, on the load balancer."
  type        = string
  default     = ""
}

variable "address_space" {
  type    = string
  default = "10.42.0.0/16"
}

variable "tags" {
  type    = map(string)
  default = {}
}

# ── the console ──────────────────────────────────────────────────────────────

variable "oidc" {
  description = "OIDC sign-in (Cognito, Okta, Entra ID, Keycloak, …). Empty issuer: generated static tokens, for a trial."
  type = object({
    issuer       = optional(string, "")
    client_id    = optional(string, "")
    scopes       = optional(string, "openid email profile groups")
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
  description = "Platform owners, comma-separated. With an OIDC issuer this turns production mode on."
  type        = string
  default     = ""
}

variable "env" {
  description = "More plain console settings (EKOS_CONSOLE_*, OLLAMA_HOST, …)."
  type        = map(string)
  default     = {}
}

variable "secrets" {
  description = "Secret console settings (model API keys, database passwords): variable name → value. Stored in Secrets Manager, injected from there."
  type        = map(string)
  default     = {}
  sensitive   = true
}

# ── sources ──────────────────────────────────────────────────────────────────

variable "git_repos" {
  description = "Repositories the git sync keeps under /sources: `name=https-url[#branch]`."
  type        = list(string)
  default     = []
}

variable "git_token" {
  description = "Access token for the repositories (stored in Secrets Manager)."
  type        = string
  default     = ""
  sensitive   = true
}

variable "git_username" {
  type    = string
  default = "x-access-token"
}

variable "git_sync_interval" {
  type    = number
  default = 300
}

# ── backups ──────────────────────────────────────────────────────────────────

variable "backup_cron" {
  description = "When the console backs the home up to S3 (crontab, UTC). Empty: no scheduled backups."
  type        = string
  default     = "17 2 * * *"
}

variable "backup_retention_days" {
  type    = number
  default = 30
}

# ── sizes ────────────────────────────────────────────────────────────────────

variable "home_size_gb" {
  description = "The EKOS home (ledgers, console database) plus checked-out sources. EFS (container) grows on its own."
  type        = number
  default     = 128
}

variable "instance_type" {
  type    = string
  default = "m7i.xlarge"
}

variable "ssh_key_name" {
  description = "VM only: an EC2 key pair for SSH. Empty: none (use SSM Session Manager)."
  type        = string
  default     = ""
}

variable "admin_cidrs" {
  description = "VM only: addresses allowed to SSH in."
  type        = list(string)
  default     = []
}

variable "allowed_cidrs" {
  description = "VM only: addresses allowed to reach the console over HTTPS."
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "container_cpu" {
  description = "Fargate task CPU units (1024 = 1 vCPU)."
  type        = number
  default     = 2048
}

variable "container_memory" {
  description = "Fargate task memory in MiB."
  type        = number
  default     = 8192
}

variable "kubernetes_namespace" {
  type    = string
  default = "ekos"
}

variable "helm_values" {
  description = "Kubernetes only: extra values files (YAML strings) for the chart. With a domain, give the ingress class and TLS here."
  type        = list(string)
  default     = []
}
