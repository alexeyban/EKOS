# compute = "kubernetes": GKE Autopilot + the EKOS Helm chart (RFC 0232). One pod with the console
# and the git sync over one ReadWriteOnce persistent disk; secrets come from Secret Manager through
# GKE's Secret Manager add-on, read by the pod's own Kubernetes service account through Workload
# Identity Federation (granted in main.tf; no Google service account key, no annotation).
#
# The helm provider is configured by the caller from this module's `kubernetes` output (see
# examples/gcp).

locals {
  gke         = var.compute == "kubernetes" ? 1 : 0
  image_parts = regex("^(.*?)(?::([^:/]+))?$", var.image)

  gke_csi = {
    for group, ids in { console = local.console_secret_ids, gitSync = local.git_sync_secret_ids } : group => {
      enabled                   = length(ids) > 0
      provider                  = "gke"
      parameters = {
        secrets = yamlencode([for env_name, id in ids : {
          resourceName = "projects/${var.project_id}/secrets/${id}/versions/latest"
          path         = env_name
        }])
      }
    }
  }
}

resource "google_container_cluster" "this" {
  count               = local.gke
  project             = var.project_id
  name                = "${var.name}-gke"
  location            = var.region
  enable_autopilot    = true
  network             = google_compute_network.this.id
  subnetwork          = google_compute_subnetwork.main.id
  deletion_protection = false
  resource_labels     = local.labels

  # VPC-native; GKE picks the pod and service ranges.
  ip_allocation_policy {}

  secret_manager_config {
    enabled = true
  }

  depends_on = [google_project_service.apis]
}

resource "helm_release" "ekos" {
  count            = local.gke
  name             = "ekos"
  chart            = "${path.module}/../../../helm/ekos"
  namespace        = var.kubernetes_namespace
  create_namespace = true
  timeout          = 1200

  values = concat([yamlencode({
    image = {
      repository = local.image_parts[0]
      tag        = coalesce(local.image_parts[1], "latest")
    }
    env = { for k, v in module.common.console_env : k => v if !contains(["EKOS_CONSOLE_INSTANCE_LEASE", "EKOS_CONSOLE_SUPERVISED"], k) }
    csi = local.gke_csi
    serviceAccount = {
      create = true
      name   = "ekos"
    }
    persistence = {
      storageClass = "standard-rwo"
      size         = "${var.home_size_gb}Gi"
    }
    gitSync = {
      enabled         = module.common.git_sync_enabled
      repos           = var.git_repos
      intervalSeconds = var.git_sync_interval
      username        = var.git_username
    }
    ingress = {
      enabled = var.domain != ""
      host    = var.domain
    }
  })], var.helm_values)

  depends_on = [
    google_secret_manager_secret_version.this,
    google_secret_manager_secret_iam_member.runtime,
    google_storage_bucket_iam_member.runtime,
  ]
}
