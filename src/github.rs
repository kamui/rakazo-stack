use serde::Deserialize;

use crate::http::{self, HttpError};
use crate::version::Version;

pub const RAKAZO_REPO: &str = "elie222/rakazo";
pub const HINDSIGHT_REPO: &str = "vectorize-io/hindsight";

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

/// The first of these that is set is sent to GitHub's API. Without one, GitHub allows
/// 60 requests an hour per address (spec §4).
const TOKEN_VARS: [&str; 2] = ["GITHUB_TOKEN", "GH_TOKEN"];

fn token() -> Option<(&'static str, String)> {
    token_from(|name| std::env::var(name).ok())
}

fn token_from(var: impl Fn(&str) -> Option<String>) -> Option<(&'static str, String)> {
    TOKEN_VARS.into_iter().find_map(|name| {
        let value = var(name)?;
        let value = value.trim();
        (!value.is_empty()).then(|| (name, value.to_string()))
    })
}

/// A GET to GitHub's API, with the token when one is set. Only `api.github.com` gets it.
pub fn api_get(
    agent: &ureq::Agent,
    url: &str,
) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
    debug_assert!(url.starts_with("https://api.github.com/"));
    let request = agent
        .get(url)
        .header("accept", "application/vnd.github+json");
    match token() {
        Some((_, token)) => request.header("authorization", format!("Bearer {token}")),
        None => request,
    }
}

fn api_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, HttpError> {
    let response = api_get(agent, url)
        .call()
        .map_err(|err| HttpError(format!("{url}: {err}")))?;
    if response.status() == 401
        && let Some((name, _)) = token()
    {
        return Err(HttpError(format!(
            "GitHub refused the token in {name}. Fix or unset it, then run this again."
        )));
    }
    http::body_if_ok(response, url)
}

pub fn fetch_stable_tags(agent: &ureq::Agent, repo: &str) -> Result<Vec<String>, HttpError> {
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=30");
    let body = api_bytes(agent, &url)?;
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
    use super::{stable_tags, token_from};

    #[test]
    fn prefers_github_token_and_skips_blank_values() {
        let vars = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(
            token_from(vars(&[("GH_TOKEN", "b"), ("GITHUB_TOKEN", " a\n")])),
            Some(("GITHUB_TOKEN", "a".to_string()))
        );
        assert_eq!(
            token_from(vars(&[("GITHUB_TOKEN", "  "), ("GH_TOKEN", "b")])),
            Some(("GH_TOKEN", "b".to_string()))
        );
        assert_eq!(token_from(vars(&[])), None);
    }

    #[test]
    fn keeps_stable_releases_in_order() {
        let json = include_str!("../tests/fixtures/github-releases.json");
        assert_eq!(
            stable_tags(json).unwrap(),
            ["v0.16.0", "v0.15.0", "v0.14.0"]
        );
    }
}
