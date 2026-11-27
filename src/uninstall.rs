//! `rakazo-stack uninstall` (spec §22): removes the Instance and what setup put on the
//! machine, in the order of the guide's steps by hand, and nothing that isn't the
//! Instance's. Every step looks before it acts, so a run again after a failure finishes
//! the job and says what was already gone.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::net::Ipv4Addr;
use std::path::Path;
use std::process::Command;

use crate::answers::Prompt;
use crate::backup::{self, BACKUPS_FOLDER, Backup};
use crate::computer_egress;
use crate::docker::Docker;
use crate::instance::InstanceFolder;
use crate::lemond;
use crate::overrides;
use crate::platform::Platform;
use crate::resolve::COMPUTER_IMAGE;
use crate::restore;
use crate::settings::Settings;
use crate::tailscale;
use crate::update;

/// Where `install.sh` puts the program, relative to HOME (spec §18).
const PROGRAM: &str = ".local/bin/rakazo-stack";
/// Rakazo v0.16.0's supervisor labels each bot-computer network with the Rakazo it's for.
const COMPUTER_OWNER_LABEL: &str = "rakazo.computerOwner";
/// On the probe network every Rakazo supervisor on the engine shares (`rakazo-cprobe`).
const PROBE_LABEL: &str = "rakazo.egressProbe";
/// The firewall's rules match every bridge named `rakazo-c+` (spec §12).
const COMPUTER_BRIDGE_PREFIX: &str = "rakazo-c";

pub struct Options {
    /// `--confirm <name>`: the Instance's name, in place of typing it.
    pub confirm: Option<String>,
    /// `--delete-images`: yes to the images question, which a run with `--confirm` skips.
    pub delete_images: bool,
}

/// Why a run stopped, and whether it had started removing.
#[derive(Debug)]
pub struct UninstallError {
    pub message: String,
    pub removing: bool,
}

impl UninstallError {
    fn before(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            removing: false,
        }
    }
}

pub struct Uninstall<'a> {
    pub platform: Platform,
    pub docker: &'a Docker,
    pub folder: &'a InstanceFolder,
    pub home: &'a Path,
}

