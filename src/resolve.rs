//! Picks the Rakazo release to install (spec §4).

use crate::version::Version;

/// The lowest Rakazo release this program drives: where setup's memory connect call
/// was proven (#45).
pub const RAKAZO_FLOOR: &str = "v0.16.0";

pub const APP_IMAGE: &str = "elie222/rakazo/app";
pub const COMPUTER_IMAGE: &str = "elie222/rakazo/computer";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RakazoRelease {
    pub tag: String,
    pub app_digest: String,
    pub computer_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub tag: String,
    pub missing: Vec<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Resolution {
    pub release: RakazoRelease,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError {
    /// No stable release at or above the floor has both images.
    NothingAtFloor {
        skipped: Vec<Skipped>,
    },
    Lookup(String),
}

pub fn below_floor(tag: &str) -> bool {
    let floor = Version::parse(RAKAZO_FLOOR).expect("valid floor");
    Version::parse(tag).is_none_or(|v| v < floor)
}

/// Walks `tags` (newest first) down to the floor and returns the first release whose
/// `app` and `computer` images both exist. `digest(image, tag)` returns `Ok(None)` for
/// an image GHCR doesn't have.
pub fn choose_rakazo<F>(tags: &[String], mut digest: F) -> Result<Resolution, ResolveError>
where
    F: FnMut(&str, &str) -> Result<Option<String>, String>,
{
    let mut skipped = Vec::new();
    for tag in tags.iter().filter(|tag| !below_floor(tag)) {
        let app = digest(APP_IMAGE, tag).map_err(ResolveError::Lookup)?;
        let computer = digest(COMPUTER_IMAGE, tag).map_err(ResolveError::Lookup)?;
        match (app, computer) {
            (Some(app_digest), Some(computer_digest)) => {
                return Ok(Resolution {
                    release: RakazoRelease {
                        tag: tag.clone(),
                        app_digest,
                        computer_digest,
                    },
                    skipped,
                });
            }
            (app, computer) => {
                let mut missing = Vec::new();
                if app.is_none() {
                    missing.push("app");
                }
                if computer.is_none() {
                    missing.push("computer");
                }
                skipped.push(Skipped {
                    tag: tag.clone(),
                    missing,
                });
            }
        }
    }
    Err(ResolveError::NothingAtFloor { skipped })
}

impl Skipped {
    pub fn describe(&self) -> String {
        format!(
            "Skipped Rakazo {}: its {} image{} isn't published yet. A later `rakazo-stack update` picks it up once it is.",
            self.tag,
            self.missing.join(" and "),
            if self.missing.len() == 1 { "" } else { "s" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// GHCR as recorded: v0.17.0 tagged but its images not yet published (404),
    /// v0.16.0 and v0.15.0 both published.
    fn recorded_ghcr() -> HashMap<(&'static str, &'static str), &'static str> {
        HashMap::from([
            ((APP_IMAGE, "v0.16.0"), "sha256:aaa"),
            ((COMPUTER_IMAGE, "v0.16.0"), "sha256:ccc"),
            ((APP_IMAGE, "v0.15.0"), "sha256:a15"),
            ((COMPUTER_IMAGE, "v0.15.0"), "sha256:c15"),
            ((APP_IMAGE, "v0.18.0"), "sha256:a18"),
        ])
    }

    fn lookup<'a>(
        ghcr: &'a HashMap<(&'static str, &'static str), &'static str>,
    ) -> impl FnMut(&str, &str) -> Result<Option<String>, String> + 'a {
        move |image, tag| {
            Ok(ghcr
                .iter()
                .find(|((i, t), _)| *i == image && *t == tag)
                .map(|(_, d)| d.to_string()))
        }
    }

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn picks_the_newest_release_with_both_images() {
        let ghcr = recorded_ghcr();
        let got = choose_rakazo(&tags(&["v0.16.0", "v0.15.0"]), lookup(&ghcr)).unwrap();
        assert_eq!(got.release.tag, "v0.16.0");
        assert_eq!(got.release.app_digest, "sha256:aaa");
        assert!(got.skipped.is_empty());
    }

    #[test]
    fn skips_releases_without_images_and_says_which() {
        let ghcr = recorded_ghcr();
        let got = choose_rakazo(&tags(&["v0.18.0", "v0.17.0", "v0.16.0"]), lookup(&ghcr)).unwrap();
        assert_eq!(got.release.tag, "v0.16.0");
        assert_eq!(
            got.skipped,
            [
                Skipped {
                    tag: "v0.18.0".into(),
                    missing: vec!["computer"]
                },
                Skipped {
                    tag: "v0.17.0".into(),
                    missing: vec!["app", "computer"]
                },
            ]
        );
        assert!(
            got.skipped[1]
                .describe()
                .contains("app and computer images")
        );
    }

    #[test]
    fn never_goes_below_the_floor() {
        let ghcr = recorded_ghcr();
        let err = choose_rakazo(&tags(&["v0.17.0", "v0.15.0"]), lookup(&ghcr)).unwrap_err();
        assert_eq!(
            err,
            ResolveError::NothingAtFloor {
                skipped: vec![Skipped {
                    tag: "v0.17.0".into(),
                    missing: vec!["app", "computer"]
                }]
            }
        );
    }

    #[test]
    fn compares_against_the_floor() {
        assert!(below_floor("v0.15.0"));
        assert!(!below_floor("v0.16.0"));
        assert!(!below_floor("v1.0.0"));
        assert!(below_floor("android"));
    }
}
