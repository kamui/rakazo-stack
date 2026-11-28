//! Lemonade (spec §4, §9): which release runs on Linux and Windows, and the running
//! Lemonade as setup's voice-model steps reach it: the `lemonade` container, or on macOS
//! the Instance's own `lemond`.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Stdio};

use serde::Deserialize;

use crate::docker::Docker;
use crate::lemond::Lemond;
use crate::platform::Platform;
use crate::runlog::{self, Running, Stdout};
use crate::voice::{Lemonade, Warmed};

pub const LEMONADE_REPO: &str = "lemonade-sdk/lemonade";
pub const LEMONADE_IMAGE: &str = "lemonade-sdk/lemonade-server";

/// The voice models setup pulls through the relay, and the relay's address in Rakazo's
/// network (spec §9).
pub const SPEECH_MODEL: &str = "kokoro-v1";
/// The program is built per OS, and only macOS runs Whisper on Metal (spec §9).
pub const TRANSCRIPTION_MODEL: &str = if cfg!(target_os = "macos") {
    METAL_WHISPER
} else {
    CPU_WHISPER
};
const METAL_WHISPER: &str = "Whisper-Large-v3-Turbo";
const CPU_WHISPER: &str = "Whisper-Small";

/// The Whisper model setup connects voice to on `platform`, for a Backup made there.
pub fn transcription_model(platform: Platform) -> &'static str {
    match platform {
        Platform::MacOs => METAL_WHISPER,
        Platform::Linux | Platform::Wsl => CPU_WHISPER,
    }
}
pub const RELAY_URL: &str = "http://lemonade-relay:13306/api/v1";
/// How Rakazo names the relay when it asks for consent: its URL's origin.
pub const RELAY_ORIGIN: &str = "http://lemonade-relay:13306";

/// `whispercpp.backend=auto` picked Vulkan on a software renderer (#24); `broadcast=false`
/// stops lemond's UDP discovery beacon, as `--no-broadcast` does for macOS's lemond.
const CONFIG: [&str; 4] = [
    "whispercpp.backend=cpu",
    "auto_evict=false",
    "auto_check_model_updates=false",
    "broadcast=false",
];

const CLIENT: &str = include_str!("../stack/voice-models.js");

/// The relay's build folder, written to the Instance folder on every run and built there
/// (spec §6, §9).
pub const RELAY_BUILD_FOLDER: [(&str, &str); 2] = [
    ("relay/Dockerfile", include_str!("../relay/Dockerfile")),
    ("relay/src/relay.ts", include_str!("../relay/src/relay.ts")),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LemonadeRelease {
    pub tag: String,
    pub digest: String,
}

impl LemonadeRelease {
    pub fn image(&self) -> String {
        format!("ghcr.io/{LEMONADE_IMAGE}:{}@{}", self.tag, self.digest)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct LemonadeResolution {
    pub release: LemonadeRelease,
    /// Newer releases whose image isn't published yet.
    pub skipped: Vec<String>,
}

/// The newest release in `tags` (newest first) whose image is published. Release
/// `vX.Y.Z` is image tag `vX.Y.Z`.
pub fn choose_lemonade<F>(tags: &[String], mut digest: F) -> Result<LemonadeResolution, String>
where
    F: FnMut(&str) -> Result<Option<String>, String>,
{
    let mut skipped = Vec::new();
    for tag in tags {
        match digest(tag)? {
            Some(digest) => {
                return Ok(LemonadeResolution {
                    release: LemonadeRelease {
                        tag: tag.clone(),
                        digest,
                    },
                    skipped,
                });
            }
            None => skipped.push(tag.clone()),
        }
    }
    Err("no Lemonade release has a published image".to_string())
}

pub fn describe_skipped(tag: &str) -> String {
    format!(
        "Skipped Lemonade {tag}: its image isn't published yet. A later `rakazo-stack update` picks it up once it is."
    )
}

/// The Instance's Lemonade, reached with `docker compose exec` from the Instance folder.
pub struct LocalLemonade<'a> {
    pub docker: &'a Docker,
    pub folder: &'a Path,
    /// macOS: configured on the host. The models still go through the relay.
    pub native: Option<&'a Lemond>,
}

#[derive(Debug, Default, Deserialize)]
struct ClientLine {
    installed: Option<Vec<String>>,
    progress: Option<Progress>,
    warmed: Option<Warmed>,
}

#[derive(Debug, Deserialize)]
struct Progress {
    file: Option<u32>,
    files: Option<u32>,
    percent: Option<u32>,
}

impl LocalLemonade<'_> {
    fn spawn(&self, service: &str, args: &[&str]) -> Result<(Child, Running), String> {
        let mut command = self.docker.compose(self.folder, ["exec", "-T", service]);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        runlog::spawn(&mut command).map_err(|err| err.to_string())
    }

    /// Runs the voice-model client in `api` and hands each line it writes to `line`.
    fn client(
        &self,
        request: serde_json::Value,
        mut line: impl FnMut(ClientLine),
    ) -> Result<(), String> {
        let (mut child, running) = self.spawn("api", &["node", "-e", CLIENT])?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        stdin
            .write_all(request.to_string().as_bytes())
            .map_err(|err| err.to_string())?;
        drop(stdin);
        let stdout = child.stdout.take().expect("stdout is piped");
        for text in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(parsed) = serde_json::from_str(&text) {
                line(parsed);
            }
        }
        finish(child, running, "api")
    }
}

