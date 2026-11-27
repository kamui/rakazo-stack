//! Memory: Hindsight (spec §10). It runs only when the Memory answer is `on` and a Model
//! provider key is set, on that key, with one memory model for all of Hindsight's work.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::docker::Docker;
use crate::model_provider::{self, Provider};
use crate::resolve::{HINDSIGHT_DB_TAG, HindsightRelease};
use crate::secrets::{PastedKey, Secrets};

pub const MEMORY_VAR: &str = "RAKAZO_STACK_MEMORY";

/// The Compose profile that holds `hindsight` and `hindsight-db` (spec §5).
pub const PROFILE: &str = "hindsight";
pub const SERVICES: [&str; 2] = ["hindsight", "hindsight-db"];

/// How the Owner of any Space connects it by hand (spec §7, "The checklist").
pub const CONNECT_STEPS: &str = "Settings → Memory → Connect memory provider → Hindsight → Advanced → Self-hosted, with http://hindsight:8888 and the key from the HINDSIGHT_API_TENANT_API_KEY line in ~/rakazo-stack/secrets.env";

/// The answer to the Memory question. It is recorded even when the key is blank, as the
/// Model provider is, so adding a key later brings back the Self-hoster's choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Memory {
    #[default]
    On,
    Off,
}

impl Memory {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "on" => Some(Self::On),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

/// Hindsight's LLM: the Model provider's key and the memory model (spec §8).
#[derive(Debug, Clone, Copy)]
pub struct HindsightModel<'a> {
    pub provider: Provider,
    pub key: &'a PastedKey,
    pub model: &'a str,
}

impl<'a> HindsightModel<'a> {
    /// `override_model` is `settings.json`'s `memory_model`, which wins over the default.
    pub fn new(
        provider: Provider,
        key: &'a PastedKey,
        override_model: Option<&'a str>,
    ) -> Result<Self, String> {
        let model = override_model.unwrap_or(default_model(provider));
        if !model_provider::is_model_id(model) {
            return Err("settings.json's memory_model isn't a model id.".to_string());
        }
        Ok(Self {
            provider,
            key,
            model,
        })
    }
}

/// This program version's memory model for each provider.
fn default_model(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenRouter => "~z-ai/glm-flash-latest",
        Provider::OpenAi => "gpt-6-luna",
        Provider::Gemini => "gemini-3.5-flash-lite",
        Provider::Anthropic => "claude-haiku-5-5",
    }
}

pub fn image_lines(release: &HindsightRelease) -> [String; 2] {
    [
        format!("HINDSIGHT_IMAGE_TAG={}@{}", release.version, release.digest),
        format!(
            "HINDSIGHT_DB_IMAGE_TAG={HINDSIGHT_DB_TAG}@{}",
            release.db_digest
        ),
    ]
}

/// What `.env` holds for memory when it's on. `compose.stack.yaml` passes the LLM lines
/// through to `hindsight` only, so Rakazo's services never see them.
pub struct MemoryEnv<'a> {
    pub release: &'a HindsightRelease,
    pub model: HindsightModel<'a>,
    pub secrets: &'a Secrets,
}

impl MemoryEnv<'_> {
    pub fn lines(&self) -> Vec<String> {
        let mut lines = image_lines(self.release).to_vec();
        lines.extend(
            self.secrets
                .hindsight()
                .map(|(name, value)| format!("{name}={value}")),
        );
        let model = &self.model;
        lines.extend([
            format!("HINDSIGHT_API_LLM_PROVIDER={}", model.provider.id()),
            format!("HINDSIGHT_API_LLM_API_KEY={}", model.key.expose()),
            format!("HINDSIGHT_API_LLM_MODEL={}", model.model),
        ]);
        if model.provider == Provider::OpenRouter {
            lines.extend([
                // Otherwise GLM reasons at `max`.
                "HINDSIGHT_API_LLM_REASONING_EFFORT=low".to_string(),
                // Single quotes keep the JSON literal in Compose's `.env` syntax.
                r#"HINDSIGHT_API_LLM_EXTRA_BODY='{"provider":{"zdr":true,"data_collection":"deny"}}'"#
                    .to_string(),
            ]);
        }
        lines
    }
}

