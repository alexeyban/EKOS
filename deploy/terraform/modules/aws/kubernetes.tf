# compute = "kubernetes": EKS Auto Mode + the EKOS Helm chart (RFC 0232). One pod with the
# console and the git sync over one ReadWriteOnce EBS volume; secrets come from Secrets Manager
# through the Secrets Store CSI driver and its AWS provider (installed here), authenticated by EKS
# Pod Identity: the pod's service account is associated with the runtime role, no keys.
#
# The helm provider is configured by the caller from this module's `kubernetes` output (see
# examples/aws).

locals {
  eks         = var.compute == "kubernetes" ? 1 : 0
  image_parts = regex("^(.*?)(?::([^:/]+))?$", var.image)
  eks_policy  = "arn:${data.aws_partition.current.partition}:iam::aws:policy"

  eks_csi = {
    for group, names in { console = local.console_secret_names, gitSync = local.git_sync_secret_names } : group => {
      enabled                     = length(names) > 0
      provider                    = "aws"
      parameters = {
        usePodIdentity = "true"
        region         = var.region
        objects = yamlencode([for env_name, secret_name in names : {
          objectName  = aws_secretsmanager_secret.this[env_name].arn
          objectType  = "secretsmanager"
          objectAlias = env_name
        }])
      }
    }
  }
}

resource "aws_iam_role" "eks_cluster" {
  count = local.eks
  name  = "${var.name}-eks-cluster"
  tags  = local.tags
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "eks.amazonaws.com" }
      Action    = ["sts:AssumeRole", "sts:TagSession"]
    }]
  })
}

resource "aws_iam_role_policy_attachment" "eks_cluster" {
  for_each   = local.eks == 1 ? toset(["AmazonEKSClusterPolicy", "AmazonEKSComputePolicy", "AmazonEKSBlockStoragePolicy", "AmazonEKSLoadBalancingPolicy", "AmazonEKSNetworkingPolicy"]) : toset([])
  role       = aws_iam_role.eks_cluster[0].name
  policy_arn = "${local.eks_policy}/${each.value}"
}

resource "aws_iam_role" "eks_node" {
  count = local.eks
  name  = "${var.name}-eks-node"
  tags  = local.tags
  assume_role_policy = jsonencode({
    Version   = "2012-10-17"
    Statement = [{ Effect = "Allow", Principal = { Service = "ec2.amazonaws.com" }, Action = ["sts:AssumeRole"] }]
  })
}

resource "aws_iam_role_policy_attachment" "eks_node" {
  for_each   = local.eks == 1 ? toset(["AmazonEKSWorkerNodeMinimalPolicy", "AmazonEC2ContainerRegistryPullOnly"]) : toset([])
  role       = aws_iam_role.eks_node[0].name
  policy_arn = "${local.eks_policy}/${each.value}"
}

resource "aws_eks_cluster" "this" {
  count                         = local.eks
  name                          = var.name
  role_arn                      = aws_iam_role.eks_cluster[0].arn
  bootstrap_self_managed_addons = false
  tags                          = local.tags

  access_config {
    authentication_mode                         = "API"
    bootstrap_cluster_creator_admin_permissions = true
  }

  compute_config {
    enabled       = true
    node_pools    = ["general-purpose"]
    node_role_arn = aws_iam_role.eks_node[0].arn
  }

  kubernetes_network_config {
    elastic_load_balancing {
      enabled = true
    }
  }

  storage_config {
    block_storage {
      enabled = true
    }
  }

  vpc_config {
    endpoint_private_access = true
    endpoint_public_access  = true
    subnet_ids              = aws_subnet.public[*].id
  }

  depends_on = [aws_iam_role_policy_attachment.eks_cluster]
}

resource "aws_eks_pod_identity_association" "ekos" {
  count           = local.eks
  cluster_name    = aws_eks_cluster.this[0].name
  namespace       = var.kubernetes_namespace
  service_account = "ekos"
  role_arn        = aws_iam_role.runtime.arn
}

# The Secrets Store CSI driver and its AWS provider (not part of Auto Mode).
resource "helm_release" "csi_driver" {
  count      = local.eks
  name       = "secrets-store-csi-driver"
  repository = "https://kubernetes-sigs.github.io/secrets-store-csi-driver/charts"
  chart      = "secrets-store-csi-driver"
  namespace  = "kube-system"
  depends_on = [aws_eks_cluster.this]
}

resource "helm_release" "csi_provider_aws" {
  count      = local.eks
  name       = "secrets-store-csi-driver-provider-aws"
  repository = "https://aws.github.io/secrets-store-csi-driver-provider-aws"
  chart      = "secrets-store-csi-driver-provider-aws"
  namespace  = "kube-system"
  depends_on = [helm_release.csi_driver]
}

resource "helm_release" "ekos" {
  count            = local.eks
  name             = "ekos"
  chart            = "${path.module}/../../../helm/ekos"
  namespace        = var.kubernetes_namespace
  create_namespace = true
  timeout          = 1200

  values = concat([yamlencode({
    image = {
      repository = local.image_parts[0]
      tag        = coalesce(local.image_parts[1], "latest")
    }
    env = { for k, v in module.common.console_env : k => v if !contains(["EKOS_CONSOLE_INSTANCE_LEASE", "EKOS_CONSOLE_SUPERVISED"], k) }
    csi = local.eks_csi
    serviceAccount = {
      create = true
      name   = "ekos"
    }
    persistence = {
      storageClass = "ekos-gp3"
      size         = "${var.home_size_gb}Gi"
      createStorageClass = {
        enabled     = true
        provisioner = "ebs.csi.eks.amazonaws.com"
        parameters  = { type = "gp3", encrypted = "true" }
      }
    }
    gitSync = {
      enabled         = module.common.git_sync_enabled
      repos           = var.git_repos
      intervalSeconds = var.git_sync_interval
      username        = var.git_username
    }
    ingress = {
      enabled = var.domain != ""
      host    = var.domain
    }
  })], var.helm_values)

  depends_on = [
    aws_eks_pod_identity_association.ekos,
    helm_release.csi_provider_aws,
    aws_secretsmanager_secret_version.this,
    aws_iam_role_policy.runtime,
  ]
}
