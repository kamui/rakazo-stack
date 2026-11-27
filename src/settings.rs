//! `settings.json`: the Self-hoster's choices and recorded state, never a secret (spec §6).
//! Later slices add fields; older files load with their defaults.

use serde::{Deserialize, Serialize};

use crate::database_settings::DatabaseSetting;
use crate::model_provider::Choice;
use crate::resolve::RakazoRelease;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub schema: u32,
    /// The program version that last wrote the file.
    pub program_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider: Option<Choice>,
    /// Overrides the program's default chat model (spec §8). Only the Self-hoster writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rakazo_model: Option<String>,
    /// The database-only settings setup has filled in, so a re-run can tell "removed in
    /// Rakazo" from "never set" (spec §7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filled_by_setup: Vec<DatabaseSetting>,
    pub components: Components,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Components {
    pub rakazo: RecordedRakazo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedRakazo {
    pub version: String,
    pub images: RakazoImages,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RakazoImages {
    pub app: String,
    pub computer: String,
}

impl Settings {
    pub fn new(release: &RakazoRelease) -> Self {
        Self {
            schema: SCHEMA,
            program_version: env!("CARGO_PKG_VERSION").to_string(),
            owner_email: None,
            model_provider: None,
            rakazo_model: None,
            filled_by_setup: Vec::new(),
            components: Components {
                rakazo: RecordedRakazo {
                    version: release.tag.clone(),
                    images: RakazoImages {
                        app: release.app_digest.clone(),
                        computer: release.computer_digest.clone(),
                    },
                },
            },
        }
    }

    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("settings serialize");
        text.push('\n');
        text
    }

    pub fn rakazo_release(&self) -> RakazoRelease {
        let rakazo = &self.components.rakazo;
        RakazoRelease {
            tag: rakazo.version.clone(),
            app_digest: rakazo.images.app.clone(),
            computer_digest: rakazo.images.computer.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let settings = Settings::new(&RakazoRelease {
            tag: "v0.16.0".into(),
            app_digest: "sha256:aaa".into(),
            computer_digest: "sha256:ccc".into(),
        });
        let text = settings.to_json();
        assert!(text.contains("\"version\": \"v0.16.0\""));
        assert_eq!(Settings::parse(&text).unwrap(), settings);
        assert_eq!(settings.rakazo_release().computer_digest, "sha256:ccc");
        assert!(!text.contains("owner_email"));
    }

    #[test]
    fn records_the_owner_email_and_what_setup_filled() {
        let mut settings = Settings::new(&RakazoRelease {
            tag: "v0.16.0".into(),
            app_digest: "sha256:aaa".into(),
            computer_digest: "sha256:ccc".into(),
        });
        settings.owner_email = Some("owner@example.com".into());
        settings.model_provider = Some(Choice(None));
        settings.rakazo_model = Some("z-ai/glm-5.3-flash".into());
        settings.filled_by_setup = vec![DatabaseSetting::Owner];
        let text = settings.to_json();
        assert!(
            text.contains("\"filled_by_setup\": [\n    \"owner\"\n  ]"),
            "{text}"
        );
        assert!(text.contains("\"model_provider\": \"none\""), "{text}");
        assert!(
            text.contains("\"rakazo_model\": \"z-ai/glm-5.3-flash\""),
            "{text}"
        );
        assert_eq!(Settings::parse(&text).unwrap(), settings);
    }
}