/// Step 10 of every Update: pgvector's `.so` comes with the image, its SQL objects with
/// this. Cheap, and safe to repeat (spec §14).
pub fn update_vector(docker: &Docker, folder: &Path) -> Result<(), String> {
    let out = docker
        .compose(
            folder,
            [
                "exec",
                "-T",
                "hindsight-db",
                "psql",
                "-X",
                "-q",
                "-v",
                "ON_ERROR_STOP=1",
                "-U",
                "hindsight",
                "-d",
                "hindsight",
                "-c",
                "ALTER EXTENSION vector UPDATE",
            ],
        )
        .output()
        .map_err(|err| err.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "`ALTER EXTENSION vector UPDATE` in hindsight-db failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// A Space connected to Hindsight, for the Checklist while memory is off.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConnectedSpace {
    pub space: String,
    /// Who connected it, when Rakazo still has that user.
    pub by: Option<String>,
}

pub fn another_space_item() -> String {
    format!("To turn memory on in another Space, its owner opens {CONNECT_STEPS}.")
}

pub fn not_removed_item(compose: &str) -> String {
    format!(
        "Hindsight's containers couldn't be removed, so memory may still be running. Run `{compose} --profile {PROFILE} rm --stop --force {}` from ~/rakazo-stack.",
        SERVICES.join(" ")
    )
}

pub fn still_connected_item(connected: &ConnectedSpace) -> String {
    // Space names are typed by users; keep terminal control characters out of the output.
    let name: String = connected
        .space
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let by = connected
        .by
        .as_deref()
        .map(|email| format!(", connected by {email},"))
        .unwrap_or_default();
    format!(
        "Memory is off, but the Space \"{name}\"{by} is still connected to Hindsight. Its owner disconnects it under Settings → Memory, because until then each reply first tries a recall that fails."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release() -> HindsightRelease {
        HindsightRelease {
            version: "1.3.0".into(),
            digest: "sha256:hhh".into(),
            db_digest: "sha256:ddd".into(),
        }
    }

    fn secrets() -> Secrets {
        let mut secrets = Secrets::default();
        secrets.fill_missing_hindsight(|n| "ab".repeat(n));
        secrets
    }

    fn lines(provider: Provider, override_model: Option<&str>) -> Vec<String> {
        let key = PastedKey::new("placeholder-key").unwrap();
        let release = release();
        let secrets = secrets();
        MemoryEnv {
            release: &release,
            model: HindsightModel::new(provider, &key, override_model).unwrap(),
            secrets: &secrets,
        }
        .lines()
    }

    #[test]
    fn each_provider_gets_its_memory_model_and_openrouter_its_extras() {
        let llm = |provider| -> Vec<String> {
            lines(provider, None)
                .into_iter()
                .filter(|l| l.starts_with("HINDSIGHT_API_LLM_"))
                .collect()
        };
        assert_eq!(
            llm(Provider::OpenRouter),
            [
                "HINDSIGHT_API_LLM_PROVIDER=openrouter",
                "HINDSIGHT_API_LLM_API_KEY=placeholder-key",
                "HINDSIGHT_API_LLM_MODEL=~z-ai/glm-flash-latest",
                "HINDSIGHT_API_LLM_REASONING_EFFORT=low",
                r#"HINDSIGHT_API_LLM_EXTRA_BODY='{"provider":{"zdr":true,"data_collection":"deny"}}'"#,
            ]
        );
        for (provider, model) in [
            (Provider::OpenAi, "gpt-6-luna"),
            (Provider::Gemini, "gemini-3.5-flash-lite"),
            (Provider::Anthropic, "claude-haiku-5-5"),
        ] {
            assert_eq!(
                llm(provider),
                [
                    format!("HINDSIGHT_API_LLM_PROVIDER={}", provider.id()),
                    "HINDSIGHT_API_LLM_API_KEY=placeholder-key".to_string(),
                    format!("HINDSIGHT_API_LLM_MODEL={model}"),
                ]
            );
        }
    }

    #[test]
    fn writes_the_images_by_digest_and_both_generated_secrets() {
        let lines = lines(Provider::Anthropic, None);
        assert_eq!(lines[0], "HINDSIGHT_IMAGE_TAG=1.3.0@sha256:hhh");
        assert_eq!(lines[1], "HINDSIGHT_DB_IMAGE_TAG=pg18@sha256:ddd");
        let password = format!("HINDSIGHT_DB_PASSWORD={}", "ab".repeat(16));
        let tenant = format!("HINDSIGHT_API_TENANT_API_KEY={}", "ab".repeat(32));
        assert!(
            lines.contains(&password) && lines.contains(&tenant),
            "{lines:?}"
        );
    }

    #[test]
    fn the_memory_model_override_wins_and_no_per_operation_model_is_set() {
        for provider in Provider::ALL {
            let lines = lines(provider, Some("z-ai/glm-5.3-flash"));
            assert!(lines.contains(&"HINDSIGHT_API_LLM_MODEL=z-ai/glm-5.3-flash".to_string()));
            for operation in ["RETAIN", "REFLECT", "CONSOLIDATION", "MENTAL_MODEL_REFRESH"] {
                assert!(
                    !lines
                        .iter()
                        .any(|l| l.starts_with(&format!("HINDSIGHT_API_{operation}_"))),
                    "{provider:?} sets a {operation} model"
                );
            }
        }
        let key = PastedKey::new("placeholder-key").unwrap();
        for bad in ["", "a model", "a\nb", "$HOME"] {
            assert!(HindsightModel::new(Provider::OpenRouter, &key, Some(bad)).is_err());
        }
    }

    #[test]
    fn memory_answers_are_on_or_off() {
        assert_eq!(Memory::parse(" OFF "), Some(Memory::Off));
        assert_eq!(Memory::parse("on"), Some(Memory::On));
        assert_eq!(Memory::parse("yes"), None);
        assert_eq!(Memory::default(), Memory::On);
        assert_eq!(serde_json::to_string(&Memory::Off).unwrap(), "\"off\"");
    }

    #[test]
    fn a_still_connected_space_is_named_without_control_characters() {
        let item = still_connected_item(&ConnectedSpace {
            space: "Team\u{1b}[31m red".into(),
            by: Some("owner@example.com".into()),
        });
        assert!(
            item.starts_with("Memory is off, but the Space \"Team[31m red\", connected by owner@example.com, is still connected to Hindsight."),
            "{item}"
        );
        let item = still_connected_item(&ConnectedSpace {
            space: "Personal".into(),
            by: None,
        });
        assert!(item.contains("the Space \"Personal\" is still"), "{item}");
    }
}
