# EKOS on Google Cloud (RFC 0232): a service account, Secret Manager, backups in Cloud Storage,
# and one of three computes (vm.tf, container.tf, kubernetes.tf).

data "google_project" "this" {
  project_id = var.project_id
}

resource "random_string" "suffix" {
  length  = 6
  special = false
  upper   = false
}

locals {
  zone   = var.zone != "" ? var.zone : "${var.region}-a"
  suffix = random_string.suffix.result
  labels = merge({ app = "ekos", managed-by = "terraform" }, var.labels)

  # Cloud Run's address is known before the service exists; the OIDC redirect needs it.
  run_url = "https://${var.name}-${data.google_project.this.number}.${var.region}.run.app"

  public_url = {
    vm         = "https://${local.vm_host}"
    container  = local.run_url
    kubernetes = var.domain != "" ? "https://${var.domain}" : "http://127.0.0.1:8000"
  }[var.compute]

  backup_url = var.backup_cron == "" ? "" : "gs://${google_storage_bucket.backups.name}/ekos/"

  console_secret_ids  = { for n in module.common.console_secret_names : n => "${var.name}-${n}" }
  git_sync_secret_ids = { for n in module.common.git_sync_secret_names : n => "${var.name}-${n}" }
  all_secret_ids      = merge(local.console_secret_ids, local.git_sync_secret_ids)

  # Who reads the secrets and writes backups: the service account on a VM and Cloud Run, the
  # pod's Kubernetes service account itself on GKE (Workload Identity Federation, no key).
  k8s_principal  = "principal://iam.googleapis.com/projects/${data.google_project.this.number}/locations/global/workloadIdentityPools/${var.project_id}.svc.id.goog/subject/ns/${var.kubernetes_namespace}/sa/ekos"
  runtime_member = var.compute == "kubernetes" ? local.k8s_principal : "serviceAccount:${google_service_account.ekos.email}"
}

resource "google_project_service" "apis" {
  for_each = toset(concat(
    ["iam.googleapis.com", "secretmanager.googleapis.com", "storage.googleapis.com", "compute.googleapis.com"],
    var.compute == "container" ? ["run.googleapis.com", "file.googleapis.com"] : [],
    var.compute == "kubernetes" ? ["container.googleapis.com"] : [],
  ))
  project            = var.project_id
  service            = each.value
  disable_on_destroy = false
}

module "common" {
  source = "../common"

  public_url         = local.public_url
  oidc               = var.oidc
  oidc_client_secret = var.oidc_client_secret
  owners             = var.owners
  instance_lease     = var.compute != "vm"
  backup_url         = local.backup_url
  backup_cron        = var.backup_cron
  env                = merge({ GOOGLE_CLOUD_PROJECT = var.project_id }, var.env)
  secrets            = var.secrets
  git_repos          = var.git_repos
  git_token          = var.git_token
  git_username       = var.git_username
  git_sync_interval  = var.git_sync_interval
}

# ── identity ─────────────────────────────────────────────────────────────────

resource "google_service_account" "ekos" {
  project      = var.project_id
  account_id   = "${var.name}-runtime"
  display_name = "EKOS console"
  depends_on   = [google_project_service.apis]
}

# ── Secret Manager ───────────────────────────────────────────────────────────

resource "google_secret_manager_secret" "this" {
  for_each  = local.all_secret_ids
  project   = var.project_id
  secret_id = each.value
  labels    = merge(local.labels, { env-name = lower(replace(each.key, "_", "-")) })

  replication {
    auto {}
  }

  depends_on = [google_project_service.apis]
}

resource "google_secret_manager_secret_version" "this" {
  for_each    = local.all_secret_ids
  secret      = google_secret_manager_secret.this[each.key].id
  secret_data = contains(keys(local.console_secret_ids), each.key) ? module.common.console_secrets[each.key] : module.common.git_sync_secrets[each.key]
}

resource "google_secret_manager_secret_iam_member" "runtime" {
  for_each  = local.all_secret_ids
  project   = var.project_id
  secret_id = google_secret_manager_secret.this[each.key].secret_id
  role      = "roles/secretmanager.secretAccessor"
  member    = local.runtime_member
}

# ── backups ──────────────────────────────────────────────────────────────────

resource "google_storage_bucket" "backups" {
  project                     = var.project_id
  name                        = "${var.name}-backups-${local.suffix}"
  location                    = var.region
  uniform_bucket_level_access = true
  public_access_prevention    = "enforced"
  labels                      = local.labels

  lifecycle_rule {
    condition {
      age            = var.backup_retention_days
      matches_prefix = ["ekos/"]
    }
    action {
      type = "Delete"
    }
  }

  depends_on = [google_project_service.apis]
}

resource "google_storage_bucket_iam_member" "runtime" {
  bucket = google_storage_bucket.backups.name
  role   = "roles/storage.objectUser"
  member = local.runtime_member
}

# ── network ──────────────────────────────────────────────────────────────────

resource "google_compute_network" "this" {
  project                 = var.project_id
  name                    = "${var.name}-vpc"
  auto_create_subnetworks = false
  depends_on              = [google_project_service.apis]
}

resource "google_compute_subnetwork" "main" {
  project                  = var.project_id
  name                     = "${var.name}-main"
  region                   = var.region
  network                  = google_compute_network.this.id
  ip_cidr_range            = var.address_space
  private_ip_google_access = true
}

# Outbound internet (image pulls, git, model APIs) for what has no public address.
resource "google_compute_router" "this" {
  project = var.project_id
  name    = "${var.name}-router"
  region  = var.region
  network = google_compute_network.this.id
}

resource "google_compute_router_nat" "this" {
  project                            = var.project_id
  name                               = "${var.name}-nat"
  router                             = google_compute_router.this.name
  region                             = var.region
  nat_ip_allocate_option             = "AUTO_ONLY"
  source_subnetwork_ip_ranges_to_nat = "ALL_SUBNETWORKS_ALL_IP_RANGES"
}
