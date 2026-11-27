//! `rakazo-stack setup` (spec §7), so far: the Owner, Model provider, Composio, Memory and
//! access questions, the key checks, Rakazo, Hindsight when memory is on, the stack's
//! Tailscale device, voice through Lemonade, the Owner, their voice and model connection and
//! the default Space's memory filled in through Rakazo's own code paths, and the Checklist.

use std::time::Duration;

use crate::access::{self, AUTH_KEY_NAME, Access};
use crate::answers::{self, FrontEnd, NoQuestions, Terminal, Wizard};
use crate::checklist::Checklist;
use crate::composio;
use crate::computer_egress;
use crate::database_settings;
use crate::docker::{ContainerSummary, Docker};
use crate::envfile::{self, EnvInputs, STACK_COMPOSE_FILE, TailscaleEnv};
use crate::github::{self, HINDSIGHT_REPO, RAKAZO_REPO};
use crate::http;
use crate::instance::InstanceFolder;
use crate::lemonade::{self, LEMONADE_IMAGE, LEMONADE_REPO, LemonadeRelease, LocalLemonade};
use crate::memory::{self, HindsightModel, Memory, MemoryEnv};
use crate::model_provider::{self, ChatModel, Choice, OwnerModel, Provider};
use crate::overrides;
use crate::platform::Platform;
use crate::preflight;
use crate::rakazo::LocalRakazo;
use crate::registry::{self, Ghcr};
use crate::resolve::{
    self, HINDSIGHT_DB_IMAGE, HINDSIGHT_DB_TAG, HINDSIGHT_FLOOR, HindsightRelease, RAKAZO_FLOOR,
    RakazoRelease, Resolution, ResolveError,
};
use crate::runlog::RunLog;
use crate::secrets::{self, HINDSIGHT_TENANT_KEY, Secrets};
use crate::settings::Settings;
use crate::tailscale::{self, Joined};
use crate::voice;

const STACK_LAYER: &str = include_str!("../stack/compose.stack.yaml");
const RAKAZO_COMPOSE_PATH: &str = "infra/compose/docker-compose.images.yml";
const WAIT_TIMEOUT_SECONDS: &str = "900";
const VOICE_SERVICES: [&str; 2] = ["lemonade", "lemonade-relay"];

#[derive(Debug)]
pub struct SetupError(pub String);

impl<E: std::fmt::Display> From<E> for SetupError {
    fn from(err: E) -> Self {
        Self(err.to_string())
    }
}

/// One run's fixed inputs to `.env`, besides `settings.json` and `secrets.env`.
struct Stack<'a> {
    project: String,
    release: RakazoRelease,
    web_port: u16,
    access: Access,
    chat: Option<ChatModel<'a>>,
    /// `None` where Lemonade runs natively.
    lemonade: Option<LemonadeRelease>,
    /// `Some` while memory is on.
    hindsight: Option<(HindsightRelease, HindsightModel<'a>)>,
}

impl Stack<'_> {
    /// Where setup itself reaches Rakazo, in every access mode.
    fn local_origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.web_port)
    }

    fn env(&self, settings: &Settings, secrets: &Secrets) -> String {
        // Without a recorded version there is no image to run, so the profile stays off.
        let image = match &self.access {
            Access::Tailscale { .. } => settings.components.tailscale.as_ref(),
            _ => None,
        };
        envfile::render(&EnvInputs {
            project: &self.project,
            rakazo: &self.release,
            lemonade: self.lemonade.as_ref(),
            secrets,
            web_port: self.web_port,
            chat: self.chat,
            addresses: &self
                .access
                .addresses(settings.tailscale_dns_name.as_deref(), self.web_port),
            tailscale: image.map(|image| TailscaleEnv {
                image,
                device_name: settings.device_name(),
                auth_key: secrets.get(AUTH_KEY_NAME),
            }),
            memory: self.hindsight.as_ref().map(|(release, model)| MemoryEnv {
                release,
                model: *model,
                secrets,
            }),
        })
    }
}

