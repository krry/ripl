use std::sync::mpsc;
use std::sync::Arc;

use crate::config::{resolve_provider_key, resolve_provider_name, Config};
use serde::{Deserialize, Serialize};

mod anthropic;
mod openai;
mod openrouter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum ApiResponse {
    TokenChunk { token: String },
    TurnComplete,
    Error { message: String },
}

pub trait Provider: Send + Sync + 'static {
    fn stream(&self, messages: &[Message], tx: mpsc::Sender<ApiResponse>);
    /// Called when the user issues a slash command not handled by the app.
    /// Provider streams responses via `tx` as if it were a normal turn.
    fn handle_command(&self, _cmd: &str, _tx: mpsc::Sender<ApiResponse>) {}
}

pub enum ProviderKind {
    Anthropic,
    OpenAi,
    OpenRouter,
}

pub struct ProviderResolved {
    pub kind: ProviderKind,
    pub api_key: String,
    pub model: String,
}

impl ProviderResolved {
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenAi => "openai",
            ProviderKind::OpenRouter => "openrouter",
        }
    }
}

pub fn resolve_provider(cfg: &Config) -> Option<ProviderResolved> {
    let name = resolve_provider_name(cfg)?;
    let api_key = resolve_provider_key(cfg)?;
    let model = cfg
        .provider
        .as_ref()
        .and_then(|p| p.model.clone())
        .unwrap_or_else(|| {
            match name.as_str() {
                "anthropic" => "claude-sonnet-4-6",
                "openai" => "gpt-4o-mini",
                "openrouter" => "openai/gpt-4o-mini",
                _ => "default",
            }
            .to_string()
        });

    let kind = match name.as_str() {
        "anthropic" => ProviderKind::Anthropic,
        "openai" => ProviderKind::OpenAi,
        "openrouter" => ProviderKind::OpenRouter,
        _ => return None,
    };

    Some(ProviderResolved { kind, api_key, model })
}

pub fn build_provider(cfg: &Config) -> Option<Arc<dyn Provider>> {
    let resolved = resolve_provider(cfg)?;
    let provider: Arc<dyn Provider> = match resolved.kind {
        ProviderKind::Anthropic => Arc::new(anthropic::AnthropicProvider {
            api_key: resolved.api_key,
            model: resolved.model,
        }),
        ProviderKind::OpenAi => Arc::new(openai::OpenAiProvider {
            api_key: resolved.api_key,
            model: resolved.model,
        }),
        ProviderKind::OpenRouter => Arc::new(openrouter::OpenRouterProvider {
            api_key: resolved.api_key,
            model: resolved.model,
        }),
    };
    Some(provider)
}
