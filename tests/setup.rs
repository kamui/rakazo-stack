//! `setup` against real Docker: starts Rakazo on localhost with voice, creates the Owner
//! and their voice, switches to a bring-your-own Instance URL, runs memory, and tears the
//! test Instance down afterwards, its built relay image included. No test starts a
//! `tailscale` container: it would try to join a real tailnet.

mod common;

use std::cell::OnceCell;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use common::{docker, is_wsl};

static NEXT_INSTANCE: AtomicUsize = AtomicUsize::new(0);
/// Test Instances run one at a time: their sidecars share the engine's firewall, and
/// their supervisors share upstream's fixed probe network name.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// One test Instance: its own HOME, Compose project and web port, removed on drop with
/// its volumes, networks, built images and the sidecar's firewall rules.
struct TestInstance {
    home: tempfile::TempDir,
    project: String,
    web_port: u16,
    /// The engine's egress state before this Instance's first `setup` started its sidecar.
    egress_before: OnceCell<EgressBaseline>,
    _turn: MutexGuard<'static, ()>,
}

/// `rakazo-c` rules on the engine, and whether another Instance's sidecar keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EgressBaseline {
    rules: bool,
    other_sidecars: bool,
}

impl EgressBaseline {
    /// Rules nobody's sidecar keeps: a stopped Instance's, or upstream's own install.
    /// They aren't this test's to remove.
    fn foreign(self) -> bool {
        self.rules && !self.other_sidecars
    }
}

impl TestInstance {
    fn new() -> Self {
        let web_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let turn = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self {
            _turn: turn,
            home: tempfile::tempdir().unwrap(),
            project: format!(
                "rkst-test-{}-{}",
                std::process::id(),
                NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed)
            ),
            web_port,
            egress_before: OnceCell::new(),
        }
    }

    fn folder(&self) -> PathBuf {
        self.home.path().join("rakazo-stack")
    }

    /// Runs `setup` with stdin closed, so there is no terminal, as on CI.
    fn setup(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        self.egress_baseline();
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
            .env("RAKAZO_STACK_NO_UPDATE_CHECK", "1")
            .env("RAKAZO_STACK_TEST_PROJECT", &self.project)
            .env("RAKAZO_STACK_TEST_WEB_PORT", self.web_port.to_string())
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .output()
            .expect("run rakazo-stack")
    }

    fn egress_baseline(&self) -> EgressBaseline {
        *self.egress_before.get_or_init(|| EgressBaseline {
            rules: !egress_rules_on_engine().is_empty(),
            other_sidecars: self.other_sidecars_running(),
        })
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

    /// Runs a Node script in `api`, where Rakazo's own voice calls start.
    fn node_in_api(&self, script: &str) -> serde_json::Value {
        let out = docker()
            .args(["compose", "exec", "-T", "api", "node", "-e", script])
            .current_dir(self.folder())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("{}", text(&out)))
    }

    /// Which of each service's name and addresses a container on a bridge of its own
    /// reaches over TCP: that is how Rakazo's supervisor attaches a bot computer.
    /// `networks` is how many networks the service is on.
    fn reachable_from_a_bot_bridge(&self, services: &[(&str, u16, usize)]) -> Vec<String> {
        let mut targets = Vec::new();
        for &(service, port, networks) in services {
            targets.push(format!("{service}:{port}"));
            let out = docker()
                .args([
                    "inspect",
                    "--format",
                    "{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}",
                    &format!("{}-{service}-1", self.project),
                ])
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", text(&out));
            let ips = String::from_utf8_lossy(&out.stdout).into_owned();
            assert_eq!(ips.split_whitespace().count(), networks, "{service}: {ips}");
            targets.extend(ips.split_whitespace().map(|ip| format!("{ip}:{port}")));
        }
        let network = format!("{}-bot-probe", self.project);
        let label = format!("com.docker.compose.project={}", self.project);
        let created = docker()
            .args(["network", "create", "--label", &label, &network])
            .output()
            .unwrap();
        assert!(created.status.success(), "{}", text(&created));
        let script = format!(
            "const net = require('node:net'); const reach = (t) => new Promise((done) => {{ const [host, port] = t.split(':'); const socket = net.connect({{ host, port: Number(port) }}); const end = (ok) => {{ socket.destroy(); done(ok); }}; socket.setTimeout(3000, () => end(false)); socket.on('connect', () => end(true)); socket.on('error', () => end(false)); }}); const reached = []; for (const t of {targets:?}) {{ if (await reach(t)) reached.push(t); }} console.log(JSON.stringify(reached));"
        );
        let probe = docker()
            .args(["run", "--rm", "--network", &network, "--label", &label])
            .arg(format!("{}-lemonade-relay", self.project))
            .args(["bun", "-e", &script])
            .output()
            .unwrap();
        let _ = docker().args(["network", "rm", &network]).output();
        assert!(probe.status.success(), "{}", text(&probe));
        serde_json::from_slice(&probe.stdout).unwrap()
    }

    /// `docker compose` for this Instance, with `stdin` on its standard input.
    fn compose_with_stdin(&self, args: &[&str], stdin: &str) -> Output {
        let mut child = docker()
            .args(["compose", "-p", &self.project])
            .args(args)
            .current_dir(self.folder())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn secret(&self, name: &str) -> String {
        fs::read_to_string(self.folder().join("secrets.env"))
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}=")).map(str::to_string))
            .unwrap_or_else(|| panic!("no {name} in secrets.env"))
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

impl TestInstance {
    /// For the weekly canary, which names these in its issues and starts its next Update
    /// from them: the Components `settings.json` records, written where `CANARY_VERSIONS`
    /// says.
    fn report_versions(&self) {
        let Some(path) = std::env::var_os("CANARY_VERSIONS") else {
            return;
        };
        let Ok(bytes) = fs::read(self.folder().join("settings.json")) else {
            return;
        };
        if let Ok(settings) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            let _ = fs::write(path, settings["components"].to_string());
        }
    }
}

impl Drop for TestInstance {
    fn drop(&mut self) {
        self.report_versions();
        self.remove_egress_rules();
        let mut down = docker();
        down.args([
            "compose",
            "-p",
            &self.project,
            "down",
            "-v",
            "--rmi",
            "local",
            "--remove-orphans",
        ]);
        if self.folder().exists() {
            down.current_dir(self.folder());
        }
        let _ = down.output();
        self.remove_launch_agent();
    }
}

