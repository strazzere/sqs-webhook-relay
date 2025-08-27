output "queue_url" {
  value       = aws_sqs_queue.webhook.url
  description = "SQS queue URL for your local relay."
}

output "public_webhook_url" {
  value       = "https://${aws_api_gateway_rest_api.api.id}.execute-api.${var.region}.amazonaws.com/${aws_api_gateway_stage.stage.stage_name}/webhook"
  description = "Public POST URL to give to GitHub/etc."
}