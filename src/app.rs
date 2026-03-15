use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::layout::Rect;

use crate::aura::{Aura, AuraGlyphMode};
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
    pub outgoing_command: Option<String>,
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
    pub provider_label: Option<String>,
    pub dev_mode: bool,
    pub pace: f32,
    pub auto_hue: bool,
    pub mouse_capture: bool,
    pub mouse_capture_dirty: bool,
    pub last_aura_area: Option<Rect>,
    root_hue_f32: f32,
    // ── Seeker fade ───────────────────────────────────────────────────────────
    pub seeker_fade_line: String,
    pub seeker_fade_ms: f32,
    pub seeker_fade_duration_ms: f32,
    // ── Priestess typewriter ──────────────────────────────────────────────────
    priestess_queue: VecDeque<char>,
    pub priestess_display: String,
    priestess_accum_ms: f32,
    pub priestess_typing: bool,
    priestess_elapsed_ms: f32,
    priestess_target_duration_ms: Option<f32>,
    priestess_line_chars: usize,
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
            outgoing_command: None,
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
            provider_label: None,
            dev_mode: false,
            pace: pace_to_scalar(5),
            auto_hue: true,
            mouse_capture: true,
            mouse_capture_dirty: false,
            last_aura_area: None,
            root_hue_f32: crate::theme::current_root_hue() as f32,
            seeker_fade_line: String::new(),
            seeker_fade_ms: 0.0,
            seeker_fade_duration_ms: 1200.0,
            priestess_queue: VecDeque::new(),
            priestess_display: String::new(),
            priestess_accum_ms: 0.0,
            priestess_typing: false,
            priestess_elapsed_ms: 0.0,
            priestess_target_duration_ms: None,
            priestess_line_chars: 0,
        }
    }

    /// The current typed text for the priestess pane (call from UI layer).
    pub fn priestess_text(&self) -> &str {
        &self.priestess_display
    }

    /// Speak and type a line — used to present history on session resume.
    pub fn greet(&mut self, raw: String) {
        self.speak_line(&raw);
        self.start_priestess(raw);
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
                    if matches!(code, KeyCode::Char('c')) && modifiers.contains(KeyModifiers::CONTROL) {
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
                                if line.starts_with('/') {
                                    self.handle_command(&line);
                                } else {
                                    self.messages.push(format!("You: {}", line));
                                    self.conversation.push(Message { role: Role::User, content: line.clone() });
                                    self.seeker_fade_line = line.clone();
                                    self.seeker_fade_ms = 1.0;
                                    self.outgoing = Some(line);
                                    self.mode = AppMode::Pending;
                                    self.session_dirty = true;
                                    self.history_offset = 0;
                                }
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
            Event::Mouse(mouse) => {
                match mouse.kind {
                    MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                        if let Some(area) = self.last_aura_area {
                            self.aura.launch_ripple_at(mouse.column, mouse.row, area, self.pace);
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        self.history_offset = self.history_offset.saturating_add(3);
                    }
                    MouseEventKind::ScrollDown => {
                        self.history_offset = self.history_offset.saturating_sub(3);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        false
    }

    pub fn on_tick(&mut self, delta: Duration) {
        if self.seeker_fade_ms > 0.0 {
            self.seeker_fade_ms += delta.as_secs_f32() * 1000.0;
            if self.seeker_fade_ms >= self.seeker_fade_duration_ms {
                self.seeker_fade_ms = 0.0;
                self.seeker_fade_line.clear();
            }
        }

        self.aura.tick(delta);

        if self.auto_hue {
            self.root_hue_f32 = (self.root_hue_f32 + delta.as_secs_f32() * 12.0) % 360.0;
            crate::theme::set_root_hue(self.root_hue_f32.round() as u16);
        }

        // Voice intensity — smooth approach to target based on current state.
        let voice_target: f32 = if self.stt_recording {
            0.0   // listening: aura quiets down (inward ripples say enough)
        } else if self.stt_transcribing {
            0.35  // processing speech: gentle aura activity
        } else if self.streaming || matches!(self.mode, AppMode::Streaming) {
            0.75  // response arriving: aura alive
        } else if matches!(self.mode, AppMode::Pending) {
            0.4   // waiting: moderate activity
        } else {
            0.0   // idle
        };
        let factor = 1.0 - (-delta.as_secs_f32() * 3.0_f32).exp();
        self.voice_intensity = (self.voice_intensity + factor * (voice_target - self.voice_intensity)).clamp(0.0, 1.0);

        // Tick-based Space PTT inference (for terminals without KeyRepeat/KeyRelease, e.g. SSH).
        if self.ptt_space_down {
            let elapsed_ms = self
                .ptt_space_last_repeat
                .map(|t| t.elapsed().as_millis())
                .unwrap_or(u128::MAX);
            let no_repeats = self.ptt_space_repeat_count == 0;

            if self.stt_active_ptt && elapsed_ms > 120 {
                // Release detected — stop recording.
                self.clear_ptt_space_state();
                self.stop_stt_recording();
            } else if !self.stt_active_ptt && no_repeats && elapsed_ms > 500 {
                // No Repeat events but held 500ms — terminal lacks keyboard enhancement.
                // Start recording; a second Press will stop it (see handle_space_ptt).
                self.start_stt_recording();
                self.stt_active_ptt = true;
                self.ptt_space_last_repeat = Some(Instant::now());
            } else if !self.stt_active_ptt && elapsed_ms > 120 {
                // Short tap (with or without a few repeats) — push space.
                let count = self.ptt_space_repeat_count;
                self.clear_ptt_space_state();
                if count < 4 {
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
                    Ok(seconds) => {
                        self.tts_error = None;
                        // Use TTS duration to sync typewriter pace.
                        self.priestess_target_duration_ms = Some(seconds * 1000.0);
                    }
                    Err(err) => {
                        self.tts_error = Some(err);
                    }
                }
                self.tts_duration_rx = None;
            }
        }

        // ── Priestess typewriter ──────────────────────────────────────────────
        if !self.priestess_queue.is_empty() || self.priestess_typing {
            let delta_ms = delta.as_secs_f32() * 1000.0;
            self.priestess_accum_ms += delta_ms;
            self.priestess_elapsed_ms += delta_ms;

            loop {
                if self.priestess_queue.is_empty() {
                    break;
                }
                let interval = self.current_char_interval_ms();
                if self.priestess_accum_ms < interval {
                    break;
                }
                self.priestess_accum_ms -= interval;
                if let Some(ch) = self.priestess_queue.pop_front() {
                    self.priestess_display.push(ch);
                }
            }

            // Update the last Assistant: line in messages in-place.
            let typed = format!("Assistant: {}", self.priestess_display);
            if let Some(last) = self.messages.last_mut() {
                if last.starts_with("Assistant:") {
                    *last = typed;
                }
            }

            if self.priestess_queue.is_empty() {
                self.priestess_typing = false;
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
        // Don't intercept space as PTT when the user is actively typing.
        if !self.input.is_empty() && !self.stt_recording {
            return false;
        }
        match kind {
            KeyEventKind::Press => {
                // Second press while recording = release (for SSH/non-kitty terminals).
                if self.stt_active_ptt && self.stt_recording {
                    self.clear_ptt_space_state();
                    self.stop_stt_recording();
                    return true;
                }
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

    fn handle_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
        match parts[0] {
            "/clear" => {
                self.messages.clear();
                self.conversation.retain(|m| m.role == Role::System);
                self.session_dirty = true;
                self.history_offset = 0;
            }
            "/reset" => {
                self.messages.clear();
                self.conversation.retain(|m| m.role == Role::System);
                self.session_dirty = true;
                self.history_offset = 0;
                self.outgoing_command = Some("/reset".to_string());
                self.mode = AppMode::Pending;
            }
            "/voice" => {
                let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");
                match arg {
                    "off" => { self.tts_mode = crate::speech::TtsMode::Off; }
                    "say" => { self.tts_mode = crate::speech::TtsMode::Say; }
                    "espeak" => { self.tts_mode = crate::speech::TtsMode::Espeak; }
                    "fish" => { self.tts_mode = crate::speech::TtsMode::Fish; }
                    _ => { self.say(format!("voice: {} (off|say|espeak|fish)", match self.tts_mode { crate::speech::TtsMode::Off => "off", crate::speech::TtsMode::Say => "say", crate::speech::TtsMode::Espeak => "espeak", crate::speech::TtsMode::Fish => "fish" })); }
                }
            }
            "/stt" => {
                let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");
                match arg {
                    "off" => { self.stt_mode = crate::speech::SttMode::Off; }
                    "whisper" => { self.stt_mode = crate::speech::SttMode::Whisper; }
                    "fish" => { self.stt_mode = crate::speech::SttMode::Fish; }
                    _ => { self.say(format!("stt: {} (off|whisper|fish)", match self.stt_mode { crate::speech::SttMode::Off => "off", crate::speech::SttMode::Whisper => "whisper", crate::speech::SttMode::Fish => "fish" })); }
                }
            }
            "/ptt" => {
                let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");
                match arg {
                    "on" => { self.push_to_talk = true; }
                    "off" => { self.push_to_talk = false; }
                    _ => { self.say(format!("ptt: {}", if self.push_to_talk { "on" } else { "off" })); }
                }
            }
            "/set" => {
                let subparts: Vec<&str> = parts.get(1).unwrap_or(&"").split_whitespace().collect();
                let key = subparts.first().copied().unwrap_or("");
                let val = subparts.get(1).copied().unwrap_or("");
                match key {
                    "color" => {
                        match val.parse::<u16>() {
                            Ok(v) if (1..=360).contains(&v) => {
                                crate::theme::set_root_hue(v);
                                self.root_hue_f32 = v as f32;
                                self.auto_hue = false;
                                self.say(format!("color → {}", v));
                            }
                            _ => { self.say("usage: /set color <1-360>".to_string()); }
                        }
                    }
                    "pace" => {
                        match val.parse::<u8>() {
                            Ok(v) if (1..=10).contains(&v) => {
                                self.pace = pace_to_scalar(v);
                                self.say(format!("pace → {}", v));
                            }
                            _ => { self.say("usage: /set pace <1-10>".to_string()); }
                        }
                    }
                    "glyph" => {
                        if val.is_empty() {
                            let next = match self.aura.glyph_mode() {
                                AuraGlyphMode::Braille  => AuraGlyphMode::Taz,
                                AuraGlyphMode::Taz      => AuraGlyphMode::Math,
                                AuraGlyphMode::Math     => AuraGlyphMode::Mahjong,
                                AuraGlyphMode::Mahjong  => AuraGlyphMode::Dominoes,
                                AuraGlyphMode::Dominoes => AuraGlyphMode::Cards,
                                AuraGlyphMode::Cards    => AuraGlyphMode::Braille,
                            };
                            let name = glyph_mode_name(next);
                            self.aura.set_glyph_mode(next);
                            self.say(format!("glyph → {}", name));
                        } else {
                            let mode = match val {
                                "braille"  => Some(AuraGlyphMode::Braille),
                                "taz"      => Some(AuraGlyphMode::Taz),
                                "math"     => Some(AuraGlyphMode::Math),
                                "mahjong"  => Some(AuraGlyphMode::Mahjong),
                                "dominoes" => Some(AuraGlyphMode::Dominoes),
                                "cards"    => Some(AuraGlyphMode::Cards),
                                _ => None,
                            };
                            match mode {
                                Some(m) => {
                                    self.aura.set_glyph_mode(m);
                                    self.say(format!("glyph → {}", val));
                                }
                                None => { self.say("usage: /set glyph [braille|taz|math|mahjong|dominoes|cards]".to_string()); }
                            }
                        }
                    }
                    _ => { self.say("usage: /set color|pace|glyph <value>".to_string()); }
                }
            }
            "/mouse" => {
                let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");
                match arg {
                    "on" => {
                        self.mouse_capture = true;
                        self.mouse_capture_dirty = true;
                    }
                    "off" => {
                        self.mouse_capture = false;
                        self.mouse_capture_dirty = true;
                    }
                    _ => { self.say(format!("mouse: {}", if self.mouse_capture { "on" } else { "off" })); }
                }
            }
            "/help" => {
                self.say([
                    "/clear — clear thread",
                    "/reset — new session",
                    "/set color <1-360> | pace <1-10> | glyph [braille|taz|math|mahjong|dominoes|cards]",
                    "/mouse [on|off]  /voice [off|say|espeak|fish]  /stt [off|whisper|fish]",
                    "/ptt [on|off] — push-to-talk",
                    "/dev [on|off] — toggle chrome",
                ].join("\n"));
            }
            "/dev" => {
                let arg = parts.get(1).map(|s| s.trim()).unwrap_or("toggle");
                match arg {
                    "on" => { self.dev_mode = true; }
                    "off" => { self.dev_mode = false; }
                    _ => { self.dev_mode = !self.dev_mode; }
                }
            }
            _ => {
                // Unknown to RIPL — forward to the provider.
                self.outgoing_command = Some(cmd.to_string());
                self.mode = AppMode::Pending;
            }
        }
    }

    pub fn take_outgoing(&mut self) -> Option<String> {
        self.outgoing.take()
    }

    pub fn take_outgoing_command(&mut self) -> Option<String> {
        self.outgoing_command.take()
    }

    pub fn handle_scaffold_input(&mut self, event: &Event) {
        let Some(selected) = self.scaffold_prompt else {
            return;
        };
        match event {
            Event::Key(KeyEvent { code, kind, .. }) if *kind == KeyEventKind::Press => {
                match code {
                    KeyCode::Char('e') | KeyCode::Char('E') => {
                        self.scaffold_prompt = Some(ScaffoldChoice::Leave);
                    }
                    KeyCode::Char('d') | KeyCode::Char('D') => {
                        self.scaffold_prompt = Some(ScaffoldChoice::Append);
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') => {
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
                    // Placeholder — replaced by typewriter on TurnComplete.
                    self.messages.push("Assistant: …".to_string());
                }
                self.assistant_buffer.push_str(&token);
            }
            ApiResponse::TurnComplete => {
                if self.streaming {
                    self.streaming = false;
                    let content = self.assistant_buffer.trim().to_string();
                    if !content.is_empty() {
                        self.conversation.push(Message { role: Role::Assistant, content: content.clone() });
                        self.session_dirty = true;
                        self.speak_line(&content);
                        self.start_priestess(content);
                    }
                }
                self.mode = AppMode::Ready;
            }
            ApiResponse::Error { message } => {
                self.streaming = false;
                self.messages.push(format!("Error: {}", message));
                self.mode = AppMode::Ready;
            }
            ApiResponse::Exit => {
                self.mode = AppMode::Ready;
            }
        }
    }

    /// Push text to the dev thread and display it via the priestess typewriter.
    fn say(&mut self, text: String) {
        self.messages.push(text.clone());
        self.start_priestess(text);
    }

    fn start_priestess(&mut self, raw: String) {
        let display = strip_audio_tags(raw.trim());
        self.priestess_line_chars = display.chars().count().max(1);
        self.priestess_queue = display.chars().collect();
        self.priestess_display.clear();
        self.priestess_accum_ms = 0.0;
        self.priestess_elapsed_ms = 0.0;
        self.priestess_target_duration_ms = None;
        self.priestess_typing = true;
        // Replace the streaming placeholder or push a new line.
        if let Some(last) = self.messages.last_mut() {
            if last.starts_with("Assistant:") {
                *last = "Assistant: ".to_string();
                return;
            }
        }
        self.messages.push("Assistant: ".to_string());
    }

    fn current_char_interval_ms(&self) -> f32 {
        if let Some(target_ms) = self.priestess_target_duration_ms {
            let typed = self.priestess_display.chars().count();
            let remaining = self.priestess_line_chars.saturating_sub(typed).max(1) as f32;
            let remaining_ms = (target_ms - self.priestess_elapsed_ms).max(0.0);
            return (remaining_ms / remaining).clamp(20.0, 200.0);
        }
        // Golden-ratio base pace: 61.8ms / pace_scalar
        (61.803_399 / self.pace).clamp(20.0, 200.0)
    }

    fn speak_line(&mut self, line: &str) {
        let text = line.trim();
        if text.is_empty() {
            return;
        }
        match self.tts_mode {
            TtsMode::Off => {}
            TtsMode::Say => {
                let clean = strip_audio_tags(text);
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("say").arg(clean).spawn();
                #[cfg(not(target_os = "macos"))]
                let _ = std::process::Command::new("espeak-ng").arg(clean)
                    .spawn()
                    .or_else(|_| std::process::Command::new("espeak").arg(clean).spawn());
            }
            TtsMode::Espeak => {
                let clean = strip_audio_tags(text);
                let _ = std::process::Command::new("espeak").arg(clean).spawn();
            }
            TtsMode::Fish => {
                // Fish.audio interprets prosody tags — enrich typography before sending.
                self.tts_error = None;
                self.tts_duration_rx = fish::spawn_fish_tts(to_fish_text(text), self.tts_voice_id.clone());
            }
        }
    }
}

fn glyph_mode_name(mode: AuraGlyphMode) -> &'static str {
    match mode {
        AuraGlyphMode::Braille  => "braille",
        AuraGlyphMode::Taz      => "taz",
        AuraGlyphMode::Math     => "math",
        AuraGlyphMode::Mahjong  => "mahjong",
        AuraGlyphMode::Dominoes => "dominoes",
        AuraGlyphMode::Cards    => "cards",
    }
}

fn pace_to_scalar(pace: u8) -> f32 {
    let p = pace.clamp(1, 10) as f32;
    0.6 + (p - 1.0) * (2.4 - 0.6) / 9.0
}

/// Convert typographic characters to Fish.audio prosody tags before TTS.
fn to_fish_text(text: &str) -> String {
    text.replace('·', " [pause] ")
}

/// Strip Fish.audio inline prosody tags (e.g. `[laugh]`, `[pause]`, `[breath]`)
/// before display or sending to speech engines that don't understand them.
/// Tags sent to Fish TTS are kept in the raw string — only call this for display
/// and non-Fish speech.
fn strip_audio_tags(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '[' {
            let mut inner = String::new();
            let mut closed = false;
            for c in chars.by_ref() {
                if c == ']' { closed = true; break; }
                inner.push(c);
            }
            let is_tag = closed
                && !inner.is_empty()
                && inner.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '=' || c == '.' || c == ' ' || c == '-' || c == ',');
            if !is_tag {
                result.push('[');
                result.push_str(&inner);
                if closed { result.push(']'); }
            }
        } else {
            result.push(ch);
        }
    }
    result.trim().to_string()
}
