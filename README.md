# ripl

A terminal AI chat client and TUI framework for macOS.

ripl provides a full-featured ratatui event loop wired to any LLM provider — Anthropic, OpenAI, Ollama, OpenRouter — with Fish Audio TTS/STT, ambient audio, hue-shifting themes, and a session cache. It ships as both a standalone binary (`ripl`) and a library for building your own AI terminal experiences on top of it.

**macOS only.** Audio features depend on `afplay` and `afinfo`.

---

## Install

```sh
cargo install ripl
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

## Use as a library

```toml
# Cargo.toml
[dependencies]
ripl = "0.3"
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
