# compute = "vm": one Compute Engine VM running the image with Docker Compose (RFC 0232). The
# EKOS home and the sources are on a persistent disk that outlives the VM; secrets are read from
# Secret Manager with the VM's service account at every start; Caddy serves HTTPS.

locals {
  vm      = var.compute == "vm" ? 1 : 0
  vm_ip   = try(google_compute_address.vm[0].address, "")
  vm_host = var.domain != "" ? var.domain : "${local.vm_ip}.sslip.io"

  vm_secrets = concat(
    [for n, id in local.console_secret_ids : { group = "console", name = n, ref = "projects/${var.project_id}/secrets/${id}" }],
    [for n, id in local.git_sync_secret_ids : { group = "git-sync", name = n, ref = "projects/${var.project_id}/secrets/${id}" }],
  )
}

resource "google_compute_address" "vm" {
  count   = local.vm
  project = var.project_id
  name    = "${var.name}-ip"
  region  = var.region
}

resource "google_compute_firewall" "https" {
  count         = local.vm
  project       = var.project_id
  name          = "${var.name}-https"
  network       = google_compute_network.this.name
  source_ranges = var.allowed_cidrs
  target_tags   = ["ekos"]
  allow {
    protocol = "tcp"
    ports    = ["80", "443"]
  }
}

resource "google_compute_firewall" "ssh" {
  count         = local.vm
  project       = var.project_id
  name          = "${var.name}-ssh"
  network       = google_compute_network.this.name
  source_ranges = concat(["35.235.240.0/20"], var.admin_cidrs)
  target_tags   = ["ekos"]
  allow {
    protocol = "tcp"
    ports    = ["22"]
  }
}

resource "google_compute_disk" "home" {
  count   = local.vm
  project = var.project_id
  name    = "${var.name}-home"
  zone    = local.zone
  type    = "pd-balanced"
  size    = var.home_size_gb
  labels  = local.labels
}

resource "google_compute_instance" "vm" {
  count        = local.vm
  project      = var.project_id
  name         = "${var.name}-vm"
  zone         = local.zone
  machine_type = var.machine_type
  tags         = ["ekos"]
  labels       = local.labels

  boot_disk {
    initialize_params {
      image = "ubuntu-os-cloud/ubuntu-2404-lts-amd64"
      size  = 50
      type  = "pd-balanced"
    }
  }

  attached_disk {
    source      = google_compute_disk.home[0].id
    device_name = "ekos-home"
  }

  network_interface {
    subnetwork = google_compute_subnetwork.main.id
    access_config {
      nat_ip = google_compute_address.vm[0].address
    }
  }

  service_account {
    email  = google_service_account.ekos.email
    scopes = ["cloud-platform"]
  }

  shielded_instance_config {
    enable_secure_boot = true
  }

  metadata = {
    enable-oslogin = "TRUE"
    user-data = templatefile("${path.module}/../../templates/cloud-init.yaml.tftpl", {
      compose   = file("${path.module}/../../templates/docker-compose.vm.yml")
      caddyfile = file("${path.module}/../../templates/Caddyfile")
      compose_env = join("\n", [
        "EKOS_IMAGE=${var.image}",
        "EKOS_DOMAIN=${local.vm_host}",
        "COMPOSE_PROFILES=${join(",", concat(["tls"], module.common.git_sync_enabled ? ["git-sync"] : []))}",
      ])
      console_env  = join("\n", [for k, v in module.common.console_env : "${k}=${v}"])
      git_sync_env = join("\n", [for k, v in module.common.git_sync_env : "${k}=${replace(v, "\n", ",")}"])
      prepare_disk = templatefile("${path.module}/../../templates/prepare-disk.sh.tftpl", {
        device = "/dev/disk/by-id/google-ekos-home"
      })
      fetch_secrets = templatefile("${path.module}/../../templates/fetch-secrets.sh.tftpl", {
        cloud           = "gcp"
        secrets         = local.vm_secrets
        azure_client_id = ""
        azure_vault_uri = ""
        aws_region      = ""
      })
    })
  }

  allow_stopping_for_update = true

  depends_on = [
    google_secret_manager_secret_version.this,
    google_secret_manager_secret_iam_member.runtime,
    google_storage_bucket_iam_member.runtime,
  ]
}
