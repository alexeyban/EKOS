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

variable "project_id" {
  type = string
}

variable "region" {
  description = "e.g. europe-west3."
  type        = string
}

variable "zone" {
  description = "Zone for the VM, its disk and Filestore. Default: the region's first zone that is up."
  type        = string
  default     = ""
}

variable "compute" {
  description = "Where EKOS runs: `vm` (one Compute Engine VM, Docker, a persistent disk), `container` (Cloud Run over Filestore NFS) or `kubernetes` (GKE Autopilot + the Helm chart)."
  type        = string
  default     = "vm"
  validation {
    condition     = contains(["vm", "container", "kubernetes"], var.compute)
    error_message = "compute must be vm, container or kubernetes."
  }
}

variable "exposure" {
  description = "`public` (default): the console is reachable from the internet. `private`: only from inside the network — Cloud Run's internal ingress; on GKE the internal Application Load Balancer (`gce-internal`), which needs ingress.tlsSecretName in helm_values. Not for `vm`: a VM without a public address needs your own way in."
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
  default     = "ghcr.io/alexeyban/ekos:latest"
}

variable "domain" {
  description = "Public DNS name for the console. VM: optional (default: <ip>.sslip.io; Caddy gets a certificate). Container: unused, the service's run.app address is used. Kubernetes: the ingress host, empty for port-forward only."
  type        = string
  default     = ""
}

variable "address_space" {
  type    = string
  default = "10.42.0.0/20"
}

variable "labels" {
  type    = map(string)
  default = {}
}

# ── the console ──────────────────────────────────────────────────────────────

variable "oidc" {
  description = "OIDC sign-in (Google: issuer https://accounts.google.com). Empty issuer: generated static tokens, for a trial."
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
  description = "Secret console settings (model API keys, database passwords): variable name → value. Stored in Secret Manager, injected from there."
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
  description = "Access token for the repositories (stored in Secret Manager)."
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
  description = "When the console backs the home up to Cloud Storage (crontab, UTC). Empty: no scheduled backups."
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

variable "machine_type" {
  type    = string
  default = "e2-standard-4"
}

variable "admin_cidrs" {
  description = "VM only: addresses allowed to SSH in. IAP TCP forwarding (35.235.240.0/20) is always allowed; use `gcloud compute ssh --tunnel-through-iap`."
  type        = list(string)
  default     = []
}

variable "allowed_cidrs" {
  description = "VM only: addresses allowed to reach the console over HTTPS."
  type        = list(string)
  default     = ["0.0.0.0/0"]
}

variable "container_cpu" {
  type    = string
  default = "2"
}

variable "container_memory" {
  type    = string
  default = "8Gi"
}

variable "filestore_tier" {
  description = "Container only. BASIC_HDD is the cheapest tier and is 1 TiB at least."
  type        = string
  default     = "BASIC_HDD"
}

variable "kubernetes_namespace" {
  type    = string
  default = "ekos"
}

variable "proxy_only_cidr" {
  description = "Kubernetes with exposure = private: the proxy-only subnet the internal Application Load Balancer needs (outside address_space)."
  type        = string
  default     = "10.42.16.0/23"
}

variable "helm_values" {
  description = "Kubernetes only: extra values files (YAML strings) for the chart, applied over the module's. A domain gets a Google-managed certificate on a reserved address by default; with exposure = private give ingress.tlsSecretName here."
  type        = list(string)
  default     = []
}
