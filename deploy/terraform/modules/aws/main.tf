# EKOS on AWS (RFC 0232): an IAM role, Secrets Manager, backups in S3, a VPC, and one of three
# computes (vm.tf, container.tf, kubernetes.tf).

data "aws_caller_identity" "current" {}
data "aws_partition" "current" {}

data "aws_availability_zones" "available" {
  state = "available"
}

resource "random_string" "suffix" {
  length  = 6
  special = false
  upper   = false
}

locals {
  suffix = random_string.suffix.result
  tags   = merge({ app = "ekos", managed-by = "terraform" }, var.tags)
  azs    = slice(data.aws_availability_zones.available.names, 0, 2)

  public_url = {
    vm         = "https://${local.vm_host}"
    container  = "https://${var.domain}"
    kubernetes = var.domain != "" ? "https://${var.domain}" : "http://127.0.0.1:8000"
  }[var.compute]

  backup_url = var.backup_cron == "" ? "" : "s3://${aws_s3_bucket.backups.bucket}/ekos/"

  console_secret_names  = { for n in module.common.console_secret_names : n => "${var.name}/${n}" }
  git_sync_secret_names = { for n in module.common.git_sync_secret_names : n => "${var.name}/${n}" }
  all_secret_names      = merge(local.console_secret_names, local.git_sync_secret_names)
}

module "common" {
  source = "../common"

  public_url         = local.public_url
  oidc               = var.oidc
  oidc_client_secret = var.oidc_client_secret
  owners             = var.owners
  instance_lease     = var.compute != "vm"
  backup_url         = local.backup_url
  backup_cron        = var.backup_cron
  # object_store needs the bucket's region; credentials come from the role.
  env               = merge({ AWS_REGION = var.region }, var.env)
  secrets           = var.secrets
  git_repos         = var.git_repos
  git_token         = var.git_token
  git_username      = var.git_username
  git_sync_interval = var.git_sync_interval
}

# ── Secrets Manager ──────────────────────────────────────────────────────────

resource "aws_secretsmanager_secret" "this" {
  for_each                = local.all_secret_names
  name                    = each.value
  description             = "EKOS: ${each.key}"
  recovery_window_in_days = 7
  tags                    = local.tags
}

resource "aws_secretsmanager_secret_version" "this" {
  for_each      = local.all_secret_names
  secret_id     = aws_secretsmanager_secret.this[each.key].id
  secret_string = contains(keys(local.console_secret_names), each.key) ? module.common.console_secrets[each.key] : module.common.git_sync_secrets[each.key]
}

# ── backups ──────────────────────────────────────────────────────────────────

resource "aws_s3_bucket" "backups" {
  bucket = "${var.name}-backups-${local.suffix}"
  tags   = local.tags
}

resource "aws_s3_bucket_public_access_block" "backups" {
  bucket                  = aws_s3_bucket.backups.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_lifecycle_configuration" "backups" {
  bucket = aws_s3_bucket.backups.id
  rule {
    id     = "expire-ekos-backups"
    status = "Enabled"
    filter {
      prefix = "ekos/"
    }
    expiration {
      days = var.backup_retention_days
    }
    abort_incomplete_multipart_upload {
      days_after_initiation = 2
    }
  }
}

# ── the runtime role ─────────────────────────────────────────────────────────
# One role for the console wherever it runs: read its own secrets, write backups. The trust
# policy names the one service the chosen compute uses.

locals {
  runtime_principal = {
    vm         = "ec2.amazonaws.com"
    container  = "ecs-tasks.amazonaws.com"
    kubernetes = "pods.eks.amazonaws.com"
  }[var.compute]
}

resource "aws_iam_role" "runtime" {
  name = "${var.name}-runtime"
  tags = local.tags
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = local.runtime_principal }
      Action    = var.compute == "kubernetes" ? ["sts:AssumeRole", "sts:TagSession"] : ["sts:AssumeRole"]
    }]
  })
}

resource "aws_iam_role_policy" "runtime" {
  name = "ekos"
  role = aws_iam_role.runtime.id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = concat(
      length(local.all_secret_names) == 0 ? [] : [{
        Sid      = "OwnSecrets"
        Effect   = "Allow"
        Action   = ["secretsmanager:GetSecretValue", "secretsmanager:DescribeSecret"]
        Resource = [for s in aws_secretsmanager_secret.this : s.arn]
      }],
      [
        {
          Sid       = "ListBackups"
          Effect    = "Allow"
          Action    = ["s3:ListBucket"]
          Resource  = aws_s3_bucket.backups.arn
          Condition = { StringLike = { "s3:prefix" = ["ekos/*", "ekos/"] } }
        },
        {
          Sid      = "WriteBackups"
          Effect   = "Allow"
          Action   = ["s3:GetObject", "s3:PutObject", "s3:DeleteObject", "s3:AbortMultipartUpload", "s3:ListMultipartUploadParts"]
          Resource = "${aws_s3_bucket.backups.arn}/ekos/*"
        },
      ],
    )
  })
}

# ── network ──────────────────────────────────────────────────────────────────
# Two public subnets: enough for a load balancer and an EKS cluster, no NAT gateway to pay for.
# Security groups admit traffic only from the load balancer (or the allowed addresses on a VM).

resource "aws_vpc" "this" {
  cidr_block           = var.address_space
  enable_dns_support   = true
  enable_dns_hostnames = true
  tags                 = merge(local.tags, { Name = "${var.name}-vpc" })
}

resource "aws_internet_gateway" "this" {
  vpc_id = aws_vpc.this.id
  tags   = merge(local.tags, { Name = "${var.name}-igw" })
}

resource "aws_subnet" "public" {
  count                   = 2
  vpc_id                  = aws_vpc.this.id
  availability_zone       = local.azs[count.index]
  cidr_block              = cidrsubnet(var.address_space, 4, count.index)
  map_public_ip_on_launch = true
  tags = merge(local.tags, {
    Name                     = "${var.name}-public-${count.index}"
    "kubernetes.io/role/elb" = "1"
  })
}

resource "aws_route_table" "public" {
  vpc_id = aws_vpc.this.id
  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.this.id
  }
  tags = merge(local.tags, { Name = "${var.name}-public" })
}

resource "aws_route_table_association" "public" {
  count          = 2
  subnet_id      = aws_subnet.public[count.index].id
  route_table_id = aws_route_table.public.id
}
