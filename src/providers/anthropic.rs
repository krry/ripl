use std::io::{BufRead, BufReader};
use std::sync::mpsc;

use reqwest::blocking::Client;
use serde_json::json;

use super::{ApiResponse, Message, Provider, Role};

pub struct AnthropicProvider {
    pub api_key: String,
    pub model: String,
}

impl Provider for AnthropicProvider {
    fn stream(&self, messages: &[Message], tx: mpsc::Sender<ApiResponse>) {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build();
        let client = match client {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(ApiResponse::Error {
                    message: format!("Anthropic client error: {}", err),
                });
                return;
            }
        };

        let mut system = String::new();
        let mut user_messages = Vec::new();
        for m in messages {
            match m.role {
                Role::System => {
                    if !system.is_empty() {
                        system.push('\n');
                    }
                    system.push_str(&m.content);
                }
                Role::User => {
                    user_messages.push(json!({ "role": "user", "content": m.content }));
                }
                Role::Assistant => {
                    user_messages.push(json!({ "role": "assistant", "content": m.content }));
                }
            }
        }

        let payload = json!({
            "model": if self.model == "default" { "claude-3-5-sonnet-20240620" } else { &self.model },
            "max_tokens": 1024,
            "stream": true,
            "system": system,
            "messages": user_messages,
        });

        let resp = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&payload)
            .send();

        let resp = match resp {
            Ok(r) => r,
            Err(err) => {
                let _ = tx.send(ApiResponse::Error {
                    message: format!("Anthropic request error: {}", err),
                });
                return;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            let _ = tx.send(ApiResponse::Error {
                message: format!("Anthropic HTTP {}: {}", status.as_u16(), body),
            });
            return;
        }

        let reader = BufReader::new(resp);
        for line in reader.lines().flatten() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let data = line.strip_prefix("data: ").unwrap_or(line);
            if data == "[DONE]" {
                let _ = tx.send(ApiResponse::TurnComplete);
                break;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            let event = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if event == "content_block_delta" {
                if let Some(token) = value
                    .get("delta")
                    .and_then(|d| d.get("text"))
                    .and_then(|t| t.as_str())
                {
                    let _ = tx.send(ApiResponse::TokenChunk {
                        token: token.to_string(),
                    });
                }
            }
            if event == "message_stop" {
                let _ = tx.send(ApiResponse::TurnComplete);
                break;
            }
        }
    }
}
