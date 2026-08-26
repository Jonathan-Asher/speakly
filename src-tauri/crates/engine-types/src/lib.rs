//! Serde types shared between the engine, the Tauri app layer, and (via
//! specta-generated bindings) the TypeScript frontend. This crate is the single
//! source of truth for languages, models, and event payloads.

use serde::{Deserialize, Serialize};

/// Identifier of a model in the registry (e.g. "he-turbo", "turbo", "turbo-q8").
pub type ModelId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationMode {
    Hold,
    Toggle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateConfig {
    /// Translate the transcript to `target_language`.
    pub enabled: bool,
    /// Clean the transcript up before pasting: drop filler sounds, false
    /// starts, and conversational scaffolding. Independent of `enabled`.
    #[serde(default)]
    pub refine: bool,
    pub provider: TranslationProvider,
    pub target_language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Provider model override (each provider has a sensible default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Base URL for the `custom` OpenAI-compatible provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationProvider {
    Anthropic,
    Openai,
    Groq,
    Google,
    Custom,
}

/// A dictation profile: one global hotkey bound to a language + model +
/// optional translation stage (e.g. "He→En" = ivrit turbo, lang he, translate to English).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub hotkey: String,
    pub mode: DictationMode,
    pub language: String,
    pub model_id: ModelId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translate: Option<TranslateConfig>,
    #[serde(default = "default_true")]
    pub auto_paste: bool,
    #[serde(default = "default_true")]
    pub restore_clipboard: bool,
}

fn default_true() -> bool {
    true
}

/// One timestamped piece of a transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationPhase {
    Idle,
    Listening,
    Transcribing,
    Translating,
    Pasting,
    Error,
}