fn finish(mut child: Child, running: Running, service: &str) -> Result<(), String> {
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    let status = child.wait();
    running.finish(
        status.as_ref().copied(),
        Stdout::Elsewhere,
        stderr.as_bytes(),
    );
    let status = status.map_err(|err| err.to_string())?;
    if status.success() {
        return Ok(());
    }
    Err(stderr
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map_or_else(
            || format!("`{service}` exited with {status}"),
            str::to_string,
        ))
}

impl Lemonade for LocalLemonade<'_> {
    fn configure(&self) -> Result<(), String> {
        if let Some(lemond) = self.native {
            return lemond.configure();
        }
        let mut args = vec!["lemonade", "config", "set"];
        args.extend(CONFIG);
        let (mut child, running) = self.spawn("lemonade", &args)?;
        drop(child.stdin.take());
        finish(child, running, "lemonade")
    }

    fn installed(&self) -> Result<Vec<String>, String> {
        let mut installed = None;
        self.client(serde_json::json!({ "op": "installed" }), |line| {
            if line.installed.is_some() {
                installed = line.installed;
            }
        })?;
        installed.ok_or_else(|| "Lemonade didn't list its models".to_string())
    }

    fn pull(&self, model: &str, progress: &mut dyn FnMut(&str)) -> Result<(), String> {
        self.client(
            serde_json::json!({ "op": "pull", "model": model }),
            |line| {
                if let Some(Progress {
                    file: Some(file),
                    files: Some(files),
                    percent: Some(percent),
                }) = line.progress
                {
                    progress(&format!("file {file} of {files}, {percent}%"));
                }
            },
        )
    }

    fn load(&self, model: &str) -> Result<(), String> {
        self.client(serde_json::json!({ "op": "load", "model": model }), drop)
    }

    fn warm(&self) -> Result<Warmed, String> {
        let mut warmed = None;
        self.client(
            serde_json::json!({
                "op": "warm",
                "speech": SPEECH_MODEL,
                "transcription": TRANSCRIPTION_MODEL,
            }),
            |line| {
                if line.warmed.is_some() {
                    warmed = line.warmed;
                }
            },
        )?;
        warmed.ok_or_else(|| "the warm-up gave no result".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn picks_the_newest_release_with_a_published_image() {
        let got = choose_lemonade(&tags(&["v2026.48.3", "v2026.47.1"]), |tag| {
            Ok((tag == "v2026.47.1").then(|| "sha256:abc".to_string()))
        })
        .unwrap();
        assert_eq!(got.release.tag, "v2026.47.1");
        assert_eq!(got.skipped, ["v2026.48.3"]);
        assert_eq!(
            got.release.image(),
            "ghcr.io/lemonade-sdk/lemonade-server:v2026.47.1@sha256:abc"
        );
        assert!(describe_skipped("v2026.48.3").contains("Skipped Lemonade v2026.48.3"));
    }

    #[test]
    fn fails_without_any_published_image() {
        assert!(choose_lemonade(&tags(&["v2026.48.3"]), |_| Ok(None)).is_err());
        assert_eq!(
            choose_lemonade(&tags(&["v2026.48.3"]), |_| Err("GHCR is down".into())),
            Err("GHCR is down".to_string())
        );
    }

    #[test]
    fn the_bundled_build_folder_holds_every_file_the_image_copies() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("relay/src");
        for entry in std::fs::read_dir(src).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            let path = format!("relay/src/{name}");
            assert!(
                RELAY_BUILD_FOLDER
                    .iter()
                    .any(|(bundled, _)| *bundled == path),
                "{path} isn't bundled"
            );
        }
        let dockerfile = RELAY_BUILD_FOLDER[0].1;
        let copies: Vec<_> = dockerfile
            .lines()
            .filter(|line| line.starts_with("COPY"))
            .collect();
        assert_eq!(copies, ["COPY src ./src"]);
    }

    #[test]
    fn reads_the_clients_lines() {
        let line: ClientLine =
            serde_json::from_str(r#"{"warmed":{"speech_ms":900,"transcription_ms":3100}}"#)
                .unwrap();
        let warmed = line.warmed.unwrap();
        assert_eq!(warmed.transcription_ms, 3100);
        let line: ClientLine =
            serde_json::from_str(r#"{"progress":{"file":4,"files":5,"percent":54}}"#).unwrap();
        assert_eq!(line.progress.unwrap().percent, Some(54));
        let line: ClientLine =
            serde_json::from_str(r#"{"installed":["kokoro-v1","Whisper-Small"]}"#).unwrap();
        assert_eq!(line.installed.unwrap().len(), 2);
    }
}
