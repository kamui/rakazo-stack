//! Reads image digests from GHCR and Docker Hub without pulling anything.

use std::collections::HashMap;

use serde::Deserialize;

use crate::http::HttpError;
use crate::settings::RecordedImage;
use crate::version::Version;

const MANIFEST_TYPES: &str = "application/vnd.oci.image.index.v1+json, \
     application/vnd.docker.distribution.manifest.list.v2+json, \
     application/vnd.oci.image.manifest.v1+json, \
     application/vnd.docker.distribution.manifest.v2+json";

pub struct Ghcr<'a> {
    agent: &'a ureq::Agent,
}

impl<'a> Ghcr<'a> {
    pub fn new(agent: &'a ureq::Agent) -> Self {
        Self { agent }
    }

    /// The digest `repository:tag` points to, or `None` when GHCR has no such tag.
    pub fn digest(&self, repository: &str, tag: &str) -> Result<Option<String>, HttpError> {
        let token = self.pull_token(repository)?;
        let url = format!("https://ghcr.io/v2/{repository}/manifests/{tag}");
        let response = self
            .agent
            .head(&url)
            .header("Authorization", &format!("Bearer {token}"))
            .header("Accept", MANIFEST_TYPES)
            .call()
            .map_err(|err| HttpError(format!("{url}: {err}")))?;
        match response.status().as_u16() {
            200 => response
                .headers()
                .get("docker-content-digest")
                .and_then(|v| v.to_str().ok())
                .map(|digest| Some(digest.to_string()))
                .ok_or_else(|| HttpError(format!("{url}: no digest in the reply"))),
            404 => Ok(None),
            status => Err(HttpError(format!("{url}: HTTP {status}"))),
        }
    }

    fn pull_token(&self, repository: &str) -> Result<String, HttpError> {
        let url = format!("https://ghcr.io/token?scope=repository:{repository}:pull");
        let body = crate::http::get_bytes(self.agent, &url)?;
        parse_token(&body).ok_or_else(|| HttpError(format!("{url}: no token in the reply")))
    }
}

/// The release behind Docker Hub's `tailscale/tailscale:stable`. GitHub releases run
/// ahead of the images, so the registry decides (spec §4).
pub fn tailscale_stable(agent: &ureq::Agent) -> Result<RecordedImage, HttpError> {
    let token = docker_hub_token(agent, "tailscale/tailscale")?;
    let url = "https://registry-1.docker.io/v2/tailscale/tailscale/manifests/stable";
    let fail = |reason: String| HttpError(format!("{url}: {reason}"));
    let mut response = agent
        .get(url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", MANIFEST_TYPES)
        .call()
        .map_err(|err| fail(err.to_string()))?;
    let status = response.status();
    if status != 200 {
        return Err(fail(format!("HTTP {status}")));
    }
    let digest = response
        .headers()
        .get("docker-content-digest")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| fail("no digest in the reply".to_string()))?;
    let manifest = response
        .body_mut()
        .read_to_vec()
        .map_err(|err| fail(err.to_string()))?;
    let version = annotated_version(&manifest)
        .ok_or_else(|| fail("no version annotation in the manifest".to_string()))?;
    Ok(RecordedImage { version, digest })
}

/// The digest Docker Hub's `repository:tag` points to, or `None` when it has no such tag.
pub fn docker_hub_digest(
    agent: &ureq::Agent,
    repository: &str,
    tag: &str,
) -> Result<Option<String>, HttpError> {
    let token = docker_hub_token(agent, repository)?;
    let url = format!("https://registry-1.docker.io/v2/{repository}/manifests/{tag}");
    let response = agent
        .head(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", MANIFEST_TYPES)
        .call()
        .map_err(|err| HttpError(format!("{url}: {err}")))?;
    match response.status().as_u16() {
        200 => response
            .headers()
            .get("docker-content-digest")
            .and_then(|v| v.to_str().ok())
            .map(|digest| Some(digest.to_string()))
            .ok_or_else(|| HttpError(format!("{url}: no digest in the reply"))),
        404 => Ok(None),
        status => Err(HttpError(format!("{url}: HTTP {status}"))),
    }
}

fn docker_hub_token(agent: &ureq::Agent, repository: &str) -> Result<String, HttpError> {
    let url = format!(
        "https://auth.docker.io/token?service=registry.docker.io&scope=repository:{repository}:pull"
    );
    let body = crate::http::get_bytes(agent, &url)?;
    parse_token(&body).ok_or_else(|| HttpError(format!("{url}: no token in the reply")))
}

/// The release tag, from the image index's version annotation: `1.2.3` is tag `v1.2.3`.
fn annotated_version(manifest: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct Manifest {
        annotations: HashMap<String, String>,
    }
    let mut manifest: Manifest = serde_json::from_slice(manifest).ok()?;
    let version = manifest
        .annotations
        .remove("org.opencontainers.image.version")?;
    let numeric = version.chars().all(|c| c.is_ascii_digit() || c == '.');
    (numeric && Version::parse(&version).is_some()).then(|| format!("v{version}"))
}

fn parse_token(body: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct Token {
        token: String,
    }
    serde_json::from_slice::<Token>(body).ok().map(|t| t.token)
}

#[cfg(test)]
mod tests {
    use super::{annotated_version, parse_token};

    #[test]
    fn reads_the_version_behind_tailscales_stable_tag() {
        // From the `stable` image index on Docker Hub, shortened.
        let index = br#"{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json",
            "manifests":[],"annotations":{"org.opencontainers.image.vendor":"Tailscale",
            "org.opencontainers.image.version":"1.104.7"}}"#;
        assert_eq!(annotated_version(index).as_deref(), Some("v1.104.7"));
        for bad in [
            r#"{"schemaVersion":2}"#,
            r#"{"annotations":{}}"#,
            r#"{"annotations":{"org.opencontainers.image.version":"stable"}}"#,
            r#"{"annotations":{"org.opencontainers.image.version":"1.2\nX=y"}}"#,
        ] {
            assert_eq!(annotated_version(bad.as_bytes()), None, "{bad}");
        }
    }

    #[test]
    fn reads_the_anonymous_pull_token() {
        assert_eq!(
            parse_token(br#"{"token":"djE6ZXhhbXBsZQ=="}"#).as_deref(),
            Some("djE6ZXhhbXBsZQ==")
        );
        assert_eq!(parse_token(b"<html>"), None);
    }
}
