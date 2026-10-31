# Devlog 315 — The first real cloud deployment: Azure VM through Azure Pipelines, demo, teardown (RFC 0232)

**Date:** 2026-10-31
**PRs:** #21 (EKOS changes); private repository `inbox0216/ekos-deploy` (Azure DevOps) for the deployment
**Branch:** `rfc-0232-cloud-deployment` → `main` (open)

---

## Summary

RFC 0232's Azure module was applied to a real subscription for the first time: `compute = "vm"` in
germanywestcentral, through Azure Pipelines with a federated service connection, the image built from PR #21 into a
private ACR. Everything the RFC promises worked on the real cloud: HTTPS from Let's Encrypt, Key Vault secrets in
tmpfs, the registry signed in with the VM's identity, git sync, a project compiled from the console, a backup to Blob
Storage in 4 s and a **restore on first start in 12 s**. Then the deployment and every shared piece were destroyed and
checked gone. The run found two EKOS bugs (fixed), one performance limit (TODO) and six facts about Azure that the
offline checks could not.

---

## The deployment project (private)

### What was built
| Component | What |
|---|---|
| `config.env` | every name: subscription, DevOps, state account, registry, the EKOS commit |
| `bootstrap/bootstrap.sh` | state storage (no shared keys), ACR Basic, app registration + service principal + roles, the WIF service connection, then the federated credential copied from what the connection reports |
| `bootstrap/teardown.sh` | refuses while the deployment exists; credentials → app → connection; the region's network watcher; the shared group |
| `infra/` | root module: EKOS's `modules/azure` by git, pinned to a commit; azurerm backend with Entra ID |
| `pipelines/{image,deploy,destroy}.yml` + `templates/terraform.yml` | ACR Tasks build; plan → apply (deployment job on environment `ekos-demo`); destroy; Terraform via `ARM_USE_OIDC` |
| `demo/` | the record (timeline, outputs, 18 screenshots, findings) and `screenshots.py` |

### EKOS changes (PR #21)
| Change | Why |
|---|---|
| `acr_id` on the Azure module | a private registry: AcrPull for the identity (and AKS's kubelet), `docker login` with an ACR refresh token exchanged from the VM's identity token at each start, Container Apps `registry { identity }`; 3 new plan tests |
| git-sync `healthcheck: disable` in the VM's Compose file | it inherited the image's `HEALTHCHECK` (the console's port) and showed *unhealthy* |
| `home_size_gb` description | a 32 GiB premium disk is a P4 (120 IOPS) |
| `docs/deployment.md` §10, TODO | what was applied, `acr_id`, the disk lesson |

---

## Knowledge Captured
- **A subscription can be refused a whole region**: westeurope answered `RequestDisallowedByAzure` — *not accepting
  new customers*. Try the region the subscription already uses.
- **Quotas are per VM family and region, and new pay-as-you-go subscriptions get 0 for many families**: here Dsv5 (the
  module's default), Bsv2/Basv2 (the only B-series offered in westeurope) were all 0. `az vm list-usage` plus
  `az vm list-skus --size` (restrictions per zone) before choosing a size; a zone restriction does not stop a regional VM.
- **Dasv6/Dsv6 sizes have NVMe disks**; the module finds its data disk by the SCSI path (`/dev/disk/azure/scsi1/lun0`),
  so pick a v4/v5 size until it handles NVMe.
- **Azure DevOps service connections created now present an Entra issuer and an `/eid1/c/pub/t/…/sc/<org>/<id>`
  subject**, not `https://vstoken.dev.azure.com/<org>` + `sc://org/project/name` (`AADSTS700211`). Create the
  connection, read `authorization.parameters.workloadIdentityFederation{Issuer,Subject}`, then the credential.
- **A deleted app keeps its federated credentials in Entra's recycle bin for 30 days**, and Azure DevOps will not
  delete a connection an app still trusts. Delete the credentials before the app (or purge through
  `DELETE /directory/deletedItems/<id>`).
- **A pipeline needs a one-time permission to use a new environment** (*Checkpoint.Authorization*):
  `PATCH _apis/pipelines/pipelinePermissions/environment/<id>`.
- **`az acr build --no-wait` still streams the log**; that stream hung for an hour after a 23-minute build and held
  the organisation's only hosted job. Use `--no-logs --no-wait` and poll `az acr task show-run`. (`list-runs` showed the
  run as *Running* long after `show-run` said *Succeeded*.)
- **ACR Tasks built EKOS in 23 minutes**, inside the hosted job's hour; a local build is faster on a big machine.
- **Commit on a network disk is write-latency-bound**: 142k objects took 48½ min on a `D2as_v4`, ~49 % I/O pressure
  at P4 and still at P20 (2,300 IOPS) — IOPS were not the limit. The objects were nearly all `TransformNode`s from
  Sakila's one-`INSERT`-per-row data scripts. Exclude data dumps; batching the fact writes is a TODO.
- **The image's `HEALTHCHECK` is inherited by every container started from it under Compose** (ECS, Container Apps
  and Kubernetes ignore it). A second service from the same image needs its own health check or none.
- **Azure creates `NetworkWatcherRG` with the first VNet in a region**; `terraform destroy` leaves it.
- **The auto-mode classifier stopped the bootstrap** (subscription-wide role grants to a new principal) and merging a
  PR; the user ran the bootstrap with `!`.

---

## Files Changed
| File | Change summary |
|---|---|
| `deploy/terraform/modules/azure/{main,variables,vm,container,kubernetes}.tf`, `templates/fetch-secrets.sh.tftpl`, `modules/{gcp,aws}/vm.tf` | `acr_id` |
| `deploy/terraform/modules/azure/tests/plan.tftest.hcl` | 3 runs for the registry |
| `deploy/terraform/templates/docker-compose.vm.yml` | git-sync health check off |
| `docs/deployment.md`, `TODO.md`, `CLAUDE.md` | the real run, `acr_id`, the disk lesson |
| *(private)* `inbox0216/ekos-deploy` | the deployment project and the demo record |
