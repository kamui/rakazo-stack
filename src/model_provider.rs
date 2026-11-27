//! The Model provider (spec §8): which one, the check of its key, and OpenRouter's ZDR
//! only probe. Each request is judged by a pure function of the reply's status and body.
//!
//! Replies are never printed: OpenAI's refusal quotes part of the key.

use serde::{Deserialize, Serialize};

use crate::answers::FrontEnd;
use crate::secrets::{PastedKey, Secrets};

pub const PROVIDER_VAR: &str = "RAKAZO_STACK_MODEL_PROVIDER";

pub const GEMINI_BILLING: &str =
    "A Gemini key must be billing-enabled: Google uses free-tier data to improve its products.";

const ZDR_OFF: &str = "ZDR only is off in OpenRouter, so Rakazo's chat can go to providers that keep data. Turn it on at openrouter.ai/workspaces/default/guardrails.";
const ZDR_OFF_ITEM: &str = "Turn on ZDR only in OpenRouter (openrouter.ai/workspaces/default/guardrails): setup found it off, so Rakazo's chat can go to providers that keep data.";
const ZDR_UNKNOWN_ITEM: &str = "Turn on ZDR only in OpenRouter (openrouter.ai/workspaces/default/guardrails): setup couldn't check whether it's on.";
const NO_KEY_ITEM: &str = "No Model provider key: each person connects a provider in Settings → Models. Connect one as the Owner before creating a bot, because Rakazo's onboarding stops at \"Connect a model\".";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenRouter,
    OpenAi,
    Gemini,
    Anthropic,
}

impl Provider {
    pub const ALL: [Self; 4] = [
        Self::OpenRouter,
        Self::OpenAi,
        Self::Gemini,
        Self::Anthropic,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::Anthropic => "anthropic",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::OpenAi => "OpenAI",
            Self::Gemini => "Gemini",
            Self::Anthropic => "Anthropic",
        }
    }

    /// The key's name in `secrets.env`, and in Rakazo's env where Rakazo reads it.
    pub fn key_name(self) -> &'static str {
        match self {
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::OpenAi => "OPENAI_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
            Self::Anthropic => "ANTHROPIC_API_KEY",
        }
    }

    pub fn api_base(self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai",
            Self::OpenAi => "https://api.openai.com",
            Self::Gemini => "https://generativelanguage.googleapis.com",
            Self::Anthropic => "https://api.anthropic.com",
        }
    }

    /// This program version's chat model for the whole Instance. OpenAI and Gemini have
    /// none: Rakazo connects them per user.
    fn instance_model(self) -> Option<&'static str> {
        match self {
            Self::OpenRouter => Some("~z-ai/glm-flash-latest"),
            Self::Anthropic => Some("claude-haiku-5-5"),
            Self::OpenAi | Self::Gemini => None,
        }
    }
}

/// The answer to the Model provider question; `Choice(None)` is "none".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Choice(pub Option<Provider>);

impl Default for Choice {
    fn default() -> Self {
        Self(Some(Provider::OpenRouter))
    }
}

impl Choice {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        if text == "none" {
            return Some(Self(None));
        }
        Provider::ALL
            .into_iter()
            .find(|provider| provider.id() == text)
            .map(|provider| Self(Some(provider)))
    }

    pub fn id(self) -> &'static str {
        self.0.map_or("none", Provider::id)
    }
}

impl From<Choice> for String {
    fn from(choice: Choice) -> Self {
        choice.id().to_string()
    }
}

impl TryFrom<String> for Choice {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text).ok_or_else(|| "unknown Model provider".to_string())
    }
}

/// `secrets.env` holds the key of the chosen provider only.
pub fn save_key(secrets: &mut Secrets, chosen: Option<(Provider, &PastedKey)>) {
    for provider in Provider::ALL {
        let key = chosen.and_then(|(chosen, key)| (chosen == provider).then_some(key));
        secrets.set_pasted(provider.key_name(), key);
    }
}

