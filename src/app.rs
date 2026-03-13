use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};

use crate::aura::Aura;
use crate::providers::{ApiResponse, Message, Role};
use crate::scaffold::ScaffoldChoice;
use crate::speech::{fish, TtsMode, SttMode};
use crate::speech::stt as stt_engine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Setup,
    Ready,
    Pending,
    Streaming,
}

pub struct App {
    pub mode: AppMode,
    pub input: String,
    pub messages: Vec<String>,
    pub conversation: Vec<Message>,
    pub aura: Aura,
    pub voice_intensity: f32,
    pub outgoing: Option<String>,
    pub stt_recording: bool,
    pub stt_transcribing: bool,
    pub stt_error: Option<String>,
    pub tts_error: Option<String>,
    pub tts_duration_rx: Option<Receiver<Result<f32, String>>>,
    pub tts_mode: TtsMode,
    pub stt_mode: SttMode,
    pub tts_voice_id: Option<String>,
    pub push_to_talk: bool,
    pub stt_recorder: Option<Child>,
    pub stt_record_path: Option<PathBuf>,
    pub stt_transcribe_rx: Option<Receiver<Result<String, String>>>,
    pub streaming: bool,
    pub assistant_buffer: String,
    pub session_dirty: bool,
    pub ptt_space_down: bool,
    pub ptt_space_repeat_count: u32,
    pub ptt_space_last_repeat: Option<Instant>,
    pub stt_active_ptt: bool,
    pub stt_ripple_accum_ms: f32,
    pub scaffold_prompt: Option<ScaffoldChoice>,
    pub scaffold_choice: Option<ScaffoldChoice>,
    pub history_offset: usize,
}

impl App {
    pub fn new() -> Self {
        let tts_mode = if fish::has_fish_key() {
            TtsMode::Fish
        } else {
            TtsMode::Say
        };
        let stt_mode = if fish::has_fish_key() {
            SttMode::Fish
        } else {
            SttMode::Whisper
        };
        App {
            mode: AppMode::Setup,
            input: String::new(),
            messages: Vec::new(),
            conversation: Vec::new(),
            aura: Aura::new(),
            voice_intensity: 0.0,
            outgoing: None,
            stt_recording: false,
            stt_transcribing: false,
            stt_error: None,
            stt_recorder: None,
            stt_record_path: None,
            stt_transcribe_rx: None,
            tts_error: None,
            tts_duration_rx: None,
            tts_mode,
            stt_mode,
            tts_voice_id: None,
            push_to_talk: true,
            streaming: false,
            assistant_buffer: String::new(),
            session_dirty: false,
            ptt_space_down: false,
            ptt_space_repeat_count: 0,
            ptt_space_last_repeat: None,
            stt_active_ptt: false,
            stt_ripple_accum_ms: 0.0,
            scaffold_prompt: None,
            scaffold_choice: None,
            history_offset: 0,
        }
    }

