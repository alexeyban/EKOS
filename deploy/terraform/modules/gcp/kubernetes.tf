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

  gke_csi = merge({ driver = "secrets-store-gke.csi.k8s.io" }, {
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
  })
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

  # Only Workload Identity and OAuth tokens; never a client certificate.
  master_auth {
    client_certificate_config {
      issue_client_certificate = false
    }
  }

  secret_manager_config {
    enabled = true
  }

  depends_on = [google_project_service.apis]
}

# The ingress preset (RFC 0232). Public: a reserved global address and a Google-managed
# certificate for `domain` on the GCE load balancer; point the domain's A record at the
# `ingress_ip` output and the certificate is issued once it resolves. Private: the internal
# Application Load Balancer, which needs a proxy-only subnet and a certificate the operator puts
# in a Kubernetes TLS secret (Google-managed certificates are external only).
locals {
  gke_ingress_public  = local.gke == 1 && var.domain != "" && var.exposure == "public"
  gke_ingress_private = local.gke == 1 && var.domain != "" && var.exposure == "private"
}

resource "google_compute_global_address" "ingress" {
  count      = local.gke_ingress_public ? 1 : 0
  project    = var.project_id
  name       = "${var.name}-ingress"
  depends_on = [google_project_service.apis]
}

resource "google_compute_subnetwork" "proxy_only" {
  count         = local.gke_ingress_private ? 1 : 0
  project       = var.project_id
  name          = "${var.name}-proxy-only"
  region        = var.region
  network       = google_compute_network.this.id
  ip_cidr_range = var.proxy_only_cidr
  purpose       = "REGIONAL_MANAGED_PROXY"
  role          = "ACTIVE"
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
      gke = {
        managedCertificate = local.gke_ingress_public
        staticIpName       = local.gke_ingress_public ? google_compute_global_address.ingress[0].name : ""
        internal           = local.gke_ingress_private
      }
    }
  })], var.helm_values)

  lifecycle {
    # The internal load balancer cannot use a Google-managed certificate.
    precondition {
      condition     = !local.gke_ingress_private || length(var.helm_values) > 0
      error_message = "compute = \"kubernetes\" with exposure = \"private\" and a domain needs helm_values with ingress.tlsSecretName (a Kubernetes TLS secret for the domain in the namespace); see docs/deployment.md."
    }
  }

  depends_on = [
    google_compute_subnetwork.proxy_only,
    google_secret_manager_secret_version.this,
    google_secret_manager_secret_iam_member.runtime,
    google_storage_bucket_iam_member.runtime,
  ]
}