impl TestInstance {
    /// On a Mac, setup loads the project's own lemond LaunchAgent. `compose down` leaves
    /// it, and `KeepAlive` would go on starting lemond from the deleted HOME.
    fn remove_launch_agent(&self) {
        if !cfg!(target_os = "macos") {
            return;
        }
        let uid = rustix::process::getuid().as_raw();
        let _ = Command::new("launchctl")
            .arg("bootout")
            .arg(format!(
                "gui/{uid}/com.github.kamui.{}.lemond",
                self.project
            ))
            .output();
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

impl TestInstance {
    fn compose(&self, args: &[&str]) -> Output {
        docker()
            .args(["compose", "-p", &self.project])
            .args(args)
            .current_dir(self.folder())
            .output()
            .unwrap()
    }

    fn sidecar_image(&self) -> String {
        format!("{}-computer-egress", self.project)
    }

    /// Runs the sidecar image once with the wrapper's arguments, in the engine's namespace.
    fn run_sidecar_image(&self, args: &[&str]) -> Output {
        docker()
            .args(["run", "--rm", "--pull", "never", "--network", "host"])
            .args(["--cap-add", "NET_ADMIN", &self.sidecar_image()])
            .args(args)
            .output()
            .unwrap()
    }

    fn other_sidecars_running(&self) -> bool {
        let out = docker()
            .args(["ps", "--filter"])
            .arg(format!(
                "label=com.docker.compose.service={COMPUTER_EGRESS}"
            ))
            .args(["--format", "{{.Label \"com.docker.compose.project\"}}"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|project| project.trim() != self.project)
    }

    /// The rules match every `rakazo-c+` bridge on the engine, so another Instance's are
    /// the same rules. They are removed only when this Instance's sidecar added them and,
    /// once it has stopped, no other sidecar keeps them. Stopping first means that of two
    /// test Instances ending together, the one that stops last still sees none.
    fn remove_egress_rules(&self) {
        let Some(before) = self.egress_before.get() else {
            return;
        };
        if before.foreign() || !self.folder().exists() {
            return;
        }
        let _ = self.compose(&["stop", COMPUTER_EGRESS]);
        if self.other_sidecars_running() {
            return;
        }
        let _ = self.run_sidecar_image(&["--remove"]);
    }

    fn service_log(&self, service: &str) -> String {
        text(&self.compose(&["logs", "--no-log-prefix", service]))
    }

    fn started_at(&self, service: &str) -> String {
        let id = self.compose(&["ps", "-q", service]);
        let id = String::from_utf8_lossy(&id.stdout).trim().to_string();
        let out = docker()
            .args(["inspect", "--format", "{{.State.StartedAt}}", &id])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// `POST /computers` on the supervisor, as Rakazo's API calls it, for a bot whose home
    /// setup makes the way upstream's data-init would.
    fn create_computer(&self, bot: &str) -> (u16, String) {
        let home = format!("/data/homes/{bot}");
        let mkdir = self.compose(&[
            "exec",
            "-T",
            "supervisor",
            "sh",
            "-c",
            &format!("mkdir -p {home} && chown 1000:1000 {home}"),
        ]);
        assert!(mkdir.status.success(), "{}", text(&mkdir));
        let out = self.compose(&[
            "exec",
            "-T",
            "supervisor",
            "node",
            "-e",
            CREATE_COMPUTER,
            bot,
            "rkst-space",
        ]);
        let reply = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let (status, body) = reply.split_once(' ').unwrap_or((&reply, ""));
        (status.parse().unwrap_or(0), body.to_string())
    }

    fn wait_for_log(&self, service: &str, needle: &str, count: usize) {
        for _ in 0..60 {
            if self.service_log(service).matches(needle).count() >= count {
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        panic!(
            "{service} never logged {needle:?}: {}",
            self.service_log(service)
        );
    }
}

const CREATE_COMPUTER: &str = r#"
const [botId, spaceId] = process.argv.slice(1);
fetch("http://127.0.0.1:7091/computers", {
  method: "POST",
  headers: {
    authorization: `Bearer ${process.env.SANDBOX_SUPERVISOR_TOKEN}`,
    "content-type": "application/json",
    "x-rakazo-bot-id": botId,
    "x-rakazo-space-id": spaceId,
  },
  body: JSON.stringify({ botId, spaceId, homePath: `/data/homes/${botId}` }),
}).then(async (r) => console.log(r.status, (await r.text()).replace(/\s+/g, " ")));
"#;

/// Bot computers are named by bot, not by Compose project, so `compose down` leaves them.
struct BotComputers(Vec<String>);

impl Drop for BotComputers {
    fn drop(&mut self) {
        for bot in &self.0 {
            let _ = docker()
                .args(["rm", "-f", "-v", &format!("rakazo-bot-{bot}")])
                .output();
            let networks = docker()
                .args(["network", "ls", "-q", "--filter"])
                .arg(format!("label=rakazo.botId={bot}"))
                .output()
                .unwrap();
            for network in String::from_utf8_lossy(&networks.stdout).split_whitespace() {
                let _ = docker().args(["network", "rm", network]).output();
            }
        }
    }
}

/// How a TCP connection from inside a container ended: 0 opened, 124 timed out (dropped),
/// anything else refused.
fn connect_from(container: &str, address: &str, port: u16) -> i32 {
    let out = docker()
        .args(["exec", container, "bash", "-c"])
        .arg(format!(
            "timeout 3 bash -c 'exec 3<>/dev/tcp/{address}/{port}' 2>/dev/null; echo $?"
        ))
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

fn addresses(container: &str, format: &str) -> Vec<String> {
    let out = docker()
        .args(["inspect", "--format", format, container])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .filter(|address| address.contains('.'))
        .map(str::to_string)
        .collect()
}

const COMPUTER_EGRESS: &str = "computer-egress";

/// Every `rakazo-c` rule in the engine's `DOCKER-USER`, on both iptables backends, read
/// from a throwaway container on the sidecar's own base image. Only that chain: Docker's
/// own chains name each running computer's `rakazo-c…` bridge too.
fn egress_rules_on_engine() -> Vec<String> {
    let base = include_str!("../computer-egress/Dockerfile")
        .lines()
        .find_map(|line| line.strip_prefix("FROM "))
        .unwrap()
        .trim();
    let name = format!(
        "rkst-test-{}-firewall-{}",
        std::process::id(),
        NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed)
    );
    let out = docker()
        .args(["run", "--rm", "--name", &name, "--network", "host"])
        .args(["--cap-add", "NET_ADMIN", base, "sh", "-c"])
        .arg(
            "apk add --quiet --no-cache iptables iptables-legacy >/dev/null && \
             for t in iptables ip6tables iptables-legacy ip6tables-legacy; do \
               $t -S DOCKER-USER 2>/dev/null || true; \
             done",
        )
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "couldn't read the engine's firewall: {}",
        text(&out)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|rule| rule.contains("rakazo-c"))
        .map(str::to_string)
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
const COMPOSIO_KEY: &str = "placeholder-composio-key";
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

const VOICE_CONSENT: &str = "The first time you use voice, Rakazo asks \"Share voice data with Self-hosted (http://lemonade-relay:13306)?\". Choose Allow: that server is this Instance's own Lemonade, so the audio stays on this machine.";

/// One speech and one transcription of it through the relay, as Rakazo makes them, and the
/// same request straight to Lemonade without its key.
const VOICE_ROUND_TRIP: &str = r#"(async () => {
  const relay = "http://lemonade-relay:13306/api/v1";
  const speech = await fetch(relay + "/audio/speech", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ model: "kokoro-v1", voice: "af_heart", input: "The quick brown fox jumps over the lazy dog.", lang_code: "a" }),
  });
  const audio = await speech.blob();
  const form = new FormData();
  form.set("model", "Whisper-Small");
  form.set("file", audio, "speech.mp3");
  const transcription = await fetch(relay + "/audio/transcriptions", { method: "POST", body: form });
  const text = transcription.ok ? (await transcription.json()).text : await transcription.text();
  const keyless = await fetch("http://lemonade:13305/api/v1/models");
  console.log(JSON.stringify({ speech: speech.status, bytes: audio.size, transcription: transcription.status, text, keyless: keyless.status }));
})();"#;

/// A signed-in Owner calling Rakazo's RPC, as the web app does.
struct OwnerSession {
    agent: ureq::Agent,
    origin: String,
    bearer: String,
}

impl OwnerSession {
    fn sign_in(origin: &str) -> Self {
        let (status, reply) = post_json(
            &format!("{origin}/api/auth/sign-in/email"),
            &serde_json::json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }),
        );
        assert_eq!(status, 200, "{reply}");
        let token: serde_json::Value = serde_json::from_str(&reply).unwrap();
        Self {
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into(),
            origin: origin.to_string(),
            bearer: format!("Bearer {}", token["token"].as_str().unwrap()),
        }
    }

    fn post(&self, path: &str, body: &serde_json::Value) -> (u16, String) {
        self.try_post(path, body).unwrap()
    }

    fn sign_out(self) {
        let (status, reply) = self.post("/api/auth/sign-out", &serde_json::json!({}));
        assert_eq!(status, 200, "{reply}");
    }

    fn try_post(&self, path: &str, body: &serde_json::Value) -> Result<(u16, String), ureq::Error> {
        let mut response = self
            .agent
            .post(format!("{}{path}", self.origin))
            .header("authorization", &self.bearer)
            .content_type("application/json")
            .send(body.to_string())?;
        let status = response.status().as_u16();
        Ok((status, response.body_mut().read_to_string()?))
    }

    /// A procedure that must succeed, with its input and output.
    fn rpc(&self, procedure: &str, input: serde_json::Value) -> serde_json::Value {
        let (status, reply) = self.post(
            &format!("/rpc/{procedure}"),
            &serde_json::json!({ "json": input }),
        );
        assert_eq!(status, 200, "{procedure}: {reply}");
        let mut reply: serde_json::Value = serde_json::from_str(&reply).unwrap();
        reply["json"].take()
    }

    /// Sends `message` to a bot, waits for its run to complete and returns what the bot
    /// answered.
    fn say(&self, bot: &str, message: &str) -> String {
        self.wait_until_idle(bot);
        let sent = self.rpc(
            "threads/send",
            serde_json::json!({ "botId": bot, "text": message }),
        );
        self.wait_until_idle(bot);
        let page = self.rpc("threads/messages", serde_json::json!({ "botId": bot }));
        // "recent" lists only the latest finished run of each chat.
        let finished = self.rpc("runs/list", serde_json::json!({ "filter": "recent" }));
        let run = finished["runs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|run| run["runId"] == sent["runId"]);
        assert_eq!(
            run.map(|run| &run["status"]),
            Some(&serde_json::json!("completed")),
            "{message:?}: {page}"
        );
        page["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["runId"] == sent["runId"] && m["role"] == "bot")
            .flat_map(|m| m["blocks"].as_array().unwrap())
            .filter(|block| block["kind"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `active` also lists runs paused on a question or approval card, which only the Owner
    /// ends, so one of those fails at once.
    fn wait_until_idle(&self, bot: &str) {
        for _ in 0..150 {
            let active = self.rpc("runs/list", serde_json::json!({ "filter": "active" }));
            let runs: Vec<_> = active["runs"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|run| run["botId"] == bot)
                .collect();
            if runs.is_empty() {
                return;
            }
            if let Some(waiting) = runs
                .iter()
                .find(|run| run["status"] == "waiting_input" || run["status"] == "waiting_takeover")
            {
                panic!("a bot's run is waiting on the Owner: {waiting}");
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        panic!("a bot's run didn't finish within 5 minutes");
    }
}

/// On a failed end-to-end check, prints what Rakazo and Hindsight did, with the key and any
/// piece of it taken out.
struct EndToEndDiagnosis<'a> {
    instance: &'a TestInstance,
    owner: &'a OwnerSession,
    bots: Vec<String>,
    key: &'a str,
}

impl Drop for EndToEndDiagnosis<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        let mut report = String::new();
        let runs = self.instance.compose(&[
            "exec",
            "-T",
            "postgres",
            "psql",
            "-X",
            "-U",
            "rakazo",
            "-d",
            "rakazo",
            "-c",
            r#"SELECT r.id, b.name AS bot, r.trigger, r.status, r."modelProvider", r."modelId", left(r.error, 400) AS error, r."createdAt", r."completedAt" FROM runs r JOIN bots b ON b.id = r."botId" ORDER BY r."createdAt""#,
        ]);
        report += &format!("--- runs\n{}\n", text(&runs));
        for bot in &self.bots {
            let (status, reply) = self
                .owner
                .try_post(
                    "/rpc/threads/messages",
                    &serde_json::json!({ "json": { "botId": bot } }),
                )
                .unwrap_or_else(|error| (0, error.to_string()));
            let page: serde_json::Value = serde_json::from_str(&reply).unwrap_or_default();
            let messages = page["json"]["messages"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let last = &messages[messages.len().saturating_sub(6)..];
            report += &format!("--- threads/messages {bot}: {status}\n");
            if status != 200 {
                report += &format!("{}\n", tail(&reply, 2000));
            }
            for message in last {
                report += &format!("{}\n", tail(&message.to_string(), 2000));
            }
        }
        let units = self.instance.compose(&[
            "exec",
            "-T",
            "hindsight-db",
            "psql",
            "-X",
            "-U",
            "hindsight",
            "-d",
            "hindsight",
            "-c",
            "SELECT bank_id, fact_type, count(*) FROM memory_units GROUP BY 1, 2 ORDER BY 1, 2",
        ]);
        report += &format!("--- memory_units\n{}\n", text(&units));
        for service in ["api", "worker", "supervisor", "hindsight"] {
            let logs = self
                .instance
                .compose(&["logs", "--no-log-prefix", "--tail", "80", service]);
            report += &format!("--- {service} logs\n{}\n", tail(&text(&logs), 12_000));
        }
        eprintln!("{}", redact(&report, self.key));
    }
}

/// The last `max` bytes of `text`, cut at a character boundary.
fn tail(text: &str, max: usize) -> &str {
    let mut start = text.len().saturating_sub(max);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// Takes out `key`, any eight characters of it in a row, and anything shaped like an
/// OpenRouter key, since provider errors can quote part of one.
fn redact(text: &str, key: &str) -> String {
    let text = text.replace(key, "<key>");
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("sk-or-") {
        out.push_str(&rest[..at]);
        out.push_str("<key>");
        rest = rest[at..]
            .trim_start_matches(|c: char| c.is_ascii_alphanumeric() || "-_*.".contains(c));
    }
    out.push_str(rest);
    let chars: Vec<char> = key.chars().collect();
    for piece in chars.windows(8) {
        out = out.replace(&piece.iter().collect::<String>(), "<key>");
    }
    out
}

#[test]
fn redact_takes_out_the_key_and_pieces_of_it() {
    let key = "sk-or-v1-0123456789abcdef0123456789abcdef";
    let text = format!(
        "{key} | Incorrect API key provided: sk-or-v1-0123****cdef | ending in 89abcdef. | ok"
    );
    assert_eq!(
        redact(&text, key),
        "<key> | Incorrect API key provided: <key> | ending in <key>. | ok"
    );
}

fn rpc_as_owner(origin: &str, path: &str, body: &serde_json::Value) -> (u16, String) {
    let session = OwnerSession::sign_in(origin);
    let reply = session.post(path, body);
    assert_eq!(
        session.post("/api/auth/sign-out", &serde_json::json!({})).0,
        200
    );
    reply
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
        [VOICE_CONSENT, NO_KEY_ITEM, DICTATION_ITEM],
        "{first_text}"
    );
    assert!(first_text.contains("Voice is ready"), "{first_text}");
    let services = instance.containers("{{.Label \"com.docker.compose.service\"}}");
    assert!(
        !services.iter().any(|s| s.starts_with("hindsight")),
        "without a key nothing of Hindsight's runs: {services:?}"
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
    assert_eq!(settings["model_provider"], "openrouter");
    for service in ["api", "worker"] {
        assert_eq!(instance.service_env(service, "COMPOSIO_API_KEY"), "");
    }
    assert_eq!(
        settings["filled_by_setup"],
        serde_json::json!(["owner", "voice"])
    );
    let lemonade = &settings["components"]["lemonade"];
    assert!(
        lemonade["version"].as_str().unwrap().starts_with('v'),
        "{settings}"
    );
    assert!(
        lemonade["digest"].as_str().unwrap().starts_with("sha256:"),
        "{settings}"
    );
    assert_eq!(
        instance.sql(&format!(
            "SELECT v.provider FROM user_voice_credentials v JOIN \"user\" u ON u.id = v.\"userId\" WHERE u.email = '{OWNER_EMAIL}'"
        )),
        "openai-compatible"
    );

    let voice = instance.node_in_api(VOICE_ROUND_TRIP);
    assert_eq!(voice["speech"], 200, "{voice}");
    assert!(voice["bytes"].as_u64().unwrap() > 1000, "{voice}");
    assert_eq!(voice["transcription"], 200, "{voice}");
    let heard = voice["text"].as_str().unwrap().to_lowercase();
    assert!(
        heard.contains("quick brown fox") && heard.contains("lazy dog"),
        "{voice}"
    );
    assert_eq!(
        voice["keyless"], 401,
        "Lemonade refuses a request without its key"
    );
    assert_eq!(
        instance
            .reachable_from_a_bot_bridge(&[("lemonade", 13305, 2), ("lemonade-relay", 13306, 2)]),
        Vec::<String>::new(),
        "a bot computer's bridge reaches neither Lemonade nor the relay"
    );

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
        "with the Owner and voice present, nothing asks for the password: {second_text}"
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

    let (status, reply) = rpc_as_owner(
        &origin,
        "/rpc/voice/disconnect",
        &serde_json::json!({ "json": { "provider": "openai-compatible" } }),
    );
    assert_eq!(status, 200, "{reply}");
    let voice_rows = "SELECT count(*) FROM user_voice_credentials";
    assert_eq!(instance.sql(voice_rows), "0");
    let removed = instance.setup(
        &["--no-questions"],
        &[("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD)],
    );
    let removed_text = text(&removed);
    assert!(removed.status.success(), "{removed_text}");
    assert_eq!(
        checklist(&removed),
        [NO_KEY_ITEM, DICTATION_ITEM],
        "{removed_text}"
    );
    assert_eq!(
        instance.sql(voice_rows),
        "0",
        "voice removed in Rakazo stays removed without questions"
    );
    assert_eq!(
        fs::read(folder.join("settings.json")).unwrap(),
        settings_before
    );

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

    // Saved keys with their services out of reach: a proxy nothing listens on, which the
    // calls to the Instance bypass. No real key is needed to see one reach Rakazo.
    let mut secrets = fs::read_to_string(folder.join("secrets.env")).unwrap();
    secrets.push_str(&format!(
        "OPENROUTER_API_KEY={MODEL_KEY}\nCOMPOSIO_API_KEY={COMPOSIO_KEY}\n"
    ));
    fs::write(folder.join("secrets.env"), secrets).unwrap();
    let third = instance.setup(
        &["--no-questions"],
        &[
            ("RAKAZO_STACK_MEMORY", "off"),
            ("ALL_PROXY", "http://127.0.0.1:9"),
            ("HTTPS_PROXY", "http://127.0.0.1:9"),
            ("NO_PROXY", "127.0.0.1"),
        ],
    );
    let third_text = text(&third);
    assert!(!third_text.contains(MODEL_KEY), "setup printed the key");
    assert!(!third_text.contains(COMPOSIO_KEY), "setup printed the key");
    assert!(third.status.success(), "{third_text}");
    assert!(
        third_text.contains("couldn't check the saved OpenRouter key"),
        "{third_text}"
    );
    assert!(
        third_text.contains("couldn't check the saved Composio key"),
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
        assert!(
            instance.service_env(service, "COMPOSIO_API_KEY") == COMPOSIO_KEY,
            "{service} doesn't have the Composio key"
        );
    }

    for secret in secret_values(&folder) {
        for output in [
            &first_text,
            &second_text,
            &removed_text,
            &switched_text,
            &third_text,
        ] {
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
    // OpenRouter and Composio themselves refuse these.
    const REJECTED_KEY: &str = "sk-or-v1-placeholder";
    const REJECTED_COMPOSIO_KEY: &str = "ak_placeholder";
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
    let cases: [(&[&str], Env, &str); 12] = [
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
        (
            &["--no-questions"],
            &[
                ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
                ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
                ("RAKAZO_STACK_COMPOSIO_API_KEY", REJECTED_COMPOSIO_KEY),
            ],
            "Composio rejected the key (HTTP 401)",
        ),
    ];
    for (args, env, expected) in cases {
        let out = instance.setup(args, env);
        let out_text = text(&out);
        assert!(!out.status.success(), "{out_text}");
        assert!(out_text.contains(expected), "{expected}: {out_text}");
        assert!(!out_text.contains("fifteen-chars!!"), "{out_text}");
        for key in [REJECTED_KEY, REJECTED_COMPOSIO_KEY] {
            assert!(!out_text.contains(key), "setup printed the key");
        }
        assert!(!out_text.contains("PLACEHOLDER"), "{out_text}");
        let left: Vec<_> = fs::read_dir(instance.folder())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, [".lock"], "only the lock is there");
        assert!(instance.containers("{{.Names}}").is_empty());
    }
}

#[test]
#[ignore = "needs Docker and pulls Rakazo's images; CI and the dev machine run it with --include-ignored"]
fn bot_computers_get_restricted_egress_and_fail_closed_without_the_sidecar() {
    let instance = TestInstance::new();
    let folder = instance.folder();
    let bots = BotComputers(vec![
        format!("{}-bot-1", instance.project),
        format!("{}-bot-2", instance.project),
    ]);

    let fake_bin = tempfile::tempdir().unwrap();
    let sudo_ran = fake_bin.path().join("sudo-ran");
    let sudo = fake_bin.path().join("sudo");
    fs::write(
        &sudo,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", sudo_ran.display()),
    )
    .unwrap();
    fs::set_permissions(&sudo, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        fake_bin.path().display(),
        std::env::var("PATH").unwrap()
    );
    assert_eq!(
        instance.egress_baseline(),
        EgressBaseline {
            rules: false,
            other_sidecars: false
        },
        "this test removes the engine's rakazo-c rules part-way, so it needs an engine \
         where no other Instance has them"
    );
    let out = instance.setup(
        &["--no-questions"],
        &[
            ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
            ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
            ("PATH", &path),
        ],
    );
    let out_text = text(&out);
    assert!(out.status.success(), "{out_text}");
    assert!(!sudo_ran.exists(), "setup ran sudo");
    assert!(
        !checklist(&out).iter().any(|item| item.contains("egress")),
        "{out_text}"
    );
    assert!(
        instance
            .service_log("supervisor")
            .contains("restricted computer egress verified")
    );
    let env = fs::read_to_string(folder.join(".env")).unwrap();
    assert!(
        env.lines()
            .any(|l| l == "SANDBOX_COMPUTER_EGRESS=restricted")
    );

    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.join("settings.json")).unwrap()).unwrap();
    let tag = settings["components"]["rakazo"]["version"]
        .as_str()
        .unwrap();
    let upstream = ureq::Agent::new_with_defaults()
        .get(format!(
            "https://raw.githubusercontent.com/elie222/rakazo/{tag}/infra/compose/restrict-computer-egress.sh"
        ))
        .call()
        .unwrap()
        .into_body()
        .read_to_vec()
        .unwrap();
    let in_image = docker()
        .args(["run", "--rm", "--pull", "never", "--entrypoint", "cat"])
        .args([
            &instance.sidecar_image(),
            "/usr/local/lib/rakazo/restrict-computer-egress.sh",
        ])
        .output()
        .unwrap();
    assert_eq!(
        in_image.stdout, upstream,
        "the script in the image is Rakazo's, byte for byte"
    );
    let printed = instance.run_sidecar_image(&["--print"]);
    let printed = String::from_utf8_lossy(&printed.stdout).into_owned();
    assert!(!printed.is_empty());
    for rule in printed.lines() {
        assert!(
            rule.contains(" -i rakazo-c+ "),
            "{rule} matches more than computer bridges"
        );
    }

    let (status, reply) = instance.create_computer(&bots.0[0]);
    assert_eq!(status, 200, "{reply}");
    let computer = format!("rakazo-bot-{}", bots.0[0]);
    assert_eq!(
        connect_from(&computer, "1.1.1.1", 443),
        0,
        "the internet is reachable"
    );
    let gateways = addresses(
        &computer,
        "{{range .NetworkSettings.Networks}}{{.Gateway}} {{end}}",
    );
    let app_gateway = addresses(
        &format!("{}-api-1", instance.project),
        "{{range .NetworkSettings.Networks}}{{.Gateway}} {{end}}",
    );
    for (what, address, port) in gateways
        .iter()
        .flat_map(|g| [("the host", g.as_str(), 22), ("the host", g.as_str(), 1)])
        .chain(
            app_gateway
                .iter()
                .map(|g| ("a private-range address", g.as_str(), 1)),
        )
        .chain([("the metadata address", "169.254.169.254", 80)])
    {
        assert_eq!(
            connect_from(&computer, address, port),
            124,
            "{what} {address}:{port} is reachable"
        );
    }
    for (service, port) in [("api", 3100), ("postgres", 5432)] {
        let container = format!("{}-{service}-1", instance.project);
        let ips = addresses(
            &container,
            "{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}",
        );
        assert!(!ips.is_empty());
        for ip in ips {
            assert_ne!(
                connect_from(&computer, &ip, port),
                0,
                "{service} at {ip} is reachable"
            );
        }
    }

    assert!(
        instance
            .compose(&["stop", COMPUTER_EGRESS])
            .status
            .success()
    );
    let removed = instance.compose(&["run", "--rm", COMPUTER_EGRESS, "--remove"]);
    assert!(removed.status.success(), "{}", text(&removed));
    assert!(egress_rules_on_engine().is_empty());
    assert!(
        instance
            .compose(&["restart", "supervisor"])
            .status
            .success()
    );
    instance.wait_for_log("supervisor", "computer egress probe failed", 1);
    let (status, reply) = instance.create_computer(&bots.0[1]);
    assert_eq!(
        status, 500,
        "without the rules a computer is refused: {reply}"
    );
    assert!(reply.contains("restrict-computer-egress.sh"), "{reply}");

    let supervisor_started = instance.started_at("supervisor");
    let applied = instance
        .service_log(COMPUTER_EGRESS)
        .matches("computer egress rules applied")
        .count();
    assert!(
        instance
            .compose(&["start", COMPUTER_EGRESS])
            .status
            .success()
    );
    instance.wait_for_log(
        COMPUTER_EGRESS,
        "computer egress rules applied",
        applied + 1,
    );
    let (status, reply) = instance.create_computer(&bots.0[1]);
    assert_eq!(
        status, 200,
        "with the rules back a computer starts: {reply}"
    );
    assert_eq!(
        instance.started_at("supervisor"),
        supervisor_started,
        "without restarting the supervisor"
    );
}

/// Setup's own calls to the Model provider go to a proxy nothing listens on, so a saved
/// placeholder key is kept unchecked; GitHub and the registries stay reachable. Hindsight
/// 1.3.0 only warns when its own LLM check fails.
const UNREACHABLE: [(&str, &str); 3] = [
    ("ALL_PROXY", "http://127.0.0.1:9"),
    ("HTTPS_PROXY", "http://127.0.0.1:9"),
    (
        "NO_PROXY",
        "127.0.0.1,*github.com,*githubusercontent.com,ghcr.io,*docker.io",
    ),
];

const ANOTHER_SPACE_ITEM: &str = "To turn memory on in another Space, its owner opens Settings → Memory → Connect memory provider → Hindsight → Advanced → Self-hosted, with http://hindsight:8888 and the key from the HINDSIGHT_API_TENANT_API_KEY line in ~/rakazo-stack/secrets.env.";

/// Run inside `hindsight`: creates banks the way a new Rakazo bot's first write does, in
/// chunks mode, which needs no LLM. Prints one JSON line of each call's status.
const NEW_BANKS: &str = r#"
import json, sys, threading, urllib.error, urllib.request
KEY = sys.stdin.readline().strip()
def call(method, path, body):
    req = urllib.request.Request("http://localhost:8888" + path, method=method,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Authorization": "Bearer " + KEY})
    try:
        with urllib.request.urlopen(req, timeout=600) as r:
            return r.status
    except urllib.error.HTTPError as e:
        return "%d %s" % (e.code, e.read()[:300].decode())
results = {}
def bank(name):
    results[name] = [
        call("PATCH", "/v1/default/banks/%s/config" % name, {"updates": {"retain_extraction_mode": "chunks"}}),
        call("POST", "/v1/default/banks/%s/memories" % name,
             {"items": [{"content": "First memory for %s: likes green tea." % name, "document_id": name + "-1"}], "async": False}),
    ]
threads = [threading.Thread(target=bank, args=(name,)) for name in sys.argv[1:]]
for t in threads: t.start()
for t in threads: t.join()
print(json.dumps(results, sort_keys=True))
"#;

#[test]
#[ignore = "needs Docker and pulls Rakazo's and Hindsight's images; CI and the dev machine run it with --include-ignored"]
fn memory_runs_hindsight_connects_the_default_space_and_turns_off_and_on() {
    let instance = TestInstance::new();
    let folder = instance.folder();
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("secrets.env"),
        format!("OPENROUTER_API_KEY={MODEL_KEY}\n"),
    )
    .unwrap();

    let mut env = vec![
        ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
        ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
    ];
    env.extend(UNREACHABLE);
    let first = instance.setup(&["--no-questions"], &env);
    let first_text = text(&first);
    assert!(first.status.success(), "{first_text}");
    assert!(!first_text.contains(MODEL_KEY), "setup printed the key");
    let items = checklist(&first);
    assert!(
        items.contains(&ANOTHER_SPACE_ITEM.to_string()),
        "{first_text}"
    );
    assert_eq!(
        items.len(),
        4,
        "voice, another Space, ZDR, dictation: {first_text}"
    );

    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["memory"], "on");
    assert_eq!(
        settings["filled_by_setup"],
        serde_json::json!(["owner", "voice", "memory"])
    );
    let hindsight = &settings["components"]["hindsight"];
    assert!(
        hindsight["version"].as_str().unwrap() >= "1.3.0",
        "{settings}"
    );
    for image in ["hindsight", "hindsight-db"] {
        assert!(
            hindsight["images"][image]
                .as_str()
                .unwrap()
                .starts_with("sha256:"),
            "{settings}"
        );
    }

    let ps = instance.compose(&["ps", "--format", "{{.Service}} {{.Health}}"]);
    let mut health: Vec<String> = String::from_utf8_lossy(&ps.stdout)
        .lines()
        .filter(|l| l.starts_with("hindsight"))
        .map(str::to_string)
        .collect();
    health.sort();
    assert_eq!(
        health,
        ["hindsight healthy", "hindsight-db healthy"],
        "{first_text}"
    );

    // The default Space only, connected through Rakazo, with no memory toggle set.
    let connected = instance.sql(
        "SELECT c.provider || ' ' || s.\"isDefault\" || ' ' || c.settings::text FROM space_memory_configs c JOIN spaces s ON s.id = c.\"spaceId\"",
    );
    assert_eq!(
        connected,
        r#"hindsight true {"mode": "local", "baseUrl": "http://hindsight:8888"}"#
    );

    for (name, value) in [
        ("HINDSIGHT_API_LLM_PROVIDER", "openrouter"),
        ("HINDSIGHT_API_LLM_MODEL", "~z-ai/glm-flash-latest"),
        ("HINDSIGHT_API_LLM_REASONING_EFFORT", "low"),
        (
            "HINDSIGHT_API_LLM_EXTRA_BODY",
            r#"{"provider":{"zdr":true,"data_collection":"deny"}}"#,
        ),
        ("HINDSIGHT_API_RERANKER_MAX_CANDIDATES", "100"),
        ("HINDSIGHT_API_WORKER_ID", "hindsight"),
        ("HINDSIGHT_API_MCP_ENABLED", "false"),
    ] {
        assert_eq!(instance.service_env("hindsight", name), value, "{name}");
    }
    assert!(instance.service_env("hindsight", "HINDSIGHT_API_LLM_API_KEY") == MODEL_KEY);
    let hindsight_env = instance.compose(&["exec", "-T", "hindsight", "env"]);
    let per_operation: Vec<String> = String::from_utf8_lossy(&hindsight_env.stdout)
        .lines()
        .filter(|l| {
            ["RETAIN", "REFLECT", "CONSOLIDATION", "MENTAL_MODEL_REFRESH"]
                .iter()
                .any(|op| l.starts_with(&format!("HINDSIGHT_API_{op}_LLM_")))
        })
        .map(|l| l.split('=').next().unwrap().to_string())
        .collect();
    assert!(per_operation.is_empty(), "{per_operation:?}");
    for service in ["api", "worker"] {
        assert_eq!(
            instance.service_env(service, "HINDSIGHT_API_LLM_API_KEY"),
            "",
            "{service} sees Hindsight's key"
        );
    }

    // hindsight-db is reachable only from hindsight, and neither is published.
    let networks = |service: &str| -> Vec<String> {
        let id = instance.compose(&["ps", "-q", service]);
        let id = String::from_utf8_lossy(&id.stdout).trim().to_string();
        let out = docker()
            .args([
                "inspect",
                "--format",
                "{{range $name, $_ := .NetworkSettings.Networks}}{{$name}} {{end}}",
                &id,
            ])
            .output()
            .unwrap();
        let mut names: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .map(|n| {
                n.trim_start_matches(&format!("{}_", instance.project))
                    .to_string()
            })
            .collect();
        names.sort();
        names
    };
    assert_eq!(networks("hindsight-db"), ["hindsight-data"]);
    assert_eq!(networks("hindsight"), ["egress", "hindsight-data", "stack"]);
    let published: Vec<String> = instance
        .containers("{{.Label \"com.docker.compose.service\"}} {{.Ports}}")
        .into_iter()
        .filter(|line| line.contains("->"))
        .collect();
    assert_eq!(published.len(), 1, "only web: {published:?}");
    let reach = r#"
const [host, port] = process.argv.slice(1);
const socket = require("node:net").connect({ host, port: Number(port), timeout: 3000 });
socket.on("connect", () => { console.log("reached"); process.exit(0); });
socket.on("error", () => { console.log("unreachable"); process.exit(0); });
socket.on("timeout", () => { console.log("unreachable"); process.exit(0); });
"#;
    for (host, port, expected) in [
        ("hindsight", "8888", "reached"),
        ("hindsight-db", "5432", "unreachable"),
    ] {
        let out = instance.compose(&["exec", "-T", "api", "node", "-e", reach, host, port]);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            expected,
            "api to {host}: {}",
            text(&out)
        );
    }
    assert_eq!(
        instance.reachable_from_a_bot_bridge(&[("hindsight", 8888, 3), ("hindsight-db", 5432, 1)]),
        Vec::<String>::new(),
        "a bot computer's bridge reaches neither Hindsight nor its database"
    );
    // The shm_size case (#48): new banks once memory_units passes 8 MB, where Postgres
    // builds their indexes in parallel. Seeded without an LLM, then grown by copying rows.
    let tenant_key = instance.secret("HINDSIGHT_API_TENANT_API_KEY");
    let new_banks = |names: &[&str]| -> String {
        let mut args = vec!["exec", "-T", "hindsight", "python", "-I", "-c", NEW_BANKS];
        args.extend(names);
        let out = instance.compose_with_stdin(&args, &format!("{tenant_key}\n"));
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert_eq!(new_banks(&["seed"]), r#"{"seed": [200, 200]}"#);
    let db_sql = |query: &str| -> String {
        let out = instance.compose(&[
            "exec",
            "-T",
            "hindsight-db",
            "psql",
            "-X",
            "-tA",
            "-v",
            "ON_ERROR_STOP=1",
            "-U",
            "hindsight",
            "-d",
            "hindsight",
            "-c",
            query,
        ]);
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let columns = db_sql(
        "SELECT string_agg(quote_ident(column_name), ',' ORDER BY ordinal_position) FROM information_schema.columns WHERE table_name = 'memory_units' AND column_name <> 'id'",
    );
    db_sql(&format!(
        "INSERT INTO memory_units (id,{columns}) SELECT gen_random_uuid(),{columns} FROM memory_units, generate_series(1, 20000)"
    ));
    let size: u64 = db_sql("SELECT pg_relation_size('memory_units')")
        .parse()
        .unwrap();
    assert!(size > 8 * 1024 * 1024, "memory_units is only {size} bytes");
    assert_eq!(
        new_banks(&["second-a", "second-b"]),
        r#"{"second-a": [200, 200], "second-b": [200, 200]}"#,
        "a second bank needs shm_size on hindsight-db"
    );
    let shm = instance.compose(&["exec", "-T", "hindsight-db", "df", "-k", "/dev/shm"]);
    assert!(
        String::from_utf8_lossy(&shm.stdout).contains(" 1048576 "),
        "{}",
        text(&shm)
    );
    let units = db_sql("SELECT count(*) FROM memory_units");

    // Off: both containers stop, the volume stays, and the still-connected Space is listed.
    let mut off_env = vec![("RAKAZO_STACK_MEMORY", "off")];
    off_env.extend(UNREACHABLE);
    let off = instance.setup(&["--no-questions"], &off_env);
    let off_text = text(&off);
    assert!(off.status.success(), "{off_text}");
    let services = instance.containers("{{.Label \"com.docker.compose.service\"}}");
    assert!(
        !services.iter().any(|s| s.starts_with("hindsight")),
        "{services:?}"
    );
    let volumes = docker()
        .args([
            "volume",
            "ls",
            "-q",
            "--filter",
            &format!("label=com.docker.compose.project={}", instance.project),
        ])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&volumes.stdout)
            .lines()
            .any(|v| v == format!("{}_hindsight-pgdata", instance.project)),
        "{}",
        text(&volumes)
    );
    let items = checklist(&off);
    assert!(
        items.iter().any(|item| item.starts_with(&format!(
            "Memory is off, but the Space \"Personal\", connected by {OWNER_EMAIL}, is still connected to Hindsight."
        ))),
        "{off_text}"
    );
    assert!(!items.contains(&ANOTHER_SPACE_ITEM.to_string()));
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["memory"], "off");
    assert_eq!(settings["components"]["hindsight"], *hindsight);

    // On again: the same version, and the old memories are back.
    let secrets_before = fs::read(folder.join("secrets.env")).unwrap();
    let mut on_env = vec![("RAKAZO_STACK_MEMORY", "on")];
    on_env.extend(UNREACHABLE);
    let on = instance.setup(&["--no-questions"], &on_env);
    let on_text = text(&on);
    assert!(on.status.success(), "{on_text}");
    assert_eq!(
        fs::read(folder.join("secrets.env")).unwrap(),
        secrets_before
    );
    assert_eq!(db_sql("SELECT count(*) FROM memory_units"), units);
    assert!(checklist(&on).contains(&ANOTHER_SPACE_ITEM.to_string()));

    for secret in secret_values(&folder) {
        for output in [&first_text, &off_text, &on_text] {
            assert!(!output.contains(&secret), "setup printed a secret");
        }
    }
}

const OTHERS_ITEM: &str = "key is the Owner's alone: everyone else connects their own provider in Rakazo, under Settings → Models.";
const OPENAI_KEY: &str = "zq-0000-0000-0000-0000";
const GEMINI_KEY: &str = "zq-1111-1111-1111-1111";

/// No real key is needed. With the provider reachable, Rakazo has it refuse the
/// placeholder. Then `api` is made unable to reach it, and Rakazo v0.16.0 saves a key it
/// can't judge, which runs the whole fill-in. Setup's own key check is cut off as in the
/// memory test, so each key is already in `secrets.env`.
#[test]
#[ignore = "needs Docker and pulls Rakazo's images; CI and the dev machine run it with --include-ignored"]
fn openai_and_gemini_connect_the_owners_chat_through_models_connect() {
    let instance = TestInstance::new();
    let folder = instance.folder();
    fs::create_dir_all(&folder).unwrap();
    let save_key = |name: &str, key: &str| {
        let path = folder.join("secrets.env");
        let mut text = fs::read_to_string(&path).unwrap_or_default();
        text.push_str(&format!("{name}={key}\n"));
        fs::write(path, text).unwrap();
    };
    let run = |provider: &str, password: bool| -> Vec<String> {
        let mut env = vec![
            ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
            ("RAKAZO_STACK_MODEL_PROVIDER", provider),
            ("RAKAZO_STACK_MEMORY", "off"),
        ];
        if password {
            env.push(("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD));
        }
        env.extend(UNREACHABLE);
        let out = instance.setup(&["--no-questions"], &env);
        let output = text(&out);
        assert!(out.status.success(), "{output}");
        for key in [OPENAI_KEY, GEMINI_KEY, MODEL_KEY] {
            assert!(!output.contains(key), "setup printed a key");
        }
        checklist(&out)
    };
    let settings = || -> serde_json::Value {
        serde_json::from_slice(&fs::read(folder.join("settings.json")).unwrap()).unwrap()
    };
    let connections = || {
        instance
            .sql("SELECT provider || ' ' || label FROM user_model_credentials ORDER BY provider")
    };
    let main_model = || {
        instance.sql(
            "SELECT r.provider || ' ' || r.\"modelId\" || ' ' || c.label FROM user_model_role_choices r JOIN user_model_credentials c ON c.id = r.\"credentialId\" WHERE r.role = 'main' AND r.position = 0",
        )
    };
    let failed = |items: &[String]| items.iter().any(|i| i.contains("wasn't connected"));
    let switched = |items: &[String]| -> Vec<String> {
        items
            .iter()
            .filter(|i| i.starts_with("You switched away from"))
            .cloned()
            .collect()
    };

    // The provider refuses the key: an item with the UI steps, and nothing saved.
    save_key("OPENAI_API_KEY", OPENAI_KEY);
    let items = run("openai", true);
    assert!(
        items.iter().any(
            |item| item.starts_with("OpenAI chat wasn't connected for the Owner: ")
                && item.contains("(HTTP 400)")
                && item.contains("choose the model gpt-6-luna")
        ),
        "{items:?}"
    );
    assert!(items.iter().any(|i| i.ends_with(OTHERS_ITEM)), "{items:?}");
    assert_eq!(connections(), "");
    assert_eq!(
        settings()["filled_by_setup"],
        serde_json::json!(["owner", "voice"])
    );

    let blocked = instance.compose(&[
        "exec",
        "-T",
        "-u",
        "0",
        "api",
        "sh",
        "-c",
        "printf '%s api.openai.com generativelanguage.googleapis.com\\n' 127.0.0.1 ::1 >> /etc/hosts",
    ]);
    assert!(blocked.status.success(), "{}", text(&blocked));

    let items = run("openai", true);
    assert!(!failed(&items), "{items:?}");
    assert_eq!(connections(), "openai OpenAI (rakazo-stack)");
    assert_eq!(main_model(), "openai gpt-6-luna OpenAI (rakazo-stack)");
    assert_eq!(
        settings()["filled_by_setup"],
        serde_json::json!(["owner", "voice", "model_connection"])
    );

    // Connected: left alone, without the password.
    let saved =
        "SELECT id || ' ' || \"secretId\" FROM user_model_credentials WHERE provider = 'openai'";
    let before = instance.sql(saved);
    let items = run("openai", false);
    assert!(!failed(&items), "{items:?}");
    assert_eq!(instance.sql(saved), before);

    // A key still due, as a pasted new one leaves it: the same connection takes it.
    let mut due = settings();
    due["model_connection_due"] = "new_key".into();
    fs::write(folder.join("settings.json"), due.to_string()).unwrap();
    let items = run("openai", true);
    assert!(!failed(&items), "{items:?}");
    let after = instance.sql(saved);
    let id = |row: &str| row.split(' ').next().unwrap().to_string();
    assert_eq!(id(&after), id(&before));
    assert_ne!(after, before, "the key was saved again");
    assert_eq!(connections(), "openai OpenAI (rakazo-stack)");
    assert!(settings()["model_connection_due"].is_null());

    save_key("GEMINI_API_KEY", GEMINI_KEY);
    let items = run("gemini", true);
    assert!(!failed(&items), "{items:?}");
    assert_eq!(
        connections(),
        "google Google (rakazo-stack)\nopenai OpenAI (rakazo-stack)"
    );
    assert_eq!(
        main_model(),
        "google gemini-3.5-flash-lite Google (rakazo-stack)"
    );
    assert_eq!(
        switched(&items),
        ["You switched away from OpenAI: remove the old connection in Settings → Models."]
    );

    save_key("OPENROUTER_API_KEY", MODEL_KEY);
    let items = run("openrouter", false);
    assert_eq!(
        switched(&items),
        ["OpenAI", "Gemini"].map(|name| format!(
            "You switched away from {name}: remove the old connection in Settings → Models, because until then it stays the Owner's default."
        ))
    );
    assert!(!items.iter().any(|i| i.ends_with(OTHERS_ITEM)), "{items:?}");

    // Removed in Rakazo: the items go, and the providers are forgotten.
    for provider in ["openai", "google"] {
        let (status, reply) = rpc_as_owner(
            &instance.origin(),
            "/rpc/models/disconnect",
            &serde_json::json!({ "json": { "provider": provider } }),
        );
        assert_eq!(status, 200, "{reply}");
    }
    let items = run("openrouter", false);
    assert_eq!(switched(&items), Vec::<String>::new());
    assert!(settings()["model_providers_left"].is_null());
}

const BACKUP_PASSPHRASE: &str = "placeholder-backup-passphrase";

impl TestInstance {
    /// Runs `backup` with stdin closed, as cron does.
    fn backup(&self, passphrase: Option<&str>) -> Output {
        self.backup_command(passphrase)
            .output()
            .expect("run rakazo-stack")
    }

    fn backup_command(&self, passphrase: Option<&str>) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rakazo-stack"));
        for (name, _) in std::env::vars_os() {
            if name.as_encoded_bytes().starts_with(b"RAKAZO_STACK_") {
                command.env_remove(name);
            }
        }
        if let Some(passphrase) = passphrase {
            command.env("RAKAZO_STACK_BACKUP_PASSPHRASE", passphrase);
        }
        command
            .arg("backup")
            .env("HOME", self.home.path())
            .env("RAKAZO_STACK_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::null());
        command
    }

    fn backups(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(self.home.path().join("rakazo-stack-backups"))
            .map(|dir| dir.map(|entry| entry.unwrap().path()).collect())
            .unwrap_or_default();
        files.sort();
        files
    }

    fn image_of(&self, service: &str) -> String {
        let out = docker()
            .args(["inspect", "--format", "{{.Image}}"])
            .arg(format!("{}-{service}-1", self.project))
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// The `age` CLI's own decryption of a Backup, run from Debian's package in a
    /// throwaway container of Rakazo's Postgres image. `age` reads a passphrase only
    /// from a terminal, so `script` gives it one.
    fn age_decrypt(&self, backup: &Path) -> Vec<u8> {
        let decrypt = r#"set -e
apt-get update -qq >/dev/null
apt-get install -qq -y --no-install-recommends age >/dev/null 2>&1
cat > /tmp/backup
printf '%s\n' "$PASSPHRASE" | script -qec 'age --decrypt --output /tmp/plain /tmp/backup' /dev/null >/dev/null
cat /tmp/plain"#;
        let mut command = docker();
        command
            .args(["run", "--rm", "-i", "--name"])
            .arg(format!("{}-age", self.project))
            .args(["-e", &format!("PASSPHRASE={BACKUP_PASSPHRASE}")])
            .args(["--entrypoint", "bash", &self.image_of("postgres")])
            .args(["-c", decrypt]);
        let out = with_stdin(command, &fs::read(backup).unwrap());
        assert!(out.status.success(), "{}", text(&out));
        out.stdout
    }
}

fn with_stdin(mut command: Command, input: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    out
}

/// Each file in a tar archive, by name.
fn tar_entries(archive: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = tar::Archive::new(archive);
    archive
        .entries()
        .unwrap()
        .map(|entry| {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}

fn part<'a>(entries: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    &entries
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no {name}"))
        .1
}

#[test]
#[ignore = "needs Docker and pulls Rakazo's and Hindsight's images; CI and the dev machine run it with --include-ignored"]
fn backup_writes_one_checked_file_and_the_instance_comes_back() {
    let instance = TestInstance::new();
    let folder = instance.folder();
    let bot = format!("{}-bot", instance.project);
    let _bots = BotComputers(vec![bot.clone()]);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("secrets.env"),
        format!("OPENROUTER_API_KEY={MODEL_KEY}\n"),
    )
    .unwrap();
    let mut env = vec![
        ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
        ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
    ];
    env.extend(UNREACHABLE);
    let first = instance.setup(&["--no-questions"], &env);
    assert!(first.status.success(), "{}", text(&first));

    // No terminal and no passphrase: nothing is stopped.
    let api_started = instance.started_at("api");
    let refused = instance.backup(None);
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("no Backup passphrase"),
        "{}",
        text(&refused)
    );
    assert_eq!(instance.started_at("api"), api_started);
    assert!(instance.backups().is_empty());

    // Some data: a bot computer whose home holds 20 MB, folders Rakazo rebuilds, and rows.
    let (status, body) = instance.create_computer(&bot);
    assert!((200..300).contains(&status), "{status} {body}");
    let seed = instance.compose(&[
        "exec",
        "-T",
        "supervisor",
        "sh",
        "-c",
        &format!(
            "head -c 20000000 /dev/urandom > /data/homes/{bot}/random.bin \
             && mkdir -p /data/homes/{bot}/staging /data/staging/run /data/mcp-packages/pkg \
             && echo kept > /data/homes/{bot}/staging/kept.txt \
             && echo rebuilt > /data/staging/run/file && echo rebuilt > /data/mcp-packages/pkg/file"
        ),
    ]);
    assert!(seed.status.success(), "{}", text(&seed));
    instance.sql("CREATE TABLE rkst_backup_marker AS SELECT generate_series(1, 10000) AS n");
    let computer = format!("rakazo-bot-{bot}");
    let running = |name: &str| {
        let out = docker()
            .args(["inspect", "--format", "{{.State.Running}}", name])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim() == "true"
    };
    assert!(running(&computer));

    let api_started = instance.started_at("api");
    let out = instance.backup(Some(BACKUP_PASSPHRASE));
    let out_text = text(&out);
    println!("{out_text}");
    assert!(out.status.success(), "{out_text}");
    assert!(!out_text.contains(BACKUP_PASSPHRASE));
    let backups = instance.backups();
    assert_eq!(backups.len(), 1, "{backups:?}");
    let backup = &backups[0];
    let name = backup.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("rakazo-stack-backup-") && name.ends_with("Z.tar.age"),
        "{name}"
    );
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(backup), 0o600);
    assert_eq!(mode(backup.parent().unwrap()), 0o700);
    assert!(!folder.join(".backup.env").exists());

    // Healthy again: the writers restarted, and so did the bot computer.
    assert_ne!(instance.started_at("api"), api_started);
    let ps = instance.compose(&["ps", "--format", "{{.Service}} {{.State}} {{.Health}}"]);
    let states = String::from_utf8_lossy(&ps.stdout).into_owned();
    for service in [
        "api",
        "worker",
        "supervisor",
        "postgres",
        "web",
        "hindsight-db",
    ] {
        assert!(
            states
                .lines()
                .any(|l| l.starts_with(&format!("{service} running")) && !l.contains("unhealthy")),
            "{service}: {states}"
        );
    }
    assert!(running(&computer));
    let (status, _) = post_json(
        &format!("{}/api/auth/sign-in/email", instance.origin()),
        &serde_json::json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }),
    );
    assert_eq!(status, 200);

    // The age CLI opens it, and it holds exactly the Backup's parts.
    let entries = tar_entries(&instance.age_decrypt(backup));
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "manifest.json",
            "settings.json",
            "secrets.env",
            "rakazo.dump",
            "hindsight.dump",
            "appdata.tar"
        ]
    );
    assert_eq!(
        part(&entries, "settings.json"),
        fs::read(folder.join("settings.json")).unwrap()
    );
    assert_eq!(
        part(&entries, "secrets.env"),
        fs::read(folder.join("secrets.env")).unwrap()
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(part(&entries, "manifest.json")).unwrap();
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.join("settings.json")).unwrap()).unwrap();
    assert_eq!(manifest["program_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest["components"], settings["components"]);

    let list = |service: &str, dump: &[u8]| {
        let mut command = docker();
        command
            .args(["compose", "-p", &instance.project, "exec", "-T", service])
            .args(["pg_restore", "--list"])
            .current_dir(&folder);
        with_stdin(command, dump)
    };
    let rakazo = list("postgres", part(&entries, "rakazo.dump"));
    assert!(rakazo.status.success(), "{}", text(&rakazo));
    assert!(text(&rakazo).contains("rkst_backup_marker"));
    let hindsight = list("hindsight-db", part(&entries, "hindsight.dump"));
    assert!(hindsight.status.success(), "{}", text(&hindsight));
    assert!(
        text(&hindsight).contains("memory_units"),
        "{}",
        text(&hindsight)
    );

    let appdata: Vec<String> = tar_entries(part(&entries, "appdata.tar"))
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    for kept in [
        format!("./homes/{bot}/random.bin"),
        format!("./homes/{bot}/staging/kept.txt"),
    ] {
        assert!(appdata.contains(&kept), "{kept}: {appdata:?}");
    }
    assert!(
        !appdata
            .iter()
            .any(|n| n.starts_with("./staging") || n.starts_with("./mcp-packages")),
        "{appdata:?}"
    );

    // A dump that fails leaves no file, and Rakazo starts again.
    let stopped = instance.compose(&["stop", "postgres"]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    let failed = instance.backup(Some(BACKUP_PASSPHRASE));
    assert!(!failed.status.success());
    assert!(
        text(&failed).contains("pg_dump in postgres failed"),
        "{}",
        text(&failed)
    );
    assert_eq!(instance.backups(), backups);
    assert!(running(&computer));
    let ps = instance.compose(&["ps", "--status", "running", "--services"]);
    let running_services = String::from_utf8_lossy(&ps.stdout).into_owned();
    for service in ["api", "worker", "supervisor", "postgres"] {
        assert!(
            running_services.lines().any(|l| l == service),
            "{service}: {running_services}"
        );
    }

    // Ctrl-C while Rakazo is stopped: no Backup, and Rakazo runs again.
    let mut child = instance
        .backup_command(Some(BACKUP_PASSPHRASE))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run rakazo-stack");
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut printed = String::new();
    loop {
        let mut line = String::new();
        assert_ne!(stdout.read_line(&mut line).unwrap(), 0, "{printed}");
        printed.push_str(&line);
        if line.starts_with("Stopping Rakazo") {
            break;
        }
    }
    let signalled = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(signalled.success());
    stdout.read_to_string(&mut printed).unwrap();
    let interrupted = child.wait_with_output().unwrap();
    let interrupted_text = format!("{printed}{}", String::from_utf8_lossy(&interrupted.stderr));
    println!("{interrupted_text}");
    assert!(!interrupted.status.success());
    assert!(
        interrupted_text.contains("interrupted, so no Backup was written"),
        "{interrupted_text}"
    );
    assert_eq!(instance.backups(), backups);
    assert!(running(&computer));
    let ps = instance.compose(&["ps", "--status", "running", "--services"]);
    let running_services = String::from_utf8_lossy(&ps.stdout).into_owned();
    for service in ["api", "worker", "supervisor"] {
        assert!(
            running_services.lines().any(|l| l == service),
            "{service}: {running_services}"
        );
    }

    // Memory off, with hindsight-pgdata kept: hindsight-db starts for the dump only.
    let mut off_env = vec![("RAKAZO_STACK_MEMORY", "off")];
    off_env.extend(UNREACHABLE);
    let off = instance.setup(&["--no-questions"], &off_env);
    assert!(off.status.success(), "{}", text(&off));
    let hindsight_containers = || {
        instance
            .containers("{{.Label \"com.docker.compose.service\"}}")
            .into_iter()
            .filter(|s| s.starts_with("hindsight"))
            .collect::<Vec<_>>()
    };
    assert!(hindsight_containers().is_empty());
    let out = instance.backup(Some(BACKUP_PASSPHRASE));
    let off_text = text(&out);
    println!("{off_text}");
    assert!(out.status.success(), "{off_text}");
    assert!(
        hindsight_containers().is_empty(),
        "{:?}",
        hindsight_containers()
    );
    assert!(!folder.join(".backup.env").exists());
    let backups = instance.backups();
    assert_eq!(backups.len(), 2, "{backups:?}");
    let entries = tar_entries(&instance.age_decrypt(&backups[1]));
    let db_digest = settings["components"]["hindsight"]["images"]["hindsight-db"]
        .as_str()
        .unwrap();
    let mut command = docker();
    command
        .args(["run", "--rm", "-i", "--network", "none", "--name"])
        .arg(format!("{}-list", instance.project))
        .args(["--entrypoint", "pg_restore"])
        .arg(format!("pgvector/pgvector:pg18@{db_digest}"))
        .arg("--list");
    let hindsight = with_stdin(command, part(&entries, "hindsight.dump"));
    assert!(hindsight.status.success(), "{}", text(&hindsight));
    assert!(text(&hindsight).contains("memory_units"));
}

impl TestInstance {
    /// `program` with this Instance's HOME, project and port, and none of the shell's
    /// own `RAKAZO_STACK_*` variables.
    fn restore_command(&self, program: &str, env: &[(&str, &str)]) -> Command {
        self.egress_baseline();
        let mut command = Command::new(program);
        for (name, _) in std::env::vars_os() {
            if name.as_encoded_bytes().starts_with(b"RAKAZO_STACK_") {
                command.env_remove(name);
            }
        }
        command
            .env("HOME", self.home.path())
            .env("RAKAZO_STACK_TEST_PROJECT", &self.project)
            .env("RAKAZO_STACK_TEST_WEB_PORT", self.web_port.to_string())
            .envs(env.iter().copied());
        command
    }

    /// Runs `restore` with stdin closed, so there is no terminal, as a script runs it.
    fn restore(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        self.restore_command(env!("CARGO_BIN_EXE_rakazo-stack"), env)
            .arg("restore")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .expect("run rakazo-stack")
    }

    /// Runs `restore` on a terminal that `script` gives it, with `typed` as its input.
    fn restore_typing(&self, args: &[&str], env: &[(&str, &str)], typed: &str) -> Output {
        let mut line = format!("'{}' restore", env!("CARGO_BIN_EXE_rakazo-stack"));
        for arg in args {
            line.push_str(&format!(" '{arg}'"));
        }
        let mut command = self.restore_command("script", env);
        command
            .args(["-qec", &line, "/dev/null"])
            .env("SHELL", "/bin/sh");
        with_stdin(command, typed.as_bytes())
    }

    fn volumes(&self) -> Vec<String> {
        let out = docker()
            .args(["volume", "ls", "-q", "--filter"])
            .arg(format!("label=com.docker.compose.project={}", self.project))
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn in_supervisor(&self, script: &str) -> String {
        let out = self.compose(&["exec", "-T", "supervisor", "sh", "-c", script]);
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A file in a volume, read in a throwaway container of Rakazo's Postgres image.
    fn read_from_volume(&self, volume: &str, path: &str) -> Output {
        docker()
            .args(["run", "--rm", "--network", "none", "-v"])
            .arg(format!("{volume}:/v:ro"))
            .args(["--entrypoint", "cat", &self.image_of("postgres")])
            .arg(format!("/v/{path}"))
            .output()
            .unwrap()
    }
}

/// Run inside `hindsight`: what a bank recalls for a query, which needs no LLM.
const RECALL: &str = r#"
import json, sys, urllib.request
KEY = sys.stdin.readline().strip()
bank, query = sys.argv[1:]
req = urllib.request.Request("http://localhost:8888/v1/default/banks/%s/memories/recall" % bank, method="POST",
    data=json.dumps({"query": query, "budget": "low"}).encode(),
    headers={"Content-Type": "application/json", "Authorization": "Bearer " + KEY})
with urllib.request.urlopen(req, timeout=600) as r:
    print(json.dumps([m.get("text", "") for m in json.load(r).get("results", [])]))
"#;

const OLD_MACHINE_OFF: &str = "RAKAZO_STACK_OLD_MACHINE_OFF";
const TAILSCALE_STATE: &str = "placeholder-tailscale-state";

/// A Backup, the Instance torn down, and the Backup restored into a fresh HOME and
/// project: the data, the secrets and the recorded versions come back, and setup's
/// hand-over needs no Owner password. This proves the `rakazo_app` role step against
/// Rakazo's images compose file. No `tailscale` container starts; the Backup holds a
/// `tailscale-state` volume with a placeholder file instead.
#[test]
#[ignore = "needs Docker and pulls Rakazo's and Hindsight's images; CI and the dev machine run it with --include-ignored"]
fn restore_brings_the_instance_back_on_its_recorded_versions() {
    let kept = tempfile::tempdir().unwrap();
    let backup = kept.path().join("rakazo-stack-backup.tar.age");
    let bot = "rkst-restore-bot";
    let (settings_before, secrets_before, home_sum) = {
        let instance = TestInstance::new();
        let folder = instance.folder();
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("secrets.env"),
            format!("OPENROUTER_API_KEY={MODEL_KEY}\n"),
        )
        .unwrap();
        let mut env = vec![
            ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
            ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
        ];
        env.extend(UNREACHABLE);
        let first = instance.setup(&["--no-questions"], &env);
        assert!(first.status.success(), "{}", text(&first));

        let home_sum = instance.in_supervisor(&format!(
            "mkdir -p /data/homes/{bot} && head -c 1000000 /dev/urandom > /data/homes/{bot}/random.bin \
             && chown -R 1000:1000 /data/homes/{bot} && sha256sum /data/homes/{bot}/random.bin"
        ));
        instance.sql("CREATE TABLE rkst_restore_marker AS SELECT generate_series(1, 1000) AS n");
        let tenant_key = instance.secret("HINDSIGHT_API_TENANT_API_KEY");
        let seeded = instance.compose_with_stdin(
            &[
                "exec",
                "-T",
                "hindsight",
                "python",
                "-I",
                "-c",
                NEW_BANKS,
                "restore-check",
            ],
            &format!("{tenant_key}\n"),
        );
        assert_eq!(
            String::from_utf8_lossy(&seeded.stdout).trim(),
            r#"{"restore-check": [200, 200]}"#,
            "{}",
            text(&seeded)
        );
        let state = format!("{}_tailscale-state", instance.project);
        let label = format!("com.docker.compose.project={}", instance.project);
        let created = docker()
            .args(["volume", "create", "--label", &label, &state])
            .output()
            .unwrap();
        assert!(created.status.success(), "{}", text(&created));
        let written = docker()
            .args(["run", "--rm", "--network", "none", "-v"])
            .arg(format!("{state}:/v"))
            .args(["--entrypoint", "sh", &instance.image_of("postgres")])
            .args([
                "-c",
                &format!("echo {TAILSCALE_STATE} > /v/tailscaled.state"),
            ])
            .output()
            .unwrap();
        assert!(written.status.success(), "{}", text(&written));

        // The Owner's voice as setup connects it on a Mac, with a voice the Owner picked.
        let session = OwnerSession::sign_in(&instance.origin());
        session.rpc(
            "voice/connect",
            serde_json::json!({
                "provider": "openai-compatible",
                "baseUrl": "http://lemonade-relay:13306/api/v1",
                "ttsModel": "kokoro-v1",
                "sttModel": MAC_WHISPER,
                "voiceId": PICKED_VOICE,
            }),
        );
        session.sign_out();
        assert_eq!(owner_voice(&instance.origin())["sttModel"], MAC_WHISPER);

        let out = instance.backup(Some(BACKUP_PASSPHRASE));
        assert!(out.status.success(), "{}", text(&out));
        fs::copy(&instance.backups()[0], &backup).unwrap();
        let _ = docker().args(["volume", "rm", &state]).output();
        (
            fs::read(folder.join("settings.json")).unwrap(),
            fs::read(folder.join("secrets.env")).unwrap(),
            home_sum,
        )
    };
    let settings_before: serde_json::Value = serde_json::from_slice(&settings_before).unwrap();
    let backup_bytes = fs::read(&backup).unwrap();
    let file = backup.to_str().unwrap();

    let instance = TestInstance::new();
    let folder = instance.folder();
    let mut env = vec![("RAKAZO_STACK_BACKUP_PASSPHRASE", BACKUP_PASSPHRASE)];
    env.extend(UNREACHABLE);

    // A wrong passphrase, a damaged file, and no answer to the Tailscale question all
    // stop before the machine changes.
    let wrong = instance.restore(
        &[file],
        &[("RAKAZO_STACK_BACKUP_PASSPHRASE", "not-the-passphrase")],
    );
    assert!(!wrong.status.success());
    assert!(
        text(&wrong).contains("the passphrase doesn't open it"),
        "{}",
        text(&wrong)
    );
    let damaged = kept.path().join("damaged.tar.age");
    let mut bytes = backup_bytes.clone();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 1;
    fs::write(&damaged, bytes).unwrap();
    let out = instance.restore(&[damaged.to_str().unwrap()], &env);
    assert!(!out.status.success(), "{}", text(&out));
    let unanswered = instance.restore(&[file], &env);
    assert!(!unanswered.status.success());
    assert!(
        text(&unanswered).contains(OLD_MACHINE_OFF),
        "{}",
        text(&unanswered)
    );
    assert!(
        !folder.exists(),
        "a refused restore left the Instance folder"
    );
    assert!(instance.volumes().is_empty(), "{:?}", instance.volumes());

    let mut off_env = env.clone();
    off_env.push((OLD_MACHINE_OFF, "yes"));
    let out = instance.restore(&[file], &off_env);
    let restored = text(&out);
    println!("{restored}");
    assert!(out.status.success(), "{restored}");
    assert!(!restored.contains(BACKUP_PASSPHRASE));
    for secret in secret_values(&folder) {
        assert!(!restored.contains(&secret), "restore printed a secret");
    }
    assert!(
        !checklist(&out)
            .iter()
            .any(|item| item.contains("RAKAZO_STACK_OWNER_PASSWORD")),
        "{restored}"
    );
    assert_eq!(fs::read(&backup).unwrap(), backup_bytes);

    // The recorded versions and digests, not the latest ones, and the same secrets.
    assert_eq!(
        fs::read(folder.join("secrets.env")).unwrap(),
        secrets_before
    );
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.join("settings.json")).unwrap()).unwrap();
    let components = &settings_before["components"];
    assert_eq!(settings["components"], *components);
    assert_eq!(
        settings["filled_by_setup"],
        settings_before["filled_by_setup"]
    );
    let rakazo = &components["rakazo"];
    assert_eq!(
        instance.service_env("web", "RAKAZO_IMAGE_TAG"),
        format!(
            "{}@{}",
            rakazo["version"].as_str().unwrap(),
            rakazo["images"]["app"].as_str().unwrap()
        )
    );
    let configured = |service: &str| {
        let out = docker()
            .args(["inspect", "--format", "{{.Config.Image}}"])
            .arg(format!("{}-{service}-1", instance.project))
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let hindsight = &components["hindsight"]["images"];
    assert!(configured("hindsight").ends_with(hindsight["hindsight"].as_str().unwrap()));
    assert!(configured("hindsight-db").ends_with(hindsight["hindsight-db"].as_str().unwrap()));

    // The Owner signs in with the old password; rows, homes and memory are back.
    let (status, _) = post_json(
        &format!("{}/api/auth/sign-in/email", instance.origin()),
        &serde_json::json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }),
    );
    assert_eq!(status, 200);
    assert_eq!(
        instance.sql("SELECT count(*) FROM rkst_restore_marker"),
        "1000"
    );
    assert_eq!(
        instance.sql("SELECT count(*) FROM space_memory_configs WHERE provider = 'hindsight'"),
        "1"
    );
    // The same OS as far as restore knows, so setup leaves the voice connection alone.
    let voice = owner_voice(&instance.origin());
    assert_eq!(
        (&voice["sttModel"], &voice["voiceId"]),
        (&MAC_WHISPER.into(), &PICKED_VOICE.into())
    );
    assert_eq!(
        instance.in_supervisor(&format!("sha256sum /data/homes/{bot}/random.bin")),
        home_sum
    );
    assert_eq!(
        instance.in_supervisor(&format!("stat -c %u:%g /data/homes/{bot}/random.bin")),
        "1000:1000"
    );
    let tenant_key = instance.secret("HINDSIGHT_API_TENANT_API_KEY");
    let recalled = instance.compose_with_stdin(
        &[
            "exec",
            "-T",
            "hindsight",
            "python",
            "-I",
            "-c",
            RECALL,
            "restore-check",
            "What does restore-check like to drink?",
        ],
        &format!("{tenant_key}\n"),
    );
    assert!(recalled.status.success(), "{}", text(&recalled));
    assert!(text(&recalled).contains("green tea"), "{}", text(&recalled));
    let state = format!("{}_tailscale-state", instance.project);
    let read = instance.read_from_volume(&state, "tailscaled.state");
    assert_eq!(
        String::from_utf8_lossy(&read.stdout).trim(),
        TAILSCALE_STATE
    );

    // An Instance here: without --replace, or with a wrong name, nothing changes.
    let api_started = instance.started_at("api");
    let refused = instance.restore(&[file], &off_env);
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("an Instance is already here"),
        "{}",
        text(&refused)
    );
    let mistyped = instance.restore_typing(&["--replace", file], &off_env, "rakazo-stack\n");
    assert!(!mistyped.status.success(), "{}", text(&mistyped));
    assert!(
        text(&mistyped).contains("the name didn't match"),
        "{}",
        text(&mistyped)
    );
    assert_eq!(instance.started_at("api"), api_started);
    assert_eq!(
        instance.sql("SELECT count(*) FROM rkst_restore_marker"),
        "1000"
    );

    // --replace with the name, while the old machine still runs: the device stays out.
    let mut on_env = env.clone();
    on_env.push((OLD_MACHINE_OFF, "no"));
    let replaced = instance.restore_typing(
        &["--replace", file],
        &on_env,
        &format!("{}\n", instance.project),
    );
    let replaced_text = text(&replaced);
    println!("{replaced_text}");
    assert!(replaced.status.success(), "{replaced_text}");
    assert_ne!(instance.started_at("api"), api_started);
    assert_eq!(
        instance.sql("SELECT count(*) FROM rkst_restore_marker"),
        "1000"
    );
    assert!(
        !instance.volumes().contains(&state),
        "{:?}",
        instance.volumes()
    );
    assert_eq!(
        fs::read(folder.join("secrets.env")).unwrap(),
        secrets_before
    );

    // A Backup from a Mac: without the Owner password the voice move waits for setup,
    // which reconnects the relay with this OS's Whisper model and keeps the voice.
    let mut from_mac = on_env.clone();
    from_mac.push(("RAKAZO_STACK_TEST_BACKUP_PLATFORM", "macos"));
    let replaced = instance.restore_typing(
        &["--replace", file],
        &from_mac,
        &format!("{}\n", instance.project),
    );
    let replaced_text = text(&replaced);
    println!("{replaced_text}");
    assert!(replaced.status.success(), "{replaced_text}");
    // `script` ends its lines with `\r\n`, which `checklist` doesn't split on.
    assert!(
        replaced_text.lines().any(|line| line.starts_with(&format!(
            "  - The Owner's voice may still transcribe with {MAC_WHISPER}"
        )) && line.contains("RAKAZO_STACK_OWNER_PASSWORD")),
        "{replaced_text}"
    );
    assert_eq!(instance.settings()["voice_model_due"], MAC_WHISPER);
    let mut env = vec![
        ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
        ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
    ];
    env.extend(UNREACHABLE);
    let rerun = instance.setup(&["--no-questions"], &env);
    assert!(rerun.status.success(), "{}", text(&rerun));
    assert!(
        !checklist(&rerun)
            .iter()
            .any(|item| item.contains("voice") || item.contains("Voice")),
        "{}",
        text(&rerun)
    );
    let voice = owner_voice(&instance.origin());
    assert_eq!(
        (
            &voice["sttModel"],
            &voice["voiceId"],
            &voice["endpoint"],
            &voice["isDefault"]
        ),
        (
            &"Whisper-Small".into(),
            &PICKED_VOICE.into(),
            &"http://lemonade-relay:13306/api/v1".into(),
            &true.into()
        )
    );
    assert_eq!(
        instance.sql("SELECT count(*) FROM user_voice_credentials"),
        "1"
    );
    assert_eq!(instance.settings().get("voice_model_due"), None);
}

