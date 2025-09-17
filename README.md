# sqs-webhook-relay

Quick and dirty, self-run `ngrok` alternative for testing webhooks locally while being able to be on the network plane of your development machine.

## Deploy relay infrastructure

```sh
terraform init
terraform apply
```

## Connecting to relay

```sh
export AWS_REGION=us-east-1
export QUEUE_URL="https://sqs.us-east-1.amazonaws.com/123456789012/webhook-demo-queue"
export LOCAL_URL="http://127.0.0.1:3000/webhook"

RUST_LOG=info cargo run --release
2025-09-17T01:59:54.552506Z  INFO 🚀 Relay starting. Queue=https://sqs.us-east-1.amazonaws.com/123456789012/webhook-demo-queue, Local=http://localhost:3000/webhook
2025-09-17T01:59:54.552557Z  INFO 🔍 Use RUST_LOG=debug for verbose output
2025-09-17T01:59:54.552559Z  INFO ⏹️  Ctrl-C to stop.
2025-09-17T02:08:16.089324Z  INFO 📥 Received 1 message(s) from SQS
2025-09-17T02:08:16.089762Z  INFO 📨 SQS → Local: action:closed
2025-09-17T02:08:16.111367Z  INFO 📤 Local → Response: 200 (attempt 1)
2025-09-17T02:08:30.322931Z  INFO 📥 Received 1 message(s) from SQS
2025-09-17T02:08:30.323542Z  INFO 📨 SQS → Local: action:opened
2025-09-17T02:08:32.706246Z  INFO 📤 Local → Response: 200 (attempt 1)
2025-09-17T02:08:33.442136Z  INFO 📥 Received 2 message(s) from SQS
2025-09-17T02:08:33.442348Z  INFO 📨 SQS → Local: {"id":33435152,"sha":"c786e6c3648a442c32... (11636 chars)
2025-09-17T02:08:33.448775Z  INFO 📤 Local → Response: 200 (attempt 1)
2025-09-17T02:08:33.561942Z  INFO 📨 SQS → Local: {"id":33435153,"sha":"c786e6c3648a442c32... (11708 chars)
2025-09-17T02:08:33.566373Z  INFO 📤 Local → Response: 200 (attempt 1)
2025-09-17T02:13:36.384091Z  INFO 📥 Received 1 message(s) from SQS
2025-09-17T02:13:36.384328Z  INFO 📨 SQS → Local: action:created
2025-09-17T02:13:38.805051Z  INFO 📤 Local → Response: 200 (attempt 1)
2025-09-17T02:13:39.145555Z  INFO 📥 Received 1 message(s) from SQS
2025-09-17T02:13:39.145757Z  INFO 📨 SQS → Local: {"id":33435263,"sha":"c786e6c3648a442c32... (11725 chars)
2025-09-17T02:13:39.151807Z  INFO 📤 Local → Response: 200 (attempt 1)
```

## Testing relay

```sh
curl -X POST \
  -H "Content-Type: application/json" \
  -H "X-GitHub-Event: push" \
  -H "X-GitHub-Delivery: test-123" \
  -H "X-Hub-Signature-256: sha256=fakedsignature" \
  -d '{"hello":"world","demo":"true"}' \
  https://abc123.execute-api.us-east-1.amazonaws.com/prod/webhook
```