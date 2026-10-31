# compute = "container": Cloud Run (RFC 0232). One service, exactly one instance with CPU always
# allocated (the console runs pipelines and schedules in the background), the git sync as a
# sidecar. The EKOS home and the sources are directories on a Filestore NFS share, reached by
# direct VPC egress. Secrets are Secret Manager references. A new revision overlaps the old one
# for a moment; the console's instance lease keeps one writer.

locals {
  run = var.compute == "container" ? 1 : 0
  nfs = local.run == 1 ? google_filestore_instance.this[0].networks[0].ip_addresses[0] : ""
}

resource "google_filestore_instance" "this" {
  count    = local.run
  project  = var.project_id
  name     = "${var.name}-files"
  location = local.zone
  tier     = var.filestore_tier
  labels   = local.labels

  file_shares {
    name        = "ekos"
    capacity_gb = max(1024, var.home_size_gb)
  }

  networks {
    network = google_compute_network.this.name
    modes   = ["MODE_IPV4"]
  }

  depends_on = [google_project_service.apis]
}

# A Filestore share starts owned by root and Cloud Run runs the image as uid 1000. This job
# (busybox, as root) creates home/ and sources/ on the share and hands them to uid 1000, once at
# creation; Terraform waits for that execution to complete (`run_execution_token`).
resource "google_cloud_run_v2_job" "prepare_share" {
  count               = local.run
  project             = var.project_id
  name                = "${var.name}-prepare-share"
  location            = var.region
  deletion_protection = false
  run_execution_token = "prepare-${local.suffix}"

  template {
    task_count = 1
    template {
      execution_environment = "EXECUTION_ENVIRONMENT_GEN2"
      max_retries           = 3
      service_account       = google_service_account.ekos.email
      containers {
        image   = "docker.io/library/busybox:1.37"
        command = ["sh", "-c", "mkdir -p /share/home /share/sources && chown 1000:1000 /share/home /share/sources"]
        volume_mounts {
          name       = "share"
          mount_path = "/share"
        }
      }
      volumes {
        name = "share"
        nfs {
          server = local.nfs
          path   = "/ekos"
        }
      }
      vpc_access {
        egress = "PRIVATE_RANGES_ONLY"
        network_interfaces {
          network    = google_compute_network.this.id
          subnetwork = google_compute_subnetwork.main.id
        }
      }
    }
  }
}

resource "google_cloud_run_v2_service" "ekos" {
  count    = local.run
  project  = var.project_id
  name     = var.name
  location = var.region
  # Private: only from the VPC (and Shared VPC / VPC Service Controls perimeters).
  ingress             = var.exposure == "private" ? "INGRESS_TRAFFIC_INTERNAL_ONLY" : "INGRESS_TRAFFIC_ALL"
  deletion_protection = false
  # The console signs people in itself (OIDC); Cloud Run's IAM check would stand in front of it.
  invoker_iam_disabled = true
  labels               = local.labels

  template {
    execution_environment            = "EXECUTION_ENVIRONMENT_GEN2"
    service_account                  = google_service_account.ekos.email
    max_instance_request_concurrency = 80
    timeout                          = "3600s"

    scaling {
      min_instance_count = 1
      max_instance_count = 1
    }

    vpc_access {
      egress = "PRIVATE_RANGES_ONLY"
      network_interfaces {
        network    = google_compute_network.this.id
        subnetwork = google_compute_subnetwork.main.id
      }
    }

    containers {
      name  = "ekos"
      image = var.image
      ports {
        container_port = 8000
      }
      resources {
        limits = {
          cpu    = var.container_cpu
          memory = var.container_memory
        }
        cpu_idle          = false
        startup_cpu_boost = true
      }

      dynamic "env" {
        for_each = module.common.console_env
        content {
          name  = env.key
          value = env.value
        }
      }
      dynamic "env" {
        for_each = local.console_secret_ids
        content {
          name = env.key
          value_source {
            secret_key_ref {
              secret  = google_secret_manager_secret.this[env.key].secret_id
              version = "latest"
            }
          }
        }
      }

      volume_mounts {
        name       = "home"
        mount_path = "/var/lib/ekos"
      }
      volume_mounts {
        name       = "sources-ro"
        mount_path = "/sources"
      }

      # Cloud Run allows period × failures of 240 s at most (300 s is refused at apply). The
      # console serves nothing until it has started, which over an empty home with a backup URL
      # includes restoring the newest backup: a larger home is restored by hand (docs §10).
      startup_probe {
        http_get {
          path = "/api/health"
          port = 8000
        }
        period_seconds    = 5
        failure_threshold = 48
      }
      liveness_probe {
        http_get {
          path = "/api/health"
          port = 8000
        }
        period_seconds = 30
      }
    }

    dynamic "containers" {
      for_each = module.common.git_sync_enabled ? [1] : []
      content {
        name    = "git-sync"
        image   = var.image
        command = ["/usr/local/bin/ekos-entrypoint", "ekos-git-sync"]
        resources {
          limits = {
            cpu    = "1"
            memory = "512Mi"
          }
          cpu_idle = false
        }
        dynamic "env" {
          for_each = merge(module.common.git_sync_env, { EKOS_GIT_SYNC_DEST = "/sources" })
          content {
            name  = env.key
            value = env.value
          }
        }
        dynamic "env" {
          for_each = local.git_sync_secret_ids
          content {
            name = env.key
            value_source {
              secret_key_ref {
                secret  = google_secret_manager_secret.this[env.key].secret_id
                version = "latest"
              }
            }
          }
        }
        volume_mounts {
          name       = "sources-rw"
          mount_path = "/sources"
        }
      }
    }

    volumes {
      name = "home"
      nfs {
        server = local.nfs
        path   = "/ekos/home"
      }
    }
    volumes {
      name = "sources-ro"
      nfs {
        server    = local.nfs
        path      = "/ekos/sources"
        read_only = true
      }
    }
    dynamic "volumes" {
      for_each = module.common.git_sync_enabled ? [1] : []
      content {
        name = "sources-rw"
        nfs {
          server = local.nfs
          path   = "/ekos/sources"
        }
      }
    }
  }

  depends_on = [
    google_cloud_run_v2_job.prepare_share,
    google_secret_manager_secret_version.this,
    google_secret_manager_secret_iam_member.runtime,
    google_storage_bucket_iam_member.runtime,
  ]
}
