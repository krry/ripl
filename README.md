# ripl

A shell-based agent chat client and TUI framework for macOS.

ripl provides a full-featured ratatui event loop wired to any LLM provider — Anthropic, OpenAI, Ollama, OpenRouter — with Fish Audio TTS/STT, ambient audio, hue-shifting themes, and a session cache. It ships as both a standalone binary (`ripl`) and a library for building your own AI terminal experiences on top of it.

**macOS only for now.** Audio features depend on `afplay` and `afinfo`.

---

## Install

```sh
cargo install ripl-tui
```

Requires a provider API key in your environment or `~/.ripl/config.toml`.

---

## Configure

```sh
ripl config          # open ~/.ripl/config.toml in $EDITOR
ripl pair anthropic
ripl pair openai
ripl pair openrouter
```

```toml
# ~/.ripl/config.toml
[provider]
name = "anthropic"
model = "claude-sonnet-4-5"

[speech]
tts = "fish"    # or "say"
stt = "fish"    # or "whisper"

[theme]
root_hue = 217
```

---

## Environment variables

ripl reads these from your shell — no config file needed to get started.

| Variable | Purpose |
|---|---|
| `ANTHROPIC_API_KEY` | Anthropic / Claude provider key |
| `OPENAI_API_KEY` | OpenAI provider key |
| `OPENROUTER_API_KEY` | OpenRouter provider key |
| `FISH_AUDIO_API_KEY` | Fish Audio TTS/STT key (alias: `FISH_API_KEY`) |
| `FISH_AUDIO_VOICE_ID` | Fish Audio voice ID (alias: `FISH_VOICE_ID`) |
| `FISH_AUDIO_MODEL` | Fish TTS model override (alias: `FISH_TTS_MODEL`) |
| `FISH_STT_LANG` | Fish STT language code (e.g. `en`) |
| `RIPL_ROOT_HUE` | Theme hue 0–360 (overrides config) |
| `RIPL_DEV` | Enable dev mode (any value) |
| `RIPL_WHISPER_CMD` | Path to `whisper` binary |
| `RIPL_WHISPER_MODEL` | Path to Whisper model file |
| `RIPL_WHISPER_LANG` | Whisper language code |
| `RIPL_STT_RECORDER` | Audio recorder command (default: `sox`) |
| `RIPL_TTS_CACHE_DIR` | TTS audio cache directory |
| `BUN_PATH` | Path to `bun` for ambient scripts |

---

## Use as a library

```toml
# Cargo.toml
[dependencies]
ripl-tui = "0.3"
```

```rust
use std::sync::Arc;
use ripl::{RunOptions, with_terminal, run_in_terminal};
use ripl::providers::Provider;

fn main() -> color_eyre::eyre::Result<()> {
    let provider: Arc<dyn Provider> = // your provider
    with_terminal(|terminal| {
        run_in_terminal(terminal, RunOptions {
            provider: Some(provider),
            label: Some("My App".to_string()),
            scaffold: false,
            ..Default::default()
        })
    })
}
```

---

## Built with ripl

- [clea](https://clea.kerry.ink) — Chief Priestess of Ouracle

---

## License

MIT