/// What uninstall reads before anything changes.
struct Survey {
    images: Vec<Image>,
    /// The joined Tailscale device's name and address, which outlive its container.
    device: Option<(String, Option<Ipv4Addr>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Image {
    id: String,
    size: u64,
}

/// One network on the engine, from `docker network inspect`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Network {
    name: String,
    bridge: String,
    owner: String,
    probe: bool,
}

impl Uninstall<'_> {
    /// The caller holds the lock, and has found an Instance folder.
    pub fn run(
        &self,
        options: &Options,
        env: &dyn Fn(&str) -> Option<String>,
        terminal: Option<&mut (dyn Prompt + '_)>,
    ) -> Result<(), UninstallError> {
        let project = overrides::compose_project();
        if let Some(name) = &options.confirm {
            confirm(Some(name), &project).map_err(UninstallError::before)?;
        } else if terminal.is_none() {
            return Err(UninstallError::before(format!(
                "uninstall asks you to type the Instance's name, which needs a terminal. Without one, pass `--confirm {project}`."
            )));
        }
        let settings = self.settings();
        let stack = self.folder.file(".env").exists();
        if !stack
            && self
                .project_present(&project)
                .map_err(UninstallError::before)?
        {
            return Err(UninstallError::before(format!(
                "~/rakazo-stack/.env is gone, but the Compose project {project} is still on this Docker engine, so nothing was removed. Run `rakazo-stack setup` to write .env again, then `rakazo-stack uninstall`."
            )));
        }
        let survey = self
            .survey(&project, settings.as_ref(), stack)
            .map_err(UninstallError::before)?;

        let mut delete_images = options.delete_images;
        let mut taken = None;
        if let (None, Some(prompt)) = (&options.confirm, terminal) {
            println!(
                "uninstall removes the Instance and all its data: its containers, bot computers, volumes and built images, ~/rakazo-stack and ~/{PROGRAM}. Backups stay."
            );
            let backup_first = yes(prompt, "Take a Backup first?", true);
            if !delete_images && !survey.images.is_empty() {
                delete_images = yes(prompt, &images_question(&survey.images), false);
            }
            let typed = prompt.line(&format!(
                "Type the Instance's name, {project}, to remove it: "
            ));
            confirm(typed.as_deref(), &project).map_err(UninstallError::before)?;
            if backup_first {
                taken = Some(self.backup(env, prompt).map_err(UninstallError::before)?);
            }
        }

        self.remove(&project, &survey, delete_images, stack)
            .map_err(|message| UninstallError {
                message,
                removing: true,
            })?;
        println!("The Instance is removed.");
        match taken {
            Some(path) => println!("Its Backup is {}.", path.display()),
            None => println!(
                "Backups stay where they were written: ~/{BACKUPS_FOLDER}, or the folder you gave `--output`."
            ),
        }
        if let Some((name, ipv4)) = &survey.device {
            println!("{}", tailscale::remove_device_step(name, *ipv4));
        }
        Ok(())
    }

    fn settings(&self) -> Option<Settings> {
        let bytes = self.folder.read("settings.json").ok().flatten()?;
        Settings::parse(&String::from_utf8_lossy(&bytes)).ok()
    }

    /// Compose looks for a compose file in parent folders too, so it runs only while the
    /// Instance's stack files are there.
    fn survey(
        &self,
        project: &str,
        settings: Option<&Settings>,
        stack: bool,
    ) -> Result<Survey, String> {
        let mut ours: BTreeSet<String> = self.project_containers(project)?.into_iter().collect();
        ours.extend(self.bot_computers(project)?);
        let everyone = self.ids(&["ps", "-a", "-q", "--no-trunc"])?;
        let others: Vec<String> = everyone
            .into_iter()
            .filter(|id| !ours.contains(id))
            .collect();
        let mut used = self.images_of(&ours.into_iter().collect::<Vec<_>>())?;
        if let Some(settings) = settings {
            let computer = format!(
                "ghcr.io/{COMPUTER_IMAGE}@{}",
                settings.components.rakazo.images.computer
            );
            if let Ok(id) = self.docker(&["image", "inspect", "--format", "{{.Id}}", &computer]) {
                used.insert(id.trim().to_string());
            }
        }
        let others = self.images_of(&others)?;
        let candidates: Vec<&String> = used.difference(&others).collect();
        let images = if candidates.is_empty() {
            Vec::new()
        } else {
            let mut args = vec![
                "image",
                "inspect",
                "--format",
                "{{.Id}}\t{{.Size}}\t{{json .RepoTags}}",
            ];
            args.extend(candidates.iter().map(|id| id.as_str()));
            pulled_images(&self.docker(&args)?)
        };

        let device = settings
            .and_then(|settings| settings.tailscale_dns_name.as_deref())
            .map(|dns_name| {
                let ipv4 = stack
                    .then(|| {
                        self.compose(&[
                            "exec",
                            "-T",
                            tailscale::SERVICE,
                            "tailscale",
                            "status",
                            "--json",
                            "--peers=false",
                        ])
                        .ok()
                    })
                    .flatten()
                    .and_then(|json| tailscale::device_ipv4(&json));
                (tailscale::device_label(dns_name).to_string(), ipv4)
            });
        Ok(Survey { images, device })
    }

    fn backup(
        &self,
        env: &dyn Fn(&str) -> Option<String>,
        prompt: &mut (dyn Prompt + '_),
    ) -> Result<std::path::PathBuf, String> {
        let passphrase = backup::passphrase(env, Some(prompt)).map_err(|err| err.0)?;
        let taken = Backup {
            docker: self.docker,
            folder: self.folder,
            home: self.home,
            output: None,
            keep_rakazo_stopped: false,
        }
        .run(&passphrase)
        .map_err(|err| format!("the Backup failed, so nothing was removed: {}", err.0))?;
        Ok(taken.path)
    }

    /// The guide's steps, in its order; then the images, the folder and the program.
    fn remove(
        &self,
        project: &str,
        survey: &Survey,
        delete_images: bool,
        stack: bool,
    ) -> Result<(), String> {
        let owner = computer_owner(project);
        let supervisors = self.other_supervisors(project)?;
        let needed_elsewhere = needed_elsewhere(&owner, &supervisors, &self.networks()?);

        match (&needed_elsewhere, stack) {
            (Some(reason), _) => println!(
                "The bot-computer firewall's rules stay, because {reason}: its bot computers rely on the same rules."
            ),
            (None, true) => {
                self.compose(&["stop", computer_egress::SERVICE])
                    .and_then(|_| {
                        self.compose(&["run", "--rm", "-T", computer_egress::SERVICE, "--remove"])
                    })
                    .map_err(|err| format!("removing the bot-computer firewall's rules: {err}"))?;
                println!("Removed the bot-computer firewall's rules.");
            }
            (None, false) => println!(
                "The computer-egress sidecar is already gone with the Compose project, so the firewall's rules are as an earlier run left them."
            ),
        }

        let computers = self.bot_computers(project)?;
        if computers.is_empty() {
            println!("No bot computers left.");
        } else {
            let mut args = vec!["rm", "--force", "--volumes"];
            args.extend(computers.iter().map(String::as_str));
            self.docker(&args)
                .map_err(|err| format!("removing the bot computers: {err}"))?;
            println!("Removed {}.", count(computers.len(), "bot computer"));
        }

        let present = self.project_present(project)?;
        if stack {
            self.compose(&["--profile", "*", "down", "--volumes", "--rmi", "local"])
                .map_err(|err| format!("removing the Compose project: {err}"))?;
        }
        if present {
            println!(
                "Removed the Compose project {project}: its containers, volumes, networks and built images."
            );
        } else {
            println!("The Compose project {project} is already gone.");
        }

        let networks = self.networks()?;
        let mut gone: Vec<&str> = networks
            .iter()
            .filter(|network| network.owner == owner)
            .map(|network| network.name.as_str())
            .collect();
        if needed_elsewhere.is_none() {
            gone.extend(
                networks
                    .iter()
                    .filter(|network| network.probe)
                    .map(|network| network.name.as_str()),
            );
        }
        if gone.is_empty() {
            println!("No bot-computer networks left.");
        } else {
            let mut args = vec!["network", "rm"];
            args.extend(gone.iter().copied());
            self.docker(&args)
                .map_err(|err| format!("removing the bot-computer networks: {err}"))?;
            println!("Removed {}.", count(gone.len(), "bot-computer network"));
        }

        if self.platform == Platform::MacOs {
            self.remove_launch_agent(project)?;
        }

        self.images(&survey.images, delete_images)?;

        match fs::remove_dir_all(self.folder.path()) {
            Ok(()) => println!("Removed ~/rakazo-stack."),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                println!("~/rakazo-stack is already gone.");
            }
            Err(err) => return Err(format!("removing ~/rakazo-stack: {err}")),
        }
        match fs::remove_file(self.home.join(PROGRAM)) {
            Ok(()) => println!("Removed ~/{PROGRAM}."),
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                println!("~/{PROGRAM} is already gone.");
            }
            Err(err) => {
                return Err(format!(
                    "removing ~/{PROGRAM}: {err}. Everything else is gone; delete it by hand."
                ));
            }
        }
        Ok(())
    }

