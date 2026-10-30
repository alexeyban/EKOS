# Devlog 313 — EKOS on Azure, Google Cloud and AWS (RFC 0232)

**Date:** 2026-10-30
**PRs:** one PR from `rfc-0232-cloud-deployment`
**Branch:** `rfc-0232-cloud-deployment` → `main`

---

## Summary

EKOS can now be deployed to any of the three main clouds with Terraform, in three shapes each: a VM
(Docker, a data disk, Caddy), a serverless container (Container Apps, Cloud Run, ECS Fargate over an
NFS share) or Kubernetes (AKS, GKE Autopilot, EKS Auto Mode with a new Helm chart). Each deployment
keeps its secrets in the cloud's secret store, runs as one workload identity with two grants (its own
secrets, its backup prefix), syncs its sources from git, and backs the home up every night to object
storage. The core changes that made this possible: `ekos home backup --to` / `restore --from` over
`object_store` (with GCS added), an instance lease in the console so a shared home has one writer,
scheduled backups, a git sync and a secrets entrypoint in the image. **None of the Terraform was
applied against a real account** — it is checked by `terraform fmt`/`validate` in CI and offline here.

---

## RFC 0232 — cloud deployment

### Problem / motivation
The image ran on one machine. In a cloud nothing said where secrets live, which identity the console
runs as, how SQLite survives a platform that overlaps revisions on a network share, how sources
arrive (the console never clones) or where backups go.

### What was built

| Component | What |
|---|---|
| `crates/cli/src/commands/home_remote.rs` | URL rules, newest-backup selection, multipart upload (8 MiB parts, 4 in flight) and streamed download through `object_store` |
| `ekos home backup --to <url>`, `ekos home restore --from <url>` | `app.rs`, `home_backup.rs::{run_backup_to, run_restore_from}`; refused without the feature before anything is written; a failed upload keeps the archive |
| `object-storage` feature (`ekos` crate) | `ekos-segment-backend/object-store` + `futures`; not `distributed` |
| `segment-backend` | `object_store/gcp` added; `IDENTITY_ENDPOINT` → `azure_msi_endpoint` for Container Apps |
| `web/api/app/cloud.py` | `InstanceLease` (take / take_over / beat / hold / release), `BackupJob`, `platform_env` |
| Console settings | `instance_lease`, `instance_lease_seconds`, `backup_url`, `backup_cron` (+ `console_file.FIELDS` rows, group "Cloud") |
| `scripts/ekos-git-sync.py` | clone / fetch + hard reset per `name=url#branch`, token as `http.extraHeader` via `GIT_CONFIG_*` |
| `scripts/ekos-entrypoint.sh` | the image's `ENTRYPOINT`: files in `EKOS_SECRETS_DIR` → variables, platform values win |
| `Dockerfile` | `ARG EKOS_FEATURES="object-storage"`, entrypoint and git sync copied in |
| `deploy/terraform/modules/{common,azure,gcp,aws}`, `examples/*`, `templates/*` | the deployments |
| `deploy/helm/ekos` | Deployment (1 replica, Recreate), PVC (kept on uninstall), SecretProviderClass ×2, optional StorageClass, Service, Ingress |
| CI `deploy` job | `terraform fmt -check`, `terraform validate` per module/example, `helm lint`/`template`, `scripts/tests` |

### Implementation details worth remembering
- **Two secret groups.** The console's and the git sync's secrets are separate everywhere (Key Vault
  names, ACA secrets, CSI SecretProviderClasses, VM tmpfs directories), so the console never holds
  the git token.
- **NFS shares start owned by root**; the image runs as uid 1000. Container Apps uses a busybox init
  container to `chown`; Cloud Run has no init containers, so a one-shot Cloud Run job does it at
  creation (`start_execution_token`) and the service waits two minutes (`time_sleep`); EFS access
  points create their roots owned by 1000; Kubernetes uses `fsGroup`.
- **Cloud Run's URL is computed** (`https://<name>-<project number>.<region>.run.app`) because the
  OIDC redirect URI is an environment variable of the same service — reading `uri` would be a cycle.
- **AKS SecretProviderClass `objects`** is a YAML string whose `array` items are themselves YAML
  strings; built with nested `yamlencode`.
