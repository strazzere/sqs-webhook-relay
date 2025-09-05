terraform {
  required_version = ">= 1.6.0"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.40"
    }
  }
}

provider "aws" {
  region = var.region
}

# -----------------------------
# Variables
# -----------------------------
variable "project_name" {
  type    = string
  default = "webhook-demo"
}

variable "region" {
  type    = string
  default = "us-east-2"
}

variable "queue_name" {
  type    = string
  default = "webhook-demo-queue"
}

# -----------------------------
# Data
# -----------------------------
data "aws_caller_identity" "me" {}

# -----------------------------
# SQS queue
# -----------------------------
resource "aws_sqs_queue" "webhook" {
  name                       = var.queue_name
  message_retention_seconds  = 1209600
  visibility_timeout_seconds = 60
}

# -----------------------------
# IAM role: API Gateway -> SQS
# -----------------------------
resource "aws_iam_role" "apigw_sqs_role" {
  name = "${var.project_name}-apigw-sqs-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17",
    Statement = [{
      Effect    = "Allow",
      Principal = { Service = "apigateway.amazonaws.com" },
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy" "apigw_sqs_policy" {
  name = "${var.project_name}-apigw-sqs-policy"
  role = aws_iam_role.apigw_sqs_role.id

  policy = jsonencode({
    Version = "2012-10-17",
    Statement = [{
      Effect   = "Allow",
      Action   = ["sqs:SendMessage"],
      Resource = aws_sqs_queue.webhook.arn
    }]
  })
}

# -----------------------------
# API Gateway (REST API v1)
# -----------------------------
resource "aws_api_gateway_rest_api" "api" {
  name = "${var.project_name}-rest"
}

# /webhook resource
resource "aws_api_gateway_resource" "webhook" {
  rest_api_id = aws_api_gateway_rest_api.api.id
  parent_id   = aws_api_gateway_rest_api.api.root_resource_id
  path_part   = "webhook"
}

# Method: POST /webhook (no auth)
resource "aws_api_gateway_method" "post_webhook" {
  rest_api_id   = aws_api_gateway_rest_api.api.id
  resource_id   = aws_api_gateway_resource.webhook.id
  http_method   = "POST"
  authorization = "NONE"
}

# Integration: AWS (SQS SendMessage)
resource "aws_api_gateway_integration" "sqs" {
  rest_api_id = aws_api_gateway_rest_api.api.id
  resource_id = aws_api_gateway_resource.webhook.id
  http_method = aws_api_gateway_method.post_webhook.http_method

  type                    = "AWS"
  integration_http_method = "POST"
  credentials             = aws_iam_role.apigw_sqs_role.arn
  # arn:aws:apigateway:{region}:sqs:path/{account-id}/{queue-name}
  uri                  = "arn:aws:apigateway:${var.region}:sqs:path/${data.aws_caller_identity.me.account_id}/${aws_sqs_queue.webhook.name}"
  passthrough_behavior = "NEVER"

  request_parameters = {
    "integration.request.header.Content-Type" = "'application/x-www-form-urlencoded'"
  }

  # Add back header forwarding using safe string concatenation (no heredocs)
  request_templates = {
    "application/json" = "Action=SendMessage&MessageBody=$util.urlEncode($util.base64Encode($input.body))&MessageAttribute.1.Name=BodyIsBase64&MessageAttribute.1.Value.DataType=String&MessageAttribute.1.Value.StringValue=true&MessageAttribute.2.Name=Content-Type&MessageAttribute.2.Value.DataType=String&MessageAttribute.2.Value.StringValue=$util.urlEncode($input.params().header.get('Content-Type'))&MessageAttribute.3.Name=X-GitHub-Event&MessageAttribute.3.Value.DataType=String&MessageAttribute.3.Value.StringValue=$util.urlEncode($input.params().header.get('X-GitHub-Event'))&MessageAttribute.4.Name=X-GitHub-Delivery&MessageAttribute.4.Value.DataType=String&MessageAttribute.4.Value.StringValue=$util.urlEncode($input.params().header.get('X-GitHub-Delivery'))&MessageAttribute.5.Name=X-Hub-Signature-256&MessageAttribute.5.Value.DataType=String&MessageAttribute.5.Value.StringValue=$util.urlEncode($input.params().header.get('X-Hub-Signature-256'))&MessageAttribute.6.Name=User-Agent&MessageAttribute.6.Value.DataType=String&MessageAttribute.6.Value.StringValue=$util.urlEncode($input.params().header.get('User-Agent'))"
    
    "text/plain" = "Action=SendMessage&MessageBody=$util.urlEncode($util.base64Encode($input.body))&MessageAttribute.1.Name=BodyIsBase64&MessageAttribute.1.Value.DataType=String&MessageAttribute.1.Value.StringValue=true&MessageAttribute.2.Name=Content-Type&MessageAttribute.2.Value.DataType=String&MessageAttribute.2.Value.StringValue=$util.urlEncode($input.params().header.get('Content-Type'))&MessageAttribute.3.Name=X-GitHub-Event&MessageAttribute.3.Value.DataType=String&MessageAttribute.3.Value.StringValue=$util.urlEncode($input.params().header.get('X-GitHub-Event'))&MessageAttribute.4.Name=X-GitHub-Delivery&MessageAttribute.4.Value.DataType=String&MessageAttribute.4.Value.StringValue=$util.urlEncode($input.params().header.get('X-GitHub-Delivery'))&MessageAttribute.5.Name=X-Hub-Signature-256&MessageAttribute.5.Value.DataType=String&MessageAttribute.5.Value.StringValue=$util.urlEncode($input.params().header.get('X-Hub-Signature-256'))&MessageAttribute.6.Name=User-Agent&MessageAttribute.6.Value.DataType=String&MessageAttribute.6.Value.StringValue=$util.urlEncode($input.params().header.get('User-Agent'))"
    
    "application/x-www-form-urlencoded" = "Action=SendMessage&MessageBody=$util.urlEncode($util.base64Encode($input.body))&MessageAttribute.1.Name=BodyIsBase64&MessageAttribute.1.Value.DataType=String&MessageAttribute.1.Value.StringValue=true&MessageAttribute.2.Name=Content-Type&MessageAttribute.2.Value.DataType=String&MessageAttribute.2.Value.StringValue=$util.urlEncode($input.params().header.get('Content-Type'))&MessageAttribute.3.Name=X-GitHub-Event&MessageAttribute.3.Value.DataType=String&MessageAttribute.3.Value.StringValue=$util.urlEncode($input.params().header.get('X-GitHub-Event'))&MessageAttribute.4.Name=X-GitHub-Delivery&MessageAttribute.4.Value.DataType=String&MessageAttribute.4.Value.StringValue=$util.urlEncode($input.params().header.get('X-GitHub-Delivery'))&MessageAttribute.5.Name=X-Hub-Signature-256&MessageAttribute.5.Value.DataType=String&MessageAttribute.5.Value.StringValue=$util.urlEncode($input.params().header.get('X-Hub-Signature-256'))&MessageAttribute.6.Name=User-Agent&MessageAttribute.6.Value.DataType=String&MessageAttribute.6.Value.StringValue=$util.urlEncode($input.params().header.get('User-Agent'))"
  }
}

# Method 200 response
resource "aws_api_gateway_method_response" "method_200" {
  rest_api_id = aws_api_gateway_rest_api.api.id
  resource_id = aws_api_gateway_resource.webhook.id
  http_method = aws_api_gateway_method.post_webhook.http_method
  status_code = "200"
}

# Integration 200 response
resource "aws_api_gateway_integration_response" "integration_200" {
  rest_api_id = aws_api_gateway_rest_api.api.id
  resource_id = aws_api_gateway_resource.webhook.id
  http_method = aws_api_gateway_method.post_webhook.http_method
  status_code = aws_api_gateway_method_response.method_200.status_code

  depends_on = [
    aws_api_gateway_integration.sqs,
    aws_api_gateway_method_response.method_200
  ]

  response_templates = {
    "application/json" = ""
  }
}

# -----------------------------
# Deploy + Stage (ensure ordering + auto-redeploy on template change)
# -----------------------------
locals {
  redeploy_hash = sha1(jsonencode({
    templates = aws_api_gateway_integration.sqs.request_templates
    params    = aws_api_gateway_integration.sqs.request_parameters
    method    = aws_api_gateway_method.post_webhook.http_method
    resource  = aws_api_gateway_resource.webhook.path
  }))
}

resource "aws_api_gateway_deployment" "deploy" {
  rest_api_id = aws_api_gateway_rest_api.api.id

  triggers = {
    redeployment = local.redeploy_hash
  }

  lifecycle {
    create_before_destroy = true
  }

  depends_on = [
    aws_api_gateway_integration.sqs,
    aws_api_gateway_method_response.method_200,
    aws_api_gateway_integration_response.integration_200
  ]
}

resource "aws_api_gateway_stage" "stage" {
  rest_api_id   = aws_api_gateway_rest_api.api.id
  deployment_id = aws_api_gateway_deployment.deploy.id
  stage_name    = "prod"

  depends_on = [
    aws_api_gateway_deployment.deploy
  ]
}

# -----------------------------
# Outputs
# -----------------------------
output "queue_url" {
  value       = aws_sqs_queue.webhook.url
  description = "SQS queue URL for your local relay."
}

output "public_webhook_url" {
  value       = "https://${aws_api_gateway_rest_api.api.id}.execute-api.${var.region}.amazonaws.com/${aws_api_gateway_stage.stage.stage_name}/webhook"
  description = "Public POST URL to give to GitHub/etc."
}
