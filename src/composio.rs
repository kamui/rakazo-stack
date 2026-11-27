//! Integrations through Composio (spec §13): the check of the project API key.
//!
//! The reply's body is never read: Composio's refusal quotes part of the key.

use crate::model_provider::KeyCheck;
use crate::secrets::PastedKey;

/// The key's name in `secrets.env` and in Rakazo's env.
pub const KEY_NAME: &str = "COMPOSIO_API_KEY";
pub const API_BASE: &str = "https://backend.composio.dev";

fn judge_key(status: u16) -> KeyCheck {
    match status {
        200 => KeyCheck::Accepted,
        401 | 403 => KeyCheck::Rejected(status),
        _ => KeyCheck::Unchecked(format!("HTTP {status}")),
    }
}

/// Asks Composio, at `base`, to list one auth config: a read that costs no tool call and
/// that Rakazo makes itself. Composio documents only user keys for its session endpoint.
pub fn check_key(agent: &ureq::Agent, base: &str, key: &PastedKey) -> KeyCheck {
    // A redirect would carry the `x-api-key` header to wherever it points.
    let reply = agent
        .get(format!("{base}/api/v3.1/auth_configs?limit=1"))
        .config()
        .max_redirects(0)
        .build()
        .header("x-api-key", key.expose())
        .call();
    match reply {
        Ok(response) => judge_key(response.status().as_u16()),
        Err(err) => KeyCheck::Unchecked(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::testing::{Request, stub};
    use std::net::TcpListener;

    const KEY: &str = "placeholder-key";

    // Recorded from Composio on 2026-11-26, sent placeholder keys. The request id and the
    // quoted part of the key are replaced.
    const INVALID: &str = r#"{"error":{"message":"Invalid API key: ak_***","code":801,"slug":"APIKey_InvalidAPIKey","status":401,"request_id":"placeholder","suggested_fix":"Please check you are using a valid API key."}}"#;
    const MALFORMED: &str = r#"{"error":{"message":"Invalid API key format. Please provide a valid API key.","code":902,"slug":"Auth_Unauthorized","status":401,"request_id":"placeholder","suggested_fix":"API keys are at least 10 characters long. Check your dashboard for a valid key."}}"#;

    // Not recorded (that takes a real key): written from the 200 schema in Composio's
    // OpenAPI document.
    const VALID: &str = r#"{"items":[],"total_pages":0,"current_page":1,"total_items":0}"#;

    fn check(status: u16, reply: &'static str) -> (KeyCheck, Request) {
        let (base, served) = stub(status, reply);
        let key = PastedKey::new(KEY).unwrap();
        let result = check_key(&crate::http::agent(), &base, &key);
        (result, served.join().unwrap())
    }

    #[test]
    fn the_check_lists_one_auth_config_with_the_key_in_x_api_key() {
        let (result, request) = check(200, VALID);
        assert_eq!(result, KeyCheck::Accepted);
        assert_eq!(request.line, "GET /api/v3.1/auth_configs?limit=1 HTTP/1.1");
        assert!(
            request
                .headers
                .iter()
                .any(|h| h.eq_ignore_ascii_case("x-api-key: placeholder-key"))
        );
    }

    #[test]
    fn a_refusal_rejects_the_key() {
        // 403 is Composio's documented "no permission" status; it wasn't recorded.
        for (status, reply) in [(401, INVALID), (401, MALFORMED), (403, "{}")] {
            assert_eq!(check(status, reply).0, KeyCheck::Rejected(status));
        }
    }

    #[test]
    fn other_replies_give_no_verdict() {
        for (status, reply) in [(302, ""), (404, "{}"), (429, "{}"), (503, "<html></html>")] {
            assert_eq!(
                check(status, reply).0,
                KeyCheck::Unchecked(format!("HTTP {status}"))
            );
        }
    }

    #[test]
    fn an_unreachable_composio_gives_no_verdict_and_never_quotes_the_key() {
        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let key = PastedKey::new(KEY).unwrap();
        let result = check_key(&crate::http::agent(), &format!("http://{closed}"), &key);
        let KeyCheck::Unchecked(reason) = result else {
            panic!("expected no verdict");
        };
        assert!(!reason.contains(KEY));
    }
}
