use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_sdk_sqs::{types::MessageAttributeValue, Client as SqsClient};
use colored::*;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use serde_json::Value;
use std::{env, time::Duration};
use tokio::signal;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;
use base64::{engine::general_purpose, Engine as _};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .with_ansi(true)
        .compact()
        .init();

    let queue_url = env::var("QUEUE_URL").context("missing QUEUE_URL")?;
    let local_url = env::var("LOCAL_URL").unwrap_or_else(|_| "http://127.0.0.1:3000/webhook".into());

    // Non-deprecated AWS config
    let shared_config = aws_config::defaults(BehaviorVersion::latest()).load().await;
    let sqs = SqsClient::new(&shared_config);

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;

    info!("🚀 Relay starting. Queue={}, Local={}", queue_url, local_url);
    info!("🔍 Use RUST_LOG=debug for verbose output");
    info!("⏹️  Ctrl-C to stop.");

    tokio::select! {
        _ = relay_loop(&sqs, &http, &queue_url, &local_url) => {},
        _ = signal::ctrl_c() => {
            info!("Received Ctrl-C, shutting down.");
        }
    }

    Ok(())
}

async fn relay_loop(sqs: &SqsClient, http: &reqwest::Client, queue_url: &str, local_url: &str) {
    debug!("🔄 Starting relay loop, polling SQS every 20 seconds...");
    
    loop {
        debug!("📡 Polling SQS for messages...");
        let resp = match sqs
            .receive_message()
            .queue_url(queue_url)
            .max_number_of_messages(10)
            .wait_time_seconds(20)   // long polling
            .visibility_timeout(60)  // time to process locally
            .message_attribute_names("All")
            .message_system_attribute_names(aws_sdk_sqs::types::MessageSystemAttributeName::ApproximateReceiveCount)
            .send()
            .await
        {
            Ok(r) => {
                debug!("✅ SQS poll successful");
                r
            },
            Err(e) => {
                error!("❌ SQS receive error: {e}");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let msgs = resp.messages();
        if msgs.is_empty() {
            debug!("No messages received from SQS");
            continue;
        }

        info!("📥 Received {} message(s) from SQS", msgs.len());

        for m in msgs {
            let Some(receipt) = m.receipt_handle() else { 
                debug!("Message missing receipt handle, skipping");
                continue; 
            };
            let body_raw = m.body().unwrap_or_default();

            let message_id = m.message_id().unwrap_or("unknown");
            debug!("🔄 Processing message ID: {}", message_id);

            // Attributes map (String -> MessageAttributeValue)
            let attrs_map = m.message_attributes();
            debug!("Message has {} attributes", attrs_map.map(|m| m.len()).unwrap_or(0));

            // Determine if MessageBody is base64 of original bytes (per API GW template)
            let body_is_b64 = attrs_map
                .and_then(|m| m.get("BodyIsBase64"))
                .and_then(|v| v.string_value())
                .map(|s| s.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

            debug!("Body is base64: {}, raw length: {} chars", body_is_b64, body_raw.len());

            // Decode to raw bytes that GitHub originally sent
            let raw_bytes: Vec<u8> = if body_is_b64 {
                debug!("Decoding base64 message body");
                match general_purpose::STANDARD.decode(body_raw) {
                    Ok(b) => {
                        debug!("Successfully decoded {} bytes from base64", b.len());
                        b
                    },
                    Err(e) => {
                        warn!("BodyIsBase64=true but base64 decode failed: {e}. Falling back to UTF-8 bytes.");
                        body_raw.as_bytes().to_vec()
                    }
                }
            } else {
                debug!("Using raw UTF-8 bytes (no base64 decoding)");
                body_raw.as_bytes().to_vec()
            };

            debug!("Final raw_bytes length: {} bytes", raw_bytes.len());

            // Build headers from MessageAttributes (lowercase keys are fine)
            let mut hdrs = HeaderMap::new();
            let mut source_ip: Option<String> = None;

            for (k, v) in attrs_to_headers(attrs_map) {
                // Construct header name/value
                if let (Ok(name), Ok(value)) =
                    (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(&v))
                {
                    match k.as_str() {
                        "sourceip" | "source-ip" | "clientip" | "client-ip" |
                        "originatingip" | "originating-ip" | "remote-addr" | "x-real-ip" => {
                            source_ip = Some(v.clone());
                            debug!("Found source IP in attribute '{}': {}", k, v);
                        }
                        _ => {}
                    }
                    hdrs.append(name, value);
                }
            }

            // Ensure Content-Type header exists
            if !hdrs.contains_key(CONTENT_TYPE) {
                hdrs.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            }

            // Sanity: warn if signature is missing (it should be present)
            if !hdrs.contains_key("x-hub-signature-256") {
                warn!("SQS message missing X-Hub-Signature-256 attribute; signature verification will fail");
            }

            // Add/extend X-Forwarded-For from attributes or JSON body (best-effort)
            if source_ip.is_none() {
                source_ip = extract_ip_from_json_bytes(&raw_bytes);
            }
            if let Some(ref ip) = source_ip {
                if let Ok(xff_value) = HeaderValue::from_str(ip) {
                    if let Some(existing_xff) = hdrs.get("x-forwarded-for") {
                        if let Ok(existing_str) = existing_xff.to_str() {
                            if let Ok(new_xff) = HeaderValue::from_str(&format!("{}, {}", existing_str, ip)) {
                                hdrs.insert("x-forwarded-for", new_xff);
                            }
                        }
                    } else {
                        hdrs.insert("x-forwarded-for", xff_value);
                    }
                    debug!("Added X-Forwarded-For header: {}", ip);
                }
            }

            // Summary for logs (decode to UTF-8 lossily for display only)
            let webhook_summary = extract_webhook_summary_from_bytes(&raw_bytes);

            // Receive count to track retries
            let receive_count: u32 = m.attributes()
                .and_then(|attrs| attrs.get(&aws_sdk_sqs::types::MessageSystemAttributeName::ApproximateReceiveCount))
                .and_then(|count_str| count_str.parse().ok())
                .unwrap_or(1);

            info!(
                "{} SQS → Local: {}{}",
                "📨".cyan(),
                webhook_summary.bright_white(),
                if let Some(ip) = &source_ip {
                    format!(" [IP: {}]", ip.bright_blue())
                } else {
                    String::new()
                }
            );

            debug!("🚀 Forwarding message {} to {}", message_id, local_url);
            debug!("Request headers: {:?}", hdrs.keys().collect::<Vec<_>>());
            for (k, v) in hdrs.iter() {
                debug!("  {}: {:?}", k, v);
            }
            debug!("Sending {} bytes to local service", raw_bytes.len());

            // POST to local server with the EXACT BYTES (this is the critical part)
            // Use Vec<u8> directly instead of cloning
            let res = http
                .post(local_url)
                .headers(hdrs)
                .body(raw_bytes)
                .send()
                .await;

            match res {
                Ok(rsp) if rsp.status().is_success() => {
                    let status_code = rsp.status().as_u16();
                    info!("{} Local → Response: {} (attempt {})", "📤".green(), colorize_status(status_code), receive_count);

                    debug!("Response headers: {:?}", rsp.headers().keys().collect::<Vec<_>>());
                    match rsp.text().await {
                        Ok(response_body) => {
                            let response_preview = preview_str(&response_body, 200);
                            if !response_preview.is_empty() {
                                debug!("Response body: {}", response_preview);
                            }
                        }
                        Err(e) => debug!("Could not read response body: {}", e)
                    }

                    if let Err(e) = sqs.delete_message().queue_url(queue_url).receipt_handle(receipt).send().await {
                        error!("Failed to delete SQS message {}: {}", message_id, e);
                    } else {
                        debug!("Message {} deleted from queue", message_id);
                    }
                }
                Ok(rsp) => {
                    let status_code = rsp.status().as_u16();
                    info!("{} Local → Response: {} (attempt {})", "📤".red(), colorize_status(status_code), receive_count);

                    debug!("Error response headers: {:?}", rsp.headers().keys().collect::<Vec<_>>());
                    let _ = match rsp.text().await {
                        Ok(response_body) => {
                            let response_preview = preview_str(&response_body, 200);
                            if !response_preview.is_empty() {
                                debug!("Error response: {}", response_preview);
                            }
                            response_body
                        }
                        Err(e) => {
                            debug!("Could not read error response body: {}", e);
                            String::new()
                        }
                    };

                    match status_code {
                        404 => {
                            // Endpoint missing; safe to drop
                            warn!("{} 404 → Deleting message (endpoint not found)", "🗑️".yellow());
                            if let Err(e) = sqs.delete_message().queue_url(queue_url).receipt_handle(receipt).send().await {
                                error!("Failed to delete SQS message: {}", e);
                            } else {
                                debug!("Message {} deleted due to 404", message_id);
                            }
                        }
                        400..=499 => {
                            // Retry once for 4xx (e.g., signature mismatch on first try)
                            if receive_count == 1 {
                                warn!("{} {} → Will retry once (attempt {})", "🔄".yellow(), colorize_status(status_code), receive_count);
                                debug!("Message {} left in queue for single retry", message_id);
                            } else {
                                warn!("{} {} → Deleting after retry (attempt {})", "🗑️".red(), colorize_status(status_code), receive_count);
                                if let Err(e) = sqs.delete_message().queue_url(queue_url).receipt_handle(receipt).send().await {
                                    error!("Failed to delete SQS message: {}", e);
                                } else {
                                    info!("Message {} deleted after failed retry", message_id);
                                }
                            }
                        }
                        500..=599 => {
                            // 5xx errors - server issues; let SQS retry
                            warn!("{} {} → Will retry (server error, attempt {})", "🔄".red(), colorize_status(status_code), receive_count);
                            debug!("Message {} left in queue for retry", message_id);
                        }
                        _ => {
                            warn!("{} {} → Will retry (unexpected status, attempt {})", "🔄".white(), colorize_status(status_code), receive_count);
                            debug!("Message {} left in queue for retry", message_id);
                        }
                    }
                }
                Err(e) => {
                    error!("{} Network error → Will retry (attempt {}): {}", "🌐".red(), receive_count, e);
                    debug!("Message {} left in queue for retry", message_id);
                }
            }
        }
    }
}

fn attrs_to_headers(
    attrs: Option<&std::collections::HashMap<String, MessageAttributeValue>>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(map) = attrs {
        for (k, v) in map {
            if let Some(s) = v.string_value() {
                // Send as header; HTTP is case-insensitive, we normalize to lowercase.
                out.push((k.to_ascii_lowercase(), s.to_string()));
            }
        }
    }
    out
}

fn extract_webhook_summary_from_bytes(bytes: &[u8]) -> String {
    // Try to parse JSON first for a meaningful summary
    if let Ok(text) = std::str::from_utf8(bytes) {
        if let Ok(json) = serde_json::from_str::<Value>(text) {
            let mut parts = Vec::new();

            if let Some(event_type) = json.get("type").and_then(|v| v.as_str()) {
                parts.push(format!("type:{}", event_type));
            } else if let Some(event) = json.get("event").and_then(|v| v.as_str()) {
                parts.push(format!("event:{}", event));
            } else if let Some(action) = json.get("action").and_then(|v| v.as_str()) {
                parts.push(format!("action:{}", action));
            }

            if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
                if id.len() > 12 {
                    parts.push(format!("id:{}...", &id[..8]));
                } else {
                    parts.push(format!("id:{}", id));
                }
            }

            if !parts.is_empty() {
                return parts.join(" ");
            }
        }
        // JSON parse failed or no interesting fields; show a preview
        preview_str(text, 40)
    } else {
        // Non-UTF8 payload; show hex preview
        preview_hex(bytes, 24)
    }
}

fn extract_ip_from_json_bytes(bytes: &[u8]) -> Option<String> {
    let Ok(text) = std::str::from_utf8(bytes) else { return None; };
    let Ok(json) = serde_json::from_str::<Value>(text) else { return None; };

    let ip_fields = [
        "sourceIp", "source_ip", "clientIp", "client_ip",
        "originatingIp", "originating_ip", "remoteAddr", "remote_addr",
        "requestContext.identity.sourceIp",
        "headers.x-forwarded-for", "headers.x-real-ip",
        "requestInfo.remoteIp", "request.ip", "ip"
    ];

    for field in &ip_fields {
        if field.contains('.') {
            let parts: Vec<&str> = field.split('.').collect();
            let mut current = &json;
            let mut found = true;

            for part in &parts {
                if let Some(next) = current.get(part) {
                    current = next;
                } else {
                    found = false;
                    break;
                }
            }

            if found {
                if let Some(ip_str) = current.as_str() {
                    debug!("Found source IP in JSON body field '{}': {}", field, ip_str);
                    return Some(ip_str.to_string());
                }
            }
        } else if let Some(ip_value) = json.get(*field) {
            if let Some(ip_str) = ip_value.as_str() {
                debug!("Found source IP in JSON body field '{}': {}", field, ip_str);
                return Some(ip_str.to_string());
            }
        }
    }
    None
}

fn colorize_status(status: u16) -> String {
    match status {
        200..=299 => format!("{}", status).green().bold().to_string(),
        400..=499 => format!("{}", status).yellow().bold().to_string(),
        500..=599 => format!("{}", status).red().bold().to_string(),
        _ => format!("{}", status).white().bold().to_string(),
    }
}

fn preview_str(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}... ({} chars)", &s[..max], s.len())
    } else {
        s.to_string()
    }
}

fn preview_hex(bytes: &[u8], max_bytes: usize) -> String {
    let shown = bytes.iter().take(max_bytes).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
    if bytes.len() > max_bytes {
        format!("hex:{}... ({} bytes)", shown, bytes.len())
    } else {
        format!("hex:{} ({} bytes)", shown, bytes.len())
    }
}