pub struct Setup<'a> {
    pub platform: Platform,
    pub docker: &'a Docker,
    pub folder: &'a InstanceFolder,
    pub no_questions: bool,
    /// From preflight: `docker info` reports the `nftables` firewall backend.
    pub nftables: bool,
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
            recorded.as_ref().and_then(|s| s.memory),
            &secrets,
            recorded.is_none(),
        )?;
        let access = access::collect(
            front.as_mut(),
            recorded.as_ref(),
            secrets.get(AUTH_KEY_NAME),
        )?;
        let model = answers.model_provider.0.zip(answers.model_key.as_ref());
        let recorded_provider = recorded.as_ref().and_then(|s| s.model_provider);
        let (rakazo_model, dropped_override) = keep_override(
            "Rakazo model",
            recorded.as_ref().and_then(|s| s.rakazo_model.clone()),
            recorded_provider,
            answers.model_provider,
        );
        let (memory_model, dropped_memory_override) = keep_override(
            "memory model",
            recorded.as_ref().and_then(|s| s.memory_model.clone()),
            recorded_provider,
            answers.model_provider,
        );
        let chat = match model {
            Some((provider, key)) => {
                ChatModel::new(provider, key, rakazo_model.as_deref()).map_err(SetupError)?
            }
            None => None,
        };
        let owner_model = match model {
            Some((provider, key)) => {
                OwnerModel::new(provider, key, rakazo_model.as_deref()).map_err(SetupError)?
            }
            None => None,
        };
        let new_key = model
            .is_some_and(|(provider, key)| secrets.get(provider.key_name()) != Some(key.expose()));
        let hindsight_model = match model {
            Some((provider, key)) if answers.memory == Some(Memory::On) => Some(
                HindsightModel::new(provider, key, memory_model.as_deref()).map_err(SetupError)?,
            ),
            _ => None,
        };
        let key_warning = match model {
            Some((provider, key)) => {
                let saved = secrets.get(provider.key_name()) == Some(key.expose());
                let check = model_provider::check_key(&agent, provider.api_base(), provider, key);
                model_provider::require_key(provider.name(), provider.key_name(), check, saved)
                    .map_err(SetupError)?
            }
            None => None,
        };
        let composio_warning = match &answers.composio_key {
            Some(key) => {
                let saved = secrets.get(composio::KEY_NAME) == Some(key.expose());
                let check = composio::check_key(&agent, composio::API_BASE, key);
                model_provider::require_key("Composio", composio::KEY_NAME, check, saved)
                    .map_err(SetupError)?
            }
            None => None,
        };
        let log = RunLog::create(self.folder.path())?;
        for note in dropped_override.iter().chain(&dropped_memory_override) {
            log.say(note);
        }
        for warning in [key_warning, composio_warning].into_iter().flatten() {
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
        // Recorded the first time memory runs; only `update` moves it after that.
        let hindsight = match (&hindsight_model, recorded.as_ref()) {
            (None, _) => None,
            (Some(_), Some(settings)) if settings.components.hindsight.is_some() => {
                settings.hindsight_release()
            }
            (Some(_), _) => Some(resolve_hindsight(&agent, &log)?),
        };
        let mut settings = recorded.unwrap_or_else(|| Settings::new(&release));
        settings.program_version = env!("CARGO_PKG_VERSION").to_string();
        if settings.owner_email.as_deref() != Some(answers.owner_email.as_str()) {
            // Everything setup fills belongs to the Owner, so a new address starts afresh.
            settings.filled_by_setup.clear();
            settings.model_connection_due = None;
            settings.model_providers_left.clear();
            settings.owner_email = Some(answers.owner_email.clone());
        }
        database_settings::note_model_change(&mut settings, answers.model_provider, new_key);
        settings.model_provider = Some(answers.model_provider);
        settings.rakazo_model.clone_from(&rakazo_model);
        let lemonade = match (self.platform, settings.lemonade_release()) {
            (Platform::MacOs, _) => None,
            (_, Some(recorded)) => Some(recorded),
            (_, None) => {
                let release = resolve_lemonade(&agent, &log)?;
                settings.record_lemonade(&release);
                Some(release)
            }
        };
        settings.memory_model.clone_from(&memory_model);
        if let Some(memory) = answers.memory {
            settings.memory = Some(memory);
        }
        let memory_ran_before = settings.components.hindsight.is_some();
        if let Some(hindsight) = &hindsight {
            settings.record_hindsight(hindsight);
        }

        access.record(&mut settings);
        secrets.fill_missing(secrets::random_hex);
        if hindsight.is_some() {
            secrets.fill_missing_hindsight(secrets::random_hex);
        }
        model_provider::save_key(&mut secrets, model);
        secrets.set_pasted(composio::KEY_NAME, answers.composio_key.as_ref());
        if let Access::Tailscale {
            auth_key: Some(key),
            ..
        } = &access
        {
            secrets.set_pasted(AUTH_KEY_NAME, Some(key.pasted()));
        }

        let mut checklist = Checklist::default();
        let tailscale_chosen = matches!(access, Access::Tailscale { .. });
        if tailscale_chosen && settings.components.tailscale.is_none() {
            match registry::tailscale_stable(&agent) {
                Ok(image) => settings.components.tailscale = Some(image),
                Err(err) => checklist.add(tailscale::unresolved_item(&err.to_string())),
            }
        }

        let rakazo_file = envfile::rakazo_compose_file(&release.tag);
        let fetched = self.folder.read(&rakazo_file)?.is_some();
        // The compose file is written last, so its presence means the script beside it
        // came from the same tag.
        let script = match self
            .folder
            .read(computer_egress::SCRIPT)?
            .filter(|_| fetched)
        {
            Some(script) => script,
            None => github::fetch_file(
                &agent,
                RAKAZO_REPO,
                &release.tag,
                computer_egress::UPSTREAM_SCRIPT,
            )?,
        };
        computer_egress::write_build_folder(self.folder, &script)?;
        if !fetched {
            let upstream =
                github::fetch_file(&agent, RAKAZO_REPO, &release.tag, RAKAZO_COMPOSE_PATH)?;
            self.folder.write(&rakazo_file, &upstream)?;
        }
        let stack = Stack {
            project: overrides::compose_project(),
            release,
            web_port: overrides::web_port(),
            access,
            chat,
            lemonade,
            hindsight: hindsight.zip(hindsight_model),
        };
        self.folder
            .write("secrets.env", secrets.to_text().as_bytes())?;
        self.folder
            .write("settings.json", settings.to_json().as_bytes())?;
        self.folder
            .write(STACK_COMPOSE_FILE, STACK_LAYER.as_bytes())?;
        for (path, contents) in lemonade::RELAY_BUILD_FOLDER {
            self.folder.write_build_file(path, contents.as_bytes())?;
        }
        self.folder
            .write(".env", stack.env(&settings, &secrets).as_bytes())?;
        self.folder.reapply_modes()?;

        // Compose leaves a service whose profile is off running, so it is removed by
        // name. Its volume stays, to rejoin as the same device later.
        if !tailscale_chosen
            && settings.components.tailscale.is_some()
            && self
                .compose(&[
                    "--profile",
                    tailscale::SERVICE,
                    "rm",
                    "--stop",
                    "--force",
                    tailscale::SERVICE,
                ])
                .is_err()
        {
            checklist.add(tailscale::not_removed_item(self.compose_command()));
        }
        if stack.hindsight.is_none() && memory_ran_before {
            let mut args = vec!["--profile", memory::PROFILE, "rm", "--stop", "--force"];
            args.extend(memory::SERVICES);
            if self.compose(&args).is_err() {
                checklist.add(memory::not_removed_item(self.compose_command()));
            }
        }
        if self.platform == Platform::Wsl {
            self.recreate_containers_made_in_wsl(&stack.project, &log)?;
        }
        let tailnet = tailscale_chosen && settings.components.tailscale.is_some();
        // Voice and Tailscale start on their own, so that their failure can't stop Rakazo's.
        let rakazo_services =
            self.services_besides(&[VOICE_SERVICES.as_slice(), &[tailscale::SERVICE]].concat())?;
        log.say(&format!("Pulling Rakazo {} images…", stack.release.tag));
        if let Some((hindsight, _)) = &stack.hindsight {
            log.say(&format!(
                "Memory is on: pulling Hindsight {}, which is about 6.4 GB…",
                hindsight.version
            ));
        }
        self.compose_for(&["pull", "--quiet", "--ignore-buildable"], &rakazo_services)?;
        log.say("Building the computer-egress sidecar…");
        self.compose(&["build", "--quiet", computer_egress::SERVICE])?;
        let voice_started = self.start_voice(&log, &mut checklist);
        log.say(if stack.hindsight.is_some() {
            "Starting Rakazo and Hindsight (Hindsight takes about a minute)…"
        } else {
            "Starting Rakazo…"
        });
        self.start(&stack, &rakazo_services)?;
        let origin = stack.local_origin();
        log.say(&format!(
            "Rakazo {} is running at {origin}",
            stack.release.tag
        ));
        if let Access::BringYourOwn(url) = &stack.access {
            log.say(&format!(
                "Its Instance URL is {}. Your proxy on this machine must forward it to {origin}.",
                url.origin()
            ));
        }

        // macOS's native lemond is its own slice; until then it has no voice models step.
        let voice_ready = voice_started
            && stack.lemonade.is_some()
            && voice::prepare_models(
                &LocalLemonade {
                    docker: self.docker,
                    folder: self.folder.path(),
                },
                || {
                    let home = self.folder.path().parent()?;
                    preflight::docker_free_disk(self.platform, self.docker, home)
                },
                &mut |line| log.say(line),
                &mut checklist,
            );
        if tailnet {
            self.join_tailnet(
                &stack,
                &rakazo_services,
                &mut settings,
                &mut secrets,
                &mut checklist,
                &log,
            )?;
        }
        if tailscale_chosen {
            checklist.add(tailscale::console_item(settings.device_name()));
        }

        computer_egress::check(
            self.docker,
            self.folder.path(),
            self.nftables,
            &log,
            &mut checklist,
        )?;
        let rakazo = LocalRakazo {
            docker: self.docker,
            folder: self.folder.path(),
            agent: &agent,
            origin: &origin,
        };
        let tenant_key = stack
            .hindsight
            .is_some()
            .then(|| secrets.get(HINDSIGHT_TENANT_KEY))
            .flatten();
        database_settings::fill_in(
            &rakazo,
            front.as_mut(),
            &answers.owner_email,
            answers.owner_password,
            database_settings::Fillable {
                voice: voice_ready,
                memory: tenant_key,
                model: owner_model.map(|model| database_settings::ModelFill {
                    model,
                    due: &mut settings.model_connection_due,
                }),
            },
            &mut settings.filled_by_setup,
            &mut checklist,
        );
        database_settings::old_connections(
            &rakazo,
            &answers.owner_email,
            answers.model_provider,
            &mut settings.model_providers_left,
            &mut checklist,
        );
        if stack.hindsight.is_some() {
            checklist.add(memory::another_space_item());
        } else {
            database_settings::still_connected(&rakazo, &mut checklist);
        }
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

    /// Builds the relay and starts it with Lemonade. A failure is a Checklist item, not a
    /// failed run, so Rakazo starts either way (spec §9).
    fn start_voice(&self, log: &RunLog, checklist: &mut Checklist) -> bool {
        log.say("Building the Lemonade relay and starting Lemonade…");
        let mut up = vec![
            "up",
            "-d",
            "--build",
            "--wait",
            "--wait-timeout",
            WAIT_TIMEOUT_SECONDS,
        ];
        up.extend(
            VOICE_SERVICES
                .iter()
                .filter(|s| self.platform != Platform::MacOs || **s != "lemonade"),
        );
        match self.compose(&up) {
            Ok(()) => true,
            Err(err) => {
                checklist.add(voice::not_set_up(&format!(
                    "Lemonade or its relay didn't start ({})",
                    err.0
                )));
                false
            }
        }
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
        let status = self.docker.compose(self.folder.path(), args).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(SetupError(format!(
                "`docker compose {}` failed ({status}).",
                args.join(" ")
            )))
        }
    }

    fn compose_for(&self, args: &[&str], services: &[String]) -> Result<(), SetupError> {
        let mut args = args.to_vec();
        args.extend(services.iter().map(String::as_str));
        self.compose(&args)
    }

    fn start(&self, stack: &Stack, services: &[String]) -> Result<(), SetupError> {
        let args = ["up", "-d", "--wait", "--wait-timeout", WAIT_TIMEOUT_SECONDS];
        self.compose_for(&args, services)?;
        wait_for_web(&http::agent(), &stack.local_origin())
    }

    fn services_besides(&self, excluded: &[&str]) -> Result<Vec<String>, SetupError> {
        let out = self
            .docker
            .compose(self.folder.path(), ["config", "--services"])
            .output()?;
        if !out.status.success() {
            return Err(SetupError(format!(
                "`docker compose config --services` failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|service| !excluded.contains(service))
            .map(str::to_string)
            .collect())
    }

    /// Step 10. A Tailscale failure becomes a Checklist item: Rakazo works without it.
    fn join_tailnet(
        &self,
        stack: &Stack,
        rakazo_services: &[String],
        settings: &mut Settings,
        secrets: &mut Secrets,
        checklist: &mut Checklist,
        log: &RunLog,
    ) -> Result<(), SetupError> {
        log.say("Waiting for Tailscale…");
        let previous = settings
            .tailscale_dns_name
            .as_deref()
            .map(tailscale::device_label);
        let joined = match self.start_tailscale(settings.device_name(), previous) {
            Ok(joined) => joined,
            Err(item) => {
                checklist.add(item);
                return Ok(());
            }
        };
        settings.tailscale_device_name = Some(joined.device_name().to_string());
        settings.tailscale_dns_name = Some(joined.dns_name.clone());
        // `tailscale-state` holds the identity now, so the key is dropped.
        secrets.set_pasted(AUTH_KEY_NAME, None);
        self.folder
            .write("settings.json", settings.to_json().as_bytes())?;
        self.folder
            .write("secrets.env", secrets.to_text().as_bytes())?;
        let env = stack.env(settings, secrets);
        if self.folder.read(".env")?.as_deref() != Some(env.as_bytes()) {
            self.folder.write(".env", env.as_bytes())?;
            log.say("Applying the Instance URL…");
            self.start(stack, rakazo_services)?;
            if let Err(item) = self.start_tailscale(joined.device_name(), None) {
                checklist.add(item);
            }
        }
        let url = format!("https://{}", joined.dns_name);
        log.say(&format!("Its Instance URL is {url}"));
        if !joined.https_ready {
            checklist.add(tailscale::no_https_item(&url, self.compose_command()));
        }
        Ok(())
    }

    fn start_tailscale(&self, requested: &str, previous: Option<&str>) -> Result<Joined, String> {
        let compose = self.compose_command();
        self.compose(&["up", "-d", "--no-deps", tailscale::SERVICE])
            .map_err(|err| tailscale::start_failed_item(err.0.trim_end_matches('.'), compose))?;
        tailscale::wait_for_join(
            requested,
            previous,
            tailscale::ATTEMPTS,
            || self.tailscale_status(),
            || std::thread::sleep(tailscale::PAUSE),
        )
        .map_err(|failure| tailscale::not_joined_item(&failure, compose))
    }

    fn tailscale_status(&self) -> Option<String> {
        let mut command = self.docker.compose(
            self.folder.path(),
            [
                "exec",
                "-T",
                tailscale::SERVICE,
                "tailscale",
                "status",
                "--json",
                "--peers=false",
            ],
        );
        let out = command.output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// What the Self-hoster types for Compose in the Instance folder (spec §17).
    fn compose_command(&self) -> &'static str {
        match self.platform {
            Platform::Wsl => "docker.exe compose",
            Platform::Linux | Platform::MacOs => "docker compose",
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

/// A Rakazo model override names a model at one provider, so a new provider drops it
/// (spec §8), with a note saying so.
fn keep_override(
    label: &str,
    model: Option<String>,
    recorded_provider: Option<Choice>,
    chosen: Choice,
) -> (Option<String>, Option<String>) {
    let Some(model) = model else {
        return (None, None);
    };
    match recorded_provider {
        Some(previous) if previous != chosen => {
            let was = previous.0.map_or("none", Provider::name);
            (
                None,
                Some(format!(
                    "Removed the {label} override `{model}`, which was set for {was}. Add one for the new provider to `settings.json` if you want it."
                )),
            )
        }
        _ => (Some(model), None),
    }
}

fn resolve_rakazo(agent: &ureq::Agent, log: &RunLog) -> Result<RakazoRelease, SetupError> {
    let tags = github::fetch_stable_tags(agent, RAKAZO_REPO)?;
    let ghcr = Ghcr::new(agent);
    let resolution = resolve::choose_rakazo(&tags, |image, tag| {
        ghcr.digest(image, tag).map_err(|err| err.to_string())
    });
    report(resolution, "Rakazo", RAKAZO_FLOOR, log)
}

fn resolve_hindsight(agent: &ureq::Agent, log: &RunLog) -> Result<HindsightRelease, SetupError> {
    let tags = github::fetch_stable_tags(agent, HINDSIGHT_REPO)?;
    let ghcr = Ghcr::new(agent);
    let resolution = resolve::choose_hindsight(
        &tags,
        |image, tag| ghcr.digest(image, tag).map_err(|err| err.to_string()),
        || {
            registry::docker_hub_digest(agent, HINDSIGHT_DB_IMAGE, HINDSIGHT_DB_TAG)
                .map_err(|err| err.to_string())
        },
    );
    report(resolution, "Hindsight", HINDSIGHT_FLOOR, log)
}

fn report<R>(
    resolution: Result<Resolution<R>, ResolveError>,
    component: &str,
    floor: &str,
    log: &RunLog,
) -> Result<R, SetupError> {
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
                "No {component} release at or above {floor} has published images yet. Try again later."
            )))
        }
        Err(ResolveError::Lookup(err)) => Err(SetupError(format!(
            "Couldn't check {component}'s releases: {err}"
        ))),
    }
}

