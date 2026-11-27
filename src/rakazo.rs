//! The running Instance's Rakazo, reached through Compose and its web origin. Secrets go
//! over stdin or an HTTP body, never as arguments: `docker.exe` doesn't see WSL's
//! environment, and arguments show up in process lists.

use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};
use std::time::Duration;

use crate::answers::Password;
use crate::database_settings::{Connect, DefaultSpace, Rakazo, Session, VoiceConnection};
use crate::docker::Docker;
use crate::lemonade::{RELAY_URL, SPEECH_MODEL, TRANSCRIPTION_MODEL};
use crate::memory::ConnectedSpace;
use crate::model_provider::{OwnerModel, Provider};

/// Upstream's defaults for `POSTGRES_USER` and `POSTGRES_DB`, which setup never changes.
/// The official Postgres image trusts local socket connections, so no password is needed.
const DB_OWNER: &str = "rakazo";
const DB_NAME: &str = "rakazo";

/// `models.connect` asks the Model provider about the key before it saves, and Rakazo
/// v0.16.0 allows that 30 s for a key check and 15 s for a first request.
const POST_TIMEOUT: Duration = Duration::from_secs(60);

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

    fn post(
        &self,
        path: &str,
        session: Option<&Session>,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<String, String> {
        let url = format!("{}{path}", self.origin);
        let mut request = self
            .agent
            .post(&url)
            .config()
            .timeout_global(Some(POST_TIMEOUT))
            .build()
            .content_type("application/json");
        if let Some(session) = session {
            request = request.header("authorization", format!("Bearer {}", session.token));
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
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

impl LocalRakazo<'_> {
    /// One read-only `SELECT EXISTS (…)` as the database owner role.
    fn exists(&self, query: &str) -> Result<bool, String> {
        match self.query(query)?.as_str() {
            "t" => Ok(true),
            "f" => Ok(false),
            other => Err(format!("unexpected reply from Postgres: {other:?}")),
        }
    }

    /// Runs a query that sets itself read-only, as the database owner role, and returns
    /// what it printed, unaligned.
    fn query(&self, query: &str) -> Result<String, String> {
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
        let output = self.exec("postgres", &args, query.as_bytes())?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl Rakazo for LocalRakazo<'_> {
    fn owner_exists(&self, email: &str) -> Result<bool, String> {
        self.exists(&owner_query(email))
    }

    fn voice_exists(&self, email: &str) -> Result<bool, String> {
        self.exists(&voice_query(email))
    }

    fn model_connection(&self, email: &str, provider: Provider) -> Result<Option<String>, String> {
        let reply = self.query(&model_connection_query(email, provider))?;
        Ok((!reply.is_empty()).then_some(reply))
    }

    fn default_space(&self, email: &str) -> Result<Option<DefaultSpace>, String> {
        let reply = self.query(&default_space_query(email))?;
        if reply.is_empty() {
            return Ok(None);
        }
        serde_json::from_str(&reply)
            .map(Some)
            .map_err(|err| format!("unexpected reply from Postgres: {err}"))
    }

    fn spaces_on_hindsight(&self) -> Result<Vec<ConnectedSpace>, String> {
        let reply = self.query(SPACES_ON_HINDSIGHT)?;
        serde_json::from_str(&reply).map_err(|err| format!("unexpected reply from Postgres: {err}"))
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
        let reply = self.post("/api/auth/sign-in/email", None, &[], &body.to_string())?;
        session_token(&reply)
            .map(|token| Session { token })
            .ok_or_else(|| "Rakazo's sign-in reply had no token".to_string())
    }

    fn connect_voice(&self, session: &Session, voice_id: Option<&str>) -> Result<(), String> {
        self.post(
            "/rpc/voice/connect",
            Some(session),
            &[],
            &voice_connect_body(voice_id),
        )
        .map(drop)
    }

    fn voice_connection(&self, session: &Session) -> Result<Option<VoiceConnection>, String> {
        let reply = self.post(
            "/rpc/voice/credentials",
            Some(session),
            &[],
            r#"{"json":{}}"#,
        )?;
        self_hosted_voice(&reply)
    }

    fn connect_model(
        &self,
        session: &Session,
        model: OwnerModel,
        connect: &Connect,
    ) -> Result<(), String> {
        self.post(
            "/rpc/models/connect",
            Some(session),
            &[],
            &connect_model_body(model, connect),
        )
        .map(drop)
        .map_err(|err| without_key(err, model.key.expose()))
    }

    fn connect_memory(
        &self,
        session: &Session,
        space_id: &str,
        tenant_key: &str,
    ) -> Result<(), String> {
        // The header names the Space, so the call can't land on another one the Owner opened.
        self.post(
            "/rpc/memory/connectProvider",
            Some(session),
            &[("x-rakazo-space-id", space_id)],
            &connect_memory_body(tenant_key),
        )
        .map(drop)
    }

    fn sign_out(&self, session: Session) -> Result<(), String> {
        self.post("/api/auth/sign-out", Some(&session), &[], "{}")
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

fn voice_query(email: &str) -> String {
    let literal = email.replace('\'', "''");
    format!(
        "SET default_transaction_read_only = on;\n\
         SELECT EXISTS (SELECT 1 FROM user_voice_credentials v JOIN \"user\" u ON u.id = v.\"userId\" \
         WHERE u.email = '{literal}');\n"
    )
}

const SELF_HOSTED_VOICE: &str = "openai-compatible";

/// oRPC's body shape. Rakazo checks the server (models, then voices) before it saves,
/// and picks the voice unless it is given one.
fn voice_connect_body(voice_id: Option<&str>) -> String {
    let mut body = serde_json::json!({
        "provider": SELF_HOSTED_VOICE,
        "baseUrl": RELAY_URL,
        "ttsModel": SPEECH_MODEL,
        "sttModel": TRANSCRIPTION_MODEL,
    });
    if let Some(voice_id) = voice_id {
        body["voiceId"] = voice_id.into();
    }
    serde_json::json!({ "json": body }).to_string()
}

/// Rakazo lists the newest connection first, the one `voice.connect` updates.
fn self_hosted_voice(reply: &str) -> Result<Option<VoiceConnection>, String> {
    #[derive(serde::Deserialize)]
    struct Listed {
        provider: String,
        #[serde(flatten)]
        connection: VoiceConnection,
    }
    #[derive(serde::Deserialize)]
    struct Reply {
        json: Vec<Listed>,
    }
    let reply: Reply = serde_json::from_str(reply)
        .map_err(|err| format!("unexpected reply from Rakazo's voice.credentials: {err}"))?;
    Ok(reply
        .json
        .into_iter()
        .find(|listed| listed.provider == SELF_HOSTED_VOICE)
        .map(|listed| listed.connection))
}

/// The name setup gives its connection, so that a new key goes to it even when the user
/// added another to the same provider.
fn connection_label(provider: Provider) -> String {
    format!("{} (rakazo-stack)", provider.rakazo_name())
}

fn model_connection_query(email: &str, provider: Provider) -> String {
    let literal = email.replace('\'', "''");
    let id = provider.rakazo_id();
    let label = connection_label(provider);
    format!(
        "SET default_transaction_read_only = on;\n\
         SELECT c.id FROM user_model_credentials c JOIN \"user\" u ON u.id = c.\"userId\" \
         WHERE u.email = '{literal}' AND c.provider = '{id}' \
         ORDER BY c.label = '{label}' DESC, c.\"updatedAt\" DESC, c.\"createdAt\" DESC, c.id DESC LIMIT 1;\n"
    )
}

/// A null `credentialId` adds a connection; without the field Rakazo would overwrite the
/// provider's newest. Rakazo keeps an updated connection's name.
fn connect_model_body(model: OwnerModel, connect: &Connect) -> String {
    let credential_id = match connect {
        Connect::Add => None,
        Connect::Update { id, .. } => Some(id),
    };
    serde_json::json!({
        "json": {
            "provider": model.provider.rakazo_id(),
            "apiKey": model.key.expose(),
            "modelId": model.model,
            "label": connection_label(model.provider),
            "credentialId": credential_id,
            "makeDefault": connect.make_default(),
            "scope": "account",
        }
    })
    .to_string()
}

/// Rakazo hides a key it is sent from its error messages. This also withholds a message
/// that quotes any four characters of it, as a provider's masked key would.
fn without_key(message: String, key: &str) -> String {
    let quoted = key
        .as_bytes()
        .windows(4.min(key.len()))
        .any(|part| message.as_bytes().windows(part.len()).any(|w| w == part));
    if quoted {
        "Rakazo's reason isn't shown, because it quotes part of the key".to_string()
    } else {
        message
    }
}

/// The Space Rakazo opens first for the user (its default, earliest joined), as JSON, and
/// whether it has a memory connection to any provider: one setup didn't make is left alone.
fn default_space_query(email: &str) -> String {
    let literal = email.replace('\'', "''");
    format!(
        "SET default_transaction_read_only = on;\n\
         SELECT json_build_object('id', m.\"spaceId\", 'memory', EXISTS \
         (SELECT 1 FROM space_memory_configs c WHERE c.\"spaceId\" = m.\"spaceId\")) \
         FROM space_members m JOIN spaces s ON s.id = m.\"spaceId\" JOIN \"user\" u ON u.id = m.\"userId\" \
         WHERE u.email = '{literal}' AND s.\"isDefault\" AND s.\"deletingAt\" IS NULL \
         ORDER BY m.\"createdAt\", m.id LIMIT 1;\n"
    )
}

const SPACES_ON_HINDSIGHT: &str = "SET default_transaction_read_only = on;\n\
     SELECT coalesce(json_agg(json_build_object('space', s.name, 'by', u.email) \
     ORDER BY s.name), '[]') FROM space_memory_configs c JOIN spaces s ON s.id = c.\"spaceId\" \
     LEFT JOIN \"user\" u ON u.id = c.\"userId\" WHERE c.provider = 'hindsight';\n";

/// Spec §7 step 9: nothing for `retainConversations` or the other toggles, so Rakazo's
/// defaults stay.
fn connect_memory_body(tenant_key: &str) -> String {
    serde_json::json!({
        "json": {
            "provider": "hindsight",
            "settings": { "mode": "local", "baseUrl": "http://hindsight:8888" },
            "credentials": { "apiKey": tenant_key },
        }
    })
    .to_string()
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
    use crate::database_settings::{DatabaseSetting, Fillable, fill_in};
    use crate::http::testing::stub;
    use crate::platform::Platform;
    use crate::secrets::PastedKey;

    #[test]
    fn quotes_the_email_in_the_owner_query() {
        let query = owner_query("o'wner@example.com");
        assert!(query.starts_with("SET default_transaction_read_only = on;"));
        assert!(query.contains("u.email = 'o''wner@example.com'"));
    }

    #[test]
    fn the_voice_query_reads_the_owners_credentials_only() {
        let query = voice_query("o'wner@example.com");
        assert!(query.starts_with("SET default_transaction_read_only = on;"));
        assert!(query.contains("FROM user_voice_credentials v JOIN \"user\" u"));
        assert!(query.contains("u.email = 'o''wner@example.com'"));
    }

    #[test]
    fn the_voice_connect_body_is_the_specs() {
        let body: serde_json::Value = serde_json::from_str(&voice_connect_body(None)).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"json":{"provider":"openai-compatible","baseUrl":"http://lemonade-relay:13306/api/v1","ttsModel":"kokoro-v1","sttModel":TRANSCRIPTION_MODEL}})
        );
        let kept: serde_json::Value =
            serde_json::from_str(&voice_connect_body(Some("af_bella"))).unwrap();
        assert_eq!(kept["json"]["voiceId"], "af_bella");
        assert_eq!(kept["json"]["sttModel"], TRANSCRIPTION_MODEL);
    }

    #[test]
    fn reads_the_newest_self_hosted_connection_from_the_credentials_list() {
        let reply = format!(
            r#"{{"json":[{{"id":"placeholder-hosted-id","provider":"elevenlabs","hasKey":true,"isDefault":false,"voiceId":"placeholder-voice","speechModel":"","transcribe":true,"endpoint":"","sttEndpoint":"","ttsModel":"","sttModel":""}},{}]}}"#,
            &RECORDED_VOICE_CONNECTED[8..RECORDED_VOICE_CONNECTED.len() - 1]
        );
        let connection = self_hosted_voice(&reply).unwrap().unwrap();
        assert_eq!(
            connection,
            VoiceConnection {
                is_default: true,
                voice_id: "af_heart".into(),
                endpoint: RELAY_URL.into(),
                stt_endpoint: String::new(),
                stt_model: "Whisper-Small".into(),
            }
        );
        assert_eq!(self_hosted_voice(r#"{"json":[]}"#).unwrap(), None);
        assert!(self_hosted_voice("<html>").is_err());
    }

    /// `POST /rpc/voice/connect` on Rakazo v0.16.0, with the id replaced.
    const RECORDED_VOICE_CONNECTED: &str = r#"{"json":{"id":"placeholder-credential-id","provider":"openai-compatible","hasKey":true,"isDefault":true,"voiceId":"af_heart","speechModel":"","transcribe":true,"endpoint":"http://lemonade-relay:13306/api/v1","sttEndpoint":"","ttsModel":"kokoro-v1","sttModel":"Whisper-Small"}}"#;

    /// The same call with a `baseUrl` nothing answers on.
    const RECORDED_VOICE_UNREACHABLE: &str = r#"{"json":{"defined":false,"code":"BAD_REQUEST","status":400,"message":"Couldn't reach the speech server at http://lemonade-relay:9."}}"#;

    #[test]
    fn reads_the_recorded_voice_connect_replies() {
        let connected: serde_json::Value = serde_json::from_str(RECORDED_VOICE_CONNECTED).unwrap();
        assert_eq!(connected["json"]["voiceId"], "af_heart");
        assert_eq!(
            failure_message(400, RECORDED_VOICE_UNREACHABLE),
            "Couldn't reach the speech server at http://lemonade-relay:9. (HTTP 400)"
        );
    }

    #[test]
    fn the_space_queries_are_read_only_and_quote_the_email() {
        let query = default_space_query("o'wner@example.com");
        assert!(query.starts_with("SET default_transaction_read_only = on;"));
        assert!(query.contains("u.email = 'o''wner@example.com'"));
        assert!(SPACES_ON_HINDSIGHT.starts_with("SET default_transaction_read_only = on;"));
    }

    #[test]
    fn the_memory_connect_body_is_the_specs_and_sets_no_memory_toggle() {
        let body: serde_json::Value =
            serde_json::from_str(&connect_memory_body("placeholder-tenant-key")).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"json":{"provider":"hindsight","settings":{"mode":"local","baseUrl":"http://hindsight:8888"},"credentials":{"apiKey":"placeholder-tenant-key"}}})
        );
    }

    // In the layout Postgres's `json_build_object` and `json_agg` print, as read from
    // Rakazo v0.16.0's database; the id is a placeholder.
    const RECORDED_DEFAULT_SPACE: &str = r#"{"id" : "placeholder-space-id", "memory" : false}"#;
    const RECORDED_SPACES: &str = "[{\"space\" : \"Personal\", \"by\" : \"owner@example.com\"}, \n {\"space\" : \"Team\", \"by\" : null}]";

    #[test]
    fn reads_the_default_space_and_the_spaces_on_hindsight() {
        let space: DefaultSpace = serde_json::from_str(RECORDED_DEFAULT_SPACE).unwrap();
        assert_eq!(
            space,
            DefaultSpace {
                id: "placeholder-space-id".into(),
                memory: false
            }
        );
        let spaces: Vec<ConnectedSpace> = serde_json::from_str(RECORDED_SPACES).unwrap();
        assert_eq!(spaces[0].space, "Personal");
        assert_eq!(spaces[0].by.as_deref(), Some("owner@example.com"));
        assert_eq!(spaces[1].by, None);
        let none: Vec<ConnectedSpace> = serde_json::from_str("[]").unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn the_model_connection_query_prefers_setups_own_and_quotes_the_email() {
        let query = model_connection_query("o'wner@example.com", Provider::Gemini);
        assert!(query.starts_with("SET default_transaction_read_only = on;"));
        assert!(query.contains("FROM user_model_credentials c JOIN \"user\" u"));
        assert!(query.contains("u.email = 'o''wner@example.com' AND c.provider = 'google'"));
        assert!(query.contains("ORDER BY c.label = 'Google (rakazo-stack)' DESC"));
    }

    #[test]
    fn the_model_connect_body_adds_or_updates_one_connection() {
        let key = PastedKey::new("placeholder-key").unwrap();
        let body = |provider, connect: &Connect| -> serde_json::Value {
            let model = OwnerModel::new(provider, &key, None).unwrap().unwrap();
            serde_json::from_str(&connect_model_body(model, connect)).unwrap()
        };
        assert_eq!(
            body(Provider::OpenAi, &Connect::Add),
            serde_json::json!({"json":{"provider":"openai","apiKey":"placeholder-key","modelId":"gpt-6-luna","label":"OpenAI (rakazo-stack)","credentialId":null,"makeDefault":true,"scope":"account"}})
        );
        let update = Connect::Update {
            id: "placeholder-credential-id".into(),
            make_default: false,
        };
        assert_eq!(
            body(Provider::Gemini, &update),
            serde_json::json!({"json":{"provider":"google","apiKey":"placeholder-key","modelId":"gemini-3.5-flash-lite","label":"Google (rakazo-stack)","credentialId":"placeholder-credential-id","makeDefault":false,"scope":"account"}})
        );
    }

    // Schema-derived, not recorded (that takes a real key): oRPC's error shape with
    // v0.16.0's sentence for a refused key.
    const MODEL_KEY_REFUSED: &str = r#"{"json":{"defined":false,"code":"BAD_REQUEST","status":400,"message":"API key was rejected by the provider."}}"#;

    // Made up: Rakazo relaying a provider's masked key, or the end of one.
    const MASKED_KEY: &str = r#"{"json":{"message":"Incorrect API key provided: sk-pla***wxyz."}}"#;
    const KEY_ENDING: &str = r#"{"json":{"message":"The key ending wxyz was refused."}}"#;

    #[test]
    fn a_failed_model_connect_posts_to_the_route_and_never_shows_part_of_the_key() {
        const WITHHELD: &str = "Rakazo's reason isn't shown, because it quotes part of the key";
        let key = PastedKey::new("sk-placeholder-0123456789wxyz").unwrap();
        let docker = Docker::find(Platform::Linux).unwrap();
        let agent = crate::http::agent();
        for (reply, shown) in [
            (
                MODEL_KEY_REFUSED,
                "API key was rejected by the provider. (HTTP 400)",
            ),
            (MASKED_KEY, WITHHELD),
            (KEY_ENDING, WITHHELD),
        ] {
            let (origin, served) = stub(400, reply);
            let rakazo = LocalRakazo {
                docker: &docker,
                folder: Path::new("."),
                agent: &agent,
                origin: &origin,
            };
            let model = OwnerModel::new(Provider::OpenAi, &key, None)
                .unwrap()
                .unwrap();
            let session = Session {
                token: "placeholder-token".into(),
            };
            let err = rakazo
                .connect_model(&session, model, &Connect::Add)
                .unwrap_err();
            assert_eq!(err, shown);
            let request = served.join().unwrap();
            assert_eq!(request.line, "POST /rpc/models/connect HTTP/1.1");
            assert!(
                request
                    .headers
                    .iter()
                    .any(|h| h.eq_ignore_ascii_case("authorization: Bearer placeholder-token"))
            );
            assert_eq!(request.body, connect_model_body(model, &Connect::Add));
        }
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
        fn voice_exists(&self, _: &str) -> Result<bool, String> {
            unreachable!()
        }
        fn connect_voice(&self, _: &Session, _: Option<&str>) -> Result<(), String> {
            unreachable!()
        }
        fn voice_connection(&self, _: &Session) -> Result<Option<VoiceConnection>, String> {
            unreachable!()
        }
        fn model_connection(&self, _: &str, _: Provider) -> Result<Option<String>, String> {
            unreachable!()
        }
        fn connect_model(&self, _: &Session, _: OwnerModel, _: &Connect) -> Result<(), String> {
            unreachable!()
        }
        fn default_space(&self, _: &str) -> Result<Option<DefaultSpace>, String> {
            unreachable!()
        }
        fn spaces_on_hindsight(&self) -> Result<Vec<ConnectedSpace>, String> {
            unreachable!()
        }
        fn connect_memory(&self, _: &Session, _: &str, _: &str) -> Result<(), String> {
            unreachable!()
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
            Fillable::default(),
            &mut filled,
            &mut checklist,
        );
        assert_eq!(filled, [DatabaseSetting::Owner]);
        let item = checklist.items().next().unwrap();
        assert!(item.contains("Unauthorized (HTTP 401)"), "{item}");
    }
}
