# EKOS in a cloud — Terraform (RFC 0232)

```
modules/
  common/   settings every deployment shares; generated secrets (no cloud resources)
  azure/    Key Vault, managed identity, Blob backups + vm | container | kubernetes
  gcp/      Secret Manager, service account, GCS backups + vm | container | kubernetes
  aws/      Secrets Manager, IAM role, S3 backups + vm | container | kubernetes
examples/
  azure/ gcp/ aws/   complete root modules: copy terraform.tfvars.example, apply
templates/  cloud-init, Compose, Caddy and the secret-fetch script for the VMs
```

| `compute` | Azure | Google Cloud | AWS |
|---|---|---|---|
| `vm` (default, cheapest) | Ubuntu VM, managed disk | Compute Engine, persistent disk | EC2, EBS |
| `container` | Container Apps, Azure Files NFS | Cloud Run, Filestore | ECS Fargate, ALB, EFS |
| `kubernetes` | AKS, Key Vault CSI add-on | GKE Autopilot, Secret Manager add-on | EKS Auto Mode, Secrets Store CSI |

```bash
cd examples/aws
cp terraform.tfvars.example terraform.tfvars
export TF_VAR_secrets='{"OPENAI_API_KEY":"sk-…"}' TF_VAR_git_token=ghp_… TF_VAR_oidc_client_secret=…
terraform init && terraform apply
terraform output url
```

Secrets passed this way end up in Terraform's state (as with any secret Terraform writes); keep the
state in an encrypted remote backend. The values go to the cloud's secret store, and the console
reads them from there.

Whoever applies needs to create role assignments (Azure: Owner or User Access Administrator on
the subscription; GCP: IAM admin on the project; AWS: IAM). On Azure the example sets
`storage_use_azuread = true`: the backups account has shared keys off.

Every deployment runs one console (the EKOS home is a single-writer SQLite + ledger store), a git
sync beside it when `git_repos` is set, and a nightly `ekos home backup --to` the deployment's
bucket. The operator's guide is [docs/deployment.md](../../docs/deployment.md#10-in-a-cloud-azure-google-cloud-aws-rfc-0232);
the design is [RFC 0232](../../ekos/docs/rfcs/0232-cloud-deployment.md).

CI runs `terraform fmt -check` and `terraform validate` on every module and example. None of these
was applied against a real account as part of RFC 0232: try a `vm` deployment first.