    pub fn on_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Key(KeyEvent { code, modifiers, kind, .. }) => {
                if self.handle_space_ptt(*code, *kind) {
                    return false;
                }
                if *kind == KeyEventKind::Press {
                    if matches!(code, KeyCode::Char('q')) && modifiers.is_empty() {
                        return true;
                    }
                    match code {
                        KeyCode::PageUp => {
                            self.history_offset = self.history_offset.saturating_add(5);
                        }
                        KeyCode::PageDown => {
                            self.history_offset = self.history_offset.saturating_sub(5);
                        }
                        KeyCode::End => {
                            self.history_offset = 0;
                        }
                        KeyCode::Enter => {
                            let line = self.input.trim().to_string();
                            self.input.clear();
                            if !line.is_empty() {
                                self.messages.push(format!("You: {}", line));
                                self.conversation.push(Message { role: Role::User, content: line.clone() });
                                self.outgoing = Some(line);
                                self.mode = AppMode::Pending;
                                self.session_dirty = true;
                                self.history_offset = 0;
                            }
                        }
                        KeyCode::Backspace => {
                            self.input.pop();
                        }
                        KeyCode::Char(c) => {
                            self.input.push(*c);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        false
    }

    pub fn on_tick(&mut self, delta: Duration) {
        self.aura.tick(delta);

        // Tick-based Space PTT release inference (for terminals without KeyRelease).
        if self.ptt_space_down {
            let elapsed_ms = self
                .ptt_space_last_repeat
                .map(|t| t.elapsed().as_millis())
                .unwrap_or(u128::MAX);
            if elapsed_ms > 300 {
                let was_active_ptt = self.stt_active_ptt;
                let count = self.ptt_space_repeat_count;
                self.clear_ptt_space_state();
                if was_active_ptt {
                    self.stop_stt_recording();
                } else if count < 4 {
                    self.input.push(' ');
                }
            }
        }

        // Inward ripple launch cadence while recording.
        if self.stt_recording {
            self.stt_ripple_accum_ms += delta.as_secs_f32() * 1000.0;
            if self.stt_ripple_accum_ms >= 500.0 {
                self.stt_ripple_accum_ms -= 500.0;
                self.aura.launch_inward_ripple();
            }
        } else {
            self.stt_ripple_accum_ms = 0.0;
        }

        if let Some(rx) = &self.stt_transcribe_rx {
            if let Ok(result) = rx.try_recv() {
                self.stt_transcribe_rx = None;
                self.stt_transcribing = false;
                match result {
                    Ok(text) => {
                        let text = text.trim();
                        if !text.is_empty() {
                            if !self.input.is_empty() && !self.input.ends_with(' ') {
                                self.input.push(' ');
                            }
                            self.input.push_str(text);
                        }
                    }
                    Err(err) => {
                        self.stt_error = Some(err);
                    }
                }
            }
        }

        if let Some(rx) = &self.tts_duration_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(_) => {
                        self.tts_error = None;
                    }
                    Err(err) => {
                        self.tts_error = Some(err);
                    }
                }
                self.tts_duration_rx = None;
            }
        }
    }

    fn start_stt_recording(&mut self) {
        if self.stt_recording {
            return;
        }
        let path = stt_engine::stt_record_path();
        let child = match stt_engine::spawn_stt_recorder(&path) {
            Ok(child) => child,
            Err(err) => {
                self.stt_error = Some(err);
                return;
            }
        };
        self.stt_error = None;
        self.stt_recording = true;
        self.stt_transcribing = false;
        self.stt_recorder = Some(child);
        self.stt_record_path = Some(path);
    }

    fn stop_stt_recording(&mut self) {
        if !self.stt_recording {
            return;
        }
        self.stt_recording = false;
        self.stt_transcribing = true;
        if let Some(mut child) = self.stt_recorder.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let Some(path) = self.stt_record_path.take() else {
            return;
        };
        self.stt_transcribe_rx = match self.stt_mode {
            SttMode::Whisper => stt_engine::spawn_stt_transcribe(path),
            SttMode::Fish => stt_engine::spawn_fish_transcribe(path),
            SttMode::Off => None,
        };
    }

    fn clear_ptt_space_state(&mut self) {
        self.ptt_space_down = false;
        self.ptt_space_repeat_count = 0;
        self.ptt_space_last_repeat = None;
        self.stt_active_ptt = false;
    }

    /// Returns true if the event was consumed by the Space PTT handler.
    pub fn handle_space_ptt(&mut self, code: KeyCode, kind: KeyEventKind) -> bool {
        if !self.push_to_talk || matches!(self.stt_mode, SttMode::Off) {
            return false;
        }
        if code != KeyCode::Char(' ') {
            return false;
        }
        match kind {
            KeyEventKind::Press => {
                self.ptt_space_down = true;
                self.ptt_space_repeat_count = 0;
                self.ptt_space_last_repeat = Some(Instant::now());
                true
            }
            KeyEventKind::Repeat => {
                if !self.ptt_space_down {
                    return false;
                }
                let elapsed_ms = self
                    .ptt_space_last_repeat
                    .map(|t| t.elapsed().as_millis())
                    .unwrap_or(u128::MAX);
                if elapsed_ms >= 150 {
                    self.ptt_space_down = false;
                    self.ptt_space_repeat_count = 0;
                    self.ptt_space_last_repeat = None;
                    return false;
                }
                self.ptt_space_last_repeat = Some(Instant::now());
                self.ptt_space_repeat_count += 1;
                if self.ptt_space_repeat_count >= 4 && !self.stt_recording {
                    self.start_stt_recording();
                    self.stt_active_ptt = true;
                }
                true
            }
            KeyEventKind::Release => {
                if !self.ptt_space_down {
                    return false;
                }
                let was_active_ptt = self.stt_active_ptt;
                let count = self.ptt_space_repeat_count;
                self.clear_ptt_space_state();
                if was_active_ptt {
                    self.stop_stt_recording();
                } else if count < 4 {
                    self.input.push(' ');
                }
                true
            }
        }
    }

