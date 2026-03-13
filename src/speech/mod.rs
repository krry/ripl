pub mod stt;
pub mod tts;
pub mod fish;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsMode {
    Fish,
    Say,
    Espeak,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttMode {
    Fish,
    Whisper,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeechConfig {
    pub tts: TtsMode,
    pub stt: SttMode,
    pub push_to_talk: bool,
}

impl Default for SpeechConfig {
    fn default() -> Self {
        SpeechConfig {
            tts: TtsMode::Say,
            stt: SttMode::Whisper,
            push_to_talk: true,
        }
    }
}
