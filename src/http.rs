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

#[cfg(test)]
pub mod testing {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    pub struct Request {
        pub line: String,
        pub headers: Vec<String>,
        pub body: String,
    }

    /// Serves one canned reply and hands back the request it answered.
    pub fn stub(status: u16, reply: &'static str) -> (String, JoinHandle<Request>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let served = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut lines = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim_end().is_empty() {
                    break;
                }
                lines.push(line.trim_end().to_string());
            }
            let length = lines
                .iter()
                .filter_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(str::trim)
                        .map(str::to_string)
                })
                .find_map(|length| length.parse().ok())
                .unwrap_or(0);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            write!(
                reader.into_inner(),
                "HTTP/1.1 {status} Reply\r\ncontent-type: application/json\r\nlocation: http://127.0.0.1:9/elsewhere\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                reply.len()
            )
            .unwrap();
            Request {
                line: lines.remove(0),
                headers: lines,
                body: String::from_utf8(body).unwrap(),
            }
        });
        (base, served)
    }
}
