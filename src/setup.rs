//! `rakazo-stack setup` (spec §7), so far: the Owner and Model provider questions, the key
//! check, Rakazo on localhost, the Owner filled in through Rakazo's own code path, and the
//! Checklist.

use std::time::Duration;

use crate::answers::{self, FrontEnd, NoQuestions, Terminal, Wizard};
use crate::checklist::Checklist;
use crate::database_settings::{self, DatabaseSetting};
use crate::docker::{ContainerSummary, Docker};
use crate::envfile::{self, EnvInputs, STACK_COMPOSE_FILE};
use crate::github::{self, RAKAZO_REPO};
use crate::http;
use crate::instance::InstanceFolder;
use crate::model_provider::{self, ChatModel, Provider};
use crate::overrides;
use crate::platform::Platform;
use crate::rakazo::LocalRakazo;
use crate::registry::Ghcr;
use crate::resolve::{self, RAKAZO_FLOOR, RakazoRelease, ResolveError};
use crate::runlog::RunLog;
use crate::secrets::{self, Secrets};
use crate::settings::Settings;

const STACK_LAYER: &str = include_str!("../stack/compose.stack.yaml");
const RAKAZO_COMPOSE_PATH: &str = "infra/compose/docker-compose.images.yml";
const WAIT_TIMEOUT_SECONDS: &str = "900";

#[derive(Debug)]
pub struct SetupError(pub String);

impl<E: std::fmt::Display> From<E> for SetupError {
    fn from(err: E) -> Self {
        Self(err.to_string())
    }
}

pub struct Setup<'a> {
    pub platform: Platform,
    pub docker: &'a Docker,
    pub folder: &'a InstanceFolder,
    pub no_questions: bool,
}

impl Setup<'_> {
    pub fn run(&self) -> Result<(), SetupError> {
        let agent = http::agent();
        let recorded = self.load_settings()?;
        let mut secrets = Secrets::parse(&self.read_text("secrets.env")?.unwrap_or_default());
        let mut front = self.front_end()?;
        let answers = answers::collect(
            front.as_mut(),
            recorded.as_ref().and_then(|s| s.owner_email.as_deref()),
            recorded.as_ref().and_then(|s| s.model_provider),
            &secrets,
            recorded.is_none(),
        )?;
        let model = answers.model_provider.0.zip(answers.model_key.as_ref());
        let rakazo_model = recorded.as_ref().and_then(|s| s.rakazo_model.clone());
        let chat = match model {
            Some((provider, key)) => {
                ChatModel::new(provider, key, rakazo_model.as_deref()).map_err(SetupError)?
            }
            None => None,
        };
        let key_warning = match model {
            Some((provider, key)) => {
                let saved = secrets.get(provider.key_name()) == Some(key.expose());
                let check = model_provider::check_key(&agent, provider.api_base(), provider, key);
                model_provider::require_key(provider, check, saved).map_err(SetupError)?
            }
            None => None,
        };
        let log = RunLog::create(self.folder.path())?;
        if let Some(warning) = key_warning {
            log.say(&warning);
        }
        if let Some((Provider::Gemini, _)) = model {
            log.say(model_provider::GEMINI_BILLING);
        }
        let zdr_item = match model {
            Some((provider @ Provider::OpenRouter, key)) => model_provider::ensure_zdr(
                || model_provider::probe_zdr(&agent, provider.api_base(), key),
                front.as_mut(),
                |line| log.say(line),
            ),
            _ => None,
        };
        let release = match &recorded {
            Some(settings) => settings.rakazo_release(),
            None => resolve_rakazo(&agent, &log)?,
        };
        let mut settings = recorded.unwrap_or_else(|| Settings::new(&release));
        settings.program_version = env!("CARGO_PKG_VERSION").to_string();
        if settings.owner_email.as_deref() != Some(answers.owner_email.as_str()) {
            settings
                .filled_by_setup
                .retain(|filled| *filled != DatabaseSetting::Owner);
            settings.owner_email = Some(answers.owner_email.clone());
        }
        settings.model_provider = Some(answers.model_provider);

        secrets.fill_missing(secrets::random_hex);
        model_provider::save_key(&mut secrets, model);

        let rakazo_file = envfile::rakazo_compose_file(&release.tag);
        if self.folder.read(&rakazo_file)?.is_none() {
            let upstream =
                github::fetch_file(&agent, RAKAZO_REPO, &release.tag, RAKAZO_COMPOSE_PATH)?;
            self.folder.write(&rakazo_file, &upstream)?;
        }
        let project = overrides::compose_project();
        let web_port = overrides::web_port();
        self.folder
            .write("secrets.env", secrets.to_text().as_bytes())?;
        self.folder
            .write("settings.json", settings.to_json().as_bytes())?;
        self.folder
            .write(STACK_COMPOSE_FILE, STACK_LAYER.as_bytes())?;
        let env = envfile::render(&EnvInputs {
            project: &project,
            rakazo: &release,
            secrets: &secrets,
            web_port,
            chat,
        });
        self.folder.write(".env", env.as_bytes())?;
        self.folder.reapply_modes()?;

        if self.platform == Platform::Wsl {
            self.recreate_containers_made_in_wsl(&project, &log)?;
        }
        log.say(&format!("Pulling Rakazo {} images…", release.tag));
        self.compose(&["pull", "--quiet"])?;
        log.say("Starting Rakazo…");
        self.compose(&["up", "-d", "--wait", "--wait-timeout", WAIT_TIMEOUT_SECONDS])?;
        let origin = format!("http://127.0.0.1:{web_port}");
        wait_for_web(&agent, &origin)?;
        log.say(&format!("Rakazo {} is running at {origin}", release.tag));

        let mut checklist = Checklist::default();
        let rakazo = LocalRakazo {
            docker: self.docker,
            folder: self.folder.path(),
            agent: &agent,
            origin: &origin,
        };
        database_settings::fill_in(
            &rakazo,
            front.as_mut(),
            &answers.owner_email,
            answers.owner_password,
            &mut settings.filled_by_setup,
            &mut checklist,
        );
        if let Some(item) = zdr_item {
            checklist.add(item);
        }
        if let Some(item) = model_provider::key_item(model.map(|(provider, _)| provider)) {
            checklist.add(item);
        }
        self.folder
            .write("settings.json", settings.to_json().as_bytes())?;
        log.say(checklist.render().trim_end());
        Ok(())
    }

    fn front_end(&self) -> Result<Box<dyn FrontEnd>, SetupError> {
        if self.no_questions {
            Ok(Box::new(NoQuestions {
                env: |name: &str| std::env::var(name).ok(),
            }))
        } else {
            Ok(Box::new(Wizard {
                prompt: Terminal::attached()?,
            }))
        }
    }

    fn load_settings(&self) -> Result<Option<Settings>, SetupError> {
        let Some(text) = self.read_text("settings.json")? else {
            return Ok(None);
        };
        Settings::parse(&text)
            .map(Some)
            .map_err(|err| SetupError(format!("settings.json can't be read: {err}")))
    }

    fn read_text(&self, name: &str) -> Result<Option<String>, SetupError> {
        Ok(self
            .folder
            .read(name)?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
    }

    fn compose(&self, args: &[&str]) -> Result<(), SetupError> {
        let mut command = self.docker.command(["compose"]);
        command.args(args).current_dir(self.folder.path());
        let status = command.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(SetupError(format!(
                "`docker compose {}` failed ({status}).",
                args.join(" ")
            )))
        }
    }

    /// A WSL-run Compose gives containers mounts served from the distro, which fail after
    /// a reboot, so containers it made are recreated through `docker.exe` (spec §7).
    fn recreate_containers_made_in_wsl(
        &self,
        project: &str,
        log: &RunLog,
    ) -> Result<(), SetupError> {
        let filter = format!("label=com.docker.compose.project={project}");
        let out =
            self.docker
                .output(["ps", "-a", "--filter", &filter, "--format", "{{json .}}"])?;
        let services = services_made_in_wsl(&String::from_utf8_lossy(&out.stdout));
        if services.is_empty() {
            return Ok(());
        }
        log.say(&format!(
            "Recreating containers made by WSL's docker: {}",
            services.join(", ")
        ));
        let mut args = vec!["up", "--no-start", "--no-deps", "--force-recreate"];
        args.extend(services.iter().map(String::as_str));
        self.compose(&args)
    }
}

