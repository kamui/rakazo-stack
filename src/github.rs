use serde::Deserialize;

use crate::http::{self, HttpError};
use crate::version::Version;

pub const RAKAZO_REPO: &str = "elie222/rakazo";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

/// Stable `vX.Y.Z` release tags, newest version first. GitHub's own order follows
/// creation dates, and Rakazo also publishes non-version releases such as `android`.
pub fn stable_tags(releases_json: &str) -> Result<Vec<String>, serde_json::Error> {
    let releases: Vec<Release> = serde_json::from_str(releases_json)?;
    let mut tags: Vec<(Version, String)> = releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease && is_release_tag(&r.tag_name))
        .filter_map(|r| Version::parse(&r.tag_name).map(|v| (v, r.tag_name)))
        .collect();
    tags.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(tags.into_iter().map(|(_, tag)| tag).collect())
}

fn is_release_tag(tag: &str) -> bool {
    tag.strip_prefix('v').is_some_and(|rest| {
        let parts: Vec<&str> = rest.split('.').collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    })
}

pub fn fetch_stable_tags(agent: &ureq::Agent, repo: &str) -> Result<Vec<String>, HttpError> {
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=30");
    let body = http::get_bytes(agent, &url)?;
    stable_tags(&String::from_utf8_lossy(&body))
        .map_err(|err| HttpError(format!("{url}: unexpected reply: {err}")))
}

/// A file from a repo at a tag, exactly as committed.
pub fn fetch_file(
    agent: &ureq::Agent,
    repo: &str,
    tag: &str,
    path: &str,
) -> Result<Vec<u8>, HttpError> {
    http::get_bytes(
        agent,
        &format!("https://raw.githubusercontent.com/{repo}/{tag}/{path}"),
    )
}

#[cfg(test)]
mod tests {
    use super::stable_tags;

    #[test]
    fn keeps_stable_releases_in_order() {
        let json = include_str!("../tests/fixtures/github-releases.json");
        assert_eq!(
            stable_tags(json).unwrap(),
            ["v0.16.0", "v0.15.0", "v0.14.0"]
        );
    }
}
