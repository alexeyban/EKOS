# Devlog 314 — Cloud deployment without a cloud: first-start restore, ingress presets, private exposure, mocked plans (RFC 0232 §6)

**Date:** 2026-10-31
**PRs:** #21 (same branch as devlog_313)
**Branch:** `rfc-0232-cloud-deployment` → `main` (open)

---

## Summary

Everything left in the RFC 0232 plan that needs no cloud account. The console now restores the newest backup when
it starts over an empty home. GKE and EKS get an HTTPS ingress for a `domain` without hand-written `helm_values`, and
`exposure = "private"` keeps the console's address inside the network. Each module is now planned in CI with mocked
providers, the release smoke test checks the image's cloud pieces, and `docs/deployment.md` §10 has a cost table.
Writing the tests found **two start-probe settings that every real apply would have rejected** (Container Apps and Cloud
Run); `terraform validate` accepts both.

---

## RFC 0232 §6 — the gaps that need no cloud account

### Problem / motivation
Phase 2 of the plan (apply on each cloud) needs accounts and money. Phase 3's gaps and most of the checks do not.

### What was built
| Component | What |
|---|---|
| `web/api/app/cloud.py` `restore_if_empty`, `home_is_empty`, `RestoreError` | first-start restore; outcome `restored` / `no-backup` / `not-empty`, anything else raises |
| `main.py` lifespan | restore after the lease, before `init_engine`; rewrite the lease after a restore |
| `settings.py`, `console_file.py` | `backup_restore_on_empty` (default on) |
| `deploy/helm/ekos` | `startupSeconds`; `ingress.gke.{managedCertificate,staticIpName,internal}`, `ingress.eksAlb.{enabled,scheme,certificateArn}`; operator annotations win over presets |
| `modules/gcp` | global address + managed certificate preset, `ingress_ip` output, proxy-only subnet for `gce-internal`, Cloud Run internal ingress, `master_auth` declared, startup probe 5 s × 48 |
| `modules/aws` | EKS Auto Mode ALB preset from `certificate_arn`, internal ALB on ECS, `internal-elb` subnet tag, `startPeriod` 300, grace 600 |
| `modules/azure` | internal Container Apps environment + private DNS zone (`*` → static IP), startup probe 60 s × 10 |
| `modules/*/variables.tf` | `exposure` with a validation refusing `private` on a VM |
| `modules/*/tests/plan.tftest.hcl` | 25 runs with mocked providers |
| `.github/workflows/ci.yml` | `terraform test` per module; `helm template` per preset, grepped for its resource |
| `.github/workflows/release.yml` | smoke: git sync, entrypoint, `object-storage` compiled in |
| docs | RFC 0232 §4/§5/§6, `docs/deployment.md` §10 (restore, presets, exposure, costs, what was checked), Terraform README, README, TODO, capabilities page |

### Implementation details worth remembering
- **A first-start restore that cannot read its location must not start empty.** Starting empty uploads an empty backup
  that night, and that becomes the newest one, so the next restore takes it. Only the CLI's `no backup under …` means
  "first deployment"; everything else is `RestoreError` and the console exits (exit 3 under uvicorn).
- **The restore replaces `control/`, lease file included** (it moves the old directory to the trash). The console
  rewrites the lease straight after, so a console starting in that gap still waits.
- **The console logs only warnings and up** (`ekos.console.*` loggers are not configured). The restore line is a
  warning so that it reaches the platform's log; the `info` lines never appear in a container.
- **Checked against the real image and MinIO:** an empty home with a backup restores it (and a restart does not restore
  again), an empty location starts empty, wrong credentials stop the start with `SignatureDoesNotMatch` in the log.

### Decisions
- **No AKS preset.** The obvious one, the app routing add-on, is NGINX-based and supported until **November 2026**
  (Microsoft's own caution on the page). Its Gateway API successor is not in azurerm 5.12 (`web_app_routing` and the
  Application Gateway controller only). Building on a component retired this month was worse than keeping
  `helm_values` for AKS.
- **`exposure` covers the console's address only.** Private endpoints for Key Vault, Secret Manager, Secrets Manager
  and the buckets would stop Terraform, run from outside the network, from writing the secrets. A VM is refused:
  without a public address it needs NAT for its egress and a way in that the module cannot know.
- **GKE private needs the operator's TLS secret**: Google-managed certificates are external only.
- **The examples keep `:latest`.** Pinning to a version that does not exist yet breaks them until the release; it is a
  TODO for the first release containing this work.
- **The cost table came from a research subagent**, prices checked on 2026-10-31 from the providers' price APIs and
  pages; the figures it could not confirm from an official page are named in §10.

---

## Knowledge Captured
- **`terraform validate` does not check the values a platform API refuses.** Container Apps allows a startup probe
  `failureThreshold` of at most 10 (the module had 60); Cloud Run allows `period × failures` of at most 240 s (the
  module had 300). Neither provider validates them. The plan tests now pin each platform's limit.
- **`terraform test` with `mock_provider { override_during = plan }`** plans a module offline with every computed value
  known. Gotchas:
  - the azurerm mock still parses resource ids, so each mocked resource referenced by `.id` needs a well-formed
    ARM id (`/subscriptions/…/resourceGroups/…/providers/…`);
  - a mock's `defaults` and an `override_resource` cannot fill a block the configuration does not declare (GKE's
    `master_auth`) — declare the block;
  - for a declared single block, the override value is an **object**, not a one-element list (`expected an object type
    for attribute ".master_auth[0]" … but found tuple`);
  - `random_*` must be mocked too, or anything derived from a random suffix is unknown at plan time;
  - a failing run skips every later run in the file, so read the first failure first.
- **Helm templates:** `default dict .X` as an argument is fragile — build the map with `dict` + `range` + `set`.
- **EKS Auto Mode picks subnets for an internal ALB by `kubernetes.io/role/internal-elb`**; a public subnet can carry
  both tags.
- **An internal Container Apps environment's default domain does not resolve anywhere** until a private DNS zone of that
  name, linked to the VNet, has `*` → the environment's static IP. In azurerm 5.x records take `private_dns_zone_id`,
  not a zone name and resource group.
- **This machine now has `terraform` (1.16) and can reach the registry**, so `terraform init`/`providers schema -json`
  work offline afterwards; running a downloaded `helm` binary is not allowed here, so the chart is checked by CI.

---

## Files Changed
| File | Change summary |
|---|---|
| `web/api/app/{cloud,main,settings,console_file}.py`, `web/api/tests/test_cloud.py` | first-start restore (+3 tests) |
| `deploy/helm/ekos/{values.yaml,templates/ingress.yaml,templates/deployment.yaml}` | presets, `startupSeconds` |
| `deploy/terraform/modules/{azure,gcp,aws}/*.tf` | `exposure`, presets, probe limits, `master_auth`, subnet tags |
| `deploy/terraform/modules/{azure,gcp,aws}/tests/plan.tftest.hcl` | new: mocked plans |
| `deploy/terraform/.gitignore` | new: provider caches, lock files, state |
| `deploy/terraform/README.md` | `terraform test` |
| `.github/workflows/{ci,release}.yml` | terraform test, helm presets; image smoke checks |
| `ekos/docs/rfcs/0232-cloud-deployment.md` | §4, §5, §6 amendment |
| `docs/deployment.md`, `README.md`, `TODO.md`, `docs/generated/ekos-self-documentation.html`, `CLAUDE.md` | docs |
