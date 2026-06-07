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
    temperature: Option<f64>,
    top_p: Option<f64>,
    top_k: Option<u32>,
}

impl ClaudeClient {
    pub fn new(api_key: String) -> Self {
        Self { http: Client::new(), api_key, temperature: None, top_p: None, top_k: None }
    }

    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            http: Client::new(),
            api_key: api_key_from_env()?,
            temperature: env_parse("ANTHROPIC_TEMPERATURE"),
            top_p: env_parse("ANTHROPIC_TOP_P"),
            top_k: env_parse("ANTHROPIC_TOP_K"),
        })
    }
}

fn api_key_from_env() -> anyhow::Result<String> {
    std::env::var("ANTHROPIC_API_KEY")
        .context("ANTHROPIC_API_KEY not set — export it before running: export ANTHROPIC_API_KEY=<your-key>")
}

fn env_parse<T: std::str::FromStr>(key: &str) -> Option<T> {
    std::env::var(key).ok()?.parse().ok()
}

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'static str,
    max_tokens: u32,
    messages: &'a [Message],
    tools: &'a [ToolSchema],
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<u32>,
}

#[derive(Deserialize)]
struct ApiUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
    stop_reason: String,
    usage: ApiUsage,
}

#[async_trait]
impl LlmClient for ClaudeClient {
    fn provider_name(&self) -> &str {
        "anthropic"
    }

    fn model_name(&self) -> &str {
        MODEL
    }

    fn model_params(&self) -> serde_json::Value {
        serde_json::json!({
            "model": MODEL,
            "api_url": API_URL,
            "temperature": self.temperature,
            "top_p": self.top_p,
            "top_k": self.top_k,
        })
    }

    async fn complete(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        max_tokens: u32,
    ) -> anyhow::Result<LlmResponse> {
        let req = ApiRequest {
            model: MODEL,
            max_tokens,
            messages: &messages,
            tools: &tools,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
        };

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
                return Ok(LlmResponse {
                    content: body.content,
                    stop_reason,
                    usage: crate::llm::TokenUsage {
                        input_tokens: body.usage.input_tokens,
                        output_tokens: body.usage.output_tokens,
                    },
                });
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