fn resolve_lemonade(agent: &ureq::Agent, log: &RunLog) -> Result<LemonadeRelease, SetupError> {
    let tags = github::fetch_stable_tags(agent, LEMONADE_REPO)
        .map_err(|err| SetupError(format!("Couldn't check Lemonade's releases: {err}")))?;
    let ghcr = Ghcr::new(agent);
    let resolution = lemonade::choose_lemonade(&tags, |tag| {
        ghcr.digest(LEMONADE_IMAGE, tag)
            .map_err(|err| err.to_string())
    })
    .map_err(|err| SetupError(format!("Couldn't pick a Lemonade release: {err}")))?;
    for tag in &resolution.skipped {
        log.say(&lemonade::describe_skipped(tag));
    }
    Ok(resolution.release)
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
    use super::*;
    use crate::access::{AccessMode, AuthKey, DeviceName};
    use crate::secrets::PastedKey;
    use crate::settings::RecordedImage;

    const KEY: &str = "tskey-auth-kPLACEHOLDER-0000000000";

    fn release(tag: &str) -> RakazoRelease {
        RakazoRelease {
            tag: tag.into(),
            app_digest: "sha256:aaa".into(),
            computer_digest: "sha256:ccc".into(),
        }
    }

    fn pasted_key() -> PastedKey {
        PastedKey::new(KEY).unwrap()
    }

    fn tailscale_stack(release: RakazoRelease, project: &str) -> Stack<'static> {
        Stack {
            project: project.into(),
            release,
            web_port: 5173,
            access: Access::Tailscale {
                device_name: DeviceName::parse("rakazo").unwrap(),
                auth_key: Some(AuthKey::parse(KEY).unwrap()),
            },
            chat: None,
            lemonade: None,
            hindsight: None,
        }
    }

    #[test]
    fn the_tailscale_profile_waits_for_a_recorded_version() {
        let stack = tailscale_stack(release("v0.16.0"), "rakazo-stack");
        let mut settings = Settings::new(&stack.release);
        let mut secrets = Secrets::default();
        secrets.set_pasted(AUTH_KEY_NAME, Some(&pasted_key()));
        let unresolved = stack.env(&settings, &secrets);
        assert!(unresolved.contains("\nCOMPOSE_PROFILES=\n"), "{unresolved}");
        assert!(!unresolved.contains(KEY));

        settings.components.tailscale = Some(RecordedImage {
            version: "v1.104.7".into(),
            digest: "sha256:ttt".into(),
        });
        let joining = stack.env(&settings, &secrets);
        assert!(joining.contains("\nCOMPOSE_PROFILES=tailscale\n"));
        assert!(joining.contains(&format!("\nTS_AUTHKEY={KEY}\n")));

        secrets.set_pasted(AUTH_KEY_NAME, None);
        settings.access = AccessMode::Tailscale;
        settings.tailscale_device_name = Some("rakazo-1".into());
        settings.tailscale_dns_name = Some("rakazo-1.example.ts.net".into());
        let joined = stack.env(&settings, &secrets);
        assert!(joined.contains("\nTS_HOSTNAME=rakazo-1\n"), "{joined}");
        assert!(joined.contains("\nRAKAZO_HOST=rakazo-1.example.ts.net\n"));
        assert!(!joined.contains("TS_AUTHKEY"));
    }

    #[test]
    fn the_layer_serves_only_the_web_app_and_publishes_nothing() {
        let (_, tailscale) = STACK_LAYER.split_once("\n  tailscale:\n").unwrap();
        let (service, rest) = tailscale.split_once("\nconfigs:\n").unwrap();
        for setting in [
            "profiles: [tailscale]",
            "TS_USERSPACE: \"true\"",
            "TS_AUTH_ONCE: \"true\"",
            "TS_STATE_DIR: /var/lib/tailscale",
            "- tailscale-state:/var/lib/tailscale",
            "networks: [app]",
        ] {
            assert!(service.contains(setting), "{setting}");
        }
        assert!(!service.contains("ports:"), "{service}");
        let (_, content) = rest.split_once("content: |\n").unwrap();
        let (json, _) = content.split_once("\n\n").unwrap();
        let serve: serde_json::Value = serde_json::from_str(&json.replace("$$", "$")).unwrap();
        assert_eq!(
            serve,
            serde_json::json!({
                "TCP": { "443": { "HTTPS": true } },
                "Web": { "${TS_CERT_DOMAIN}:443": { "Handlers": { "/": { "Proxy": "http://web:5173" } } } }
            })
        );
        assert!(json.contains("$${TS_CERT_DOMAIN}"), "escaped for Compose");
    }

    /// Renders a test Instance folder with Tailscale on and asks Compose for the merged
    /// model. Nothing is created or started, so nothing tries to join a tailnet.
    #[test]
    #[ignore = "needs Docker, GitHub and Docker Hub; CI and the dev machine run it with --include-ignored"]
    fn compose_accepts_the_tailscale_service() {
        let home = tempfile::tempdir().unwrap();
        let folder = InstanceFolder::for_home(home.path());
        let _lock = folder.lock().unwrap();
        let agent = http::agent();
        let project = format!("rkst-config-{}", std::process::id());
        let stack = tailscale_stack(release(RAKAZO_FLOOR), &project);
        let image = registry::tailscale_stable(&agent).unwrap();
        assert!(image.version.starts_with('v'), "{image:?}");
        assert!(image.digest.starts_with("sha256:"), "{image:?}");
        let mut settings = Settings::new(&stack.release);
        settings.components.tailscale = Some(image.clone());
        stack.access.record(&mut settings);
        let mut secrets = Secrets::default();
        secrets.fill_missing(secrets::random_hex);
        secrets.set_pasted(AUTH_KEY_NAME, Some(&pasted_key()));

        let upstream =
            github::fetch_file(&agent, RAKAZO_REPO, RAKAZO_FLOOR, RAKAZO_COMPOSE_PATH).unwrap();
        folder
            .write(&envfile::rakazo_compose_file(RAKAZO_FLOOR), &upstream)
            .unwrap();
        folder
            .write(STACK_COMPOSE_FILE, STACK_LAYER.as_bytes())
            .unwrap();
        folder
            .write(".env", stack.env(&settings, &secrets).as_bytes())
            .unwrap();

        let docker = Docker::find(Platform::detect()).unwrap();
        let compose = |args: &[&str]| {
            let out = docker
                .command(["compose"])
                .args(args)
                .current_dir(folder.path())
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        };
        let services = compose(&["config", "--services"]);
        assert!(services.lines().any(|s| s == "tailscale"), "{services}");
        assert!(!services.lines().any(|s| s == "tailscale-ingress"));

        let model: serde_json::Value =
            serde_json::from_str(&compose(&["config", "--format", "json"])).unwrap();
        let service = &model["services"]["tailscale"];
        assert_eq!(
            service["image"],
            format!("tailscale/tailscale:{}@{}", image.version, image.digest)
        );
        assert_eq!(service["environment"]["TS_AUTHKEY"], KEY);
        assert_eq!(service["environment"]["TS_HOSTNAME"], "rakazo");
        assert_eq!(
            service["environment"]["TS_SERVE_CONFIG"],
            "/config/serve.json"
        );
        assert_eq!(service["configs"][0]["target"], "/config/serve.json");
        assert!(service["ports"].is_null(), "{service}");
        assert_eq!(
            service["networks"].as_object().unwrap().keys().next(),
            Some(&"app".to_string())
        );
        assert_eq!(service["volumes"][0]["source"], "tailscale-state");
        let content = model["configs"]["tailscale-serve"]["content"]
            .as_str()
            .unwrap();
        // `config` writes `$` as `$$`, so that its output can be read back.
        let serve: serde_json::Value = serde_json::from_str(&content.replace("$$", "$")).unwrap();
        assert!(
            serve["Web"]["${TS_CERT_DOMAIN}:443"].is_object(),
            "{content}"
        );
    }

    #[test]
    fn a_new_provider_drops_the_model_overrides() {
        let openrouter = Choice(Some(Provider::OpenRouter));
        let model = || Some("z-ai/glm-5.3-flash".to_string());

        let same = keep_override("Rakazo model", model(), Some(openrouter), openrouter);
        assert_eq!(same, (model(), None));

        let anthropic = Choice(Some(Provider::Anthropic));
        let (kept, note) = keep_override("memory model", model(), Some(openrouter), anthropic);
        assert_eq!(kept, None);
        let note = note.unwrap();
        assert!(
            note.starts_with("Removed the memory model override")
                && note.contains("set for OpenRouter"),
            "{note}"
        );

        assert_eq!(
            keep_override("Rakazo model", None, None, Choice(None)),
            (None, None)
        );
    }

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
