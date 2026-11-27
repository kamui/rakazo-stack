//! Reads image digests from GHCR without pulling anything.

use serde::Deserialize;

use crate::http::HttpError;

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

fn parse_token(body: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct Token {
        token: String,
    }
    serde_json::from_slice::<Token>(body).ok().map(|t| t.token)
}

#[cfg(test)]
mod tests {
    use super::parse_token;

    #[test]
    fn reads_the_anonymous_pull_token() {
        assert_eq!(
            parse_token(br#"{"token":"djE6ZXhhbXBsZQ=="}"#).as_deref(),
            Some("djE6ZXhhbXBsZQ==")
        );
        assert_eq!(parse_token(b"<html>"), None);
    }
}
