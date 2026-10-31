# Plans of the Google Cloud module with mocked providers (RFC 0232): no project, no network. They
# check the module's own logic — which resources each compute creates, the ingress preset, the
# private exposure, the preconditions — not what Google Cloud would accept, which only an apply
# shows (docs/deployment.md §10).

mock_provider "google" {
  override_during = plan
  mock_data "google_project" {
    defaults = { number = "123456789012" }
  }
  mock_data "google_compute_zones" {
    defaults = { names = ["europe-west1-b", "europe-west1-c"] }
  }
  mock_resource "google_compute_global_address" {
    defaults = { address = "34.120.0.10" }
  }
}
# The cluster's computed endpoint and certificate (a mock's defaults do not reach a block's
# computed attributes).
override_resource {
  target          = google_container_cluster.this
  override_during = plan
  values = {
    endpoint = "10.0.0.2"
    master_auth = {
      cluster_ca_certificate = "bW9jaw=="
      client_certificate     = ""
      client_key             = ""
    }
  }
}

mock_provider "helm" {
  override_during = plan
}
mock_provider "random" {
  override_during = plan
  mock_resource "random_string" {
    defaults = { result = "a1b2c3" }
  }
  mock_resource "random_password" {
    defaults = { result = "generated-for-the-plan-only" }
  }
}

variables {
  name       = "ekos"
  project_id = "acme-ekos"
  region     = "europe-west1"
}

run "vm_by_default" {
  command = plan

  assert {
    condition     = length(google_compute_instance.vm) == 1 && length(google_cloud_run_v2_service.ekos) == 0 && length(google_container_cluster.this) == 0
    error_message = "compute = vm creates the VM only."
  }
  assert {
    condition     = module.common.console_env["EKOS_CONSOLE_INSTANCE_LEASE"] == "false"
    error_message = "a VM does not need the instance lease."
  }
  assert {
    condition     = startswith(output.backup_url, "gs://") && module.common.console_env["EKOS_CONSOLE_BACKUP_URL"] == output.backup_url
    error_message = "the console backs up to the module's bucket."
  }
}

run "cloud_run_is_public_by_default" {
  command = plan
  variables {
    compute = "container"
  }

  assert {
    condition     = google_cloud_run_v2_service.ekos[0].ingress == "INGRESS_TRAFFIC_ALL"
    error_message = "public by default."
  }
  assert {
    condition     = output.url == "https://ekos-123456789012.europe-west1.run.app"
    error_message = "the service's run.app address, known before it exists."
  }
  # Cloud Run refuses a startup window over 240 s (the module once asked for 300).
  assert {
    condition     = google_cloud_run_v2_service.ekos[0].template[0].containers[0].startup_probe[0].period_seconds * google_cloud_run_v2_service.ekos[0].template[0].containers[0].startup_probe[0].failure_threshold <= 240
    error_message = "period × failures of the startup probe is at most 240 s."
  }
  assert {
    condition     = module.common.console_env["EKOS_CONSOLE_INSTANCE_LEASE"] == "true"
    error_message = "a network share needs the lease."
  }
}

run "cloud_run_private_is_internal_only" {
  command = plan
  variables {
    compute  = "container"
    exposure = "private"
  }

  assert {
    condition     = google_cloud_run_v2_service.ekos[0].ingress == "INGRESS_TRAFFIC_INTERNAL_ONLY"
    error_message = "private: VPC traffic only."
  }
}

run "private_is_refused_on_a_vm" {
  command = plan
  variables {
    exposure = "private"
  }
  expect_failures = [var.exposure]
}

run "gke_without_a_domain_has_no_ingress_or_address" {
  command = plan
  variables {
    compute = "kubernetes"
  }

  assert {
    condition     = yamldecode(helm_release.ekos[0].values[0]).ingress.enabled == false
    error_message = "no ingress without a domain."
  }
  assert {
    condition     = length(google_compute_global_address.ingress) == 0 && output.ingress_ip == null
    error_message = "no address without a domain."
  }
}

run "gke_with_a_domain_gets_a_managed_certificate_on_a_reserved_address" {
  command = plan
  variables {
    compute = "kubernetes"
    domain  = "ekos.example.com"
  }

  assert {
    condition     = yamldecode(helm_release.ekos[0].values[0]).ingress.gke.managedCertificate == true
    error_message = "a public domain gets a Google-managed certificate."
  }
  assert {
    condition     = yamldecode(helm_release.ekos[0].values[0]).ingress.gke.staticIpName == google_compute_global_address.ingress[0].name
    error_message = "the ingress uses the reserved address."
  }
  assert {
    condition     = output.ingress_ip == "34.120.0.10"
    error_message = "the address to point the domain at."
  }
  assert {
    condition     = output.url == "https://ekos.example.com" && module.common.console_env["EKOS_CONSOLE_INSTANCE_LEASE"] == "true"
    error_message = "the console's address is the domain."
  }
  assert {
    condition     = length(google_compute_subnetwork.proxy_only) == 0
    error_message = "no proxy-only subnet for the external load balancer."
  }
}

run "gke_private_needs_a_tls_secret" {
  command = plan
  variables {
    compute  = "kubernetes"
    domain   = "ekos.internal.example.com"
    exposure = "private"
  }
  expect_failures = [helm_release.ekos]
}

run "gke_private_uses_the_internal_load_balancer" {
  command = plan
  variables {
    compute     = "kubernetes"
    domain      = "ekos.internal.example.com"
    exposure    = "private"
    helm_values = ["ingress: {tlsSecretName: ekos-tls}"]
  }

  assert {
    condition     = yamldecode(helm_release.ekos[0].values[0]).ingress.gke.internal == true && yamldecode(helm_release.ekos[0].values[0]).ingress.gke.managedCertificate == false
    error_message = "private: gce-internal, no managed certificate."
  }
  assert {
    condition     = google_compute_subnetwork.proxy_only[0].purpose == "REGIONAL_MANAGED_PROXY" && google_compute_subnetwork.proxy_only[0].ip_cidr_range == "10.42.16.0/23"
    error_message = "the internal load balancer's proxy-only subnet."
  }
  assert {
    condition     = length(google_compute_global_address.ingress) == 0
    error_message = "no public address when private."
  }
  assert {
    condition     = helm_release.ekos[0].values[1] == "ingress: {tlsSecretName: ekos-tls}"
    error_message = "the operator's values come after the module's."
  }
}
