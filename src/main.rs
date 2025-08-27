use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_sdk_sqs::{types::MessageAttributeValue, Client as SqsClient};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use std::{env, time::Duration};
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
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

    info!("Relay starting. Queue={}, Local={}", queue_url, local_url);
    info!("Ctrl-C to stop.");

    tokio::select! {
        _ = relay_loop(&sqs, &http, &queue_url, &local_url) => {},
        _ = signal::ctrl_c() => {
            info!("Received Ctrl-C, shutting down.");
        }
    }

    Ok(())
}

async fn relay_loop(sqs: &SqsClient, http: &reqwest::Client, queue_url: &str, local_url: &str) {
    const MAX_NUMBER_OF_MESSAGES: i32 = 10;
    // long polling
    const WAIT_TIME_SECONDS: i32 = 20;
    // window of time to process locally
    const VISIBILITIY_TIMEOUT: i32 = 60;
    loop {
        let resp = match sqs
            .receive_message()
            .queue_url(queue_url)
            .max_number_of_messages(MAX_NUMBER_OF_MESSAGES)
            .wait_time_seconds(WAIT_TIME_SECONDS)
            .visibility_timeout(VISIBILITIY_TIMEOUT)
            .message_attribute_names("All")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                error!("SQS receive error: {e}");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let msgs = resp.messages();
        if msgs.is_empty() {
            continue;
        }

        for m in msgs {
            let Some(receipt) = m.receipt_handle() else { continue; };
            let body = m.body().unwrap_or_default().to_string();

            // Build headers from MessageAttributes
            let mut hdrs = HeaderMap::new();
            for (k, v) in attrs_to_headers(m.message_attributes()) {
                if let (Ok(name), Ok(value)) =
                    (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(&v))
                {
                    // use append to allow repeated headers, if ever needed - is it needed?
                    hdrs.append(name, value);
                }
            }
            if !hdrs.contains_key(CONTENT_TYPE) {
                hdrs.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            }

            // POST to local server
            let res = http.post(local_url).headers(hdrs).body(body).send().await;

            match res {
                Ok(rsp) if rsp.status().is_success() => {
                    if let Err(e) = sqs.delete_message().queue_url(queue_url).receipt_handle(receipt).send().await {
                        error!("Failed to delete SQS message: {e}");
                    } else {
                        info!("Delivered → {} ({})", local_url, rsp.status());
                    }
                }
                Ok(rsp) => {
                    error!("Local returned {}. Leaving message for retry.", rsp.status());
                }
                Err(e) => {
                    error!("HTTP error forwarding to local: {e}. Leaving message for retry.");
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
                out.push((k.to_ascii_lowercase(), s.to_string()));
            }
        }
    }
    out
}