    /// lemond runs from the Instance folder, so it stops before the folder goes.
    fn remove_launch_agent(&self, project: &str) -> Result<(), String> {
        let uid = rustix::process::getuid().as_raw();
        let layout = lemond::Layout::new(self.folder.path(), project);
        let loaded = lemond::launchctl_print(uid, &layout.label).is_some();
        if loaded {
            let out = Command::new("launchctl")
                .args(["bootout", &lemond::service(uid, &layout.label)])
                .output()
                .map_err(|err| format!("unloading the lemond LaunchAgent: {err}"))?;
            backup::checked("launchctl", &["bootout"], out)
                .map_err(|err| format!("unloading the lemond LaunchAgent: {}", err.0))?;
        }
        let plist = layout.plist();
        let removed = match fs::remove_file(&plist) {
            Ok(()) => true,
            Err(err) if err.kind() == io::ErrorKind::NotFound => false,
            Err(err) => return Err(format!("removing {}: {err}", plist.display())),
        };
        if loaded || removed {
            println!("Unloaded voice's LaunchAgent and removed its plist.");
        } else {
            println!("Voice's LaunchAgent is already gone.");
        }
        Ok(())
    }

    /// Deletes the offered images only when the Self-hoster said yes, and never one a
    /// container uses now, whoever's it is.
    fn images(&self, offered: &[Image], delete: bool) -> Result<(), String> {
        if offered.is_empty() {
            return Ok(());
        }
        if !delete {
            println!(
                "Kept {} ({}) the Instance downloaded; `docker image ls` lists them.",
                count(offered.len(), "image"),
                gigabytes(offered.iter().map(|image| image.size).sum())
            );
            return Ok(());
        }
        let everyone = self.ids(&["ps", "-a", "-q", "--no-trunc"])?;
        let used = self.images_of(&everyone)?;
        let (mut deleted, mut freed) = (0, 0);
        for image in offered.iter().filter(|image| !used.contains(&image.id)) {
            if update::remove_pulled_image(self.docker, &image.id)
                .map_err(|err| format!("deleting the images: {err}"))?
            {
                deleted += 1;
                freed += image.size;
            }
        }
        println!(
            "Deleted {} ({}).",
            count(deleted, "image"),
            gigabytes(freed)
        );
        if deleted < offered.len() {
            println!(
                "Kept {}: another container uses it now, or it's gone already.",
                count(offered.len() - deleted, "image")
            );
        }
        Ok(())
    }

