use std::time::Duration;

/// One HTTP client for GitHub, GHCR and the Instance. Status codes come back as
/// values, so callers can tell a missing image (404) from a failed request.
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .user_agent(concat!("rakazo-stack/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

#[derive(Debug)]
pub struct HttpError(pub String);

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// GETs `url` and returns its body, treating any non-200 reply as an error.
pub fn get_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, HttpError> {
    let response = agent
        .get(url)
        .call()
        .map_err(|err| HttpError(format!("{url}: {err}")))?;
    body_if_ok(response, url)
}

/// The body of a 200 reply; any other status is an error.
pub fn body_if_ok(
    mut response: ureq::http::Response<ureq::Body>,
    url: &str,
) -> Result<Vec<u8>, HttpError> {
    let status = response.status();
    if status != 200 {
        return Err(HttpError(format!("{url}: HTTP {status}")));
    }
    response
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_to_vec()
        .map_err(|err| HttpError(format!("{url}: {err}")))
}
