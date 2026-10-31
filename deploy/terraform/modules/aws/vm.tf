# compute = "vm": one EC2 instance running the image with Docker Compose (RFC 0232). The EKOS
# home and the sources are on an EBS volume that outlives the instance; secrets are read from
# Secrets Manager with the instance role at every start; Caddy serves HTTPS. Shell access is SSM
# Session Manager unless an SSH key is given.

locals {
  vm      = var.compute == "vm" ? 1 : 0
  vm_ip   = try(aws_eip.vm[0].public_ip, "")
  vm_host = var.domain != "" ? var.domain : "${local.vm_ip}.sslip.io"

  vm_secrets = concat(
    [for n, s in local.console_secret_names : { group = "console", name = n, ref = s }],
    [for n, s in local.git_sync_secret_names : { group = "git-sync", name = n, ref = s }],
  )
}

data "aws_ami" "ubuntu" {
  count       = local.vm
  most_recent = true
  owners      = ["099720109477"] # Canonical
  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd-gp3/ubuntu-noble-24.04-amd64-server-*"]
  }
}

resource "aws_iam_instance_profile" "vm" {
  count = local.vm
  name  = "${var.name}-vm"
  role  = aws_iam_role.runtime.name
}

resource "aws_iam_role_policy_attachment" "ssm" {
  count      = local.vm
  role       = aws_iam_role.runtime.name
  policy_arn = "arn:${data.aws_partition.current.partition}:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_security_group" "vm" {
  count       = local.vm
  name        = "${var.name}-vm"
  description = "EKOS VM: HTTPS from the allowed addresses, SSH from the admin addresses"
  vpc_id      = aws_vpc.this.id
  tags        = local.tags

  ingress {
    description = "HTTP (certificate challenge, redirect)"
    from_port   = 80
    to_port     = 80
    protocol    = "tcp"
    cidr_blocks = var.allowed_cidrs
  }

  ingress {
    description = "HTTPS"
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = var.allowed_cidrs
  }

  dynamic "ingress" {
    for_each = length(var.admin_cidrs) > 0 ? [1] : []
    content {
      description = "SSH"
      from_port   = 22
      to_port     = 22
      protocol    = "tcp"
      cidr_blocks = var.admin_cidrs
    }
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_eip" "vm" {
  count  = local.vm
  domain = "vpc"
  tags   = merge(local.tags, { Name = "${var.name}-vm" })
}

resource "aws_ebs_volume" "home" {
  count             = local.vm
  availability_zone = local.azs[0]
  size              = var.home_size_gb
  type              = "gp3"
  encrypted         = true
  tags              = merge(local.tags, { Name = "${var.name}-home" })
}

resource "aws_instance" "vm" {
  count                  = local.vm
  ami                    = data.aws_ami.ubuntu[0].id
  instance_type          = var.instance_type
  subnet_id              = aws_subnet.public[0].id
  vpc_security_group_ids = [aws_security_group.vm[0].id]
  iam_instance_profile   = aws_iam_instance_profile.vm[0].name
  key_name               = var.ssh_key_name != "" ? var.ssh_key_name : null
  tags                   = merge(local.tags, { Name = "${var.name}-vm" })

  root_block_device {
    volume_size = 50
    volume_type = "gp3"
    encrypted   = true
  }

  # IMDSv2 only; two hops so the containers (backups) reach it through Docker's bridge.
  metadata_options {
    http_tokens                 = "required"
    http_put_response_hop_limit = 2
  }

  user_data_replace_on_change = true
  user_data = templatefile("${path.module}/../../templates/cloud-init.yaml.tftpl", {
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
      # Nitro instances expose EBS as NVMe; the by-id name carries the volume id without its dash.
      device = "/dev/disk/by-id/nvme-Amazon_Elastic_Block_Store_${replace(aws_ebs_volume.home[0].id, "-", "")}"
    })
    fetch_secrets = templatefile("${path.module}/../../templates/fetch-secrets.sh.tftpl", {
      cloud           = "aws"
      secrets         = local.vm_secrets
      azure_client_id = ""
      azure_vault_uri = ""
      azure_registry  = ""
      aws_region      = var.region
    })
  })

  depends_on = [aws_secretsmanager_secret_version.this, aws_iam_role_policy.runtime]
}

resource "aws_volume_attachment" "home" {
  count       = local.vm
  device_name = "/dev/sdf"
  volume_id   = aws_ebs_volume.home[0].id
  instance_id = aws_instance.vm[0].id
}

resource "aws_eip_association" "vm" {
  count         = local.vm
  instance_id   = aws_instance.vm[0].id
  allocation_id = aws_eip.vm[0].id
}
