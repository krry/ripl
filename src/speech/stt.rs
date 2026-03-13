use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::speech::fish;

pub fn stt_record_path() -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let key = format!("{:04x}", ts & 0xffff);
    stt_record_dir().join(format!("stt_{}_{}.wav", ts, key))
}

pub fn spawn_stt_recorder(path: &Path) -> Result<Child, String> {
    let cmd = std::env::var("RIPL_STT_RECORDER").unwrap_or_else(|_| "sox".to_string());
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("stt record dir error: {e}"))?;
    }
    let mut command = Command::new(&cmd);
    if cmd == "sox" {
        command.args(["-d", "-c", "1", "-r", "16000", "-b", "16", "-e", "signed-integer"]);
    } else if let Ok(args) = std::env::var("RIPL_STT_RECORDER_ARGS") {
        command.args(args.split_whitespace());
    }
    command.arg(path);
    command.spawn().map_err(|e| format!("stt record spawn error: {e}"))
}

pub fn spawn_stt_transcribe(path: PathBuf) -> Option<Receiver<Result<String, String>>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = whisper_transcribe(&path);
        let _ = tx.send(result);
    });
    Some(rx)
}

pub fn spawn_fish_transcribe(path: PathBuf) -> Option<Receiver<Result<String, String>>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = fish::fish_stt(&path);
        let _ = tx.send(result);
    });
    Some(rx)
}

fn whisper_transcribe(path: &Path) -> Result<String, String> {
    let cmd = whisper_cmd()?;
    let model_path = whisper_model_path()?;
    let out_dir = stt_transcript_dir();
    fs::create_dir_all(&out_dir).map_err(|e| format!("stt transcript dir error: {e}"))?;
    let key = format!("{:05x}", (SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() % 0xfffff));
    let out_base = out_dir.join(format!("stt_{}", key));
    let mut command = Command::new(cmd);
    command
        .arg("-m")
        .arg(model_path)
        .arg("-f")
        .arg(path)
        .arg("-otxt")
        .arg("-of")
        .arg(&out_base);
    if let Ok(lang) = std::env::var("RIPL_WHISPER_LANG") {
        let lang = lang.trim();
        if !lang.is_empty() {
            command.arg("-l").arg(lang);
        }
    }
    let status = command.status().map_err(|e| format!("whisper spawn error: {e}"))?;
    if !status.success() {
        return Err(format!("whisper exited with status {}", status));
    }
    let out_txt = out_base.with_extension("txt");
    let text = fs::read_to_string(&out_txt).map_err(|e| format!("whisper output read error: {e}"))?;
    Ok(clean_transcript(&text))
}

fn whisper_cmd() -> Result<String, String> {
    if let Ok(cmd) = std::env::var("RIPL_WHISPER_CMD") {
        return Ok(cmd);
    }
    for candidate in ["whisper", "whisper-cpp"] {
        if Command::new(candidate).arg("--help").output().is_ok() {
            return Ok(candidate.to_string());
        }
    }
    Err("Whisper command not found. Install whisper.cpp or set RIPL_WHISPER_CMD.".to_string())
}

fn whisper_model_path() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("RIPL_WHISPER_MODEL") {
        return Ok(PathBuf::from(path));
    }
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from("/opt/homebrew/share/whisper.cpp/models/ggml-base.en.bin"));
    candidates.push(PathBuf::from("/usr/local/share/whisper.cpp/models/ggml-base.en.bin"));
    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(home).join(".local/share/whisper.cpp/models/ggml-base.en.bin"));
    }
    for path in candidates {
        if path.exists() {
            return Ok(path);
        }
    }
    Err("Whisper model not found. Set RIPL_WHISPER_MODEL to a ggml model path.".to_string())
}

fn stt_record_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RIPL_STT_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".ripl").join("stt_recordings")
}

fn stt_transcript_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RIPL_STT_TRANSCRIPT_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".ripl").join("stt_transcripts")
}

fn clean_transcript(text: &str) -> String {
    text.lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
