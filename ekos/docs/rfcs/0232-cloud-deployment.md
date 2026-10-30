# RFC 0232 — EKOS on Azure, Google Cloud and AWS

**Status:** Accepted
**Date:** 2026-10-30
**Depends on:** RFC 0179 (installer and image), RFC 0183 (home backup and restore), RFC 0184 (console
settings, keychain off in a container), RFC 0113 (`object_store` backend)

## 1. Problem

EKOS runs on one machine: the image (RFC 0179) with Compose, or the installer. An organisation that
wants it in its cloud has to work out, alone, five things the image does not answer:

1. **Where the secrets live.** The console reads secrets from the environment or an OS keychain
   (RFC 0184). A container has no keychain; the cloud has a secret store (Key Vault, Secret Manager,
   Secrets Manager) that nothing reads.
2. **Who may do what.** Nothing says which identity the console runs as, or what it needs.
3. **Where the state lives.** `EKOS_HOME` holds SQLite (the console's database, ledgers). On a
   serverless container platform that is a network share, and a new revision starts while the old
   one still runs: two consoles writing one SQLite database over NFS or SMB corrupt it.
4. **How the sources arrive.** The console reads `/sources`; it never clones. In a cloud nobody
   checks repositories out by hand.
5. **How the home is backed up.** `ekos home backup` writes a local file; in a cloud it belongs in
   object storage, and the `object_store` code that could write it was only in the `distributed`
   build, without Google Cloud Storage at all.

## 2. Design

One deployment shape on each cloud, three ways to run it, in Terraform:

| | Azure | Google Cloud | AWS |
|---|---|---|---|
| Secret store | Key Vault (RBAC) | Secret Manager | Secrets Manager |
| Identity | user-assigned managed identity | service account / Workload Identity Federation | IAM role (instance profile, task role, Pod Identity) |
| Backups | Blob Storage + lifecycle rule | Cloud Storage + lifecycle rule | S3 + lifecycle rule |
| `vm` | Ubuntu VM, managed data disk | Compute Engine, persistent disk | EC2, EBS volume |
| `container` | Container Apps, Azure Files NFS | Cloud Run, Filestore NFS | ECS Fargate + ALB, EFS |
| `kubernetes` | AKS + Key Vault CSI add-on | GKE Autopilot + Secret Manager add-on | EKS Auto Mode + Secrets Store CSI (AWS provider) |

`deploy/terraform/modules/{azure,gcp,aws}` take the same inputs (`compute`, `oidc`, `owners`,
`secrets`, `git_repos`, `git_token`, `backup_cron`, sizes); `modules/common` derives what all three
share (the console's settings, which are secret, generated secrets). `deploy/terraform/examples/*`
are complete root modules. `deploy/helm/ekos` is the chart the three `kubernetes` computes install.

### 2.1 Secrets: the store is the source, the platform injects

Terraform writes every secret into the cloud's store (the generated session secret, static tokens
when there is no OIDC, the OIDC client secret, the git token, and the operator's `secrets` map —
model keys, database passwords). The runtime identity may read exactly those secrets.

They reach the process two ways, both ending as environment variables, which is what the console
and `ekos` already read:

* **Injected by the platform** — Container Apps (Key Vault references), Cloud Run (`secret_key_ref`),
  ECS (`secrets`).
* **As files** — the CSI driver on all three Kubernetes services, and the VM's boot script
  (`ekos-fetch-secrets`, which reads the store with the VM's identity onto tmpfs at every start).
  The image's new entrypoint, `ekos-entrypoint`, turns each file in `EKOS_SECRETS_DIR` named like a
  variable into that variable, unless the platform already set it.

The console's secrets and the git sync's are separate groups: the console never sees the git token.
No secret is written to disk on a VM, put in a URL or passed as an argument.

### 2.2 Identity and permissions

One runtime identity per deployment, with two grants: read its own secrets, read/write objects under
`ekos/` in its backup bucket. Kubernetes uses workload identity (AKS federated credential, GKE
principal binding on the Kubernetes service account, EKS Pod Identity); no key exists anywhere.
People's access inside EKOS stays the console's own (OIDC + roles, RFC 0180); with an issuer and an
owner the modules turn production mode on.

### 2.3 One writer over the home

* `vm` and `kubernetes`: a local or ReadWriteOnce disk, one container (Kubernetes: `replicas: 1`,
  `strategy: Recreate`). Two writers cannot happen by construction.
* `container`: an NFS share (SQLite needs POSIX locks; SMB does not give them reliably). ECS stops
  the old task first (minimum healthy 0 %). Container Apps and Cloud Run overlap revisions.

For the overlap, the console gets an **instance lease** (`instance_lease = true`, `web/api/app/cloud.py`):
a file `control/instance.lease` holding the holder's id, generation and heartbeat, replaced by
rename. A starting console writes itself in, waits 1.5 heartbeats if the previous holder is alive,
then starts. The previous holder sees another id at its next heartbeat and shuts down as on SIGTERM.
Last start wins; at most one heartbeat (10 s) of overlap, during which the new console has not yet
opened the database. A holder whose heartbeat is older than three intervals is gone. The modules turn
the lease on for `container` and `kubernetes`.

### 2.4 Sources: the git sync

`ekos-git-sync` (in the image) clones each `name=https-url[#branch]` into `/sources/name`, then
fetches and hard-resets it every `interval` seconds. It runs beside the console with `/sources`
mounted read-write (the console mounts it read-only): a sidecar container on every platform, a
Compose profile on a VM. The token reaches git as an `http.extraHeader` through `GIT_CONFIG_*`
variables of the git process only. Full history by default: the git observer, freshness and blame
read it. A directory that exists and is not a checkout is never touched; one failing repository does
not stop the others.

### 2.5 Backups to object storage

* `ekos home backup --to <url>` writes the RFC 0183 archive, uploads it under the location by its own
  name (multipart, 8 MiB parts), then removes the local copy. A failed upload keeps it and says where.
* `ekos home restore --from <url>` takes one backup's URL, or a location ending in `/` for the newest
  `ekos-backup-*.tar.zst` directly under it.
* URLs: `s3://bucket/prefix/`, `gs://bucket/prefix/`,
  `https://<account>.blob.core.windows.net/<container>/prefix/`. Never a query string: credentials
  come from the identity (`ObjectStoreBackend::parse_url` forwards `AWS_*`/`AZURE_*`/`GOOGLE_*`; for
  Container Apps, `IDENTITY_ENDPOINT` becomes `azure_msi_endpoint`).
* Built with the new `object-storage` feature of the `ekos` crate: the `object_store` client only,
  now with GCS (`object_store/gcp`). Not the `distributed` feature (RFC 0113 keeps that out of stock
  builds). The image is built with it; the install packages are not, and say so when `--to` is used.
* The console runs it on `backup_cron` (UTC) when `backup_url` is set, through `platform_env`: the
  base environment plus the cloud identity variables, never a tenant's keys. Retention is the
  bucket's lifecycle rule (`backup_retention_days`), not something EKOS deletes.

### 2.6 TLS and addresses

Container Apps and Cloud Run give an HTTPS name; ECS uses an ALB with the operator's ACM certificate
(`domain` + `certificate_arn` required). A VM runs Caddy, which gets a certificate for `domain`, or
by default for `<label>.<region>.cloudapp.azure.com` (Azure) or `<ip>.sslip.io` (GCP, AWS).
Kubernetes: the chart's Ingress when `domain` is set — its class and TLS come from the operator's
`helm_values`, since each cluster serves an Ingress differently — otherwise `kubectl port-forward`. The OIDC
redirect URI is derived from that address.

## 3. Alternatives considered

* **A secrets backend in the console** (read Key Vault/Secret Manager/Secrets Manager at run time,
  write back from the settings page). Three SDKs in the console, and `ekos` subprocesses would still
  need the values as environment variables. Platform injection is what each cloud recommends and
  needs no code.
* **SQLite on SMB / GCS FUSE / S3 mounts.** No reliable POSIX locking; rejected. NFS (Azure Files
  NFS, Filestore, EFS) or block storage only.
* **A database server instead of SQLite.** A much larger change to the console and the ledger, for a
  single-writer workload. Not now.
* **Scaling the console out.** It is a single writer by design; the distributed query path (RFC 0113)
  is the scale-out story, not more console replicas.

## 4. Not built

* Private networking for the console (private endpoints, internal load balancers); the modules expose
  HTTPS publicly, restricted by `allowed_cidrs` where the platform allows it.
* A GCS FUSE or EFS-backed `kubernetes` storage option; one ReadWriteOnce disk only.
* Restore on first start from the newest backup (a fresh deployment over an empty share). Run
  `ekos home restore --from <backup_url>` once by hand.
* An ingress controller or certificate for Kubernetes; the operator supplies them.

## 5. Verification

* Rust: `home_remote` unit tests (URL rules, newest-backup selection, a multipart round trip through
  `object_store`'s local-filesystem store, a sub-prefix object ignored); `home_backup` CLI test for
  the flags with and without the feature; the Container Apps identity mapping in `segment-backend`.
* Console: `tests/test_cloud.py` — lease take-over, stale holder, damaged file, `hold` shutting down
  on loss, release, concurrent replacement without torn files; `platform_env` keeps identity
  variables and drops tenant and console secrets; the backup job's argv, trigger and result.
* Scripts: `scripts/tests/test_git_sync.py` (real git, local repositories: clone, follow a branch,
  drop local edits, refuse a non-checkout, one failure does not stop the rest, config validation,
  token only in the header variable); `scripts/tests/test_entrypoint.py`.
* Deployment: CI's new `deploy` job runs `terraform fmt -check`, `terraform validate` on every module
  and example, `helm lint` and `helm template`. None of the three clouds was applied for this RFC.
