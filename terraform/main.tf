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

# Data
data "aws_caller_identity" "me" {}

# SQS queue
resource "aws_sqs_queue" "webhook" {
  name                       = var.queue_name
  message_retention_seconds  = 1209600
  visibility_timeout_seconds = 60
}

# IAM role: API Gateway -> SQS
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

# API Gateway (REST API v1)
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

# SQS SendMessage
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

  # Map JSON body + select headers into SQS form-encoded params
  # This is very specific to github webhooks, modify as needed
  request_templates = {
    "application/json" = <<-EOT
      Action=SendMessage&MessageBody=$util.urlEncode($input.body)
      #set($ct = $input.params().header.get('Content-Type'))
      #if($ct) &MessageAttribute.1.Name=ContentType&MessageAttribute.1.Value.DataType=String&MessageAttribute.1.Value.StringValue=$util.urlEncode($ct) #end
      #set($ev = $input.params().header.get('X-GitHub-Event'))
      #if($ev) &MessageAttribute.2.Name=X-GitHub-Event&MessageAttribute.2.Value.DataType=String&MessageAttribute.2.Value.StringValue=$util.urlEncode($ev) #end
      #set($dl = $input.params().header.get('X-GitHub-Delivery'))
      #if($dl) &MessageAttribute.3.Name=X-GitHub-Delivery&MessageAttribute.3.Value.DataType=String&MessageAttribute.3.Value.StringValue=$util.urlEncode($dl) #end
      #set($sg = $input.params().header.get('X-Hub-Signature-256'))
      #if($sg) &MessageAttribute.4.Name=X-Hub-Signature-256&MessageAttribute.4.Value.DataType=String&MessageAttribute.4.Value.StringValue=$util.urlEncode($sg) #end
    EOT
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

# Deploy + Stage
resource "aws_api_gateway_deployment" "deploy" {
  rest_api_id = aws_api_gateway_rest_api.api.id

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
}
