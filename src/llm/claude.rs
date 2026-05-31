use anyhow::{bail, Context};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde::Serialize;
use std::time::Duration;
use tokio::time::sleep;

use super::{ContentBlock, LlmClient, LlmResponse, Message, StopReason, ToolSchema};

const MODEL: &str = "claude-sonnet-4-6";
const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_HTTP_RETRIES: u32 = 2;

pub struct ClaudeClient {
    http: Client,
    api_key: String,
}

impl ClaudeClient {
    pub fn from_env() -> anyhow::Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY not set")?;
        Ok(Self { http: Client::new(), api_key })
    }
}

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'static str,
    max_tokens: u32,
    messages: &'a [Message],
    tools: &'a [ToolSchema],
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
    stop_reason: String,
}

#[async_trait]
impl LlmClient for ClaudeClient {
    async fn complete(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        max_tokens: u32,
    ) -> anyhow::Result<LlmResponse> {
        let req = ApiRequest { model: MODEL, max_tokens, messages: &messages, tools: &tools };

        let mut delay = Duration::from_secs(1);
        let mut last_err = String::new();

        for attempt in 0..=MAX_HTTP_RETRIES {
            if attempt > 0 {
                sleep(delay).await;
                delay *= 2;
            }

            let resp = self
                .http
                .post(API_URL)
                .header("X-Api-Key", &self.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .json(&req)
                .send()
                .await
                .context("HTTP request failed")?;

            let status = resp.status();

            if status.is_success() {
                let body: ApiResponse = resp.json().await.context("failed to parse API response")?;
                let stop_reason = match body.stop_reason.as_str() {
                    "tool_use" => StopReason::ToolUse,
                    "max_tokens" => StopReason::MaxTokens,
                    _ => StopReason::EndTurn,
                };
                return Ok(LlmResponse { content: body.content, stop_reason });
            }

            // Retry on rate limit and transient server errors.
            if status.as_u16() == 429 || status.is_server_error() {
                last_err = format!("status {status}");
                continue;
            }

            let body = resp.text().await.unwrap_or_default();
            bail!("API error {status}: {body}");
        }

        bail!("API request failed after {} attempts: {last_err}", MAX_HTTP_RETRIES + 1);
    }
}
