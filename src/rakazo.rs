//! The running Instance's Rakazo, reached through Compose and its web origin. Secrets go
//! over stdin or an HTTP body, never as arguments: `docker.exe` doesn't see WSL's
//! environment, and arguments show up in process lists.

use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};

use crate::answers::Password;
use crate::database_settings::{Rakazo, Session};
use crate::docker::Docker;

/// Upstream's defaults for `POSTGRES_USER` and `POSTGRES_DB`, which setup never changes.
/// The official Postgres image trusts local socket connections, so no password is needed.
const DB_OWNER: &str = "rakazo";
const DB_NAME: &str = "rakazo";

/// Reads the Owner's email and password as JSON on stdin, then runs Rakazo's own
/// `bootstrap-owner` with them in its environment, as upstream's docs do with `-e`.
const BOOTSTRAP_OWNER: &str = r#"let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => { input += chunk; });
process.stdin.on("end", () => {
  const { email, password } = JSON.parse(input);
  process.env.OWNER_EMAIL = email;
  process.env.OWNER_PASSWORD = password;
  const script = require("node:path").resolve("dist/cli/bootstrap-owner.js");
  import(require("node:url").pathToFileURL(script).href).catch((err) => {
    console.error(err instanceof Error ? err.message : String(err));
    process.exit(1);
  });
});"#;

pub struct LocalRakazo<'a> {
    pub docker: &'a Docker,
    pub folder: &'a Path,
    pub agent: &'a ureq::Agent,
    pub origin: &'a str,
}

impl LocalRakazo<'_> {
    fn exec(&self, service: &str, args: &[&str], stdin: &[u8]) -> Result<Output, String> {
        let mut command = self.docker.compose(self.folder, ["exec", "-T", service]);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|err| err.to_string())?;
        let mut pipe = child.stdin.take().expect("stdin is piped");
        pipe.write_all(stdin).map_err(|err| err.to_string())?;
        drop(pipe);
        let output = child.wait_with_output().map_err(|err| err.to_string())?;
        if output.status.success() {
            Ok(output)
        } else {
            Err(last_line(&output.stderr)
                .unwrap_or_else(|| format!("`{service}` exited with {}", output.status)))
        }
    }

    fn post(&self, path: &str, session: Option<&Session>, body: &str) -> Result<String, String> {
        let url = format!("{}{path}", self.origin);
        let mut request = self.agent.post(&url).content_type("application/json");
        if let Some(session) = session {
            request = request.header("authorization", format!("Bearer {}", session.token));
        }
        let mut response = request.send(body).map_err(|err| err.to_string())?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|err| err.to_string())?;
        if (200..300).contains(&status) {
            Ok(text)
        } else {
            Err(failure_message(status, &text))
        }
    }
}

impl Rakazo for LocalRakazo<'_> {
    fn owner_exists(&self, email: &str) -> Result<bool, String> {
        let args = [
            "psql",
            "-X",
            "-q",
            "-t",
            "-A",
            "-v",
            "ON_ERROR_STOP=1",
            "-U",
            DB_OWNER,
            "-d",
            DB_NAME,
            "-f",
            "-",
        ];
        let output = self.exec("postgres", &args, owner_query(email).as_bytes())?;
        match String::from_utf8_lossy(&output.stdout).trim() {
            "t" => Ok(true),
            "f" => Ok(false),
            other => Err(format!("unexpected reply from Postgres: {other:?}")),
        }
    }

    fn bootstrap_owner(&self, email: &str, password: &Password) -> Result<(), String> {
        let input = serde_json::json!({ "email": email, "password": password.expose() });
        self.exec(
            "api",
            &["node", "-e", BOOTSTRAP_OWNER],
            input.to_string().as_bytes(),
        )
        .map(drop)
    }

    fn sign_in(&self, email: &str, password: &Password) -> Result<Session, String> {
        let body = serde_json::json!({ "email": email, "password": password.expose() });
        let reply = self.post("/api/auth/sign-in/email", None, &body.to_string())?;
        session_token(&reply)
            .map(|token| Session { token })
            .ok_or_else(|| "Rakazo's sign-in reply had no token".to_string())
    }

    fn sign_out(&self, session: Session) -> Result<(), String> {
        self.post("/api/auth/sign-out", Some(&session), "{}")
            .map(drop)
    }
}

