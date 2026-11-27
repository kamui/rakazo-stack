//! `settings.json`: the Self-hoster's choices and recorded state, never a secret (spec §6).
//! Later slices add fields; older files load with their defaults.

use serde::{Deserialize, Serialize};

use crate::access::{AccessMode, DEFAULT_DEVICE_NAME};
use crate::database_settings::DatabaseSetting;
use crate::lemonade::LemonadeRelease;
use crate::memory::Memory;
use crate::model_provider::Choice;
use crate::resolve::{HindsightRelease, RakazoRelease};

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
    /// The answer to the Memory question (spec §7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<Memory>,
    /// Overrides the program's default memory model (spec §8). Only the Self-hoster writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_model: Option<String>,
    /// The database-only settings setup has filled in, so a re-run can tell "removed in
    /// Rakazo" from "never set" (spec §7).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filled_by_setup: Vec<DatabaseSetting>,
    #[serde(default)]
    pub access: AccessMode,
    /// The requested name until Tailscale assigns one, then the assigned name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailscale_device_name: Option<String>,
    /// `<device>.<tailnet>.ts.net`, recorded once the Instance has joined a tailnet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailscale_dns_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bring_your_own_url: Option<String>,
    pub components: Components,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Components {
    pub rakazo: RecordedRakazo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tailscale: Option<RecordedImage>,
    /// Linux and Windows; on macOS Lemonade runs natively.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lemonade: Option<RecordedImage>,
    /// Recorded the first time memory runs, and kept while it is off, so turning it on
    /// again runs the version that migrated `hindsight-pgdata`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hindsight: Option<RecordedHindsight>,
}

