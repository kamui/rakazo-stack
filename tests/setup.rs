//! `setup` against real Docker: starts Rakazo on localhost, creates the Owner, switches to
//! a bring-your-own Instance URL, and tears the test Instance down afterwards. No test
//! starts a `tailscale` container: it would try to join a real tailnet.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{docker, is_wsl};

static NEXT_INSTANCE: AtomicUsize = AtomicUsize::new(0);

/// One test Instance: its own HOME, Compose project and web port, removed on drop with
/// its volumes and networks.
struct TestInstance {
    home: tempfile::TempDir,
    project: String,
    web_port: u16,
}

impl TestInstance {
    fn new() -> Self {
        let web_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        Self {
            home: tempfile::tempdir().unwrap(),
            project: format!(
                "rkst-test-{}-{}",
                std::process::id(),
                NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed)
            ),
            web_port,
        }
    }

    fn folder(&self) -> PathBuf {
        self.home.path().join("rakazo-stack")
    }

    /// Runs `setup` with stdin closed, so there is no terminal, as on CI.
    fn setup(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rakazo-stack"));
        for (name, _) in std::env::vars_os() {
            if name.as_encoded_bytes().starts_with(b"RAKAZO_STACK_") {
                command.env_remove(name);
            }
        }
        command
            .arg("setup")
            .args(args)
            .env("HOME", self.home.path())
            .env("RAKAZO_STACK_TEST_PROJECT", &self.project)
            .env("RAKAZO_STACK_TEST_WEB_PORT", self.web_port.to_string())
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .output()
            .expect("run rakazo-stack")
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.web_port)
    }

    fn sql(&self, query: &str) -> String {
        let out = docker()
            .args([
                "compose",
                "-p",
                &self.project,
                "exec",
                "-T",
                "postgres",
                "psql",
                "-X",
                "-tA",
                "-U",
                "rakazo",
                "-d",
                "rakazo",
                "-c",
                query,
            ])
            .current_dir(self.folder())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A variable as a running service sees it.
    fn service_env(&self, service: &str, name: &str) -> String {
        let out = docker()
            .args(["compose", "-p", &self.project, "exec", "-T", service])
            .args(["printenv", name])
            .current_dir(self.folder())
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn containers(&self, format: &str) -> Vec<String> {
        let filter = format!("label=com.docker.compose.project={}", self.project);
        let out = docker()
            .args(["ps", "-a", "--filter", &filter, "--format", format])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect()
    }
}

impl Drop for TestInstance {
    fn drop(&mut self) {
        let mut down = docker();
        down.args([
            "compose",
            "-p",
            &self.project,
            "down",
            "-v",
            "--remove-orphans",
        ]);
        if self.folder().exists() {
            down.current_dir(self.folder());
        }
        let _ = down.output();
    }
}

/// Each host port mapping of the Instance's containers, after its service's name.
fn published_ports(instance: &TestInstance) -> Vec<String> {
    instance
        .containers("{{.Label \"com.docker.compose.service\"}} {{.Ports}}")
        .iter()
        .filter_map(|line| line.split_once(' '))
        .flat_map(|(service, ports)| {
            ports
                .split(", ")
                .filter(|port| port.contains("->"))
                .map(move |port| format!("{service} {port}"))
        })
        .collect()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

const OWNER_EMAIL: &str = "owner@example.com";
const OWNER_PASSWORD: &str = "placeholder-owner-password";
const MODEL_KEY: &str = "placeholder-model-key";
const DICTATION_ITEM: &str = "For private dictation, use Firefox or the Rakazo desktop app.";
const NO_KEY_ITEM: &str = "No Model provider key: each person connects a provider in Settings → Models. Connect one as the Owner before creating a bot, because Rakazo's onboarding stops at \"Connect a model\".";

fn post_json(url: &str, body: &serde_json::Value) -> (u16, String) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .post(url)
        .content_type("application/json")
        .send(body.to_string())
        .unwrap();
    let status = response.status().as_u16();
    (status, response.body_mut().read_to_string().unwrap())
}

/// Every file in the Instance folder, logs included.
fn folder_files(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![folder.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

fn checklist(out: &Output) -> Vec<String> {
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let (_, list) = stdout.split_once("Checklist:\n").expect("a Checklist");
    list.lines()
        .filter_map(|line| line.strip_prefix("  - "))
        .map(str::to_string)
        .collect()
}

fn secret_values(folder: &Path) -> Vec<String> {
    fs::read_to_string(folder.join("secrets.env"))
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(_, value)| value.to_string())
        .collect()
}

#[test]
#[ignore = "needs Docker and pulls Rakazo's images; CI and the dev machine run it with --include-ignored"]
fn a_scripted_first_run_creates_the_owner_and_a_rerun_changes_nothing() {
    let instance = TestInstance::new();
    let folder = instance.folder();
    let origin = instance.origin();

    let first = instance.setup(
        &["--no-questions"],
        &[
            ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
            ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
        ],
    );
    let first_text = text(&first);
    assert!(first.status.success(), "{first_text}");
    assert!(
        first_text.contains(&format!("is running at {origin}")),
        "{first_text}"
    );
    assert_eq!(
        checklist(&first),
        [NO_KEY_ITEM, DICTATION_ITEM],
        "{first_text}"
    );
    assert_eq!(
        instance.sql("SELECT count(*) FROM session"),
        "0",
        "setup signed out, so its session is gone"
    );

    let (status, reply) = post_json(
        &format!("{origin}/api/auth/sign-in/email"),
        &serde_json::json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }),
    );
    assert_eq!(status, 200, "the Owner signs in: {reply}");
    let (status, reply) = post_json(
        &format!("{origin}/api/auth/sign-up/email"),
        &serde_json::json!({ "email": "other@example.com", "password": OWNER_PASSWORD, "name": "Other" }),
    );
    assert!(
        (400..500).contains(&status),
        "sign-ups are off: {status} {reply}"
    );
    let agent = ureq::Agent::new_with_defaults();
    assert!(agent.get(&origin).call().unwrap().status().is_success());

    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.join("settings.json")).unwrap()).unwrap();
    let rakazo = &settings["components"]["rakazo"];
    let tag = rakazo["version"].as_str().unwrap();
    assert!(tag.starts_with('v'), "{settings}");
    for image in ["app", "computer"] {
        assert!(
            rakazo["images"][image]
                .as_str()
                .unwrap()
                .starts_with("sha256:"),
            "{settings}"
        );
    }
    assert_eq!(settings["program_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(settings["owner_email"], OWNER_EMAIL);
    assert_eq!(settings["filled_by_setup"], serde_json::json!(["owner"]));
    assert_eq!(settings["model_provider"], "openrouter");

    let upstream = agent
        .get(format!(
            "https://raw.githubusercontent.com/elie222/rakazo/{tag}/infra/compose/docker-compose.images.yml"
        ))
        .call()
        .unwrap()
        .into_body()
        .read_to_vec()
        .unwrap();
    assert_eq!(
        fs::read(folder.join(format!("rakazo.{tag}.images.yml"))).unwrap(),
        upstream,
        "Rakazo's compose file must be written byte for byte"
    );

    assert_eq!(
        published_ports(&instance),
        [format!("web 127.0.0.1:{}->5173/tcp", instance.web_port)],
        "only the web port is published"
    );

    let ps = docker()
        .args(["compose", "ps", "--format", "{{.Service}}"])
        .current_dir(&folder)
        .output()
        .unwrap();
    assert!(ps.status.success(), "{}", text(&ps));
    assert!(
        String::from_utf8_lossy(&ps.stdout)
            .lines()
            .any(|s| s == "web"),
        "`docker compose ps` from the Instance folder sees the project: {}",
        text(&ps)
    );

    if is_wsl() && Path::new("/usr/bin/docker").exists() {
        let wsl_up = Command::new("/usr/bin/docker")
            .args([
                "compose",
                "-p",
                &instance.project,
                "up",
                "-d",
                "--no-deps",
                "--force-recreate",
                "worker",
            ])
            .current_dir(&folder)
            .output()
            .unwrap();
        assert!(wsl_up.status.success(), "{}", text(&wsl_up));
    }

    let secrets_before = fs::read(folder.join("secrets.env")).unwrap();
    let settings_before = fs::read(folder.join("settings.json")).unwrap();
    // Shell variables named like `.env`'s must not reach Compose: this password would
    // lock Rakazo out of its own database.
    let second = instance.setup(
        &["--no-questions"],
        &[
            ("POSTGRES_PASSWORD", "placeholder-from-the-shell"),
            ("COMPOSE_PROFILES", "placeholder-profile"),
        ],
    );
    let second_text = text(&second);
    assert!(second.status.success(), "{second_text}");
    assert_eq!(
        checklist(&second),
        [NO_KEY_ITEM, DICTATION_ITEM],
        "with the Owner present, nothing asks for the password: {second_text}"
    );
    assert_eq!(
        fs::read(folder.join("secrets.env")).unwrap(),
        secrets_before
    );
    assert_eq!(
        fs::read(folder.join("settings.json")).unwrap(),
        settings_before
    );

    let made_in_wsl: Vec<String> = instance
        .containers("{{.Label \"com.docker.compose.project.working_dir\"}}")
        .into_iter()
        .filter(|dir| is_wsl() && dir.starts_with('/'))
        .collect();
    assert!(made_in_wsl.is_empty(), "{made_in_wsl:?}");

    let url = "https://rakazo.example.com";
    let switched = instance.setup(
        &["--no-questions"],
        &[
            ("RAKAZO_STACK_ACCESS", "bring-your-own"),
            ("RAKAZO_STACK_INSTANCE_URL", url),
        ],
    );
    let switched_text = text(&switched);
    assert!(switched.status.success(), "{switched_text}");
    assert!(
        switched_text.contains("signs everyone out"),
        "a scripted switch warns: {switched_text}"
    );
    let env = fs::read_to_string(folder.join(".env")).unwrap();
    for line in [
        format!("BETTER_AUTH_URL={url}"),
        format!("WEB_ORIGIN={url}"),
        format!("API_URL={url}"),
        "RAKAZO_HOST=rakazo.example.com".to_string(),
    ] {
        assert!(env.lines().any(|l| l == line), "missing {line}:\n{env}");
    }
    assert_eq!(
        published_ports(&instance),
        [format!("web 127.0.0.1:{}->5173/tcp", instance.web_port)],
        "web stays on loopback behind a bring-your-own URL"
    );
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["access"], "bring-your-own");
    assert_eq!(settings["bring_your_own_url"], url);
    // As the Self-hoster's proxy would forward a sign-in made at the Instance URL.
    let response = agent
        .post(format!("{origin}/api/auth/sign-in/email"))
        .header("Host", "rakazo.example.com")
        .header("Origin", url)
        .content_type("application/json")
        .send(serde_json::json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }).to_string())
        .unwrap();
    assert_eq!(response.status(), 200, "sign-in at the Instance URL");

    // A saved key with the provider out of reach: a proxy nothing listens on, which the
    // calls to the Instance bypass. No real key is needed to see it reach Rakazo.
    let mut secrets = fs::read_to_string(folder.join("secrets.env")).unwrap();
    secrets.push_str(&format!("OPENROUTER_API_KEY={MODEL_KEY}\n"));
    fs::write(folder.join("secrets.env"), secrets).unwrap();
    let third = instance.setup(
        &["--no-questions"],
        &[
            ("ALL_PROXY", "http://127.0.0.1:9"),
            ("HTTPS_PROXY", "http://127.0.0.1:9"),
            ("NO_PROXY", "127.0.0.1"),
        ],
    );
    let third_text = text(&third);
    assert!(!third_text.contains(MODEL_KEY), "setup printed the key");
    assert!(third.status.success(), "{third_text}");
    assert!(
        third_text.contains("couldn't check the saved OpenRouter key"),
        "{third_text}"
    );
    let items = checklist(&third);
    assert!(
        items.len() == 2 && items[0].ends_with("setup couldn't check whether it's on."),
        "{third_text}"
    );
    for service in ["api", "worker"] {
        assert_eq!(
            instance.service_env(service, "PI_DEFAULT_PROVIDER"),
            "openrouter"
        );
        assert_eq!(
            instance.service_env(service, "PI_DEFAULT_MODEL"),
            "~z-ai/glm-flash-latest"
        );
        assert!(
            instance.service_env(service, "OPENROUTER_API_KEY") == MODEL_KEY,
            "{service} doesn't have the key"
        );
    }

    for secret in secret_values(&folder) {
        for output in [&first_text, &second_text, &switched_text, &third_text] {
            assert!(!output.contains(&secret), "setup printed a secret");
        }
    }
    assert!(!first_text.contains(OWNER_PASSWORD) && !second_text.contains(OWNER_PASSWORD));
    let files = folder_files(&folder);
    assert!(
        files.iter().any(|f| f.parent().unwrap().ends_with("logs")),
        "setup writes run logs: {files:?}"
    );
    for file in files {
        let contents = String::from_utf8_lossy(&fs::read(&file).unwrap()).into_owned();
        assert!(
            !contents.contains(OWNER_PASSWORD),
            "the Owner password is in {}",
            file.display()
        );
        if file.parent().unwrap().ends_with("logs") {
            for secret in secret_values(&folder) {
                assert!(
                    !contents.contains(&secret),
                    "a secret is in {}",
                    file.display()
                );
            }
        }
    }
}

