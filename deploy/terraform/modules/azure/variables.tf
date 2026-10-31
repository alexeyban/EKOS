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

variable "location" {
  description = "Azure region, e.g. westeurope."
  type        = string
}

variable "resource_group_name" {
  description = "Resource group to create. Default: <name>-rg."
  type        = string
  default     = ""
}

variable "compute" {
  description = "Where EKOS runs: `vm` (one VM, Docker, a managed data disk), `container` (Azure Container Apps over Azure Files NFS) or `kubernetes` (AKS + the Helm chart)."
  type        = string
  default     = "vm"
  validation {
    condition     = contains(["vm", "container", "kubernetes"], var.compute)
    error_message = "compute must be vm, container or kubernetes."
  }
}

variable "exposure" {
  description = "`public` (default): the console is reachable from the internet. `private`: only from inside the network — Container Apps' internal environment with a private DNS zone in the VNet; on AKS the module adds no ingress, so give an internal one in helm_values. Not for `vm`: a VM without a public address needs your own way in."
  type        = string
  default     = "public"
  validation {
    condition     = contains(["public", "private"], var.exposure)
    error_message = "exposure must be public or private."
  }
  validation {
    condition     = var.exposure == "public" || var.compute != "vm"
    error_message = "exposure = \"private\" is for compute = container or kubernetes; a VM without a public address needs your own way in (a VPN, a bastion)."
  }
}

variable "image" {
  description = "The EKOS image."
  type        = string
  default     = "ghcr.io/alexeyban/ekos:1.1.0"
}

variable "acr_id" {
  description = "Resource id of an Azure Container Registry holding `image` (a private registry). The deployment's identity (and AKS's kubelet) gets AcrPull on it; the VM signs in with that identity at each start, Container Apps through its registry identity. Empty: the image is pulled anonymously."
  type        = string
  default     = ""
}

variable "domain" {
  description = "Public DNS name for the console. VM: optional (default: the public IP's <label>.<region>.cloudapp.azure.com; Caddy gets a certificate). Container: informational only, the app's own HTTPS name is used. Kubernetes: the ingress host, empty for port-forward only."
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
  description = "OIDC sign-in (Entra ID: issuer https://login.microsoftonline.com/<tenant>/v2.0). Empty issuer: generated static tokens, for a trial."
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
  description = "Secret console settings (model API keys, database passwords): variable name → value. Stored in Key Vault, injected from there."
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
  description = "Access token for the repositories (stored in Key Vault)."
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
  description = "When the console backs the home up to Blob Storage (crontab, UTC). Empty: no scheduled backups."
  type        = string
  default     = "17 2 * * *"
}

variable "backup_retention_days" {
  type    = number
  default = 30
}

# ── sizes ────────────────────────────────────────────────────────────────────

variable "home_size_gb" {
  description = "The EKOS home (ledgers, console database) plus checked-out sources."
  type        = number
  default     = 128
}

variable "vm_size" {
  type    = string
  default = "Standard_D4s_v5"
}

variable "ssh_public_key" {
  description = "VM only: the admin user's SSH public key (required by Azure)."
  type        = string
  default     = ""
}

variable "admin_cidrs" {
  description = "VM only: addresses allowed to SSH in. Empty: no SSH from the internet."
  type        = list(string)
  default     = []
}

variable "allowed_cidrs" {
  description = "VM only: addresses allowed to reach the console over HTTPS."
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "container_cpu" {
  type    = number
  default = 2
}

variable "container_memory" {
  description = "Container Apps memory, e.g. 4Gi (Consumption: 2 GiB per vCPU)."
  type        = string
  default     = "4Gi"
}

variable "aks_node_size" {
  type    = string
  default = "Standard_D4s_v5"
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