/// A Component that runs one image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedImage {
    pub version: String,
    pub digest: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedHindsight {
    pub version: String,
    pub images: HindsightImages,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HindsightImages {
    pub hindsight: String,
    #[serde(rename = "hindsight-db")]
    pub hindsight_db: String,
}

impl Settings {
    pub fn new(release: &RakazoRelease) -> Self {
        Self {
            schema: SCHEMA,
            program_version: env!("CARGO_PKG_VERSION").to_string(),
            owner_email: None,
            model_provider: None,
            rakazo_model: None,
            memory: None,
            memory_model: None,
            filled_by_setup: Vec::new(),
            access: AccessMode::default(),
            tailscale_device_name: None,
            tailscale_dns_name: None,
            bring_your_own_url: None,
            components: Components {
                rakazo: RecordedRakazo {
                    version: release.tag.clone(),
                    images: RakazoImages {
                        app: release.app_digest.clone(),
                        computer: release.computer_digest.clone(),
                    },
                },
                tailscale: None,
                lemonade: None,
                hindsight: None,
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

    pub fn device_name(&self) -> &str {
        self.tailscale_device_name
            .as_deref()
            .unwrap_or(DEFAULT_DEVICE_NAME)
    }

    pub fn lemonade_release(&self) -> Option<LemonadeRelease> {
        self.components
            .lemonade
            .as_ref()
            .map(|lemonade| LemonadeRelease {
                tag: lemonade.version.clone(),
                digest: lemonade.digest.clone(),
            })
    }

    pub fn record_lemonade(&mut self, release: &LemonadeRelease) {
        self.components.lemonade = Some(RecordedImage {
            version: release.tag.clone(),
            digest: release.digest.clone(),
        });
    }

    pub fn rakazo_release(&self) -> RakazoRelease {
        let rakazo = &self.components.rakazo;
        RakazoRelease {
            tag: rakazo.version.clone(),
            app_digest: rakazo.images.app.clone(),
            computer_digest: rakazo.images.computer.clone(),
        }
    }

    pub fn hindsight_release(&self) -> Option<HindsightRelease> {
        self.components
            .hindsight
            .as_ref()
            .map(|hindsight| HindsightRelease {
                version: hindsight.version.clone(),
                digest: hindsight.images.hindsight.clone(),
                db_digest: hindsight.images.hindsight_db.clone(),
            })
    }

    pub fn record_hindsight(&mut self, release: &HindsightRelease) {
        self.components.hindsight = Some(RecordedHindsight {
            version: release.version.clone(),
            images: HindsightImages {
                hindsight: release.digest.clone(),
                hindsight_db: release.db_digest.clone(),
            },
        });
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
        assert!(!text.contains("hindsight") && !text.contains("memory"));
    }

    #[test]
    fn records_memory_and_hindsights_two_digests() {
        let mut settings = Settings::new(&RakazoRelease {
            tag: "v0.16.0".into(),
            app_digest: "sha256:aaa".into(),
            computer_digest: "sha256:ccc".into(),
        });
        settings.memory = Some(Memory::Off);
        settings.memory_model = Some("z-ai/glm-5.3-flash".into());
        settings.filled_by_setup = vec![DatabaseSetting::Owner, DatabaseSetting::Memory];
        let release = HindsightRelease {
            version: "1.3.0".into(),
            digest: "sha256:hhh".into(),
            db_digest: "sha256:ddd".into(),
        };
        settings.record_hindsight(&release);
        let text = settings.to_json();
        for expected in [
            "\"memory\": \"off\"",
            "\"memory_model\": \"z-ai/glm-5.3-flash\"",
            "\"hindsight\": {\n      \"version\": \"1.3.0\",\n      \"images\": {\n        \"hindsight\": \"sha256:hhh\",\n        \"hindsight-db\": \"sha256:ddd\"",
            "\"memory\"\n  ]",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        let parsed = Settings::parse(&text).unwrap();
        assert_eq!(parsed, settings);
        assert_eq!(parsed.hindsight_release(), Some(release));
    }

    #[test]
    fn records_the_access_choice_and_older_files_stay_on_localhost() {
        let older = r#"{"schema":1,"program_version":"0.1.0","components":{"rakazo":{"version":"v0.16.0","images":{"app":"sha256:aaa","computer":"sha256:ccc"}}}}"#;
        let mut settings = Settings::parse(older).unwrap();
        assert_eq!(settings.access, AccessMode::Localhost);
        assert_eq!(settings.components.tailscale, None);

        settings.access = AccessMode::BringYourOwn;
        settings.bring_your_own_url = Some("https://rakazo.example.com".into());
        settings.tailscale_device_name = Some("rakazo".into());
        settings.tailscale_dns_name = Some("rakazo.example.ts.net".into());
        settings.components.tailscale = Some(RecordedImage {
            version: "v1.104.7".into(),
            digest: "sha256:ttt".into(),
        });
        let text = settings.to_json();
        assert!(text.contains("\"access\": \"bring-your-own\""), "{text}");
        assert_eq!(Settings::parse(&text).unwrap(), settings);
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
        settings.filled_by_setup = vec![DatabaseSetting::Owner, DatabaseSetting::Voice];
        let text = settings.to_json();
        assert!(
            text.contains("\"filled_by_setup\": [\n    \"owner\",\n    \"voice\"\n  ]"),
            "{text}"
        );
        assert!(text.contains("\"model_provider\": \"none\""), "{text}");
        assert!(
            text.contains("\"rakazo_model\": \"z-ai/glm-5.3-flash\""),
            "{text}"
        );
        assert_eq!(Settings::parse(&text).unwrap(), settings);
    }

    #[test]
    fn records_lemonade_and_reads_files_without_it() {
        let mut settings = Settings::new(&RakazoRelease {
            tag: "v0.16.0".into(),
            app_digest: "sha256:aaa".into(),
            computer_digest: "sha256:ccc".into(),
        });
        let older = settings.to_json();
        assert!(!older.contains("lemonade"));
        assert_eq!(Settings::parse(&older).unwrap().lemonade_release(), None);
        let release = LemonadeRelease {
            tag: "v2026.48.3".into(),
            digest: "sha256:lll".into(),
        };
        settings.record_lemonade(&release);
        let text = settings.to_json();
        assert!(
            text.contains("\"lemonade\": {\n      \"version\": \"v2026.48.3\",\n      \"digest\": \"sha256:lll\""),
            "{text}"
        );
        assert_eq!(
            Settings::parse(&text).unwrap().lemonade_release(),
            Some(release)
        );
    }
}
