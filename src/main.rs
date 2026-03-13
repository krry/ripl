use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::{
    event,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

mod app;
mod aura;
mod config;
mod providers;
mod scaffold;
mod speech;
mod session;
mod theme;
mod ui;

use crate::app::{App, AppMode};
use crate::providers::{build_provider, resolve_provider, Message, Role};
use crate::config::scaffold_bootstrap_enabled;
use crate::config::{resolve_fish_voice_id, resolve_stt_mode, resolve_tts_mode};

fn main() -> Result<()> {
    color_eyre::install()?;
    let args: Vec<String> = std::env::args().collect();
    if let Some(cmd) = args.get(1).map(|s| s.as_str()) {
        match cmd {
            "config" => {
                config::open_config_file()?;
                return Ok(());
            }
            "pair" => {
                let provider = args.get(2).map(|s| s.as_str()).unwrap_or("");
                config::pair_provider(provider)?;
                return Ok(());
            }
            _ => {}
        }
    }
    run()
}

fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let res = app_loop(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

fn app_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let cfg = config::Config::load();
    let provider = build_provider(&cfg);
    let mut app = App::new();
    app.tts_mode = match resolve_tts_mode(&cfg).as_str() {
        "fish" => crate::speech::TtsMode::Fish,
        "espeak" => crate::speech::TtsMode::Espeak,
        "off" => crate::speech::TtsMode::Off,
        _ => crate::speech::TtsMode::Say,
    };
    app.stt_mode = match resolve_stt_mode(&cfg).as_str() {
        "fish" => crate::speech::SttMode::Fish,
        "off" => crate::speech::SttMode::Off,
        _ => crate::speech::SttMode::Whisper,
    };
    app.tts_voice_id = resolve_fish_voice_id(&cfg);
    app.push_to_talk = cfg
        .speech
        .as_ref()
        .and_then(|s| s.push_to_talk)
        .unwrap_or(true);
    if provider.is_some() {
        app.mode = AppMode::Ready;
    }
    if let Some(resolved) = resolve_provider(&cfg) {
        app.provider_label = Some(format!("{} / {}", resolved.kind_name(), resolved.model));
    }
    // Prepend scaffold context as system message (always fresh from CWD files).
    if let Some(ctx) = scaffold::load_context() {
        app.conversation.push(Message { role: Role::System, content: ctx });
    }
    if let Some(cache) = session::load() {
        // Session only contains user/assistant messages (system filtered on save).
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

        if let Some(choice) = app.take_scaffold_choice() {
            let _ = scaffold::apply_scaffold(choice);
        }

        if let Some(_line) = app.take_outgoing() {
            if let Some(provider) = provider.clone() {
                let tx = resp_tx.clone();
                let messages = app.conversation.clone();
                thread::spawn(move || {
                    provider.stream(&messages, tx);
                });
            } else {
                app.messages.push("No provider configured. Set an API key.".to_string());
                app.mode = AppMode::Setup;
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
