use std::io::{BufRead, BufReader};
use std::sync::mpsc;

use reqwest::blocking::Client;
use serde_json::json;

use super::{ApiResponse, Message, Provider, Role};

/// Ouracle API provider — connects to an Ouracle backend `/chat` SSE endpoint.
///
/// Config via `~/.ripl/config.toml`:
/// ```toml
/// [provider]
/// name = "ouracle"
/// api_key = "<access_token>"     # JWT access token from POST /auth/token
/// model = "http://127.0.0.1:3737"  # base URL (overloads the model field)
/// ```
pub struct OuracleProvider {
    pub access_token: String,
    pub base_url: String,
}

impl OuracleProvider {
    /// Extract the session_id from the conversation if one was previously recorded.
    fn session_id(messages: &[Message]) -> Option<String> {
        for m in messages {
            if m.role == Role::System && m.content.starts_with("ouracle:session:") {
                return Some(m.content["ouracle:session:".len()..].trim().to_string());
            }
        }
        None
    }

    /// Extract the last user message from the conversation.
    fn last_user_message(messages: &[Message]) -> Option<String> {
        messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.clone())
    }
}

impl Provider for OuracleProvider {
    fn stream(&self, messages: &[Message], tx: mpsc::Sender<ApiResponse>) {
        let client = match Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
        {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(ApiResponse::Error {
                    message: format!("Ouracle client error: {}", err),
                });
                return;
            }
        };

        let url = format!("{}/chat", self.base_url.trim_end_matches('/'));
        let session_id = Self::session_id(messages);
        let message = Self::last_user_message(messages);

        let mut body = json!({});
        if let Some(sid) = &session_id {
            body["session_id"] = json!(sid);
        }
        if let Some(msg) = &message {
            body["message"] = json!(msg);
        }

        let resp = client
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&body)
            .send();

        let resp = match resp {
            Ok(r) => r,
            Err(err) => {
                let _ = tx.send(ApiResponse::Error {
                    message: format!("Ouracle request error: {}", err),
                });
                return;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            let _ = tx.send(ApiResponse::Error {
                message: format!("Ouracle HTTP {}: {}", status.as_u16(), body),
            });
            return;
        }

        let reader = BufReader::new(resp);
        let mut seen_session_id: Option<String> = None;

        for line in reader.lines().flatten() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let data = match line.strip_prefix("data: ") {
                Some(d) => d,
                None => continue,
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match event_type {
                "session" => {
                    if let Some(sid) = value.get("session_id").and_then(|v| v.as_str()) {
                        seen_session_id = Some(sid.to_string());
                    }
                }
                "token" => {
                    if let Some(token) = value.get("token").and_then(|v| v.as_str()) {
                        let _ = tx.send(ApiResponse::TokenChunk {
                            token: token.to_string(),
                        });
                    }
                }
                "break" => {
                    // Paragraph break within the stream — emit a newline token.
                    let _ = tx.send(ApiResponse::TokenChunk {
                        token: "\n\n".to_string(),
                    });
                }
                "complete" => {
                    // Embed session_id as a special system-like marker so the app
                    // can persist it in the conversation for the next turn.
                    if let Some(sid) = seen_session_id.take()
                        .or_else(|| value.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                    {
                        let _ = tx.send(ApiResponse::TokenChunk {
                            token: format!("\x00ouracle:session:{}", sid),
                        });
                    }
                    let _ = tx.send(ApiResponse::TurnComplete);
                    return;
                }
                "error" => {
                    let msg = value
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Ouracle error")
                        .to_string();
                    let _ = tx.send(ApiResponse::Error { message: msg });
                    return;
                }
                _ => {}
            }
        }

        // Stream ended without a complete event.
        let _ = tx.send(ApiResponse::TurnComplete);
    }
}
