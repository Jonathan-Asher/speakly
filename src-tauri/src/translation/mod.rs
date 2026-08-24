//! Post-transcription translation stage (the He→En flow): blocking HTTP to the
//! configured provider with a 10 s timeout and one retry. On failure the caller
//! pastes the untranslated source — the user's words are never eaten.

use std::time::Duration;

use serde_json::{json, Value};
use speakly_engine_types::{TranslateConfig, TranslationProvider};

const DEFAULT_SYSTEM: &str =
    "Translate the user's text to {targetLanguage}. Output only the translation, nothing else.";
const TIMEOUT: Duration = Duration::from_secs(10);

pub fn provider_slug(p: TranslationProvider) -> &'static str {
    match p {
        TranslationProvider::Anthropic => "anthropic",
        TranslationProvider::Openai => "openai",
        TranslationProvider::Groq => "groq",
        TranslationProvider::Google => "google",
        TranslationProvider::Custom => "custom",
    }
}

pub fn parse_provider(s: &str) -> Option<TranslationProvider> {
    Some(match s {
        "anthropic" => TranslationProvider::Anthropic,
        "openai" => TranslationProvider::Openai,
        "groq" => TranslationProvider::Groq,
        "google" => TranslationProvider::Google,
        "custom" => TranslationProvider::Custom,
        _ => return None,
    })
}

/// Translate with one retry. Returns the translated text, trimmed.
pub fn translate(cfg: &TranslateConfig, api_key: &str, text: &str) -> Result<String, String> {
    let mut last_err = String::new();
    for attempt in 0..2 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(300));
        }
        match translate_once(cfg, api_key, text) {
            Ok(t) if !t.trim().is_empty() => return Ok(t.trim().to_string()),
            Ok(_) => last_err = "provider returned an empty translation".into(),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn translate_once(cfg: &TranslateConfig, api_key: &str, text: &str) -> Result<String, String> {
    let system = cfg
        .system_prompt
        .clone()
        .unwrap_or_else(|| DEFAULT_SYSTEM.to_string())
        .replace("{targetLanguage}", &cfg.target_language);
    let model = cfg.model.as_deref();

    match cfg.provider {
        TranslationProvider::Anthropic => {
            anthropic(api_key, model.unwrap_or("claude-sonnet-5"), &system, text)
        }
        TranslationProvider::Openai => openai_compatible(
            "https://api.openai.com/v1/chat/completions",
            api_key,
            model.unwrap_or("gpt-4o-mini"),
            &system,
            text,
        ),
        TranslationProvider::Groq => openai_compatible(
            "https://api.groq.com/openai/v1/chat/completions",
            api_key,
            model.unwrap_or("llama-3.3-70b-versatile"),
            &system,
            text,
        ),
        TranslationProvider::Custom => {
            let endpoint = cfg
                .endpoint
                .as_deref()
                .ok_or("custom provider endpoint not configured")?;
            let url = if endpoint.contains("/chat/completions") {
                endpoint.to_string()
            } else {
                format!("{}/chat/completions", endpoint.trim_end_matches('/'))
            };
            let model = model.ok_or("custom provider model not configured")?;
            openai_compatible(&url, api_key, model, &system, text)
        }
        TranslationProvider::Google => google(api_key, &cfg.target_language, text),
    }
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| format!("http client: {e}"))
}

fn post_json(
    req: reqwest::blocking::RequestBuilder,
    body: &Value,
    provider: &str,
) -> Result<Value, String> {
    let resp = req
        .header("content-type", "application/json")
        .json(body)
        .send()
        .map_err(|e| format!("{provider}: {e}"))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .map_err(|e| format!("{provider}: bad response: {e}"))?;
    if !status.is_success() {
        let detail = body["error"]["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| truncate(&body.to_string(), 200));
        return Err(format!("{provider}: HTTP {status}: {detail}"));
    }
    Ok(body)
}

/// Anthropic Messages API. No `temperature` — current Claude models reject
/// sampling params; the minimal request shape is also the most compatible.
fn anthropic(api_key: &str, model: &str, system: &str, text: &str) -> Result<String, String> {
    let body = json!({
        "model": model,
        "max_tokens": 2048,
        "system": system,
        "messages": [{ "role": "user", "content": text }],
    });
    let resp = post_json(
        client()?
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        &body,
        "anthropic",
    )?;
    if resp["stop_reason"].as_str() == Some("refusal") {
        return Err("anthropic: request refused".into());
    }
    resp["content"]
        .as_array()
        .and_then(|blocks| {
            blocks
                .iter()
                .find(|b| b["type"].as_str() == Some("text"))
                .and_then(|b| b["text"].as_str())
        })
        .map(str::to_string)
        .ok_or("anthropic: no text in response".into())
}

fn openai_compatible(
    url: &str,
    api_key: &str,
    model: &str,
    system: &str,
    text: &str,
) -> Result<String, String> {
    let body = json!({
        "model": model,
        "temperature": 0.1,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": text },
        ],
    });
    let resp = post_json(
        client()?.post(url).bearer_auth(api_key),
        &body,
        "translation provider",
    )?;
    resp["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or("translation provider: no content in response".into())
}

/// Google Cloud Translation v2 (API key). Wants ISO codes, not language names.
fn google(api_key: &str, target_language: &str, text: &str) -> Result<String, String> {
    let target = google_lang_code(target_language);
    let body = json!({ "q": text, "target": target, "format": "text" });
    let url = format!("https://translation.googleapis.com/language/translate/v2?key={api_key}");
    let resp = post_json(client()?.post(&url), &body, "google")?;
    resp["data"]["translations"][0]["translatedText"]
        .as_str()
        .map(str::to_string)
        .ok_or("google: no translation in response".into())
}

fn google_lang_code(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "english" => "en".into(),
        "hebrew" => "he".into(),
        "arabic" => "ar".into(),
        "russian" => "ru".into(),
        "french" => "fr".into(),
        "spanish" => "es".into(),
        "german" => "de".into(),
        other => other.to_string(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_slugs_round_trip() {
        for p in [
            TranslationProvider::Anthropic,
            TranslationProvider::Openai,
            TranslationProvider::Groq,
            TranslationProvider::Google,
            TranslationProvider::Custom,
        ] {
            assert_eq!(parse_provider(provider_slug(p)), Some(p));
        }
        assert_eq!(parse_provider("nope"), None);
    }

    #[test]
    fn google_codes_map_names() {
        assert_eq!(google_lang_code("English"), "en");
        assert_eq!(google_lang_code("he"), "he");
    }
}
