# RIPL Abstraction Spec

**Date:** 2026-03-13
**Status:** Draft

---

## 1. Summary
RIPL is a provider-agnostic, speech-first terminal REPL for AI conversations. It renders a distinctive breathing aura UI, supports push-to-talk, and uses lightweight project memory from local “agent MDs.” RIPL is distributed as a Rust crate and a compiled binary. Ouracle becomes a consumer via its backend API rather than embedding domain logic in the client.

---

## 2. Goals
- Standalone Rust crate + binary with minimal dependencies.
- Provider-agnostic message streaming (Anthropic, OpenAI, OpenRouter; extensible).
- Speech-first UX: PTT + STT + TTS.
- Project memory scaffold: `README.md`, `.claude/CLAUDE.md`, `skills/README.md`.
- Safe bootstrap: **default is leave files unchanged**; optional append/overwrite.
- Works as:
  - prebuilt binary (typical use)
  - `cargo install`/compile-from-source
  - library crate (`cargo add ripl`)

---

## 3. Non-goals
- Ouracle-specific states (Covenant/Inquiry/etc.).
- Complex plugin architecture or app trait system.
- Mandatory async runtime.
- GUI integrations beyond terminal TUI.

---

## 4. User experience
### 4.1 Launch
- Running `ripl` in a project directory triggers scaffold detection.
- If scaffold files are missing and `scaffold.bootstrap = true`, prompt user:
  - `Leave alone` (default)
  - `Append` (add a RIPL section)
  - `Overwrite`
  - Prompt is shown inside the aura in the TUI (not a separate CLI prompt).

### 4.2 Input
- Normal typing → line-based message.
- `/` commands for runtime configuration (voice, glyphs, etc.).
- PTT:
  - Hold Space: start recording (PTT enabled)
  - Release: stop recording, transcribe into input

### 4.3 Visual states
- `Setup`: no provider configured
- `Ready`: idle
- `Pending`: request in flight
- `Streaming`: token stream

---

## 5. Data model
```rust
pub enum Role { System, User, Assistant }

pub struct Message {
    pub role: Role,
    pub content: String,
}
```

---

## 6. Provider interface
```rust
pub enum ApiResponse {
    TokenChunk { token: String },
    TurnComplete,
    Error { message: String },
}

pub trait Provider: Send + 'static {
    fn stream(&self, messages: &[Message], tx: mpsc::Sender<ApiResponse>);
}
```

- Providers stream tokens through an mpsc channel to the UI loop.
- Providers run in worker threads; blocking HTTP is acceptable.

---

## 7. Config
### 7.1 File
`~/.ripl/config.toml`

### 7.2 Example
```
[provider]
name = "anthropic"          # anthropic | openai | openrouter
model = "claude-sonnet-4-6"
api_key = "..."              # optional if env var exists

[scaffold]
bootstrap = true
history_max_turns = 10

[theme]
root_hue = 217               # or RIPL_ROOT_HUE

[speech]
tts = "fish"                # fish | say | espeak | none
stt = "fish"                # fish | whisper | none
push_to_talk = true
fish_api_key = "..."        # or FISH_API_KEY
fish_voice_id = "..."
```

### 7.3 Resolution
Priority: config > env vars > defaults.

Env var auto-detect for provider (no config):
1. `ANTHROPIC_API_KEY`
2. `OPENAI_API_KEY`
3. `OPENROUTER_API_KEY`

If multiple keys exist and no explicit config, select in order and warn in UI.

---

## 8. Scaffold + memory
- Files read from CWD on launch:
  - `README.md`
  - `.claude/CLAUDE.md`
  - `skills/README.md`
- These are concatenated into a system prompt context block.
- No automatic summaries or memory write-back. RIPL recommends using `ctx` (ai-context-bridge) for durable project memory management.
- Summary size capped by `scaffold.history_max_turns`.

---

## 9. Session persistence
- Minimal local cache under `~/.ripl/sessions/` keyed by project hash.
- Cache contains:
  - recent turns
  - last model/provider

---

## 10. Speech
### 10.1 STT
- `whisper` (local CLI) or `fish` (Fish.audio).

### 10.2 TTS
- `say`, `espeak`, or `fish`.

### 10.3 Fish.audio
- Single module handles both STT and TTS.
- Uses `FISH_API_KEY` or `speech.fish_api_key`.

---

## 11. Module layout
```
src/
  main.rs        # terminal lifecycle + event loop
  app.rs         # state machine, input handling
  ui.rs          # rendering
  aura.rs        # breathing field + ripples
  theme.rs       # tri-hue HSL system
  config.rs      # config + env resolution
  session.rs     # local cache + history
  providers/     # anthropic/openai/openrouter
  speech/        # stt/tts/fish
```

---

## 12. Error handling
- Provider and speech errors surface in the UI status line.
- Recoverable errors do not crash the app.

---

## 13. Open questions
- None