- **The helm provider is configured by the example roots**, from module outputs; EKS uses `exec`
  (`aws eks get-token`) because `aws_eks_cluster_auth` would be read before the cluster exists.
- **ECS** deploys with minimum healthy 0 % / maximum 100 %: the old task stops before the new one
  starts, so the home never has two writers there even without the lease.
- **`object_store` forwards every `AWS_*`/`AZURE_*`/`GOOGLE_*` variable** as a builder option; the
  Azure module sets `AZURE_CLIENT_ID` so the managed-identity client picks the user-assigned
  identity, the AWS module sets `AWS_REGION`.

### Decisions (alternatives considered, why this choice)
- **Platform injection, not a vault client in the console.** Three SDKs and a write-back path for
  no gain: `ekos` subprocesses need environment variables anyway.
- **No SQLite on SMB, GCS FUSE or S3 mounts.** NFS or block storage only.
- **Last start wins** for the lease: the platform starts the new revision expecting it to become
  healthy; making it wait for the old one would deadlock the rollout.
- **sslip.io for VMs without a domain** (GCP, AWS), the cloudapp name on Azure: a real certificate
  from the first boot, no DNS work required. A `domain` replaces it.
- **One module per cloud with a `compute` switch** instead of nine modules: identity, secrets,
  backups and network are shared by the three computes.

---

## Knowledge Captured
- `object_store` 0.14's `buffered` module needs its `tokio` feature; `WriteMultipart` (always
  available) plus `put_multipart` is enough for a streamed upload.
- `object_store::parse_url_opts` reads no environment by itself; Container Apps' managed identity is
  `IDENTITY_ENDPOINT` + `IDENTITY_HEADER` (the latter `object_store` reads itself), not IMDS.
- Container Apps NFS storage: `share_name` is `/<account>/<share>`, `nfs_server_url` is
  `<account>.file.core.windows.net`; the account must have HTTPS-only off (NFS has no TLS) and allow
  the environment's subnet; only premium `FileStorage` accounts have NFS shares.
- `nonsensitive()` no longer errors on a non-sensitive value in current Terraform, but `for_each`
  (including a `dynamic` block's) over a map that holds sensitive values does — iterate over names.
- azurerm 5.x docs list `node_provisioning_profile` on `azurerm_kubernetes_cluster` as Required
  while its own example omits it; the module sets `mode = "Manual"`.
- curl ≥ 7.75 signs AWS requests itself (`--aws-sigv4`), so an Ubuntu VM reads Secrets Manager with
  IMDSv2 credentials and no AWS CLI.
- This session had no `terraform`/`helm` binaries (no route to their downloads). The modules were
  checked with python-hcl2 against each provider's docs (every argument name), a reference checker,
  `hclwrite.Format` built from source (what `terraform fmt` uses) and a small Go renderer for the
  chart; CI runs the real tools.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0232-cloud-deployment.md` | new RFC |
| `ekos/crates/cli/src/commands/home_remote.rs` | new: object-storage transfer for backups |
| `ekos/crates/cli/src/commands/home_backup.rs`, `app.rs`, `commands/mod.rs` | `--to`, `--from` |
| `ekos/crates/cli/Cargo.toml` | `object-storage` feature, optional `futures` |
| `ekos/crates/cli/tests/home_backup.rs` | flags test |
| `ekos/crates/segment-backend/{Cargo.toml,src/object_store_backend.rs}` | GCS; Container Apps identity |
| `web/api/app/cloud.py`, `main.py`, `scheduler.py`, `settings.py`, `console_file.py` | lease, scheduled backups |
| `web/api/tests/test_cloud.py` | new |
| `scripts/ekos-git-sync.py`, `scripts/ekos-entrypoint.sh`, `scripts/tests/test_{git_sync,entrypoint}.py` | new |
| `Dockerfile` | feature, entrypoint, git sync |
| `deploy/terraform/**`, `deploy/helm/ekos/**` | new |
| `.github/workflows/ci.yml` | `deploy` job; object-storage clippy and tests |
| `docs/deployment.md`, `README.md`, `docs/generated/ekos-self-documentation.html`, `TODO.md`, `CLAUDE.md` | docs |