/// Rakazo's Instance-wide chat model, which setup writes to `.env`.
#[derive(Debug, Clone, Copy)]
pub struct ChatModel<'a> {
    pub provider: Provider,
    pub key: &'a PastedKey,
    pub model: &'a str,
}

impl<'a> ChatModel<'a> {
    /// `None` for a provider Rakazo can't take from env. `override_model` is
    /// `settings.json`'s, which wins over the program's default.
    pub fn new(
        provider: Provider,
        key: &'a PastedKey,
        override_model: Option<&'a str>,
    ) -> Result<Option<Self>, String> {
        let Some(default) = provider.instance_model() else {
            return Ok(None);
        };
        let model = override_model.unwrap_or(default);
        if !is_model_id(model) {
            return Err("settings.json's rakazo_model isn't a model id.".to_string());
        }
        Ok(Some(Self {
            provider,
            key,
            model,
        }))
    }
}

/// A model id that can go into a `.env` line unquoted.
pub fn is_model_id(model: &str) -> bool {
    !model.is_empty()
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "~-_./:@".contains(c))
}

/// The Checklist item about the key itself, if any.
pub fn key_item(chosen: Option<Provider>) -> Option<String> {
    match chosen {
        None => Some(NO_KEY_ITEM.to_string()),
        Some(provider @ (Provider::OpenAi | Provider::Gemini)) => Some(format!(
            "Setup saved your {} key but doesn't connect it to Rakazo's chat yet: each person, the Owner first, connects a provider in Settings → Models.",
            provider.name()
        )),
        Some(Provider::OpenRouter | Provider::Anthropic) => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyCheck {
    Accepted,
    Rejected(u16),
    /// No verdict either way, with the reason.
    Unchecked(String),
}

fn judge_key(provider: Provider, status: u16, body: &str) -> KeyCheck {
    match (provider, status) {
        (_, 200) => KeyCheck::Accepted,
        // A restricted OpenAI key that may not list models is still a working key.
        (Provider::OpenAi, 401 | 403) if body.contains("Missing scopes") => KeyCheck::Accepted,
        // Gemini answers a wrong key with 400.
        (Provider::Gemini, 400) if body.contains("API_KEY_INVALID") => KeyCheck::Rejected(status),
        (_, 401 | 403) => KeyCheck::Rejected(status),
        _ => KeyCheck::Unchecked(format!("HTTP {status}")),
    }
}

/// Asks the provider, at `base`, about the key with a request that costs nothing.
pub fn check_key(agent: &ureq::Agent, base: &str, provider: Provider, key: &PastedKey) -> KeyCheck {
    let bearer = format!("Bearer {}", key.expose());
    let (path, header, value) = match provider {
        // `/models` is public on OpenRouter; `/key` describes the calling key.
        Provider::OpenRouter => ("/api/v1/key", "authorization", bearer.as_str()),
        Provider::OpenAi => ("/v1/models", "authorization", bearer.as_str()),
        Provider::Gemini => ("/v1beta/models", "x-goog-api-key", key.expose()),
        Provider::Anthropic => ("/v1/models", "x-api-key", key.expose()),
    };
    // A redirect would carry the `x-…` key headers to wherever it points.
    let mut request = agent
        .get(format!("{base}{path}"))
        .config()
        .max_redirects(0)
        .build()
        .header(header, value);
    if provider == Provider::Anthropic {
        request = request.header("anthropic-version", "2023-06-01");
    }
    match request.call() {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let body = response.body_mut().read_to_string().unwrap_or_default();
            judge_key(provider, status, &body)
        }
        Err(err) => KeyCheck::Unchecked(err.to_string()),
    }
}

/// What a check means for setup: `Ok` carries on, perhaps with a warning, and `Err`
/// stops it. A new key is saved only once accepted; a saved one was accepted before, so
/// a provider that can't be reached doesn't stop a re-run.
pub fn require_key(
    name: &str,
    key_name: &str,
    check: KeyCheck,
    saved: bool,
) -> Result<Option<String>, String> {
    match check {
        KeyCheck::Accepted => Ok(None),
        KeyCheck::Rejected(status) if saved => Err(format!(
            "{name} rejected the saved key (HTTP {status}), so nothing was changed. Run `rakazo-stack setup` and paste a new one, or set RAKAZO_STACK_{key_name}."
        )),
        KeyCheck::Rejected(status) => Err(format!(
            "{name} rejected the key (HTTP {status}), so it wasn't saved and nothing was changed."
        )),
        KeyCheck::Unchecked(reason) if saved => Ok(Some(format!(
            "Warning: couldn't check the saved {name} key ({reason}), so setup keeps it."
        ))),
        KeyCheck::Unchecked(reason) => Err(format!(
            "couldn't check the {name} key ({reason}), so it wasn't saved and nothing was changed. Try again."
        )),
    }
}

/// Whether OpenRouter's ZDR only guardrail covers the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zdr {
    On,
    Off,
    Unknown,
}