/// macOS's Whisper model (spec §9), and a Kokoro voice other than the `af_heart` Rakazo
/// picks by itself.
const MAC_WHISPER: &str = "Whisper-Large-v3-Turbo";
const PICKED_VOICE: &str = "af_bella";

/// The Owner's self-hosted voice connection, as Rakazo's settings page lists it.
fn owner_voice(origin: &str) -> serde_json::Value {
    let session = OwnerSession::sign_in(origin);
    let mut listed = session.rpc("voice/credentials", serde_json::json!({}));
    session.sign_out();
    let connections = listed.as_array_mut().unwrap();
    let position = connections
        .iter()
        .position(|c| c["provider"] == "openai-compatible")
        .expect("a self-hosted voice connection");
    connections.swap_remove(position)
}

impl TestInstance {
    /// Runs `update` with stdin closed, so only `--yes` lets it go ahead.
    fn update(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rakazo-stack"));
        for (name, _) in std::env::vars_os() {
            if name.as_encoded_bytes().starts_with(b"RAKAZO_STACK_") {
                command.env_remove(name);
            }
        }
        command
            .arg("update")
            .args(args)
            .env("HOME", self.home.path())
            .env("RAKAZO_STACK_TEST_PROJECT", &self.project)
            .env("RAKAZO_STACK_TEST_WEB_PORT", self.web_port.to_string())
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .output()
            .expect("run rakazo-stack")
    }