    pub fn take_outgoing(&mut self) -> Option<String> {
        self.outgoing.take()
    }

    pub fn handle_scaffold_input(&mut self, event: &Event) {
        let Some(selected) = self.scaffold_prompt else {
            return;
        };
        match event {
            Event::Key(KeyEvent { code, kind, .. }) if *kind == KeyEventKind::Press => {
                match code {
                    KeyCode::Char('l') | KeyCode::Char('L') => {
                        self.scaffold_prompt = Some(ScaffoldChoice::Leave);
                    }
                    KeyCode::Char('a') | KeyCode::Char('A') => {
                        self.scaffold_prompt = Some(ScaffoldChoice::Append);
                    }
                    KeyCode::Char('o') | KeyCode::Char('O') => {
                        self.scaffold_prompt = Some(ScaffoldChoice::Overwrite);
                    }
                    KeyCode::Enter => {
                        self.scaffold_choice = Some(selected);
                        self.scaffold_prompt = None;
                    }
                    KeyCode::Esc => {
                        self.scaffold_choice = Some(ScaffoldChoice::Leave);
                        self.scaffold_prompt = None;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub fn take_scaffold_choice(&mut self) -> Option<ScaffoldChoice> {
        self.scaffold_choice.take()
    }

    pub fn handle_api_response(&mut self, resp: ApiResponse) {
        match resp {
            ApiResponse::TokenChunk { token } => {
                if !self.streaming {
                    self.streaming = true;
                    self.assistant_buffer.clear();
                    self.mode = AppMode::Streaming;
                }
                self.assistant_buffer.push_str(&token);
                self.update_streaming_line();
            }
            ApiResponse::TurnComplete => {
                if self.streaming {
                    self.streaming = false;
                    let content = self.assistant_buffer.trim().to_string();
                    if !content.is_empty() {
                        self.conversation.push(Message { role: Role::Assistant, content: content.clone() });
                        self.session_dirty = true;
                        self.speak_line(&content);
                    }
                    self.update_streaming_line();
                }
                self.mode = AppMode::Ready;
            }
            ApiResponse::Error { message } => {
                self.streaming = false;
                self.messages.push(format!("Error: {}", message));
                self.mode = AppMode::Ready;
            }
        }
    }

    fn update_streaming_line(&mut self) {
        let line = if self.assistant_buffer.trim().is_empty() {
            String::new()
        } else {
            format!("Assistant: {}", self.assistant_buffer.trim())
        };
        if let Some(last) = self.messages.last_mut() {
            if last.starts_with("Assistant:") {
                *last = line;
                return;
            }
        }
        if !line.is_empty() {
            self.messages.push(line);
        }
    }

    fn speak_line(&mut self, line: &str) {
        let text = line.trim();
        if text.is_empty() {
            return;
        }
        match self.tts_mode {
            TtsMode::Off => {}
            TtsMode::Say => {
                let _ = std::process::Command::new("say").arg(text).spawn();
            }
            TtsMode::Espeak => {
                let _ = std::process::Command::new("espeak").arg(text).spawn();
            }
            TtsMode::Fish => {
                self.tts_error = None;
                self.tts_duration_rx = fish::spawn_fish_tts(text.to_string(), self.tts_voice_id.clone());
            }
        }
    }
}
