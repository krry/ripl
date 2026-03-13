use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Config {
    pub provider: Option<ProviderConfig>,
    pub scaffold: Option<ScaffoldConfig>,
    pub theme: Option<ThemeConfig>,
    pub speech: Option<SpeechConfig>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ProviderConfig {
    pub name: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ScaffoldConfig {
    pub bootstrap: Option<bool>,
    pub history_max_turns: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ThemeConfig {
    pub root_hue: Option<u16>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct SpeechConfig {
    pub tts: Option<String>,
    pub stt: Option<String>,
    pub push_to_talk: Option<bool>,
    pub fish_api_key: Option<String>,
    pub fish_voice_id: Option<String>,
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(raw) = fs::read_to_string(path) {
            toml::from_str(&raw).unwrap_or_else(|_| Config::default())
        } else {
            Config::default()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            provider: None,
            scaffold: None,
            theme: None,
            speech: None,
        }
    }
}

pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".ripl").join("config.toml")
}

pub fn resolve_provider_key(cfg: &Config) -> Option<String> {
    if let Some(provider) = &cfg.provider {
        if let Some(key) = &provider.api_key {
            if !key.is_empty() {
                return Some(key.clone());
            }
        }
    }
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        return Some(key);
    }
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        return Some(key);
    }
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        return Some(key);
    }
    None
}

pub fn resolve_provider_name(cfg: &Config) -> Option<String> {
    if let Some(provider) = &cfg.provider {
        if let Some(name) = &provider.name {
            if !name.is_empty() {
                return Some(name.clone());
            }
        }
    }
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        return Some("anthropic".to_string());
    }
    if std::env::var("OPENAI_API_KEY").is_ok() {
        return Some("openai".to_string());
    }
    if std::env::var("OPENROUTER_API_KEY").is_ok() {
        return Some("openrouter".to_string());
    }
    None
}

pub fn scaffold_bootstrap_enabled(cfg: &Config) -> bool {
    cfg.scaffold
        .as_ref()
        .and_then(|s| s.bootstrap)
        .unwrap_or(true)
}

pub fn resolve_tts_mode(cfg: &Config) -> String {
    if let Some(speech) = &cfg.speech {
        if let Some(tts) = &speech.tts {
            if !tts.is_empty() {
                return tts.clone();
            }
        }
    }
    if std::env::var("FISH_API_KEY").is_ok() {
        return "fish".to_string();
    }
    "say".to_string()
}

pub fn resolve_stt_mode(cfg: &Config) -> String {
    if let Some(speech) = &cfg.speech {
        if let Some(stt) = &speech.stt {
            if !stt.is_empty() {
                return stt.clone();
            }
        }
    }
    if std::env::var("FISH_API_KEY").is_ok() {
        return "fish".to_string();
    }
    "whisper".to_string()
}

pub fn resolve_fish_voice_id(cfg: &Config) -> Option<String> {
    if let Some(speech) = &cfg.speech {
        if let Some(id) = &speech.fish_voice_id {
            if !id.is_empty() {
                return Some(id.clone());
            }
        }
    }
    std::env::var("FISH_VOICE_ID").ok()
}

pub fn open_config_file() -> Result<(), std::io::Error> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    if !path.exists() {
        fs::write(&path, default_config_template())?;
    }
    if cfg!(target_os = "macos") {
        let _ = Command::new("open").arg(&path).status();
    } else if cfg!(target_os = "linux") {
        let _ = Command::new("xdg-open").arg(&path).status();
    } else {
        println!("{}", path.display());
    }
    Ok(())
}

pub fn pair_provider(provider: &str) -> Result<(), std::io::Error> {
    let url = match provider {
        "openai" => "https://platform.openai.com/api-keys",
        "anthropic" => "https://console.anthropic.com/settings/keys",
        "openrouter" => "https://openrouter.ai/keys",
        _ => {
            println!("Usage: ripl pair <openai|anthropic|openrouter>");
            return Ok(());
        }
    };
    if cfg!(target_os = "macos") {
        let _ = Command::new("open").arg(url).status();
    } else if cfg!(target_os = "linux") {
        let _ = Command::new("xdg-open").arg(url).status();
    } else {
        println!("{}", url);
    }
    println!("Paste API key for {provider}:");
    let mut key = String::new();
    std::io::stdin().read_line(&mut key)?;
    let key = key.trim();
    if key.is_empty() {
        return Ok(());
    }
    let mut cfg = Config::load();
    cfg.provider = Some(ProviderConfig {
        name: Some(provider.to_string()),
        model: cfg.provider.as_ref().and_then(|p| p.model.clone()),
        api_key: Some(key.to_string()),
    });
    let path = config_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let raw = toml::to_string_pretty(&cfg).unwrap_or_else(|_| default_config_template());
    fs::write(path, raw)?;
    Ok(())
}

fn default_config_template() -> String {
    r#"[provider]
name = "openai"
model = "gpt-4o-mini"

[scaffold]
bootstrap = true
history_max_turns = 10

[theme]
root_hue = 217

[speech]
tts = "say"
stt = "whisper"
push_to_talk = true
"#
    .to_string()
}
