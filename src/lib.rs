pub mod app;
pub mod aura;
pub mod config;
pub mod providers;
pub mod scaffold;
pub mod session;
pub mod speech;
pub mod theme;
pub mod ui;

use std::io;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::{
    event,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{App, AppMode};
use crate::providers::{build_provider, resolve_provider, Message, Role};
use crate::config::{scaffold_bootstrap_enabled, resolve_fish_voice_id, resolve_stt_mode, resolve_tts_mode};

/// Run RIPL using the provider resolved from `~/.ripl/config.toml` or env vars.
pub fn run() -> Result<()> {
    with_terminal(|t| run_in_terminal(t, None, None))
}

/// Run RIPL with a custom provider, bypassing config-based provider resolution.
/// `label` is shown in the status bar (e.g. `"Ouracle / dev"`).
pub fn run_with_provider(provider: Arc<dyn providers::Provider>, label: Option<String>) -> Result<()> {
    with_terminal(|t| run_in_terminal(t, Some(provider), label))
}

pub fn with_terminal<F>(f: F) -> Result<()>
where
    F: FnOnce(&mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()>,
{
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    let res = f(&mut terminal);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    res
}

pub fn run_in_terminal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    override_provider: Option<Arc<dyn providers::Provider>>,
    override_label: Option<String>,
) -> Result<()> {
    let cfg = config::Config::load();
    let provider = override_provider.or_else(|| build_provider(&cfg));

    let mut app = App::new();
    if std::env::var("RIPL_DEV").is_ok() {
        app.dev_mode = true;
    }
    app.tts_mode = match resolve_tts_mode(&cfg).as_str() {
        "fish" => speech::TtsMode::Fish,
        "espeak" => speech::TtsMode::Espeak,
        "off" => speech::TtsMode::Off,
        _ => speech::TtsMode::Say,
    };
    app.stt_mode = match resolve_stt_mode(&cfg).as_str() {
        "fish" => speech::SttMode::Fish,
        "off" => speech::SttMode::Off,
        _ => speech::SttMode::Whisper,
    };
    app.tts_voice_id = resolve_fish_voice_id(&cfg);
    app.push_to_talk = cfg.speech.as_ref().and_then(|s| s.push_to_talk).unwrap_or(true);

    if provider.is_some() {
        app.mode = AppMode::Ready;
    }

    app.provider_label = override_label.or_else(|| {
        resolve_provider(&cfg).map(|r| format!("{} / {}", r.kind_name(), r.model))
    });

    if let Some(ctx) = scaffold::load_context() {
        app.conversation.push(Message { role: Role::System, content: ctx });
    }
    if let Some(cache) = session::load() {
        for msg in &cache.conversation {
            let label = match msg.role {
                Role::User => "You",
                Role::Assistant => "Assistant",
                Role::System => continue,
            };
            app.conversation.push(msg.clone());
            app.messages.push(format!("{label}: {}", msg.content));
        }
    }
    if scaffold_bootstrap_enabled(&cfg) && scaffold::detect_and_prompt().is_some() {
        app.scaffold_prompt = Some(scaffold::ScaffoldChoice::Leave);
    }

    let (resp_tx, resp_rx) = mpsc::channel();
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(100);

    loop {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::from_secs(0));

        if event::poll(timeout)? {
            let ev = event::read()?;
            if app.scaffold_prompt.is_some() {
                app.handle_scaffold_input(&ev);
                if app.scaffold_prompt.is_some() {
                    continue;
                }
            }
            if app.on_event(&ev) {
                return Ok(());
            }
        }

        if app.mouse_capture_dirty {
            app.mouse_capture_dirty = false;
            if app.mouse_capture {
                let _ = execute!(terminal.backend_mut(), EnableMouseCapture);
            } else {
                let _ = execute!(terminal.backend_mut(), DisableMouseCapture);
            }
        }

        if let Some(choice) = app.take_scaffold_choice() {
            let _ = scaffold::apply_scaffold(choice);
        }

        if let Some(_line) = app.take_outgoing() {
            if let Some(p) = provider.clone() {
                let tx = resp_tx.clone();
                let messages = app.conversation.clone();
                thread::spawn(move || {
                    p.stream(&messages, tx);
                });
            } else {
                app.messages.push("No provider configured. Run: ripl pair anthropic".to_string());
                app.mode = AppMode::Setup;
            }
        }

        if let Some(cmd) = app.take_outgoing_command() {
            if let Some(p) = provider.clone() {
                let tx = resp_tx.clone();
                thread::spawn(move || {
                    p.handle_command(&cmd, tx);
                });
            }
        }

        while let Ok(resp) = resp_rx.try_recv() {
            app.handle_api_response(resp);
        }

        if app.session_dirty {
            session::save(&session::SessionCache {
                conversation: app.conversation.clone(),
                provider: None,
                model: None,
            });
            app.session_dirty = false;
        }

        if last_tick.elapsed() >= tick_rate {
            app.on_tick(last_tick.elapsed());
            last_tick = Instant::now();
        }
    }
}
