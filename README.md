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

cargo run --release
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