#[test]
#[ignore = "needs Docker for preflight; CI and the dev machine run it with --include-ignored"]
fn invalid_input_stops_before_anything_changes() {
    let instance = TestInstance::new();
    type Env<'a> = &'a [(&'a str, &'a str)];
    // OpenRouter itself refuses this one.
    const REJECTED_KEY: &str = "sk-or-v1-placeholder";
    let owner = |extra: &[(&'static str, &'static str)]| {
        let mut env = vec![
            ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
            ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
        ];
        env.extend_from_slice(extra);
        env
    };
    let no_key = owner(&[("RAKAZO_STACK_ACCESS", "tailscale")]);
    let ephemeral = owner(&[
        ("RAKAZO_STACK_ACCESS", "tailscale"),
        ("RAKAZO_STACK_TS_AUTHKEY", "tskey-client-kPLACEHOLDER"),
    ]);
    let bad_url = owner(&[
        ("RAKAZO_STACK_ACCESS", "bring-your-own"),
        ("RAKAZO_STACK_INSTANCE_URL", "http://rakazo.example.com"),
    ]);
    let both = owner(&[
        ("RAKAZO_STACK_ACCESS", "tailscale"),
        ("RAKAZO_STACK_TS_AUTHKEY", "tskey-auth-kPLACEHOLDER"),
        ("RAKAZO_STACK_INSTANCE_URL", "https://rakazo.example.com"),
    ]);
    let cases: [(&[&str], Env, &str); 11] = [
        (&["--no-questions"], &no_key, "RAKAZO_STACK_TS_AUTHKEY"),
        (&["--no-questions"], &ephemeral, "ephemeral"),
        (&["--no-questions"], &bad_url, "must look like"),
        (
            &["--no-questions"],
            &both,
            "can't be combined with Tailscale",
        ),
        (
            &["--no-questions"],
            &[
                ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
                ("RAKAZO_STACK_OWNER_PASSWORD", "fifteen-chars!!"),
            ],
            "at least 16 characters",
        ),
        (
            &["--no-questions"],
            &[("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD)],
            "RAKAZO_STACK_OWNER_EMAIL",
        ),
        (
            &["--no-questions"],
            &[("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL)],
            "RAKAZO_STACK_OWNER_PASSWORD",
        ),
        (&[], &[], "--no-questions"),
        (
            &["--no-questions"],
            &[
                ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
                ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
                ("RAKAZO_STACK_MODEL_PROVIDER", "mistral"),
            ],
            "RAKAZO_STACK_MODEL_PROVIDER",
        ),
        (
            &["--no-questions"],
            &[
                ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
                ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
                ("RAKAZO_STACK_OPENROUTER_API_KEY", "placeholder key"),
            ],
            "character no key has",
        ),
        (
            &["--no-questions"],
            &[
                ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
                ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
                ("RAKAZO_STACK_OPENROUTER_API_KEY", REJECTED_KEY),
            ],
            "OpenRouter rejected the key (HTTP 401)",
        ),
    ];
    for (args, env, expected) in cases {
        let out = instance.setup(args, env);
        let out_text = text(&out);
        assert!(!out.status.success(), "{out_text}");
        assert!(out_text.contains(expected), "{expected}: {out_text}");
        assert!(!out_text.contains("fifteen-chars!!"), "{out_text}");
        assert!(!out_text.contains(REJECTED_KEY), "setup printed the key");
        assert!(!out_text.contains("PLACEHOLDER"), "{out_text}");
        let left: Vec<_> = fs::read_dir(instance.folder())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, [".lock"], "only the lock is there");
        assert!(instance.containers("{{.Names}}").is_empty());
    }
}
