# Plans of the AWS module with mocked providers (RFC 0232): no account, no network. They check the
# module's own logic — which resources each compute creates, the ingress preset, the private
# exposure, the preconditions — not what AWS would accept, which only an apply shows
# (docs/deployment.md §10).

mock_provider "aws" {
  override_during = plan
  mock_data "aws_availability_zones" {
    defaults = { names = ["eu-west-1a", "eu-west-1b", "eu-west-1c"] }
  }
  mock_data "aws_caller_identity" {
    defaults = { account_id = "123456789012" }
  }
  mock_data "aws_partition" {
    defaults = { partition = "aws" }
  }
  mock_data "aws_ami" {
    defaults = { id = "ami-0123456789abcdef0" }
  }
  mock_resource "aws_iam_role" {
    defaults = { arn = "arn:aws:iam::123456789012:role/ekos-runtime" }
  }
  mock_resource "aws_secretsmanager_secret" {
    defaults = { arn = "arn:aws:secretsmanager:eu-west-1:123456789012:secret:ekos/X-AbCdEf" }
  }
  mock_resource "aws_lb" {
    defaults = {
      arn      = "arn:aws:elasticloadbalancing:eu-west-1:123456789012:loadbalancer/app/ekos/0123456789abcdef"
      dns_name = "ekos-1234.eu-west-1.elb.amazonaws.com"
    }
  }
  mock_resource "aws_lb_target_group" {
    defaults = { arn = "arn:aws:elasticloadbalancing:eu-west-1:123456789012:targetgroup/ekos/0123456789abcdef" }
  }
  mock_resource "aws_eks_cluster" {
    defaults = {
      endpoint              = "https://ABCDEF.gr7.eu-west-1.eks.amazonaws.com"
      certificate_authority = [{ data = "bW9jaw==" }]
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
  name   = "ekos"
  region = "eu-west-1"
}

run "vm_by_default" {
  command = plan

  assert {
    condition     = length(aws_instance.vm) == 1 && length(aws_ecs_service.this) == 0 && length(aws_eks_cluster.this) == 0
    error_message = "compute = vm creates the instance only."
  }
  assert {
    condition     = module.common.console_env["EKOS_CONSOLE_INSTANCE_LEASE"] == "false"
    error_message = "a VM does not need the instance lease."
  }
  assert {
    condition     = startswith(output.backup_url, "s3://") && module.common.console_env["EKOS_CONSOLE_BACKUP_URL"] == output.backup_url
    error_message = "the console backs up to the module's bucket."
  }
}

run "ecs_needs_a_domain_and_certificate" {
  command = plan
  variables {
    compute = "container"
  }
  expect_failures = [aws_lb_listener.https]
}

run "ecs_is_internet_facing_by_default" {
  command = plan
  variables {
    compute         = "container"
    domain          = "ekos.example.com"
    certificate_arn = "arn:aws:acm:eu-west-1:123456789012:certificate/00000000-0000-0000-0000-000000000000"
  }

  assert {
    condition     = aws_lb.this[0].internal == false && output.url == "https://ekos.example.com"
    error_message = "public by default, at the domain."
  }
  assert {
    condition     = module.common.console_env["EKOS_CONSOLE_INSTANCE_LEASE"] == "true"
    error_message = "EFS is a network share: the lease is on."
  }
  # ECS refuses a health check start period over 300 s.
  assert {
    condition     = jsondecode(aws_ecs_task_definition.this[0].container_definitions)[0].healthCheck.startPeriod <= 300
    error_message = "the start period is at most 300 s."
  }
}

run "ecs_private_is_an_internal_load_balancer" {
  command = plan
  variables {
    compute         = "container"
    exposure        = "private"
    domain          = "ekos.internal.example.com"
    certificate_arn = "arn:aws:acm:eu-west-1:123456789012:certificate/00000000-0000-0000-0000-000000000000"
  }

  assert {
    condition     = aws_lb.this[0].internal == true
    error_message = "private: an internal load balancer."
  }
}

run "private_is_refused_on_a_vm" {
  command = plan
  variables {
    exposure = "private"
  }
  expect_failures = [var.exposure]
}

run "eks_without_a_domain_has_no_ingress" {
  command = plan
  variables {
    compute = "kubernetes"
  }

  assert {
    condition     = yamldecode(helm_release.ekos[0].values[0]).ingress.enabled == false && yamldecode(helm_release.ekos[0].values[0]).ingress.eksAlb.enabled == false
    error_message = "no ingress without a domain."
  }
  assert {
    condition     = output.url == "http://127.0.0.1:8000"
    error_message = "no domain: port-forward."
  }
}

run "eks_with_a_domain_needs_a_certificate_or_its_own_ingress" {
  command = plan
  variables {
    compute = "kubernetes"
    domain  = "ekos.example.com"
  }
  expect_failures = [helm_release.ekos]
}

run "eks_with_a_domain_and_certificate_gets_the_auto_mode_alb" {
  command = plan
  variables {
    compute         = "kubernetes"
    domain          = "ekos.example.com"
    certificate_arn = "arn:aws:acm:eu-west-1:123456789012:certificate/00000000-0000-0000-0000-000000000000"
  }

  assert {
    condition     = yamldecode(helm_release.ekos[0].values[0]).ingress.eksAlb.enabled == true && yamldecode(helm_release.ekos[0].values[0]).ingress.eksAlb.scheme == "internet-facing"
    error_message = "a public ALB."
  }
  assert {
    condition     = yamldecode(helm_release.ekos[0].values[0]).ingress.eksAlb.certificateArn == "arn:aws:acm:eu-west-1:123456789012:certificate/00000000-0000-0000-0000-000000000000"
    error_message = "the ACM certificate is on the ALB."
  }
  # Auto Mode finds the ALB's subnets by these tags.
  assert {
    condition     = alltrue([for s in aws_subnet.public : s.tags["kubernetes.io/role/elb"] == "1" && s.tags["kubernetes.io/role/internal-elb"] == "1"])
    error_message = "the subnets are tagged for public and internal load balancers."
  }
}

run "eks_private_is_an_internal_alb" {
  command = plan
  variables {
    compute         = "kubernetes"
    exposure        = "private"
    domain          = "ekos.internal.example.com"
    certificate_arn = "arn:aws:acm:eu-west-1:123456789012:certificate/00000000-0000-0000-0000-000000000000"
  }

  assert {
    condition     = yamldecode(helm_release.ekos[0].values[0]).ingress.eksAlb.scheme == "internal"
    error_message = "private: an internal ALB."
  }
}

run "eks_operator_ingress_without_a_certificate_is_accepted" {
  command = plan
  variables {
    compute     = "kubernetes"
    domain      = "ekos.example.com"
    helm_values = ["ingress: {className: nginx, tlsSecretName: ekos-tls}"]
  }

  assert {
    condition     = yamldecode(helm_release.ekos[0].values[0]).ingress.eksAlb.enabled == false
    error_message = "no certificate: the preset stays off and the operator's ingress is used."
  }
}
