use anyhow::Context;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{ContentBlock, LlmClient, LlmResponse, Message, Role, StopReason, TokenUsage, ToolSchema};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "gemma4:12b";

pub struct OllamaClient {
    http: Client,
    base_url: String,
    model: String,
}

impl OllamaClient {
    pub fn new(base_url: String, model: String) -> Self {
        Self { http: Client::new(), base_url, model }
    }

    pub fn from_env() -> Self {
        let base_url = std::env::var("OLLAMA_HOST")
            .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        let model = std::env::var("OLLAMA_MODEL")
            .unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        Self::new(base_url, model)
    }
}

#[derive(Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Serialize, Deserialize)]
struct OllamaToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    function: OllamaFunction,
}

#[derive(Serialize, Deserialize)]
struct OllamaFunction {
    name: String,
    arguments: serde_json::Value,
}

#[derive(Serialize)]
struct OllamaTool {
    r#type: &'static str,
    function: OllamaToolDef,
}

#[derive(Serialize)]
struct OllamaToolDef {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Serialize)]
struct OllamaOptions {
    num_predict: u32,
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<OllamaTool>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
    done_reason: Option<String>,
    #[serde(default)]
    prompt_eval_count: u32,
    #[serde(default)]
    eval_count: u32,
}

fn to_ollama_messages(messages: &[Message]) -> Vec<OllamaMessage> {
    let mut out = Vec::new();

    for msg in messages {
        let role = match msg.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };

        // User messages may contain tool results — emit each as a "tool" role message.
        let has_tool_results = msg.content.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }));
        if has_tool_results {
            for block in &msg.content {
                if let ContentBlock::ToolResult { content, .. } = block {
                    out.push(OllamaMessage {
                        role: "tool".to_string(),
                        content: content.clone(),
                        tool_calls: None,
                    });
                }
            }
            continue;
        }

        let text: String = msg
            .content
            .iter()
            .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
            .collect::<Vec<_>>()
            .join("\n");

        let tool_calls: Vec<OllamaToolCall> = msg
            .content
            .iter()
            .filter_map(|b| {
                if let ContentBlock::ToolUse { id, name, input } = b {
                    Some(OllamaToolCall {
                        id: Some(id.clone()),
                        function: OllamaFunction { name: name.clone(), arguments: input.clone() },
                    })
                } else {
                    None
                }
            })
            .collect();

        out.push(OllamaMessage {
            role: role.to_string(),
            content: text,
            tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
        });
    }

    out
}

fn to_ollama_tools(tools: &[ToolSchema]) -> Vec<OllamaTool> {
    tools
        .iter()
        .map(|t| OllamaTool {
            r#type: "function",
            function: OllamaToolDef {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.input_schema.clone(),
            },
        })
        .collect()
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn complete(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        max_tokens: u32,
    ) -> anyhow::Result<LlmResponse> {
        let req = OllamaRequest {
            model: self.model.clone(),
            messages: to_ollama_messages(&messages),
            tools: to_ollama_tools(&tools),
            stream: false,
            options: OllamaOptions { num_predict: max_tokens },
        };

        let url = format!("{}/api/chat", self.base_url);

        let resp = self
            .http
            .post(&url)
            .json(&req)
            .send()
            .await
            .context("HTTP request to Ollama failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama API error {status}: {body}");
        }

        let body: OllamaResponse =
            resp.json().await.context("failed to parse Ollama response")?;

        let mut content = Vec::new();

        if let Some(tool_calls) = body.message.tool_calls {
            for (i, tc) in tool_calls.into_iter().enumerate() {
                let id = tc.id.unwrap_or_else(|| format!("call_{i}"));
                content.push(ContentBlock::ToolUse {
                    id,
                    name: tc.function.name,
                    input: tc.function.arguments,
                });
            }
        }

        if !body.message.content.is_empty() {
            content.push(ContentBlock::Text { text: body.message.content });
        }

        let has_tool_use = content.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }));
        let stop_reason = match body.done_reason.as_deref() {
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            _ if has_tool_use => StopReason::ToolUse,
            _ => StopReason::EndTurn,
        };

        Ok(LlmResponse {
            content,
            stop_reason,
            usage: TokenUsage {
                input_tokens: body.prompt_eval_count,
                output_tokens: body.eval_count,
            },
        })
    }
}
