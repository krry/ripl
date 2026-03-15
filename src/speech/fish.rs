use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use reqwest::blocking::Client;
use reqwest::blocking::multipart;
use serde_json::json;
use sha2::Digest;

fn fish_api_key() -> Result<String, String> {
    std::env::var("FISH_AUDIO_API_KEY")
        .or_else(|_| std::env::var("FISH_API_KEY"))
        .map_err(|_| "FISH_AUDIO_API_KEY not set".to_string())
}

pub fn has_fish_key() -> bool {
    fish_api_key().is_ok()
}

pub fn fish_tts(text: &str, voice_id: Option<&str>) -> Result<f32, String> {
    let api_key = fish_api_key()?;
    let model = std::env::var("FISH_AUDIO_MODEL")
        .or_else(|_| std::env::var("FISH_TTS_MODEL"))
        .unwrap_or_else(|_| "s1".to_string());
    let cache_path = tts_cache_path(text, &model, voice_id);
    if cache_path.exists() {
        let duration = afinfo_duration_seconds(&cache_path).unwrap_or_else(|| estimate_tts_seconds(text));
        let _ = Command::new("afplay").arg(&cache_path).spawn();
        return Ok(duration);
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("fish client error: {e}"))?;
    let url = "https://api.fish.audio/v1/tts";
    let backend = if model == "s2" { "s2-pro" } else { model.as_str() };
    let body = if let Some(reference_id) = voice_id {
        json!({ "text": text, "reference_id": reference_id, "format": "mp3" })
    } else {
        json!({ "text": text, "format": "mp3" })
    };
    let resp = client
        .post(url)
        .bearer_auth(api_key)
        .header("model", backend)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("fish http error: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_else(|_| "<no body>".to_string());
        return Err(format!("fish http {}: {}", status.as_u16(), body));
    }
    let bytes = resp.bytes().map_err(|e| format!("fish read error: {e}"))?;
    let cache_dir = tts_cache_dir();
    fs::create_dir_all(&cache_dir).map_err(|e| format!("fish cache dir error: {e}"))?;
    fs::write(&cache_path, &bytes).map_err(|e| format!("fish write error: {e}"))?;

    let duration = afinfo_duration_seconds(&cache_path).unwrap_or_else(|| estimate_tts_seconds(text));
    let _ = Command::new("afplay").arg(&cache_path).spawn();
    Ok(duration)
}

pub fn spawn_fish_tts(text: String, voice_id: Option<String>) -> Option<Receiver<Result<f32, String>>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let res = fish_tts(&text, voice_id.as_deref());
        let _ = tx.send(res);
    });
    Some(rx)
}

pub fn fish_stt(_path: &std::path::Path) -> Result<String, String> {
    let api_key = fish_api_key()?;
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("fish client error: {e}"))?;
    let url = "https://api.fish.audio/v1/asr";
    let bytes = fs::read(_path).map_err(|e| format!("fish stt read error: {e}"))?;
    let file_part = multipart::Part::bytes(bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("fish stt mime error: {e}"))?;
    let mut form = multipart::Form::new().part("audio", file_part);
    if let Ok(lang) = std::env::var("FISH_STT_LANG") {
        if !lang.trim().is_empty() {
            form = form.text("language", lang);
        }
    }
    form = form.text("ignore_timestamps", "true");
    let resp = client
        .post(url)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .map_err(|e| format!("fish stt http error: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_else(|_| "<no body>".to_string());
        return Err(format!("fish stt http {}: {}", status.as_u16(), body));
    }
    let value: serde_json::Value = resp.json().map_err(|e| format!("fish stt json error: {e}"))?;
    let text = value
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if text.trim().is_empty() {
        return Err("fish stt empty transcript".to_string());
    }
    Ok(text)
}

fn estimate_tts_seconds(text: &str) -> f32 {
    let chars = text.chars().count().max(1) as f32;
    chars / 13.0
}

fn tts_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RIPL_TTS_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".ripl").join("tts_cache")
}

fn tts_cache_key(text: &str, model: &str, model_id: Option<&str>) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(model.as_bytes());
    hasher.update(b"\n");
    if let Some(id) = model_id {
        hasher.update(id.as_bytes());
    }
    hasher.update(b"\n");
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn tts_cache_path(text: &str, model: &str, model_id: Option<&str>) -> PathBuf {
    let key = tts_cache_key(text, model, model_id);
    tts_cache_dir().join(format!("fish_{}.mp3", key))
}

fn afinfo_duration_seconds(path: &std::path::Path) -> Option<f32> {
    let output = Command::new("afinfo").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("estimated duration:") {
            let token = rest.trim().split_whitespace().next()?;
            if let Ok(val) = token.parse::<f32>() {
                return Some(val);
            }
        }
    }
    None
}
