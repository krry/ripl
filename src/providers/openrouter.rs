use std::io::{BufRead, BufReader};
use std::sync::mpsc;

use reqwest::blocking::Client;
use serde_json::json;

use super::{ApiResponse, Message, Provider, Role};

pub struct OpenRouterProvider {
    pub api_key: String,
    pub model: String,
}

impl Provider for OpenRouterProvider {
    fn stream(&self, messages: &[Message], tx: mpsc::Sender<ApiResponse>) {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build();
        let client = match client {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(ApiResponse::Error {
                    message: format!("OpenRouter client error: {}", err),
                });
                return;
            }
        };

        let payload = json!({
            "model": if self.model == "default" { "openai/gpt-4o-mini" } else { &self.model },
            "stream": true,
            "messages": messages.iter().map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                json!({ "role": role, "content": m.content })
            }).collect::<Vec<_>>(),
        });

        let resp = client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .json(&payload)
            .send();

        let resp = match resp {
            Ok(r) => r,
            Err(err) => {
                let _ = tx.send(ApiResponse::Error {
                    message: format!("OpenRouter request error: {}", err),
                });
                return;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            let _ = tx.send(ApiResponse::Error {
                message: format!("OpenRouter HTTP {}: {}", status.as_u16(), body),
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
            if let Some(token) = value
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("delta"))
                .and_then(|d| d.get("content"))
                .and_then(|c| c.as_str())
            {
                let _ = tx.send(ApiResponse::TokenChunk {
                    token: token.to_string(),
                });
            }
        }
    }
}
