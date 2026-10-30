variable "public_url" {
  description = "The console's address as people reach it (https://…); OIDC sends the browser back here."
  type        = string
}

variable "oidc" {
  description = "OIDC sign-in. An empty issuer leaves the console on generated static tokens (a trial, not production)."
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
  description = "Platform owners (e-mail addresses or OIDC subjects, comma-separated). With an OIDC issuer this turns production mode on."
  type        = string
  default     = ""
}

variable "instance_lease" {
  description = "RFC 0232 single-writer lease: on for a home on a network share."
  type        = bool
}

variable "backup_url" {
  type    = string
  default = ""
}

variable "backup_cron" {
  type    = string
  default = "17 2 * * *"
}

variable "env" {
  description = "More plain settings for the console."
  type        = map(string)
  default     = {}
}

variable "secrets" {
  description = "Secret settings for the console (model API keys, database passwords): variable name → value."
  type        = map(string)
  default     = {}
  sensitive   = true
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

variable "git_username" {
  type    = string
  default = "x-access-token"
}

variable "git_sync_interval" {
  type    = number
  default = 300
}

variable "git_sync_depth" {
  type    = number
  default = 0
}