    fn project_containers(&self, project: &str) -> Result<Vec<String>, String> {
        let filter = format!("label=com.docker.compose.project={project}");
        self.ids(&["ps", "-a", "-q", "--no-trunc", "--filter", &filter])
    }

    /// Rakazo starts these outside Compose; the Instance's mount its `appdata` volume.
    fn bot_computers(&self, project: &str) -> Result<Vec<String>, String> {
        let appdata = format!("volume={}", backup::volume(project, "appdata"));
        self.ids(&[
            "ps",
            "-a",
            "-q",
            "--no-trunc",
            "--filter",
            "label=rakazo.managed=true",
            "--filter",
            &appdata,
        ])
    }

    fn project_present(&self, project: &str) -> Result<bool, String> {
        Ok(!self.project_containers(project)?.is_empty() || !self.volumes(project)?.is_empty())
    }

    fn volumes(&self, project: &str) -> Result<Vec<String>, String> {
        let listing = self.docker(&[
            "volume",
            "ls",
            "--format",
            "{{.Name}}\t{{.Label \"com.docker.compose.project\"}}",
        ])?;
        Ok(restore::instance_volumes(&listing, project))
    }

    /// Other Compose projects with Rakazo's `supervisor` running.
    fn other_supervisors(&self, project: &str) -> Result<Vec<String>, String> {
        let listing = self.docker(&[
            "ps",
            "--filter",
            "label=com.docker.compose.service=supervisor",
            "--format",
            "{{.Label \"com.docker.compose.project\"}}",
        ])?;
        let mut others: Vec<String> = listing
            .lines()
            .map(str::trim)
            .filter(|name| *name != project)
            .map(str::to_string)
            .collect();
        others.sort_unstable();
        others.dedup();
        Ok(others)
    }

    fn networks(&self) -> Result<Vec<Network>, String> {
        let ids = self.ids(&["network", "ls", "-q", "--no-trunc"])?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let format = format!(
            "{{{{.Name}}}}\t{{{{index .Options \"com.docker.network.bridge.name\"}}}}\t{{{{index .Labels \"{COMPUTER_OWNER_LABEL}\"}}}}\t{{{{index .Labels \"{PROBE_LABEL}\"}}}}"
        );
        let mut args = vec!["network", "inspect", "--format", format.as_str()];
        args.extend(ids.iter().map(String::as_str));
        Ok(parse_networks(&self.docker(&args)?))
    }

    /// The image each container runs.
    fn images_of(&self, containers: &[String]) -> Result<BTreeSet<String>, String> {
        if containers.is_empty() {
            return Ok(BTreeSet::new());
        }
        let mut args = vec!["inspect", "--format", "{{.Image}}"];
        args.extend(containers.iter().map(String::as_str));
        Ok(self
            .docker(&args)?
            .split_whitespace()
            .map(str::to_string)
            .collect())
    }

