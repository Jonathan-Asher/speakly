//! Post-transcription translation stage (the He→En flow): blocking HTTP to the
//! configured provider with a 10 s timeout and one retry. On failure the caller
//! pastes the untranslated source — the user's words are never eaten.

use std::time::Duration;

use serde_json::{json, Value};
use speakly_engine_types::{TranslateConfig, TranslationProvider};

const DEFAULT_SYSTEM: &str =
    "Translate the user's text to {targetLanguage}. Output only the translation, nothing else.";

/// The cleanup instruction, calibrated against real dictations: strip the
/// conversational scaffolding, keep the substance verbatim. The examples are
/// load-bearing — without them models under-clean lead-ins like "listen, so
/// basically".
const REFINE_INSTRUCTION: &str = "Turn dictated speech into the message the speaker meant to \
    write. Remove filler sounds (uh, um, אה, אמם), conversational lead-ins and discourse markers \
    (listen, so, basically, you know, I mean, תשמע, אז, כאילו, בעצם), false starts, \
    self-corrections, repetitions, and asides that aren't part of the message. Fix punctuation \
    and capitalization. Keep the speaker's own words, language, tone, and meaning — never add \
    content, answer questions, or rephrase what is already clear.\n\
    Example — input: `Listen, so basically, uh... basically the client's company.` → output: \
    `The client's company.`\n\
    Example — input: `אה… תשמע, בעצם, אני צריך לשלוח, אני צריך לשלוח את המסמך ללקוח.` → output: \
    `אני צריך לשלוח את המסמך ללקוח.`";

/// System prompt for the profile's AI stage: refine, translate, or both.
fn stage_prompt(cfg: &TranslateConfig) -> String {
    match (cfg.refine, cfg.enabled) {
        (true, true) => format!(
            "{REFINE_INSTRUCTION}\nThen translate the result to {}. Output only the clean \
             translation.",
            cfg.target_language
        ),
        (true, false) => format!("{REFINE_INSTRUCTION}\nOutput only the cleaned text."),
        // Translate-only keeps the user's custom prompt override.
        (false, _) => cfg
            .system_prompt
            .clone()
            .unwrap_or_else(|| DEFAULT_SYSTEM.to_string())
            .replace("{targetLanguage}", &cfg.target_language),
    }
}
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
    let system = stage_prompt(cfg);
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
            model.unwrap_or("openai/gpt-oss-120b"),
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
        TranslationProvider::Google if cfg.refine => Err(
            "Refine needs an LLM provider — Google can only translate. Pick Groq, OpenAI, \
             Anthropic, or a custom provider."
                .to_string(),
        ),
        TranslationProvider::Google => google(api_key, &cfg.target_language, text).map_err(|e| {
            if e.contains("403") {
                format!(
                    "{e} — enable the \"Cloud Translation API\" for your key's Google \
                         Cloud project (console.cloud.google.com → APIs & Services), and check \
                         the key's API restrictions"
                )
            } else {
                e
            }
        }),
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
        .map_err(|e| {
            if e.is_timeout() || e.is_connect() {
                format!("{provider} didn't respond — check your internet connection")
            } else {
                format!("{provider}: {e}")
            }
        })?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .map_err(|e| format!("{provider}: bad response: {e}"))?;
    if !status.is_success() {
        let detail = body["error"]["message"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| truncate(&body.to_string(), 200));
        tracing::warn!("{provider} HTTP {status}: {detail}");
        return Err(humanize_http(provider, status.as_u16(), &detail));
    }
    Ok(body)
}

/// Turn provider HTTP failures into messages a user can act on. The raw
/// detail goes to the log above; only actionable text reaches the UI.
fn humanize_http(provider: &str, status: u16, detail: &str) -> String {
    match status {
        401 | 403 => {
            let mut msg = format!(
                "{provider}: the API key was rejected — check it under Dictation → Translation"
            );
            if provider == "google" && detail.contains("blocked") {
                msg = "google: the Cloud Translation API isn't enabled for this key's project — \
                     enable it at console.cloud.google.com (APIs & Services), and check the \
                     key's API restrictions"
                    .to_string();
            }
            msg
        }
        404 => format!(
            "{provider}: that model isn't available — pick a different model in the profile's \
             translation settings ({})",
            truncate(detail, 120)
        ),
        429 => format!("{provider}: rate limited — try again in a moment"),
        500..=599 => format!("{provider} is having trouble right now — try again shortly"),
        _ => format!(
            "{provider}: request failed ({status}): {}",
            truncate(detail, 120)
        ),
    }
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

#[cfg(test)]
mod stage_tests {
    use super::*;

    fn cfg(refine: bool, enabled: bool) -> TranslateConfig {
        TranslateConfig {
            enabled,
            refine,
            provider: TranslationProvider::Groq,
            target_language: "English".into(),
            system_prompt: None,
            model: None,
            endpoint: None,
        }
    }

    #[test]
    fn refine_only_asks_for_cleanup_not_translation() {
        let p = stage_prompt(&cfg(true, false));
        assert!(p.contains("Remove filler sounds"));
        assert!(p.contains("Output only the cleaned text"));
        assert!(!p.contains("translate the result"));
    }

    #[test]
    fn combined_cleans_then_translates() {
        let p = stage_prompt(&cfg(true, true));
        assert!(p.contains("Remove filler sounds"));
        assert!(p.contains("translate the result to English"));
    }

    #[test]
    fn translate_only_keeps_the_classic_prompt() {
        let p = stage_prompt(&cfg(false, true));
        assert!(p.contains("Translate the user's text to English"));
        assert!(!p.contains("filler"));
    }

    #[test]
    fn custom_prompt_overrides_translate_only() {
        let mut c = cfg(false, true);
        c.system_prompt = Some("Say it in {targetLanguage}, pirate style.".into());
        assert_eq!(stage_prompt(&c), "Say it in English, pirate style.");
    }
}