/// Read-only by setting, with the email as a standard SQL literal.
fn owner_query(email: &str) -> String {
    let literal = email.replace('\'', "''");
    format!(
        "SET default_transaction_read_only = on;\n\
         SELECT EXISTS (SELECT 1 FROM \"user\" u JOIN account a ON a.\"userId\" = u.id \
         WHERE u.email = '{literal}' AND u.\"emailVerified\" AND a.\"providerId\" = 'credential');\n"
    )
}

fn session_token(reply: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(reply).ok()?;
    value.get("token")?.as_str().map(str::to_string)
}

/// The message from a non-2xx reply: `/rpc` wraps it as `{"json":{"code","status","message"}}`,
/// Better Auth's routes send `{"code","message"}`.
pub fn failure_message(status: u16, body: &str) -> String {
    let value: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let message = value.as_ref().and_then(|value| {
        value
            .pointer("/json/message")
            .or_else(|| value.get("message"))
            .and_then(serde_json::Value::as_str)
    });
    match message {
        Some(message) if !message.is_empty() => format!("{message} (HTTP {status})"),
        _ => format!("HTTP {status}"),
    }
}

fn last_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checklist::Checklist;
    use crate::database_settings::{DatabaseSetting, fill_in};

    #[test]
    fn quotes_the_email_in_the_owner_query() {
        let query = owner_query("o'wner@example.com");
        assert!(query.starts_with("SET default_transaction_read_only = on;"));
        assert!(query.contains("u.email = 'o''wner@example.com'"));
    }

    #[test]
    fn reads_the_signed_token_from_a_sign_in_reply() {
        let reply = r#"{"redirect":false,"token":"placeholder.signature","user":{"email":"owner@example.com"}}"#;
        assert_eq!(
            session_token(reply).as_deref(),
            Some("placeholder.signature")
        );
        assert_eq!(session_token(r#"{"user":{}}"#), None);
    }

    #[test]
    fn reads_failure_messages_from_both_reply_shapes() {
        assert_eq!(
            failure_message(401, RECORDED_RPC_UNAUTHORIZED),
            "Unauthorized (HTTP 401)"
        );
        assert_eq!(
            failure_message(
                401,
                r#"{"code":"INVALID_EMAIL_OR_PASSWORD","message":"Invalid email or password"}"#
            ),
            "Invalid email or password (HTTP 401)"
        );
        assert_eq!(failure_message(502, "<html>Bad Gateway</html>"), "HTTP 502");
    }

    /// `POST /rpc/voice/connect` without a session on Rakazo v0.16.0.
    const RECORDED_RPC_UNAUTHORIZED: &str =
        r#"{"json":{"defined":false,"code":"UNAUTHORIZED","status":401,"message":"Unauthorized"}}"#;

    struct RejectingRakazo;

    impl Rakazo for RejectingRakazo {
        fn owner_exists(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
        fn bootstrap_owner(&self, _: &str, _: &Password) -> Result<(), String> {
            Ok(())
        }
        fn sign_in(&self, _: &str, _: &Password) -> Result<Session, String> {
            Err(failure_message(401, RECORDED_RPC_UNAUTHORIZED))
        }
        fn sign_out(&self, _: Session) -> Result<(), String> {
            unreachable!()
        }
    }

    #[test]
    fn a_non_2xx_reply_becomes_a_checklist_item() {
        let mut checklist = Checklist::default();
        let mut filled = Vec::new();
        let mut front = crate::answers::NoQuestions {
            env: |_: &str| None,
        };
        fill_in(
            &RejectingRakazo,
            &mut front,
            "owner@example.com",
            Some(Password::new("correct-horse-battery".into()).unwrap()),
            &mut filled,
            &mut checklist,
        );
        assert_eq!(filled, [DatabaseSetting::Owner]);
        let item = checklist.items().next().unwrap();
        assert!(item.contains("Unauthorized (HTTP 401)"), "{item}");
    }
}
