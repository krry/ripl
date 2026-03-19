pub(crate) mod app;
pub mod aura;
pub mod config;
pub mod providers;
pub(crate) mod scaffold;
pub mod session;
pub mod speech;
pub mod theme;
pub(crate) mod ui;

use std::io;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::Result;
use crossterm::{
    event,
    event::{
        DisableMouseCapture, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::app::{App, AppMode};
use crate::providers::{build_provider, resolve_provider, Message, Role};
use crate::config::{scaffold_bootstrap_enabled, resolve_fish_voice_id, resolve_stt_mode, resolve_tts_mode};

// Global ambient PID — readable from the SIGTERM handler (which can't capture closures).
static AMBIENT_PID: AtomicI32 = AtomicI32::new(0);

// ─── RunOptions ───────────────────────────────────────────────────────────────

/// Configuration passed to [`run_in_terminal`] by the caller.
///
/// `provider` — the chat backend (Claude, GPT, Ouracle, …).  When `None`,
/// RIPL falls back to `~/.ripl/config.toml`.
///
/// `label` — optional status-bar label (e.g. `"Ouracle"`).
///
/// `ambient_cmd` — optional path to an ambient audio runner script/binary.
/// RIPL will spawn it at startup and kill it on exit or SIGTERM.
///
/// `voice_id` — optional Fish TTS voice ID, overrides config.
pub struct RunOptions {
    pub provider: Option<Arc<dyn providers::Provider>>,
    pub label: Option<String>,
    pub ambient_cmd: Option<PathBuf>,
    pub voice_id: Option<String>,
    pub scaffold: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions { provider: None, label: None, ambient_cmd: None, voice_id: None, scaffold: true }
    }
}

// ─── Public entry points ──────────────────────────────────────────────────────

/// Run RIPL using the provider resolved from `~/.ripl/config.toml` or env vars.
pub fn run() -> Result<()> {
    with_terminal(|t| run_in_terminal(t, RunOptions::default()))
}

/// Run RIPL with a specific provider.
pub fn run_with_provider(provider: Arc<dyn providers::Provider>, label: Option<String>) -> Result<()> {
    with_terminal(|t| run_in_terminal(t, RunOptions { provider: Some(provider), label, ..Default::default() }))
}

/// Set up the terminal (raw mode, alternate screen, mouse, kitty keyboard) and
/// run `f` inside it, restoring everything on exit, panic, or SIGTERM.
pub fn with_terminal<F>(f: F) -> Result<()>
where
    F: FnOnce(&mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()>,
{
    // Restore terminal on panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    // Restore terminal + kill ambient on SIGTERM (cargo watch / system kill).
    unsafe {
        libc::signal(libc::SIGTERM, sigterm_handler as *const () as libc::sighandler_t);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // Kitty keyboard protocol — Press/Repeat/Release needed for PTT.
    // Silently ignored by terminals that don't support it.
    let _ = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
    );
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    let res = f(&mut terminal);

    // Restore unconditionally — don't let an error in cleanup skip later steps.
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        PopKeyboardEnhancementFlags,
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();

    // Kill ambient before exit so it doesn't outlive us.
    kill_ambient();

    // Flush stdout — process::exit skips Rust's BufWriter, so escape sequences
    // (LeaveAlternateScreen etc.) must be flushed explicitly before we exit.
    let _ = io::Write::flush(&mut io::stdout());

    std::process::exit(if res.is_ok() { 0 } else { 1 });
}

/// Run the RIPL event loop inside an already-initialised terminal.
/// Use [`with_terminal`] to set one up, or call [`run`] / [`run_with_provider`]
/// which handle both steps together.
pub fn run_in_terminal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    opts: RunOptions,
) -> Result<()> {
    let cfg = config::Config::load();
    let provider = opts.provider.or_else(|| build_provider(&cfg));

    let mut app = App::new();
    app.provider = provider.clone();
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
    // Caller-supplied voice_id takes precedence over config.
    app.tts_voice_id = opts.voice_id.or_else(|| resolve_fish_voice_id(&cfg));
    app.push_to_talk = cfg.speech.as_ref().and_then(|s| s.push_to_talk).unwrap_or(true);

    if provider.is_some() {
        app.mode = AppMode::Ready;
    }

    app.provider_label = opts.label.or_else(|| {
        resolve_provider(&cfg).map(|r| format!("{} / {}", r.kind_name(), r.model))
    });

    if let Some(ctx) = scaffold::load_context() {
        app.conversation.push(Message { role: Role::System, content: ctx });
    }
    if let Some(cache) = session::load() {
        let mut last_assistant: Option<String> = None;
        for msg in &cache.conversation {
            let label = match msg.role {
                Role::User => "You",
                Role::Assistant => {
                    last_assistant = Some(msg.content.clone());
                    "Assistant"
                }
                Role::System => continue,
            };
            app.conversation.push(msg.clone());
            app.messages.push(format!("{label}: {}", msg.content));
        }
        if let Some(content) = last_assistant {
            app.greet(content);
        }
    }
    if opts.scaffold && scaffold_bootstrap_enabled(&cfg) {
        match scaffold::detect_scaffold() {
            scaffold::ScaffoldState::AutoWrite => {
                let _ = scaffold::apply_scaffold(scaffold::ScaffoldChoice::Overwrite);
            }
            scaffold::ScaffoldState::Prompt => {
                app.scaffold_prompt = Some(scaffold::ScaffoldChoice::Leave);
            }
            scaffold::ScaffoldState::NoneNeeded => {}
        }
    }

    // Spawn ambient audio if requested.  The guard kills it when dropped (normal
    // exit).  The global PID lets the SIGTERM handler kill it too.
    let _ambient = opts.ambient_cmd.and_then(|cmd| spawn_ambient(&cmd));

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

        let mut should_exit = false;
        while let Ok(resp) = resp_rx.try_recv() {
            if matches!(resp, crate::providers::ApiResponse::Exit) {
                should_exit = true;
            }
            app.handle_api_response(resp);
        }
        if should_exit {
            return Ok(());
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

// ─── Ambient audio ────────────────────────────────────────────────────────────

struct AmbientGuard(Child);

impl Drop for AmbientGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        AMBIENT_PID.store(0, Ordering::Relaxed);
    }
}

fn spawn_ambient(cmd: &PathBuf) -> Option<AmbientGuard> {
    if !cmd.exists() {
        return None;
    }
    // .js scripts are run with bun; everything else is executed directly.
    let child = if cmd.extension().and_then(|e| e.to_str()) == Some("js") {
        let bun = std::env::var("BUN_PATH")
            .unwrap_or_else(|_| "bun".to_string());
        std::process::Command::new(bun)
            .arg(cmd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    } else {
        std::process::Command::new(cmd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    };
    match child {
        Ok(c) => {
            AMBIENT_PID.store(c.id() as i32, Ordering::Relaxed);
            Some(AmbientGuard(c))
        }
        Err(_) => None,
    }
}

// ─── Signal handling ──────────────────────────────────────────────────────────

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
}

fn kill_ambient() {
    let pid = AMBIENT_PID.load(Ordering::Relaxed);
    if pid > 0 {
        unsafe { libc::kill(pid, libc::SIGTERM); }
        AMBIENT_PID.store(0, Ordering::Relaxed);
    }
}

extern "C" fn sigterm_handler(_: libc::c_int) {
    restore_terminal();
    kill_ambient();
    std::process::exit(0);
}