    fn ids(&self, args: &[&str]) -> Result<Vec<String>, String> {
        Ok(self
            .docker(args)?
            .split_whitespace()
            .map(str::to_string)
            .collect())
    }

    fn compose(&self, args: &[&str]) -> Result<String, String> {
        let out = self
            .docker
            .compose(self.folder.path(), args)
            .output()
            .map_err(|err| err.to_string())?;
        backup::checked("docker compose", args, out).map_err(|err| err.0)
    }

    fn docker(&self, args: &[&str]) -> Result<String, String> {
        let out = self.docker.output(args).map_err(|err| err.to_string())?;
        backup::checked("docker", args, out).map_err(|err| err.0)
    }
}

/// The owner label Rakazo v0.16.0 puts on its bot-computer networks: SHA-256 of
/// `volume:<name>` for the volume at the supervisor's `/data`, here `appdata`, cut to
/// 32 hex characters (`computerNetworkOwnerFor` in its `computer-spec.ts`).
fn computer_owner(project: &str) -> String {
    let root = format!("volume:{}", backup::volume(project, "appdata"));
    ring::digest::digest(&ring::digest::SHA256, root.as_bytes())
        .as_ref()
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Why the firewall's rules must stay: another Compose project runs Rakazo's supervisor,
/// or a `rakazo-c` bridge on the engine is another Rakazo's. The shared probe network
/// is no one Rakazo's.
fn needed_elsewhere(owner: &str, supervisors: &[String], networks: &[Network]) -> Option<String> {
    if !supervisors.is_empty() {
        return Some(format!(
            "another Compose project on this Docker engine runs Rakazo's supervisor ({})",
            supervisors.join(", ")
        ));
    }
    let foreign: Vec<&str> = networks
        .iter()
        .filter(|network| {
            network.bridge.starts_with(COMPUTER_BRIDGE_PREFIX)
                && !network.probe
                && network.owner != owner
        })
        .map(|network| network.name.as_str())
        .collect();
    (!foreign.is_empty()).then(|| {
        format!(
            "another Rakazo's bot-computer networks are on this Docker engine ({})",
            foreign.join(", ")
        )
    })
}

/// Lines of `name<TAB>bridge<TAB>owner<TAB>probe`.
fn parse_networks(listing: &str) -> Vec<Network> {
    listing
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t').map(|field| {
                let field = field.trim();
                if field == "<no value>" { "" } else { field }
            });
            let name = fields.next().filter(|name| !name.is_empty())?;
            Some(Network {
                name: name.to_string(),
                bridge: fields.next().unwrap_or_default().to_string(),
                owner: fields.next().unwrap_or_default().to_string(),
                probe: fields.next() == Some("true"),
            })
        })
        .collect()
}

/// Lines of `id<TAB>size<TAB>RepoTags as JSON`, keeping the images the program pulled by
/// digest: one that also has a `name:tag` is someone else's too.
fn pulled_images(listing: &str) -> Vec<Image> {
    listing
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let id = fields.next()?.trim();
            let size = fields.next()?.trim().parse().ok()?;
            let tags: Vec<String> = serde_json::from_str(fields.next()?.trim()).ok()?;
            (!id.is_empty() && update::pulled_by_digest(&tags)).then(|| Image {
                id: id.to_string(),
                size,
            })
        })
        .collect()
}

fn images_question(images: &[Image]) -> String {
    format!(
        "Delete the {} ({}) this Instance used that no other container uses?",
        count(images.len(), "image"),
        gigabytes(images.iter().map(|image| image.size).sum())
    )
}

/// The Instance's name, typed or given with `--confirm`.
fn confirm(given: Option<&str>, project: &str) -> Result<(), String> {
    if given.map(str::trim) == Some(project) {
        Ok(())
    } else {
        Err("the name didn't match, so nothing was changed.".into())
    }
}