    fn settings(&self) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.folder().join("settings.json")).unwrap()).unwrap()
    }

    fn hindsight_sql(&self, query: &str) -> String {
        let out = self.compose(&[
            "exec",
            "-T",
            "hindsight-db",
            "psql",
            "-X",
            "-tA",
            "-v",
            "ON_ERROR_STOP=1",
            "-U",
            "hindsight",
            "-d",
            "hindsight",
            "-c",
            query,
        ]);
        assert!(out.status.success(), "{}", text(&out));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}

/// The digest a registry's `repository:tag` points to, read without pulling it.
fn registry_digest(registry: &str, repository: &str, tag: &str) -> String {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let (token_url, manifest_url) = match registry {
        "ghcr.io" => (
            format!("https://ghcr.io/token?scope=repository:{repository}:pull"),
            format!("https://ghcr.io/v2/{repository}/manifests/{tag}"),
        ),
        _ => (
            format!(
                "https://auth.docker.io/token?service=registry.docker.io&scope=repository:{repository}:pull"
            ),
            format!("https://registry-1.docker.io/v2/{repository}/manifests/{tag}"),
        ),
    };
    let body = agent
        .get(&token_url)
        .call()
        .unwrap()
        .body_mut()
        .read_to_string()
        .unwrap();
    let token: serde_json::Value = serde_json::from_str(&body).unwrap();
    let response = agent
        .head(&manifest_url)
        .header(
            "Authorization",
            &format!("Bearer {}", token["token"].as_str().unwrap()),
        )
        .header(
            "Accept",
            "application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json",
        )
        .call()
        .unwrap();
    assert_eq!(response.status(), 200, "{manifest_url}");
    response
        .headers()
        .get("docker-content-digest")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

/// An image an Update replaced is gone, unless something else also tagged it, as the
/// dev machine's own Rakazo has.
fn gone_unless_tagged_elsewhere(id: &str) -> bool {
    let out = docker()
        .args(["image", "inspect", "--format", "{{json .RepoTags}}", id])
        .output()
        .unwrap();
    if !out.status.success() {
        return true;
    }
    let names: Vec<String> = serde_json::from_slice(&out.stdout).unwrap();
    names.iter().any(|name| !name.contains('@'))
}

const OLD_RAKAZO: &str = "v0.15.0";
const OLD_LEMONADE: &str = "v2026.47.1";

/// §19's "Update across Rakazo releases": an Instance installed on Rakazo v0.15.0 (below
/// the floor, so with the test floor) and an older Lemonade updates with the layer file,
/// keeping its Owner, behind one Backup; Holds keep and release Lemonade; a Lemonade-only
/// Update takes no Backup; a second Update finds nothing newer.
#[test]
#[ignore = "needs Docker and pulls two Rakazo releases; CI and the dev machine run it with --include-ignored"]
fn update_moves_rakazo_across_releases_behind_a_backup_and_honours_holds() {
    let instance = TestInstance::new();
    let folder = instance.folder();
    let origin = instance.origin();
    fs::create_dir_all(&folder).unwrap();
    let seeded = serde_json::json!({
        "schema": 1,
        "program_version": "0.1.0",
        "owner_email": OWNER_EMAIL,
        "model_provider": "none",
        "components": {
            "rakazo": {
                "version": OLD_RAKAZO,
                "images": {
                    "app": registry_digest("ghcr.io", "elie222/rakazo/app", OLD_RAKAZO),
                    "computer": registry_digest("ghcr.io", "elie222/rakazo/computer", OLD_RAKAZO),
                },
            },
            "lemonade": {
                "version": OLD_LEMONADE,
                "digest": registry_digest("ghcr.io", "lemonade-sdk/lemonade-server", OLD_LEMONADE),
            },
        },
    });
    fs::write(folder.join("settings.json"), seeded.to_string()).unwrap();
    let installed = instance.setup(
        &["--no-questions"],
        &[
            ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
            ("RAKAZO_STACK_TEST_RAKAZO_FLOOR", OLD_RAKAZO),
        ],
    );
    assert!(installed.status.success(), "{}", text(&installed));
    let old_api = instance.image_of("api");
    let old_lemonade = instance.image_of("lemonade");
    let sign_in = || {
        post_json(
            &format!("{origin}/api/auth/sign-in/email"),
            &serde_json::json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }),
        )
    };
    assert_eq!(sign_in().0, 200);
    let unchanged = fs::read(folder.join("settings.json")).unwrap();
    let passphrase = [("RAKAZO_STACK_BACKUP_PASSPHRASE", BACKUP_PASSPHRASE)];

    let below_floor = instance.update(&["--yes", "--hold", "rakazo=v0.15.0"], &passphrase);
    assert!(!below_floor.status.success());
    assert!(
        text(&below_floor).contains("below the v0.16.0 this rakazo-stack needs"),
        "{}",
        text(&below_floor)
    );
    let no_passphrase = instance.update(&["--yes"], &[]);
    assert!(!no_passphrase.status.success());
    assert!(
        text(&no_passphrase).contains("RAKAZO_STACK_BACKUP_PASSPHRASE, which isn't set"),
        "{}",
        text(&no_passphrase)
    );
    let unreachable = instance.update(
        &["--yes"],
        &[
            passphrase[0],
            ("ALL_PROXY", "http://127.0.0.1:9"),
            ("HTTPS_PROXY", "http://127.0.0.1:9"),
            ("NO_PROXY", "127.0.0.1"),
        ],
    );
    assert!(!unreachable.status.success(), "{}", text(&unreachable));
    // Step 2, the check for a newer program, is the first to need GitHub.
    assert!(
        text(&unreachable).contains("can't check for a newer rakazo-stack"),
        "{}",
        text(&unreachable)
    );
    assert_eq!(fs::read(folder.join("settings.json")).unwrap(), unchanged);
    assert!(instance.backups().is_empty());
    for left in [
        ".update.env".to_string(),
        "rakazo.v0.16.0.images.yml".to_string(),
    ] {
        assert!(!folder.join(&left).exists(), "{left}");
    }
    assert_eq!(instance.image_of("api"), old_api, "still the old version");

    let held = instance.update(&["--yes", "--hold", "lemonade=v2026.47.1"], &passphrase);
    let held_text = text(&held);
    assert!(held.status.success(), "{held_text}");
    let settings = instance.settings();
    let new_rakazo = settings["components"]["rakazo"]["version"]
        .as_str()
        .unwrap();
    assert_ne!(new_rakazo, OLD_RAKAZO, "{settings}");
    assert!(
        held_text.contains(&format!("Rakazo {OLD_RAKAZO} → {new_rakazo}")),
        "{held_text}"
    );
    assert!(
        held_text.contains(&format!(
            "Lemonade is held at {OLD_LEMONADE}; the latest is"
        )),
        "{held_text}"
    );
    assert!(
        held_text.contains(&format!(
            "https://github.com/elie222/rakazo/releases/tag/{new_rakazo}"
        )),
        "{held_text}"
    );
    assert!(
        !held_text.contains("Bot computers are off"),
        "the new egress script is verified: {held_text}"
    );
    assert_eq!(
        settings["holds"],
        serde_json::json!({ "lemonade": OLD_LEMONADE })
    );
    assert_eq!(settings["components"]["lemonade"]["version"], OLD_LEMONADE);
    assert_eq!(instance.backups().len(), 1, "{held_text}");
    assert_eq!(sign_in().0, 200, "the Owner is still there");
    assert_ne!(instance.image_of("api"), old_api);
    assert!(gone_unless_tagged_elsewhere(&old_api), "{held_text}");
    assert!(!folder.join(".update.env").exists());
    assert!(
        folder
            .join(format!("rakazo.{new_rakazo}.images.yml"))
            .exists()
    );
    assert_eq!(instance.image_of("lemonade"), old_lemonade);

    let lemonade_only = instance.update(&["--yes", "--unhold", "lemonade"], &[]);
    let lemonade_text = text(&lemonade_only);
    assert!(lemonade_only.status.success(), "{lemonade_text}");
    assert!(
        lemonade_text.contains(&format!("Lemonade {OLD_LEMONADE} → ")),
        "{lemonade_text}"
    );
    assert_eq!(instance.backups().len(), 1, "no Backup: {lemonade_text}");
    let settings = instance.settings();
    assert!(settings["holds"].is_null(), "{settings}");
    assert_ne!(settings["components"]["lemonade"]["version"], OLD_LEMONADE);
    assert!(
        gone_unless_tagged_elsewhere(&old_lemonade),
        "{lemonade_text}"
    );

    let updated = fs::read(folder.join("settings.json")).unwrap();
    let again = instance.update(&["--yes"], &[]);
    assert!(again.status.success(), "{}", text(&again));
    assert!(
        text(&again).contains("Everything is up to date."),
        "{}",
        text(&again)
    );
    assert_eq!(fs::read(folder.join("settings.json")).unwrap(), updated);
}

