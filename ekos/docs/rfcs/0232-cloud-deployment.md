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

* Private endpoints for the secret stores and the backup buckets. They stay public endpoints
  restricted to the deployment's identity: behind a private endpoint, Terraform run from outside
  the network could not write the secrets. `exposure = "private"` (§6) covers the console's own
  address only, and not on a VM.
* A GCS FUSE or EFS-backed `kubernetes` storage option; one ReadWriteOnce disk only.
* An ingress preset for AKS (§6.2).
* A restore larger than the platform's start allowance (§6.1); that one is run by hand.

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
  and example, `terraform test` (§6.4), `helm lint` and `helm template` (each preset rendered). None
  of the three clouds was applied for this RFC.

## 6. Amendment (2026-10-31): the gaps that need no cloud account

### 6.1 Restore on first start

With `backup_url` set and `backup_restore_on_empty` (default on), the console checks the home after
taking the instance lease and before opening its database. Empty — no `data/*/*/ekos.toml` and no
console database — means `ekos home restore --from <backup_url>/` (`cloud.restore_if_empty`), with
the backup job's environment (`platform_env`). The outcome decides the start:

| Restore | Console |
|---|---|
| succeeded | starts over the restored home; logs a warning so the platform's log shows it; rewrites the lease at once, since the restore moved `control/` (lease file included) to the trash |
| `no backup under …` | a first deployment: starts empty |
| anything else (wrong bucket, credentials, network) | **does not start** (`RestoreError`) |

The last row is the point: a console that starts empty over a location it cannot read uploads an
empty backup that night, and that becomes the newest one. A crash-looping console is the safe
failure. The restore runs before `/api/health` answers, so it must fit the start allowance, and two
allowances in the modules were wrong for the platform, refused at apply (no provider validates
them): Container Apps' startup probe asked for 60 failures (the maximum is 10; now 10 × 60 s) and
Cloud Run's for 300 s (the maximum of period × failures is 240 s; now 5 s × 48). ECS's `startPeriod`
is now its maximum, 300 s, with 600 s of load-balancer grace; the chart's start allowance is
`startupSeconds` (1800). The plan tests pin each platform's limit.

### 6.2 Ingress presets for Kubernetes

A `domain` no longer needs hand-written `helm_values` on GKE and EKS; `helm_values` still apply over
the module's and replace a preset.

* **GKE**: `google_compute_global_address` + a `ManagedCertificate` (chart
  `ingress.gke.managedCertificate`, `staticIpName`), GCE ingress class, HTTP off. Output
  `ingress_ip` for the domain's A record.
* **EKS Auto Mode**: the chart's `IngressClass` (controller `eks.amazonaws.com/alb`) +
  `IngressClassParams` (`scheme`, `certificateARNs` from `certificate_arn`), HTTPS listener. Both
  public subnets carry `kubernetes.io/role/elb` and `kubernetes.io/role/internal-elb`, the tags
  Auto Mode chooses subnets by. Without `certificate_arn` the preset is off and a domain needs
  `helm_values`.
* **AKS: not built.** The obvious preset, the application routing add-on, is NGINX-based and
  supported only until November 2026; its replacement (the Gateway API implementation) is not in
  azurerm 5.12, which offers only `web_app_routing` (NGINX) and the Application Gateway ingress
  controller. A domain on AKS keeps needing `helm_values`.

### 6.3 `exposure = "public" | "private"`

| | `private` |
|---|---|
| Container Apps | `internal_load_balancer_enabled`; a private DNS zone named after the environment's default domain, linked to the VNet, `*` → its static IP |
| Cloud Run | `INGRESS_TRAFFIC_INTERNAL_ONLY` |
| ECS | `internal = true` on the ALB |
| EKS | `IngressClassParams.scheme = internal` |
| GKE | `gce-internal` + a `REGIONAL_MANAGED_PROXY` subnet (`proxy_only_cidr`); needs `ingress.tlsSecretName` |
| AKS | nothing to change: the module adds no ingress |
| VM | refused by a variable validation |

### 6.4 Plans with mocked providers

`modules/{azure,gcp,aws}/tests/plan.tftest.hcl` (25 runs) plan each module with
`mock_provider … { override_during = plan }`: no account, no network, run in CI. They check which
resources each `compute` creates, what reaches the console's environment (lease, backup URL,
production mode, OIDC redirect), each preset and `exposure`, every precondition, and the platform
limits above. A mutation check confirmed they fail when the switch they cover is broken. Two
things were learned writing them: the azurerm mock still parses resource ids, so each mocked
resource needs a well-formed ARM id; and a mock cannot fill a block the configuration does not
declare, so the GKE cluster now declares `master_auth` (no client certificate), which it should
have anyway.

### 6.5 The image

`release.yml`'s smoke test now also checks that the image has `ekos-git-sync`, `ekos-entrypoint` and
an `ekos` built with `object-storage` (a restore from `s3://` with no network must fail at the
connection, not be refused as unsupported). The examples keep `:latest` until a release contains
this work; pin them to that version then.