fn yes(prompt: &mut (dyn Prompt + '_), question: &str, default: bool) -> bool {
    let choices = if default { "[Y/n]" } else { "[y/N]" };
    prompt
        .line(&format!("{question} {choices} "))
        .and_then(|answer| match answer.trim() {
            "" => Some(default),
            answer => restore::yes_or_no(answer),
        })
        .unwrap_or(false)
}

fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct Typed(VecDeque<&'static str>);

    impl Prompt for Typed {
        fn line(&mut self, _question: &str) -> Option<String> {
            self.0.pop_front().map(str::to_string)
        }

        fn secret(&mut self, _question: &str) -> Option<String> {
            self.0.pop_front().map(str::to_string)
        }
    }

    fn network(name: &str, bridge: &str, owner: &str, probe: bool) -> Network {
        Network {
            name: name.into(),
            bridge: bridge.into(),
            owner: owner.into(),
            probe,
        }
    }

    #[test]
    fn the_owner_label_is_rakazos_hash_of_the_appdata_volume() {
        // `printf 'volume:rakazo-stack_appdata' | sha256sum | cut -c1-32`
        assert_eq!(
            computer_owner("rakazo-stack"),
            "93a74eda03ea7aa3451a17f7ff3d179b"
        );
        assert_ne!(
            computer_owner("rkst-test-1"),
            computer_owner("rakazo-stack")
        );
    }

    #[test]
    fn the_rules_stay_while_another_rakazo_relies_on_them() {
        let ours = computer_owner("rakazo-stack");
        let mine = network("rakazo-computer-a-1", "rakazo-c1234567", &ours, false);
        let probe = network("rakazo-egress-probe", "rakazo-cprobe", "", true);
        let plain = network("bridge", "docker0", "", false);
        assert_eq!(
            needed_elsewhere(&ours, &[], &[mine.clone(), probe.clone(), plain.clone()]),
            None
        );

        let reason = needed_elsewhere(&ours, &["rkst-b".into()], &[]).unwrap();
        assert!(
            reason.contains("runs Rakazo's supervisor (rkst-b)"),
            "{reason}"
        );

        let theirs = network("rakazo-computer-b-2", "rakazo-c7654321", "other", false);
        let reason = needed_elsewhere(&ours, &[], &[mine, theirs, probe]).unwrap();
        assert!(reason.contains("(rakazo-computer-b-2)"), "{reason}");

        let unlabelled = network("hand-made", "rakazo-cabc", "", false);
        assert!(needed_elsewhere(&ours, &[], &[unlabelled]).is_some());
    }

    #[test]
    fn reads_networks_with_and_without_options() {
        let listing = "bridge\tdocker0\t\t\n\
            rakazo-egress-probe\trakazo-cprobe\t<no value>\ttrue\n\
            rakazo-computer-a-1\trakazo-c1234567\tabc\t<no value>\n";
        assert_eq!(
            parse_networks(listing),
            vec![
                network("bridge", "docker0", "", false),
                network("rakazo-egress-probe", "rakazo-cprobe", "", true),
                network("rakazo-computer-a-1", "rakazo-c1234567", "abc", false),
            ]
        );
    }

    #[test]
    fn offers_only_images_pulled_by_digest() {
        let listing = "sha256:a\t2000000000\t[]\n\
            sha256:b\t500000000\t[\"ghcr.io/elie222/rakazo/app@sha256:b\"]\n\
            sha256:c\t100\t[\"postgres:16\"]\n";
        let images = pulled_images(listing);
        assert_eq!(
            images.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["sha256:a", "sha256:b"]
        );
        assert_eq!(
            images_question(&images),
            "Delete the 2 images (2.5 GB) this Instance used that no other container uses?"
        );
    }

    #[test]
    fn only_the_exact_name_confirms() {
        assert_eq!(confirm(Some(" rakazo-stack "), "rakazo-stack"), Ok(()));
        for wrong in [Some("rakazo"), Some(""), None] {
            assert_eq!(
                confirm(wrong, "rakazo-stack"),
                Err("the name didn't match, so nothing was changed.".into())
            );
        }
    }

    #[test]
    fn questions_take_their_default_on_enter() {
        let mut typed = Typed(VecDeque::from(["", "", "n", "y", "maybe"]));
        assert!(yes(&mut typed, "Take a Backup first?", true));
        assert!(!yes(&mut typed, "Delete?", false));
        assert!(!yes(&mut typed, "Take a Backup first?", true));
        assert!(yes(&mut typed, "Delete?", false));
        assert!(!yes(&mut typed, "Delete?", false));
        assert!(!yes(&mut typed, "Closed terminal?", true));
    }
}