/// Services whose containers carry a Linux working directory: `docker.exe` records a
/// `\\wsl.localhost\…` path instead.
fn services_made_in_wsl(ps_output: &str) -> Vec<String> {
    let mut services: Vec<String> = ContainerSummary::parse_lines(ps_output)
        .iter()
        .filter(|c| {
            c.label("com.docker.compose.project.working_dir")
                .is_some_and(|dir| dir.starts_with('/'))
        })
        .filter_map(|c| c.label("com.docker.compose.service").map(str::to_string))
        .collect();
    services.sort();
    services.dedup();
    services
}

fn resolve_rakazo(agent: &ureq::Agent, log: &RunLog) -> Result<RakazoRelease, SetupError> {
    let tags = github::fetch_stable_tags(agent, RAKAZO_REPO)?;
    let ghcr = Ghcr::new(agent);
    let resolution = resolve::choose_rakazo(&tags, |image, tag| {
        ghcr.digest(image, tag).map_err(|err| err.to_string())
    });
    match resolution {
        Ok(resolution) => {
            for skipped in &resolution.skipped {
                log.say(&skipped.describe());
            }
            Ok(resolution.release)
        }
        Err(ResolveError::NothingAtFloor { skipped }) => {
            for skipped in &skipped {
                log.say(&skipped.describe());
            }
            Err(SetupError(format!(
                "No Rakazo release at or above {RAKAZO_FLOOR} has published images yet. Try again later."
            )))
        }
        Err(ResolveError::Lookup(err)) => Err(SetupError(format!(
            "Couldn't check Rakazo's releases: {err}"
        ))),
    }
}

fn wait_for_web(agent: &ureq::Agent, origin: &str) -> Result<(), SetupError> {
    for _ in 0..30 {
        if let Ok(response) = agent.get(origin).call()
            && response.status().is_success()
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err(SetupError(format!(
        "Rakazo's services are up, but {origin} doesn't answer."
    )))
}

#[cfg(test)]
mod tests {
    use super::services_made_in_wsl;

    #[test]
    fn finds_containers_made_by_wsls_docker() {
        let ps = concat!(
            r#"{"Labels":"com.docker.compose.project=rakazo-stack,com.docker.compose.project.working_dir=/home/someone/rakazo-stack,com.docker.compose.service=api","Ports":""}"#,
            "\n",
            r#"{"Labels":"com.docker.compose.project=rakazo-stack,com.docker.compose.project.working_dir=\\\\wsl.localhost\\Ubuntu\\home\\someone\\rakazo-stack,com.docker.compose.service=web","Ports":""}"#,
            "\n",
            r#"{"Labels":"com.docker.compose.project=rakazo-stack,com.docker.compose.project.working_dir=/home/someone/rakazo-stack,com.docker.compose.service=api","Ports":""}"#,
            "\n",
        );
        assert_eq!(services_made_in_wsl(ps), ["api"]);
    }
}
