# compute = "container": ECS on Fargate behind an Application Load Balancer (RFC 0232). One task:
# the console and the git sync. The EKOS home and the sources are EFS access points owned by uid
# 1000; secrets are Secrets Manager references injected by ECS. Deployments stop the old task
# before starting the new one (minimum healthy 0 %, maximum 100 %), so two consoles never share
# the home; the instance lease is a second guard.

locals {
  ecs = var.compute == "container" ? 1 : 0
}

resource "aws_cloudwatch_log_group" "ecs" {
  count             = local.ecs
  name              = "/ekos/${var.name}"
  retention_in_days = 30
  tags              = local.tags
}

resource "aws_ecs_cluster" "this" {
  count = local.ecs
  name  = var.name
  tags  = local.tags
}

# ECS itself reads the secrets to inject them, with the execution role.
resource "aws_iam_role" "execution" {
  count = local.ecs
  name  = "${var.name}-ecs-execution"
  tags  = local.tags
  assume_role_policy = jsonencode({
    Version   = "2012-10-17"
    Statement = [{ Effect = "Allow", Principal = { Service = "ecs-tasks.amazonaws.com" }, Action = "sts:AssumeRole" }]
  })
}

resource "aws_iam_role_policy_attachment" "execution" {
  count      = local.ecs
  role       = aws_iam_role.execution[0].name
  policy_arn = "arn:${data.aws_partition.current.partition}:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

resource "aws_iam_role_policy" "execution_secrets" {
  count = local.ecs
  name  = "ekos-secrets"
  role  = aws_iam_role.execution[0].id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["secretsmanager:GetSecretValue"]
      Resource = [for s in aws_secretsmanager_secret.this : s.arn]
    }]
  })
}

# ── storage ──────────────────────────────────────────────────────────────────

resource "aws_security_group" "efs" {
  count       = local.ecs
  name        = "${var.name}-efs"
  description = "EKOS EFS: NFS from the task only"
  vpc_id      = aws_vpc.this.id
  tags        = local.tags
  ingress {
    from_port       = 2049
    to_port         = 2049
    protocol        = "tcp"
    security_groups = [aws_security_group.task[0].id]
  }
}

resource "aws_efs_file_system" "this" {
  count          = local.ecs
  creation_token = "${var.name}-${local.suffix}"
  encrypted      = true
  tags           = merge(local.tags, { Name = "${var.name}-home" })
}

resource "aws_efs_mount_target" "this" {
  count           = local.ecs == 1 ? 2 : 0
  file_system_id  = aws_efs_file_system.this[0].id
  subnet_id       = aws_subnet.public[count.index].id
  security_groups = [aws_security_group.efs[0].id]
}

resource "aws_efs_access_point" "this" {
  for_each       = local.ecs == 1 ? toset(["home", "sources"]) : toset([])
  file_system_id = aws_efs_file_system.this[0].id
  posix_user {
    uid = 1000
    gid = 1000
  }
  root_directory {
    path = "/${each.key}"
    creation_info {
      owner_uid   = 1000
      owner_gid   = 1000
      permissions = "0750"
    }
  }
  tags = merge(local.tags, { Name = "${var.name}-${each.key}" })
}

# ── load balancer ────────────────────────────────────────────────────────────

resource "aws_security_group" "alb" {
  count       = local.ecs
  name        = "${var.name}-alb"
  description = "EKOS load balancer: HTTPS from the allowed addresses"
  vpc_id      = aws_vpc.this.id
  tags        = local.tags
  ingress {
    from_port   = 80
    to_port     = 80
    protocol    = "tcp"
    cidr_blocks = var.allowed_cidrs
  }
  ingress {
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = var.allowed_cidrs
  }
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_security_group" "task" {
  count       = local.ecs
  name        = "${var.name}-task"
  description = "EKOS task: the console port from the load balancer only"
  vpc_id      = aws_vpc.this.id
  tags        = local.tags
  ingress {
    from_port       = 8000
    to_port         = 8000
    protocol        = "tcp"
    security_groups = [aws_security_group.alb[0].id]
  }
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_lb" "this" {
  count              = local.ecs
  name               = "${var.name}-${local.suffix}"
  load_balancer_type = "application"
  security_groups    = [aws_security_group.alb[0].id]
  subnets            = aws_subnet.public[*].id
  # Server-sent events from long pipeline runs.
  idle_timeout = 3600
  tags         = local.tags
}

resource "aws_lb_target_group" "this" {
  count       = local.ecs
  name        = "${var.name}-${local.suffix}"
  port        = 8000
  protocol    = "HTTP"
  target_type = "ip"
  vpc_id      = aws_vpc.this.id
  # One task; let it go quickly on a deployment.
  deregistration_delay = 15
  health_check {
    path                = "/api/health"
    interval            = 15
    healthy_threshold   = 2
    unhealthy_threshold = 4
  }
  tags = local.tags
}

