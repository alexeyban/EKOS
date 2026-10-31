# What every cloud's EKOS deployment shares (RFC 0232): the console's settings, which of them are
# secrets, the generated secrets, and the git sync's configuration. No cloud resources here; the
# azure, gcp and aws modules put these values into their secret store and their compute.

terraform {
  required_version = ">= 1.9"
  required_providers {
    random = {
      source  = "hashicorp/random"
      version = "~> 3.6"
    }
  }
}

resource "random_password" "session_secret" {
  length  = 48
  special = false
}

# Static tokens are only for a console without OIDC (a trial). With OIDC, production mode turns
# them off and they are not created.
resource "random_password" "read_token" {
  count   = var.oidc.issuer == "" ? 1 : 0
  length  = 40
  special = false
}

resource "random_password" "write_token" {
  count   = var.oidc.issuer == "" ? 1 : 0
  length  = 40
  special = false
}

locals {
  production = var.oidc.issuer != "" && var.owners != ""

  console_env = merge(
    {
      EKOS_CONSOLE_PRODUCTION     = tostring(local.production)
      EKOS_CONSOLE_INSTANCE_LEASE = tostring(var.instance_lease)
      # The image runs uvicorn directly; a platform restarts the container, not `ekos console`.
      EKOS_CONSOLE_SUPERVISED = "false"
    },
    var.owners == "" ? {} : { EKOS_CONSOLE_OWNERS = var.owners },
    var.oidc.issuer == "" ? {} : {
      EKOS_CONSOLE_OIDC_ISSUER       = var.oidc.issuer
      EKOS_CONSOLE_OIDC_CLIENT_ID    = var.oidc.client_id
      EKOS_CONSOLE_OIDC_REDIRECT_URI = "${trimsuffix(var.public_url, "/")}/api/auth/callback"
      EKOS_CONSOLE_OIDC_SCOPES       = var.oidc.scopes
      EKOS_CONSOLE_OIDC_ROLE_CLAIM   = var.oidc.role_claim
      EKOS_CONSOLE_OIDC_WRITE_VALUES = var.oidc.write_values
    },
    var.backup_url == "" ? {} : {
      EKOS_CONSOLE_BACKUP_URL  = var.backup_url
      EKOS_CONSOLE_BACKUP_CRON = var.backup_cron
    },
    var.env,
  )

  console_secrets = merge(
    { EKOS_CONSOLE_SESSION_SECRET = random_password.session_secret.result },
    var.oidc.issuer == "" ? {
      EKOS_CONSOLE_CONSOLE_TOKEN       = random_password.read_token[0].result
      EKOS_CONSOLE_CONSOLE_WRITE_TOKEN = random_password.write_token[0].result
    } : {},
    var.oidc_client_secret == "" ? {} : { EKOS_CONSOLE_OIDC_CLIENT_SECRET = var.oidc_client_secret },
    var.secrets,
  )

  git_sync_enabled = length(var.git_repos) > 0

  git_sync_env = local.git_sync_enabled ? {
    EKOS_GIT_SYNC_REPOS    = join("\n", var.git_repos)
    EKOS_GIT_SYNC_INTERVAL = tostring(var.git_sync_interval)
    EKOS_GIT_SYNC_DEPTH    = tostring(var.git_sync_depth)
    EKOS_GIT_SYNC_USERNAME = var.git_username
  } : {}

  git_sync_secrets = local.git_sync_enabled && var.git_token != "" ? {
    EKOS_GIT_SYNC_TOKEN = var.git_token
  } : {}

  # Names are not secret; values are. A `for_each` needs the names in the clear.
  console_secret_names  = sort(nonsensitive(keys(local.console_secrets)))
  git_sync_secret_names = sort(nonsensitive(keys(local.git_sync_secrets)))
}
