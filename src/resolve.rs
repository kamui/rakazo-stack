//! Picks the Rakazo and Hindsight releases to install (spec §4).

use crate::overrides;
use crate::version::Version;

/// The lowest Rakazo release this program drives: where setup's memory connect call
/// was proven (#45).
pub const RAKAZO_FLOOR: &str = "v0.16.0";

pub const APP_IMAGE: &str = "elie222/rakazo/app";
pub const COMPUTER_IMAGE: &str = "elie222/rakazo/computer";

/// The lowest Hindsight release this program drives: the one tested (#1).
pub const HINDSIGHT_FLOOR: &str = "1.3.0";

/// On GHCR. The full image, not `-slim`: it has the embeddings and reranker models baked in.
pub const HINDSIGHT_IMAGE: &str = "vectorize-io/hindsight-api";

/// On Docker Hub. It floats within PostgreSQL 18, never to a new major.
pub const HINDSIGHT_DB_IMAGE: &str = "pgvector/pgvector";
pub const HINDSIGHT_DB_TAG: &str = "pg18";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RakazoRelease {
    pub tag: String,
    pub app_digest: String,
    pub computer_digest: String,
}

/// A Hindsight release and the `hindsight-db` image that goes with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HindsightRelease {
    /// The image tag, which is the release tag without its `v`.
    pub version: String,
    pub digest: String,
    pub db_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub component: &'static str,
    pub tag: String,
    pub missing: Vec<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Resolution<R> {
    pub release: R,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError {
    /// No stable release at or above the floor has its images.
    NothingAtFloor {
        skipped: Vec<Skipped>,
    },
    Lookup(String),
}

/// [`RAKAZO_FLOOR`], unless a test lowers it.
pub fn rakazo_floor() -> String {
    overrides::rakazo_floor().unwrap_or_else(|| RAKAZO_FLOOR.to_string())
}

pub fn below_floor(tag: &str) -> bool {
    below(&rakazo_floor(), tag)
}

pub fn hindsight_below_floor(version: &str) -> bool {
    below(HINDSIGHT_FLOOR, version)
}

fn below(floor: &str, tag: &str) -> bool {
    let floor = Version::parse(floor).expect("valid floor");
    Version::parse(tag).is_none_or(|v| v < floor)
}

/// Walks `tags` (newest first) down to the floor and returns the first release whose
/// `app` and `computer` images both exist. `digest(image, tag)` returns `Ok(None)` for
/// an image GHCR doesn't have.
pub fn choose_rakazo<F>(
    tags: &[String],
    digest: F,
) -> Result<Resolution<RakazoRelease>, ResolveError>
where
    F: FnMut(&str, &str) -> Result<Option<String>, String>,
{
    let images = [("app", APP_IMAGE), ("computer", COMPUTER_IMAGE)];
    let floor = rakazo_floor();
    let (tag, digests, skipped) = choose("Rakazo", &floor, tags, &images, |tag| tag, digest)?;
    let [app_digest, computer_digest] = digests;
    Ok(Resolution {
        release: RakazoRelease {
            tag,
            app_digest,
            computer_digest,
        },
        skipped,
    })
}

/// As [`choose_rakazo`], for Hindsight's image: release `vX.Y.Z` is image tag `X.Y.Z`.
/// `db_digest` is `hindsight-db`'s, looked up once a release is chosen.
pub fn choose_hindsight<F>(
    tags: &[String],
    digest: F,
    db_digest: impl FnOnce() -> Result<Option<String>, String>,
) -> Result<Resolution<HindsightRelease>, ResolveError>
where
    F: FnMut(&str, &str) -> Result<Option<String>, String>,
{
    let images = [("hindsight-api", HINDSIGHT_IMAGE)];
    let (tag, [digest], skipped) = choose(
        "Hindsight",
        HINDSIGHT_FLOOR,
        tags,
        &images,
        hindsight_image_tag,
        digest,
    )?;
    let db_digest = db_digest().map_err(ResolveError::Lookup)?.ok_or_else(|| {
        ResolveError::Lookup(format!(
            "{HINDSIGHT_DB_IMAGE}:{HINDSIGHT_DB_TAG} isn't on Docker Hub"
        ))
    })?;
    Ok(Resolution {
        release: HindsightRelease {
            version: hindsight_image_tag(&tag).to_string(),
            digest,
            db_digest,
        },
        skipped,
    })
}

fn hindsight_image_tag(release: &str) -> &str {
    release.strip_prefix('v').unwrap_or(release)
}

/// The newest tag at or above `floor` with every image in `images` (label, repository),
/// with their digests in the same order.
fn choose<const N: usize, F>(
    component: &'static str,
    floor: &str,
    tags: &[String],
    images: &[(&'static str, &str); N],
    image_tag: fn(&str) -> &str,
    mut digest: F,
) -> Result<(String, [String; N], Vec<Skipped>), ResolveError>
where
    F: FnMut(&str, &str) -> Result<Option<String>, String>,
{
    let mut skipped = Vec::new();
    for tag in tags.iter().filter(|tag| !below(floor, tag)) {
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for (label, repository) in images {
            match digest(repository, image_tag(tag)).map_err(ResolveError::Lookup)? {
                Some(digest) => found.push(digest),
                None => missing.push(*label),
            }
        }
        match found.try_into() {
            Ok(digests) if missing.is_empty() => return Ok((tag.clone(), digests, skipped)),
            _ => skipped.push(Skipped {
                component,
                tag: tag.clone(),
                missing,
            }),
        }
    }
    Err(ResolveError::NothingAtFloor { skipped })
}

impl Skipped {
    pub fn describe(&self) -> String {
        format!(
            "Skipped {} {}: its {} image{} isn't published yet. A later `rakazo-stack update` picks it up once it is.",
            self.component,
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
                    component: "Rakazo",
                    tag: "v0.18.0".into(),
                    missing: vec!["computer"]
                },
                Skipped {
                    component: "Rakazo",
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
                    component: "Rakazo",
                    tag: "v0.17.0".into(),
                    missing: vec!["app", "computer"]
                }]
            }
        );
    }

    /// GHCR as recorded on 2026-11-26: Hindsight's tags have no `v`. 1.4.0 stands for a
    /// release tagged before its image is published.
    fn recorded_hindsight(image: &str, tag: &str) -> Result<Option<String>, String> {
        assert_eq!(image, HINDSIGHT_IMAGE);
        Ok(match tag {
            "1.3.0" => Some(
                "sha256:c054765e9196fb1fe63497be06bcb55256c9024cc9314f951dc59858985ab242".into(),
            ),
            "1.2.3" => Some("sha256:123".into()),
            _ => None,
        })
    }

    fn pg18() -> Result<Option<String>, String> {
        Ok(Some(
            "sha256:38681dc6eaf55308e2a4c6de392d131652dcfac70e1263e35cd9b5ea3fe39782".into(),
        ))
    }

    #[test]
    fn maps_hindsight_releases_to_image_tags_and_skips_unpublished_ones() {
        let got = choose_hindsight(
            &tags(&["v1.4.0", "v1.3.0", "v1.2.3"]),
            recorded_hindsight,
            pg18,
        )
        .unwrap();
        assert_eq!(got.release.version, "1.3.0");
        assert!(got.release.digest.starts_with("sha256:c054"));
        assert!(got.release.db_digest.starts_with("sha256:3868"));
        assert_eq!(
            got.skipped,
            [Skipped {
                component: "Hindsight",
                tag: "v1.4.0".into(),
                missing: vec!["hindsight-api"]
            }]
        );
        assert_eq!(
            got.skipped[0].describe(),
            "Skipped Hindsight v1.4.0: its hindsight-api image isn't published yet. A later `rakazo-stack update` picks it up once it is."
        );
    }

    #[test]
    fn never_picks_hindsight_below_its_floor_or_without_its_database() {
        let err =
            choose_hindsight(&tags(&["v1.2.3", "v1.0.0"]), recorded_hindsight, pg18).unwrap_err();
        assert_eq!(err, ResolveError::NothingAtFloor { skipped: vec![] });
        let err =
            choose_hindsight(&tags(&["v1.3.0"]), recorded_hindsight, || Ok(None)).unwrap_err();
        assert!(matches!(err, ResolveError::Lookup(msg) if msg.contains("pgvector/pgvector:pg18")));
    }

    #[test]
    fn compares_against_the_floor() {
        assert!(below_floor("v0.15.0"));
        assert!(!below_floor("v0.16.0"));
        assert!(!below_floor("v1.0.0"));
        assert!(below_floor("android"));
    }
}