resource "aws_lb_listener" "https" {
  count             = local.ecs
  load_balancer_arn = aws_lb.this[0].arn
  port              = 443
  protocol          = "HTTPS"
  ssl_policy        = "ELBSecurityPolicy-TLS13-1-2-2021-06"
  certificate_arn   = var.certificate_arn
  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.this[0].arn
  }

  lifecycle {
    precondition {
      condition     = var.domain != "" && var.certificate_arn != ""
      error_message = "compute = \"container\" on AWS needs domain and certificate_arn (an ACM certificate for it)."
    }
  }
}

resource "aws_lb_listener" "http" {
  count             = local.ecs
  load_balancer_arn = aws_lb.this[0].arn
  port              = 80
  protocol          = "HTTP"
  default_action {
    type = "redirect"
    redirect {
      port        = "443"
      protocol    = "HTTPS"
      status_code = "HTTP_301"
    }
  }
}

# ── the task ─────────────────────────────────────────────────────────────────

locals {
  ecs_log = local.ecs == 1 ? {
    logDriver = "awslogs"
    options = {
      awslogs-group         = aws_cloudwatch_log_group.ecs[0].name
      awslogs-region        = var.region
      awslogs-stream-prefix = "ekos"
    }
  } : null

  ecs_containers = concat(
    [{
      name         = "ekos"
      image        = var.image
      essential    = true
      portMappings = [{ containerPort = 8000, protocol = "tcp" }]
      environment  = [for k, v in module.common.console_env : { name = k, value = v }]
      secrets      = [for n in module.common.console_secret_names : { name = n, valueFrom = aws_secretsmanager_secret.this[n].arn }]
      mountPoints = [
        { sourceVolume = "home", containerPath = "/var/lib/ekos", readOnly = false },
        { sourceVolume = "sources", containerPath = "/sources", readOnly = true },
      ]
      healthCheck = {
        command     = ["CMD", "python", "-c", "import urllib.request,sys; sys.exit(0 if urllib.request.urlopen('http://127.0.0.1:8000/api/health', timeout=4).status == 200 else 1)"]
        interval    = 30
        timeout     = 5
        retries     = 3
        startPeriod = 120
      }
      logConfiguration = local.ecs_log
    }],
    module.common.git_sync_enabled ? [{
      name        = "git-sync"
      image       = var.image
      essential   = false
      command     = ["ekos-git-sync"]
      environment = [for k, v in merge(module.common.git_sync_env, { EKOS_GIT_SYNC_DEST = "/sources" }) : { name = k, value = v }]
      secrets     = [for n in module.common.git_sync_secret_names : { name = n, valueFrom = aws_secretsmanager_secret.this[n].arn }]
      mountPoints = [
        { sourceVolume = "sources", containerPath = "/sources", readOnly = false },
      ]
      logConfiguration = local.ecs_log
    }] : [],
  )
}

resource "aws_ecs_task_definition" "this" {
  count                    = local.ecs
  family                   = var.name
  requires_compatibilities = ["FARGATE"]
  network_mode             = "awsvpc"
  cpu                      = var.container_cpu
  memory                   = var.container_memory
  execution_role_arn       = aws_iam_role.execution[0].arn
  task_role_arn            = aws_iam_role.runtime.arn
  container_definitions    = jsonencode(local.ecs_containers)
  tags                     = local.tags

  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "X86_64"
  }

  dynamic "volume" {
    for_each = aws_efs_access_point.this
    content {
      name = volume.key
      efs_volume_configuration {
        file_system_id     = aws_efs_file_system.this[0].id
        transit_encryption = "ENABLED"
        authorization_config {
          access_point_id = volume.value.id
          iam             = "DISABLED"
        }
      }
    }
  }
}

resource "aws_ecs_service" "this" {
  count           = local.ecs
  name            = var.name
  cluster         = aws_ecs_cluster.this[0].id
  task_definition = aws_ecs_task_definition.this[0].arn
  desired_count   = 1
  launch_type     = "FARGATE"
  # Stop the old task first: never two consoles over one home.
  deployment_minimum_healthy_percent = 0
  deployment_maximum_percent         = 100
  health_check_grace_period_seconds  = 180
  tags                               = local.tags

  network_configuration {
    subnets          = aws_subnet.public[*].id
    security_groups  = [aws_security_group.task[0].id]
    assign_public_ip = true
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.this[0].arn
    container_name   = "ekos"
    container_port   = 8000
  }

  depends_on = [aws_lb_listener.https, aws_efs_mount_target.this, aws_iam_role_policy.runtime]
}