fn judge_zdr(status: u16, body: &str) -> Zdr {
    match status {
        200 => Zdr::Off,
        404 if body.contains("zdr-violation-by-guardrail") => Zdr::On,
        _ => Zdr::Unknown,
    }
}

/// One embeddings call pinned to OpenAI's endpoint, which isn't ZDR: the guardrail
/// refuses it when ZDR only is on.
pub fn probe_zdr(agent: &ureq::Agent, base: &str, key: &PastedKey) -> Zdr {
    let body = serde_json::json!({
        "model": "openai/text-embedding-3-small",
        "input": "zdr check",
        "provider": { "only": ["openai"], "allow_fallbacks": false },
    });
    let reply = agent
        .post(format!("{base}/api/v1/embeddings"))
        .config()
        .max_redirects(0)
        .build()
        .header("authorization", format!("Bearer {}", key.expose()))
        .content_type("application/json")
        .send(body.to_string());
    match reply {
        Ok(mut response) => {
            let status = response.status().as_u16();
            let body = response.body_mut().read_to_string().unwrap_or_default();
            judge_zdr(status, &body)
        }
        Err(_) => Zdr::Unknown,
    }
}

/// Probes until the guardrail is on, can't be checked, or the front end carries on
/// without it. Returns the Checklist item that leaves, if any. It never stops setup.
pub fn ensure_zdr(
    mut probe: impl FnMut() -> Zdr,
    front: &mut dyn FrontEnd,
    mut say: impl FnMut(&str),
) -> Option<&'static str> {
    loop {
        match probe() {
            Zdr::On => return None,
            Zdr::Unknown => return Some(ZDR_UNKNOWN_ITEM),
            Zdr::Off => {
                say(ZDR_OFF);
                if !front.check_zdr_again() {
                    return Some(ZDR_OFF_ITEM);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::answers::{NoQuestions, Prompt, Wizard};
    use crate::http::testing::{Request, stub};
    use std::net::TcpListener;

    const KEY: &str = "placeholder-key";

    // Recorded from the providers on 2026-11-26, sent a placeholder key. Request ids and
    // the quoted part of the key are replaced.
    const OPENROUTER_INVALID: &str = r#"{"error":{"message":"User not found.","code":401}}"#;
    const OPENAI_INVALID: &str = r#"{"error":{"message":"Incorrect API key provided: sk-proj-***. You can find your API key at https://platform.openai.com/account/api-keys.","type":"invalid_request_error","param":null,"code":"invalid_api_key"}}"#;
    const GEMINI_INVALID: &str = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"API_KEY_INVALID","domain":"googleapis.com","metadata":{"service":"generativelanguage.googleapis.com"}}]}}"#;
    const GEMINI_INVALID_AQ: &str = r#"{"error":{"code":401,"message":"API key not valid. Please pass a valid API key.","status":"UNAUTHENTICATED","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"ACCESS_TOKEN_TYPE_UNSUPPORTED","metadata":{"service":"generativelanguage.googleapis.com"}}]}}"#;
    const ANTHROPIC_INVALID: &str = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"},"request_id":"req_placeholder"}"#;

    // Not recorded (that takes a real key): shortened from each provider's documented
    // 200 reply.
    const OPENROUTER_VALID: &str = r#"{"data":{"label":"sk-or-v1-...","limit":null,"usage":0,"is_free_tier":false,"is_management_key":false}}"#;
    const OPENAI_VALID: &str =
        r#"{"object":"list","data":[{"id":"gpt-6-luna","object":"model","owned_by":"system"}]}"#;
    const GEMINI_VALID: &str = r#"{"models":[{"name":"models/gemini-3.5-flash-lite","displayName":"Gemini 3.5 Flash-Lite"}]}"#;
    const ANTHROPIC_VALID: &str = r#"{"data":[{"type":"model","id":"claude-haiku-5-5","display_name":"Claude Haiku 5.5"}],"has_more":false}"#;

    // Not recorded either: the account's refusal carries the code the spec names, seen on
    // a ZDR only account in https://github.com/kamui/rakazo-stack/issues/37, and an
    // account without it gets an ordinary embeddings reply.
    const ZDR_ACCOUNT: &str = r#"{"error":{"message":"No endpoints available matching your guardrail restrictions and data policy. ZDR violation (guardrail): 1 endpoint excluded","code":404,"metadata":{"reasons":["zdr-violation-by-guardrail"]}}}"#;
    const NON_ZDR_ACCOUNT: &str = r#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":[0.0023,-0.0093]}],"model":"openai/text-embedding-3-small","usage":{"prompt_tokens":2,"total_tokens":2}}"#;

    fn key() -> PastedKey {
        PastedKey::new(KEY).unwrap()
    }

    fn check(provider: Provider, status: u16, reply: &'static str) -> (KeyCheck, Request) {
        let (base, served) = stub(status, reply);
        let result = check_key(&crate::http::agent(), &base, provider, &key());
        (result, served.join().unwrap())
    }

    #[test]
    fn each_check_sends_the_key_in_its_providers_header() {
        for (provider, path, header, valid) in [
            (
                Provider::OpenRouter,
                "/api/v1/key",
                "authorization: bearer placeholder-key",
                OPENROUTER_VALID,
            ),
            (
                Provider::OpenAi,
                "/v1/models",
                "authorization: bearer placeholder-key",
                OPENAI_VALID,
            ),
            (
                Provider::Gemini,
                "/v1beta/models",
                "x-goog-api-key: placeholder-key",
                GEMINI_VALID,
            ),
            (
                Provider::Anthropic,
                "/v1/models",
                "x-api-key: placeholder-key",
                ANTHROPIC_VALID,
            ),
        ] {
            let (result, request) = check(provider, 200, valid);
            assert_eq!(result, KeyCheck::Accepted, "{provider:?}");
            assert_eq!(request.line, format!("GET {path} HTTP/1.1"));
            let headers: Vec<String> = request
                .headers
                .iter()
                .map(|h| h.to_ascii_lowercase())
                .collect();
            assert!(headers.iter().any(|h| h == header), "{provider:?}");
            assert_eq!(
                headers.iter().any(|h| h == "anthropic-version: 2023-06-01"),
                provider == Provider::Anthropic
            );
        }
    }

    #[test]
    fn recorded_refusals_reject_the_key() {
        for (provider, status, reply) in [
            (Provider::OpenRouter, 401, OPENROUTER_INVALID),
            (Provider::OpenAi, 401, OPENAI_INVALID),
            (Provider::Gemini, 400, GEMINI_INVALID),
            (Provider::Gemini, 401, GEMINI_INVALID_AQ),
            (Provider::Anthropic, 401, ANTHROPIC_INVALID),
        ] {
            assert_eq!(
                check(provider, status, reply).0,
                KeyCheck::Rejected(status),
                "{provider:?}"
            );
        }
    }

    #[test]
    fn other_replies_give_no_verdict() {
        for (status, reply) in [(429, "{}"), (503, "<html></html>"), (302, "")] {
            assert_eq!(
                check(Provider::OpenRouter, status, reply).0,
                KeyCheck::Unchecked(format!("HTTP {status}"))
            );
        }
        assert_eq!(
            judge_key(
                Provider::Gemini,
                400,
                r#"{"error":{"status":"INVALID_ARGUMENT"}}"#
            ),
            KeyCheck::Unchecked("HTTP 400".into())
        );
        assert_eq!(
            judge_key(
                Provider::OpenAi,
                403,
                r#"{"error":{"message":"Missing scopes: api.model.read"}}"#
            ),
            KeyCheck::Accepted
        );
    }

    #[test]
    fn an_unreachable_provider_gives_no_verdict_and_never_quotes_the_key() {
        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let base = format!("http://{closed}");
        let result = check_key(&crate::http::agent(), &base, Provider::Anthropic, &key());
        let KeyCheck::Unchecked(reason) = result else {
            panic!("expected no verdict");
        };
        assert!(!reason.contains(KEY));
        assert_eq!(
            probe_zdr(&crate::http::agent(), &base, &key()),
            Zdr::Unknown
        );
    }

    #[test]
    fn only_an_accepted_key_or_an_unreachable_saved_one_carries_on() {
        let require = |check, saved| require_key("OpenRouter", "OPENROUTER_API_KEY", check, saved);
        let rejected = KeyCheck::Rejected(401);
        let unchecked = KeyCheck::Unchecked("HTTP 503".into());
        for saved in [false, true] {
            assert_eq!(require(KeyCheck::Accepted, saved), Ok(None));
            let err = require(rejected.clone(), saved).unwrap_err();
            assert!(err.contains("rejected"), "{err}");
        }
        assert!(require(unchecked.clone(), false).is_err());
        let warning = require(unchecked, true).unwrap().unwrap();
        assert!(warning.contains("HTTP 503"), "{warning}");
    }

    #[test]
    fn the_probe_is_the_specs_embeddings_call() {
        let (base, served) = stub(404, ZDR_ACCOUNT);
        assert_eq!(probe_zdr(&crate::http::agent(), &base, &key()), Zdr::On);
        let request = served.join().unwrap();
        assert_eq!(request.line, "POST /api/v1/embeddings HTTP/1.1");
        assert!(
            request
                .headers
                .iter()
                .any(|h| h.eq_ignore_ascii_case("authorization: Bearer placeholder-key"))
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&request.body).unwrap(),
            serde_json::json!({
                "model": "openai/text-embedding-3-small",
                "input": "zdr check",
                "provider": { "only": ["openai"], "allow_fallbacks": false },
            })
        );
    }

    struct Replies(Vec<&'static str>);

    impl Prompt for Replies {
        fn line(&mut self, _: &str) -> Option<String> {
            (!self.0.is_empty()).then(|| self.0.remove(0).to_string())
        }
        fn secret(&mut self, question: &str) -> Option<String> {
            self.line(question)
        }
    }

    /// Runs the probe against one recorded reply per attempt.
    fn ensure(
        replies: &[(u16, &'static str)],
        front: &mut dyn FrontEnd,
    ) -> (Option<&'static str>, usize) {
        let mut replies = replies.iter();
        let mut warnings = 0;
        let item = ensure_zdr(
            || {
                let (status, reply) = replies.next().expect("another probe");
                let (base, served) = stub(*status, reply);
                let zdr = probe_zdr(&crate::http::agent(), &base, &key());
                served.join().unwrap();
                zdr
            },
            front,
            |_| warnings += 1,
        );
        assert!(replies.next().is_none(), "a reply was never asked for");
        (item, warnings)
    }

    #[test]
    fn a_zdr_only_account_leaves_no_item() {
        let mut front = NoQuestions {
            env: |_: &str| None,
        };
        assert_eq!(ensure(&[(404, ZDR_ACCOUNT)], &mut front), (None, 0));
    }

    #[test]
    fn a_scripted_run_warns_and_adds_the_item_when_zdr_only_is_off() {
        let mut front = NoQuestions {
            env: |_: &str| None,
        };
        assert_eq!(
            ensure(&[(200, NON_ZDR_ACCOUNT)], &mut front),
            (Some(ZDR_OFF_ITEM), 1)
        );
    }

    #[test]
    fn any_other_reply_adds_the_item_without_a_warning_or_a_question() {
        for reply in [
            (
                404,
                r#"{"error":{"message":"No endpoints found","code":404}}"#,
            ),
            (401, OPENROUTER_INVALID),
            (429, "{}"),
        ] {
            let mut wizard = Wizard {
                prompt: Replies(vec![]),
            };
            assert_eq!(
                ensure(&[reply], &mut wizard),
                (Some(ZDR_UNKNOWN_ITEM), 0),
                "{reply:?}"
            );
        }
    }

    #[test]
    fn the_wizard_checks_again_until_it_passes_or_the_answer_is_to_continue() {
        let mut wizard = Wizard {
            prompt: Replies(vec!["c", "c"]),
        };
        let replies = [
            (200, NON_ZDR_ACCOUNT),
            (200, NON_ZDR_ACCOUNT),
            (404, ZDR_ACCOUNT),
        ];
        assert_eq!(ensure(&replies, &mut wizard), (None, 2));

        let mut enter = Wizard {
            prompt: Replies(vec![""]),
        };
        assert_eq!(
            ensure(&[(200, NON_ZDR_ACCOUNT)], &mut enter),
            (Some(ZDR_OFF_ITEM), 1)
        );
    }

    #[test]
    fn instance_models_come_from_the_override_or_the_default() {
        let key = key();
        let default = |provider| {
            ChatModel::new(provider, &key, None)
                .unwrap()
                .map(|c| c.model)
        };
        assert_eq!(
            default(Provider::OpenRouter),
            Some("~z-ai/glm-flash-latest")
        );
        assert_eq!(default(Provider::Anthropic), Some("claude-haiku-5-5"));
        assert_eq!(default(Provider::OpenAi), None);
        assert_eq!(default(Provider::Gemini), None);

        let chat = ChatModel::new(Provider::OpenRouter, &key, Some("z-ai/glm-5.3-flash"));
        assert_eq!(chat.unwrap().unwrap().model, "z-ai/glm-5.3-flash");
        for bad in ["", "a model", "m\nSIGNUPS_ENABLED=true", "${HOME}"] {
            assert!(
                ChatModel::new(Provider::Anthropic, &key, Some(bad)).is_err(),
                "{bad:?}"
            );
        }
        assert!(
            !format!(
                "{chat:?}",
                chat = ChatModel::new(Provider::Anthropic, &key, None)
            )
            .contains(KEY)
        );
    }

    #[test]
    fn choices_round_trip_as_lowercase_words() {
        for word in ["openrouter", "openai", "gemini", "anthropic", "none"] {
            let choice = Choice::parse(word).unwrap();
            let json = serde_json::to_string(&choice).unwrap();
            assert_eq!(json, format!("\"{word}\""));
            assert_eq!(serde_json::from_str::<Choice>(&json).unwrap(), choice);
        }
        assert_eq!(Choice::parse(" OpenRouter "), Some(Choice::default()));
        assert_eq!(Choice::parse("mistral"), None);
        assert!(serde_json::from_str::<Choice>("\"mistral\"").is_err());
    }

    #[test]
    fn only_the_chosen_providers_key_is_kept() {
        let mut secrets = Secrets::parse("ENCRYPTION_KEY=keepme\nOPENROUTER_API_KEY=old\n");
        save_key(&mut secrets, Some((Provider::Anthropic, &key())));
        assert_eq!(secrets.get("OPENROUTER_API_KEY"), None);
        assert_eq!(secrets.get("ANTHROPIC_API_KEY"), Some(KEY));
        save_key(&mut secrets, None);
        assert_eq!(secrets, Secrets::parse("ENCRYPTION_KEY=keepme\n"));
    }

    #[test]
    fn only_a_blank_key_or_a_per_user_provider_adds_a_key_item() {
        assert!(key_item(None).unwrap().contains("Connect a model"));
        assert!(
            key_item(Some(Provider::Gemini))
                .unwrap()
                .contains("Settings → Models")
        );
        assert_eq!(key_item(Some(Provider::OpenRouter)), None);
        assert_eq!(key_item(Some(Provider::Anthropic)), None);
    }
}