/// A Hindsight Update keeps memories, and `ALTER EXTENSION vector UPDATE` brings the
/// extension up to the new image's pgvector: `hindsight-db` starts on pgvector 0.8.1's pg18
/// build and moves to the current `pg18`.
#[test]
#[ignore = "needs Docker and pulls Rakazo's and Hindsight's images; CI and the dev machine run it with --include-ignored"]
fn a_hindsight_update_keeps_memories_and_updates_pgvector() {
    let instance = TestInstance::new();
    let folder = instance.folder();
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("secrets.env"),
        format!("OPENROUTER_API_KEY={MODEL_KEY}\n"),
    )
    .unwrap();
    let old_db = registry_digest("docker.io", "pgvector/pgvector", "0.8.1-pg18");
    let seeded = serde_json::json!({
        "schema": 1,
        "program_version": "0.1.0",
        "owner_email": OWNER_EMAIL,
        "model_provider": "openrouter",
        "memory": "on",
        "components": {
            "rakazo": {
                "version": "v0.16.0",
                "images": {
                    "app": registry_digest("ghcr.io", "elie222/rakazo/app", "v0.16.0"),
                    "computer": registry_digest("ghcr.io", "elie222/rakazo/computer", "v0.16.0"),
                },
            },
            "hindsight": {
                "version": "1.3.0",
                "images": {
                    "hindsight": registry_digest("ghcr.io", "vectorize-io/hindsight-api", "1.3.0"),
                    "hindsight-db": old_db,
                },
            },
        },
    });
    fs::write(folder.join("settings.json"), seeded.to_string()).unwrap();
    let mut env = vec![("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD)];
    env.extend(UNREACHABLE);
    let installed = instance.setup(&["--no-questions"], &env);
    assert!(installed.status.success(), "{}", text(&installed));
    let extension = "SELECT extversion FROM pg_extension WHERE extname = 'vector'";
    assert_eq!(instance.hindsight_sql(extension), "0.8.1");

    let tenant_key = instance.secret("HINDSIGHT_API_TENANT_API_KEY");
    let seeded_bank = instance.compose_with_stdin(
        &[
            "exec",
            "-T",
            "hindsight",
            "python",
            "-I",
            "-c",
            NEW_BANKS,
            "kept",
        ],
        &format!("{tenant_key}\n"),
    );
    assert_eq!(
        String::from_utf8_lossy(&seeded_bank.stdout).trim(),
        r#"{"kept": [200, 200]}"#,
        "{}",
        text(&seeded_bank)
    );
    let memories = "SELECT count(*) FROM memory_units";
    let before = instance.hindsight_sql(memories);
    assert_ne!(before, "0");
    let old_db_image = instance.image_of("hindsight-db");

    env.push(("RAKAZO_STACK_BACKUP_PASSPHRASE", BACKUP_PASSPHRASE));
    let updated = instance.update(&["--yes"], &env);
    let updated_text = text(&updated);
    assert!(updated.status.success(), "{updated_text}");
    assert!(updated_text.contains("Hindsight 1.3.0"), "{updated_text}");
    assert!(
        updated_text.contains("Updating pgvector's extension in Hindsight's database"),
        "{updated_text}"
    );
    assert_eq!(instance.backups().len(), 1, "{updated_text}");
    let settings = instance.settings();
    assert_ne!(
        settings["components"]["hindsight"]["images"]["hindsight-db"], old_db,
        "{settings}"
    );
    let version = instance.hindsight_sql(extension);
    assert_ne!(version, "0.8.1", "the extension moved with the image");
    assert_eq!(instance.hindsight_sql(memories), before, "memories kept");
    assert!(
        gone_unless_tagged_elsewhere(&old_db_image),
        "{updated_text}"
    );
    let ps = instance.compose(&["ps", "--format", "{{.Service}} {{.Health}}"]);
    assert!(
        String::from_utf8_lossy(&ps.stdout)
            .lines()
            .any(|line| line == "hindsight healthy"),
        "{}",
        text(&ps)
    );
}

/// The weekly canary's update part (spec §19): an Instance on the versions its last
/// passing run recorded, or on the program's floors when none is recorded, updates to the
/// newest releases and keeps working.
#[test]
#[ignore = "needs Docker and pulls Rakazo's and Hindsight's images; CI and the dev machine run it with --include-ignored"]
fn canary_update_starts_from_the_last_passing_versions() {
    let instance = TestInstance::new();
    let folder = instance.folder();
    let origin = instance.origin();
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("secrets.env"),
        format!("OPENROUTER_API_KEY={MODEL_KEY}\n"),
    )
    .unwrap();
    let from: serde_json::Value = match std::env::var_os("CANARY_LAST_PASSING") {
        Some(path) => serde_json::from_slice(&fs::read(path).unwrap()).unwrap(),
        None => floor_components(),
    };
    println!("Updating from {from}");
    let seeded = serde_json::json!({
        "schema": 1,
        "program_version": "0.1.0",
        "owner_email": OWNER_EMAIL,
        "model_provider": "openrouter",
        "memory": "on",
        "components": from,
    });
    fs::write(folder.join("settings.json"), seeded.to_string()).unwrap();
    let mut env = vec![("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD)];
    env.extend(UNREACHABLE);
    let mut install_env = env.clone();
    install_env.push((
        "RAKAZO_STACK_TEST_RAKAZO_FLOOR",
        from["rakazo"]["version"].as_str().unwrap(),
    ));
    let installed = instance.setup(&["--no-questions"], &install_env);
    assert!(installed.status.success(), "{}", text(&installed));
    let settings = instance.settings();
    for (component, record) in from.as_object().unwrap() {
        assert_eq!(&settings["components"][component], record, "{component}");
    }
    let tenant_key = instance.secret("HINDSIGHT_API_TENANT_API_KEY");
    let seeded_bank = instance.compose_with_stdin(
        &[
            "exec",
            "-T",
            "hindsight",
            "python",
            "-I",
            "-c",
            NEW_BANKS,
            "kept",
        ],
        &format!("{tenant_key}\n"),
    );
    assert_eq!(
        String::from_utf8_lossy(&seeded_bank.stdout).trim(),
        r#"{"kept": [200, 200]}"#,
        "{}",
        text(&seeded_bank)
    );
    let memories = "SELECT count(*) FROM memory_units";
    let before = instance.hindsight_sql(memories);
    assert_ne!(before, "0");

    env.push(("RAKAZO_STACK_BACKUP_PASSPHRASE", BACKUP_PASSPHRASE));
    let updated = instance.update(&["--yes"], &env);
    let updated_text = text(&updated);
    println!("{updated_text}");
    assert!(updated.status.success(), "{updated_text}");
    println!("Updated to {}", instance.settings()["components"]);
    assert!(
        !updated_text.contains("Bot computers are off"),
        "{updated_text}"
    );
    OwnerSession::sign_in(&origin);
    assert_eq!(instance.hindsight_sql(memories), before, "memories kept");
    let ps = instance.compose(&["ps", "--format", "{{.Service}} {{.Health}}"]);
    assert!(
        String::from_utf8_lossy(&ps.stdout)
            .lines()
            .any(|line| line == "hindsight healthy"),
        "{}",
        text(&ps)
    );
    let voice = instance.node_in_api(VOICE_ROUND_TRIP);
    assert_eq!(voice["speech"], 200, "{voice}");
    assert_eq!(voice["transcription"], 200, "{voice}");
}

/// Where the canary's Update starts before any run has passed: Rakazo's and Hindsight's
/// floors, with the latest Lemonade, which has no floor.
fn floor_components() -> serde_json::Value {
    serde_json::json!({
        "rakazo": {
            "version": "v0.16.0",
            "images": {
                "app": registry_digest("ghcr.io", "elie222/rakazo/app", "v0.16.0"),
                "computer": registry_digest("ghcr.io", "elie222/rakazo/computer", "v0.16.0"),
            },
        },
        "hindsight": {
            "version": "1.3.0",
            "images": {
                "hindsight": registry_digest("ghcr.io", "vectorize-io/hindsight-api", "1.3.0"),
                "hindsight-db": registry_digest("docker.io", "pgvector/pgvector", "pg18"),
            },
        },
    })
}

/// The only test that uses a real Model provider key.
const REAL_KEY_VAR: &str = "CANARY_OPENROUTER_API_KEY";

/// The weekly canary's end-to-end part (spec §19): what one bot is told in a few turns on
/// the default model, another bot in the same Space recalls. Both share the Space's team
/// bank, and the second never saw the first one's conversation.
#[test]
#[ignore = "needs Docker and a real OpenRouter key; only the weekly canary gives it one"]
fn canary_a_real_key_learns_and_recalls_through_rakazo() {
    let Some(key) = std::env::var(REAL_KEY_VAR)
        .ok()
        .filter(|key| !key.is_empty())
    else {
        println!("Skipped: {REAL_KEY_VAR} isn't set.");
        return;
    };
    let instance = TestInstance::new();
    let installed = instance.setup(
        &["--no-questions"],
        &[
            ("RAKAZO_STACK_OWNER_EMAIL", OWNER_EMAIL),
            ("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD),
            ("RAKAZO_STACK_OPENROUTER_API_KEY", &key),
        ],
    );
    let installed_text = text(&installed);
    assert!(!installed_text.contains(&key), "setup printed the key");
    assert!(installed.status.success(), "{installed_text}");

    let owner = OwnerSession::sign_in(&instance.origin());
    let [teller, asker] = ["Teller", "Asker"].map(|name| {
        let bot = owner.rpc("bots/create", serde_json::json!({ "name": name }));
        let id = bot["id"].as_str().unwrap().to_string();
        owner.rpc(
            "bots/update",
            serde_json::json!({ "botId": id, "memoryScope": "shared" }),
        );
        // A new bot's hidden intro run asks the Owner what it is for and waits on that card.
        // Stopping it, as the chat's Stop button does, leaves every turn the Owner's own.
        owner.rpc("threads/stop", serde_json::json!({ "botId": id }));
        id
    });
    let _diagnosis = EndToEndDiagnosis {
        instance: &instance,
        owner: &owner,
        bots: vec![teller.clone(), asker.clone()],
        key: &key,
    };
    for turn in [
        "My dog is a beagle called Quillon.",
        "Quillon goes to the vet for vaccinations every May.",
        "I walk Quillon by the river before breakfast.",
    ] {
        owner.say(&teller, turn);
    }
    // Automatic recall reads observations only, which Hindsight consolidates after it
    // retains a turn (#45).
    let learned = "SELECT count(*) FROM memory_units WHERE fact_type = 'observation' AND bank_id ~ '^rakazo-[0-9a-f]{64}$' AND text ILIKE '%quillon%'";
    let observed = (0..120).any(|_| {
        std::thread::sleep(std::time::Duration::from_secs(5));
        instance.hindsight_sql(learned) != "0"
    });
    assert!(
        observed,
        "the Space's team bank learned nothing about the dog within 10 minutes"
    );
    let reply = owner.say(&asker, "What is my dog's name?");
    assert!(
        reply.to_lowercase().contains("quillon"),
        "the second bot didn't recall the name: {reply}"
    );
}

impl TestInstance {
    /// Runs `update` on a terminal that `script` gives it. Each answer is typed only once
    /// its question has been printed, so no child process that shares the terminal can
    /// read it first.
    fn update_answering(
        &self,
        args: &[&str],
        env: &[(&str, &str)],
        answers: &[(&str, &str)],
    ) -> Output {
        use std::io::Read;
        use std::sync::Arc;

        let mut line = format!("'{}' update", env!("CARGO_BIN_EXE_rakazo-stack"));
        for arg in args {
            line.push_str(&format!(" '{arg}'"));
        }
        let mut command = self.restore_command("script", env);
        let mut child = command
            .args(["-qec", &line, "/dev/null"])
            .env("SHELL", "/bin/sh")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let printed = Arc::new(Mutex::new(Vec::new()));
        let mut stdout = child.stdout.take().unwrap();
        let reader = {
            let printed = Arc::clone(&printed);
            std::thread::spawn(move || {
                let mut chunk = [0; 4096];
                while let Ok(n @ 1..) = stdout.read(&mut chunk) {
                    printed.lock().unwrap().extend_from_slice(&chunk[..n]);
                }
            })
        };
        let mut stdin = child.stdin.take().unwrap();
        for (question, answer) in answers {
            let asked = || String::from_utf8_lossy(&printed.lock().unwrap()).contains(question);
            for _ in 0..1800 {
                if asked() || child.try_wait().unwrap().is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            assert!(
                asked(),
                "never asked {question:?}: {}",
                String::from_utf8_lossy(&printed.lock().unwrap())
            );
            stdin.write_all(answer.as_bytes()).unwrap();
        }
        let mut stderr = Vec::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_end(&mut stderr)
            .unwrap();
        let status = child.wait().unwrap();
        drop(stdin);
        reader.join().unwrap();
        let stdout = printed.lock().unwrap().clone();
        Output {
            status,
            stdout,
            stderr,
        }
    }

    fn postgres_major(&self) -> String {
        self.sql("SHOW server_version")
            .split('.')
            .next()
            .unwrap()
            .to_string()
    }
}

/// A test copy of Rakazo's compose file in which `from` must appear and becomes `to`.
fn edited(compose: &str, from: &str, to: &str) -> String {
    assert!(compose.contains(from), "{from}");
    compose.replace(from, to)
}

/// §19's "Postgres 16 → 18 move" and "Rollback path", on Rakazo's images compose layout.
/// Rakazo's file is never edited: each Update goes to a fake newer tag that runs the
/// latest release's images, with a test copy of the installed compose file on
/// `postgres:18`, and, for a failed Update, with a `migrate` that exits 3. A failed
/// Update is rolled back on a terminal, then the move goes through; under `--yes` a
/// failure prints the rollback command, and with `--no-backup` none is offered.
#[test]
#[ignore = "needs Docker and pulls Rakazo's images and Postgres 18; CI and the dev machine run it with --include-ignored"]
fn update_moves_postgres_16_to_18_and_rolls_a_failed_update_back() {
    const NEXT_VAR: &str = "RAKAZO_STACK_TEST_NEXT_RAKAZO";
    const RAKAZO: &str = "v0.16.0";
    let instance = TestInstance::new();
    let folder = instance.folder();
    let origin = instance.origin();
    fs::create_dir_all(&folder).unwrap();
    let seeded = serde_json::json!({
        "schema": 1,
        "program_version": "0.1.0",
        "owner_email": OWNER_EMAIL,
        "model_provider": "none",
        "components": {
            "rakazo": {
                "version": RAKAZO,
                "images": {
                    "app": registry_digest("ghcr.io", "elie222/rakazo/app", RAKAZO),
                    "computer": registry_digest("ghcr.io", "elie222/rakazo/computer", RAKAZO),
                },
            },
        },
    });
    fs::write(folder.join("settings.json"), seeded.to_string()).unwrap();
    let installed = instance.setup(
        &["--no-questions"],
        &[("RAKAZO_STACK_OWNER_PASSWORD", OWNER_PASSWORD)],
    );
    assert!(installed.status.success(), "{}", text(&installed));
    let sign_in = || {
        post_json(
            &format!("{origin}/api/auth/sign-in/email"),
            &serde_json::json!({ "email": OWNER_EMAIL, "password": OWNER_PASSWORD }),
        )
        .0
    };
    assert_eq!(sign_in(), 200);
    instance.sql("CREATE TABLE rkst_kept AS SELECT g AS n FROM generate_series(1, 10000) g");
    let kept = "SELECT count(*) || ' ' || sum(n) FROM rkst_kept";
    assert_eq!(instance.sql(kept), "10000 50005000");
    assert_eq!(instance.postgres_major(), "16");
    let old_postgres = instance.image_of("postgres");

    let upstream = fs::read_to_string(folder.join(format!("rakazo.{RAKAZO}.images.yml"))).unwrap();
    let start = upstream.find("${POSTGRES_IMAGE:-").unwrap() + "${POSTGRES_IMAGE:-".len();
    let end = start + upstream[start..].find('}').unwrap();
    let pg18 = format!(
        "postgres:18@{}",
        registry_digest("docker.io", "library/postgres", "18")
    );
    let on_18 = edited(
        &edited(&upstream, &upstream[start..end], &pg18),
        "pgdata:/var/lib/postgresql/data",
        "pgdata:/var/lib/postgresql",
    );
    let failing = edited(
        &on_18,
        r#"command: ["pnpm", "db:deploy"]"#,
        r#"command: ["sh", "-c", "exit 3"]"#,
    );
    let copies = instance.home.path().join("next");
    fs::create_dir(&copies).unwrap();
    fs::write(copies.join("on-18.yml"), &on_18).unwrap();
    fs::write(copies.join("failing.yml"), &failing).unwrap();
    let next = |tag: &str, copy: &str| format!("{tag}={}", copies.join(copy).display());
    let env = |next: &str| -> Vec<(&str, String)> {
        vec![
            ("RAKAZO_STACK_BACKUP_PASSPHRASE", BACKUP_PASSPHRASE.into()),
            (NEXT_VAR, next.to_string()),
        ]
    };
    let update = |args: &[&str], next: &str| {
        let env = env(next);
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        instance.update(args, &env)
    };
    let unchanged = fs::read(folder.join("settings.json")).unwrap();

    let no_backup = update(&["--yes", "--no-backup"], &next("v0.16.1", "on-18.yml"));
    let no_backup_text = text(&no_backup);
    assert!(!no_backup.status.success(), "{no_backup_text}");
    assert!(
        no_backup_text.contains("Rakazo's Postgres 16 → 18: a Backup is required")
            && no_backup_text.contains("--no-backup is refused when Postgres moves"),
        "{no_backup_text}"
    );
    assert_eq!(fs::read(folder.join("settings.json")).unwrap(), unchanged);
    assert!(instance.backups().is_empty());
    assert!(!folder.join("rakazo.v0.16.1.images.yml").exists());

    let failing_env = env(&next("v0.16.1", "failing.yml"));
    let failing_env: Vec<(&str, &str)> =
        failing_env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let rolled_back = instance.update_answering(
        &[],
        &failing_env,
        &[
            ("Update? [Y/n]", "\n"),
            (
                "Roll back to the previous versions from the Backup taken at",
                "y\n",
            ),
        ],
    );
    let rolled_back_text = text(&rolled_back);
    assert!(!rolled_back.status.success(), "{rolled_back_text}");
    for line in [
        "Moving Rakazo's database from Postgres 16 to 18",
        "Update failed at the switch. Roll back",
        "the Update failed at the switch and was rolled back",
    ] {
        assert!(
            rolled_back_text.contains(line),
            "{line}: {rolled_back_text}"
        );
    }
    let settings = instance.settings();
    assert_eq!(settings["components"]["rakazo"]["version"], RAKAZO);
    assert_eq!(
        settings["failed_update"]["to"],
        serde_json::json!({ "rakazo": "v0.16.1" })
    );
    assert!(settings["holds"].is_null(), "no Hold: {settings}");
    assert_eq!(instance.postgres_major(), "16");
    assert_eq!(instance.sql(kept), "10000 50005000");
    assert_eq!(sign_in(), 200);
    assert_eq!(instance.backups().len(), 1);
    assert!(
        fs::read_to_string(folder.join(".env"))
            .unwrap()
            .contains(&format!("COMPOSE_FILE=rakazo.{RAKAZO}.images.yml:"))
    );

    let moved = update(&["--yes"], &next("v0.16.1", "on-18.yml"));
    let moved_text = text(&moved);
    assert!(moved.status.success(), "{moved_text}");
    let failed_on = settings["failed_update"]["on"].as_str().unwrap();
    for line in [
        format!("The last update to Rakazo v0.16.1 failed on {failed_on} and was rolled back."),
        "Rakazo's Postgres 16 → 18: a Backup is required".to_string(),
        "Restoring Rakazo's database".to_string(),
    ] {
        assert!(moved_text.contains(&line), "{line}: {moved_text}");
    }
    let settings = instance.settings();
    assert_eq!(settings["components"]["rakazo"]["version"], "v0.16.1");
    assert!(settings["failed_update"].is_null(), "{settings}");
    assert!(settings["holds"].is_null(), "{settings}");
    assert_eq!(instance.postgres_major(), "18");
    assert_eq!(instance.sql(kept), "10000 50005000", "all data kept");
    assert_eq!(sign_in(), 200, "the Owner is still there");
    assert_eq!(instance.backups().len(), 2);
    assert!(gone_unless_tagged_elsewhere(&old_postgres), "{moved_text}");

    let failed = update(&["--yes"], &next("v0.16.2", "failing.yml"));
    let failed_text = text(&failed);
    assert!(!failed.status.success(), "{failed_text}");
    let backups = instance.backups();
    assert_eq!(backups.len(), 3, "{failed_text}");
    assert!(
        failed_text.contains(&format!(
            "run:\n  rakazo-stack restore --replace {}",
            backups[2].display()
        )),
        "{failed_text}"
    );
    assert!(!failed_text.contains("Roll back to"), "{failed_text}");
    assert_eq!(
        instance.settings()["components"]["rakazo"]["version"],
        "v0.16.2",
        "the new versions stay"
    );

    let unbacked = update(&["--yes", "--no-backup"], &next("v0.16.3", "failing.yml"));
    let unbacked_text = text(&unbacked);
    assert!(!unbacked.status.success(), "{unbacked_text}");
    assert!(
        unbacked_text.contains("No Backup was taken before it")
            && !unbacked_text.contains("restore --replace"),
        "{unbacked_text}"
    );
    assert_eq!(instance.backups().len(), 3);
    assert_eq!(
        instance.settings()["components"]["rakazo"]["version"],
        "v0.16.3",
        "never swapped back"
    );
    assert_eq!(instance.postgres_major(), "18");
}
