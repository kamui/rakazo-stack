//! Voice models on Linux and Windows (spec §7 step 9, §9): configure the running Lemonade,
//! pull and load the models through the relay, and warm them. A failure becomes a
//! Checklist item and setup carries on without connecting voice.

use serde::Deserialize;

use crate::checklist::Checklist;
use crate::lemonade::{SPEECH_MODEL, TRANSCRIPTION_MODEL};

/// The two models are about 0.9 GB and Lemonade's backends add more; the rest is headroom
/// so the pull doesn't fill the disk Docker stores everything on.
pub const MODELS_FREE_DISK: u64 = 3_000_000_000;

pub trait Lemonade {
    /// The stack's Lemonade settings (spec §9), set against the running server.
    fn configure(&self) -> Result<(), String>;
    /// The models already downloaded.
    fn installed(&self) -> Result<Vec<String>, String>;
    /// `progress` gets a short status line now and then while the model downloads.
    fn pull(&self, model: &str, progress: &mut dyn FnMut(&str)) -> Result<(), String>;
    fn load(&self, model: &str) -> Result<(), String>;
    /// One speech and one transcription of it.
    fn warm(&self) -> Result<Warmed, String>;
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Warmed {
    pub speech_ms: u64,
    pub transcription_ms: u64,
}

/// The Checklist item for every voice failure that leaves the Owner without voice.
pub fn not_set_up(reason: &str) -> String {
    let logs = if cfg!(target_os = "macos") {
        "`docker compose logs lemonade-relay` and logs/lemond.log in ~/rakazo-stack show"
    } else {
        "`docker compose logs lemonade lemonade-relay` in ~/rakazo-stack (`docker.exe compose …` on Windows) shows"
    };
    format!(
        "Voice isn't set up: {reason}. Run `rakazo-stack setup` again; if it keeps failing, {logs} why."
    )
}

/// Returns whether voice is ready, so the voice connect call runs only against models
/// that are there.
pub fn prepare_models(
    lemonade: &dyn Lemonade,
    free_disk: impl FnOnce() -> Option<u64>,
    say: &mut dyn FnMut(&str),
    checklist: &mut Checklist,
) -> bool {
    match prepare(lemonade, free_disk, say) {
        Ok(()) => true,
        Err(reason) => {
            checklist.add(not_set_up(&reason));
            false
        }
    }
}

fn prepare(
    lemonade: &dyn Lemonade,
    free_disk: impl FnOnce() -> Option<u64>,
    say: &mut dyn FnMut(&str),
) -> Result<(), String> {
    lemonade
        .configure()
        .map_err(|err| format!("setup couldn't configure Lemonade ({err})"))?;
    let installed = lemonade
        .installed()
        .map_err(|err| format!("setup couldn't reach Lemonade through the relay ({err})"))?;
    let missing: Vec<&str> = [SPEECH_MODEL, TRANSCRIPTION_MODEL]
        .into_iter()
        .filter(|model| !installed.iter().any(|have| have == model))
        .collect();
    if !missing.is_empty()
        && let Some(free) = free_disk().filter(|free| *free < MODELS_FREE_DISK)
    {
        return Err(format!(
            "the voice models need about {:.0} GB free where Docker stores its data, and {:.1} GB is free. Free some space first",
            MODELS_FREE_DISK as f64 / 1e9,
            free as f64 / 1e9
        ));
    }
    for model in &missing {
        say(&format!("Downloading the voice model {model}…"));
        lemonade
            .pull(model, &mut |status| say(&format!("  {model}: {status}")))
            .map_err(|err| format!("the voice model {model} didn't download ({err})"))?;
    }
    for model in [SPEECH_MODEL, TRANSCRIPTION_MODEL] {
        lemonade
            .load(model)
            .map_err(|err| format!("Lemonade couldn't load {model} ({err})"))?;
    }
    let warmed = lemonade
        .warm()
        .map_err(|err| format!("the voice check failed ({err})"))?;
    say(&format!(
        "Voice is ready: speech took {:.1} s and transcription {:.1} s.",
        warmed.speech_ms as f64 / 1e3,
        warmed.transcription_ms as f64 / 1e3
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeLemonade {
        installed: Vec<String>,
        fail: Option<&'static str>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeLemonade {
        fn step(&self, name: String) -> Result<(), String> {
            let failed = self.fail.is_some_and(|step| name.starts_with(step));
            self.calls.borrow_mut().push(name);
            if failed {
                Err("HTTP 500".into())
            } else {
                Ok(())
            }
        }
    }

    impl Lemonade for FakeLemonade {
        fn configure(&self) -> Result<(), String> {
            self.step("configure".into())
        }
        fn installed(&self) -> Result<Vec<String>, String> {
            self.step("installed".into())
                .map(|()| self.installed.clone())
        }
        fn pull(&self, model: &str, progress: &mut dyn FnMut(&str)) -> Result<(), String> {
            progress("file 1 of 1, 50%");
            self.step(format!("pull {model}"))
        }
        fn load(&self, model: &str) -> Result<(), String> {
            self.step(format!("load {model}"))
        }
        fn warm(&self) -> Result<Warmed, String> {
            self.step("warm".into()).map(|()| Warmed {
                speech_ms: 900,
                transcription_ms: 3100,
            })
        }
    }

    fn run(lemonade: &FakeLemonade, free: Option<u64>) -> (bool, Vec<String>, Checklist) {
        let mut said = Vec::new();
        let mut checklist = Checklist::default();
        let ready = prepare_models(
            lemonade,
            || free,
            &mut |line| said.push(line.to_string()),
            &mut checklist,
        );
        (ready, said, checklist)
    }

    #[test]
    fn a_first_run_configures_pulls_loads_and_warms() {
        let lemonade = FakeLemonade::default();
        let (ready, said, checklist) = run(&lemonade, Some(50_000_000_000));
        assert!(ready);
        assert_eq!(
            *lemonade.calls.borrow(),
            [
                "configure",
                "installed",
                "pull kokoro-v1",
                "pull Whisper-Small",
                "load kokoro-v1",
                "load Whisper-Small",
                "warm"
            ]
        );
        assert!(said.contains(&"  kokoro-v1: file 1 of 1, 50%".to_string()));
        assert_eq!(
            said.last().unwrap(),
            "Voice is ready: speech took 0.9 s and transcription 3.1 s."
        );
        assert_eq!(checklist.items().count(), 1, "only the standing item");
    }

    #[test]
    fn a_rerun_with_the_models_there_skips_the_pull_and_the_disk_check() {
        let lemonade = FakeLemonade {
            installed: vec!["Whisper-Small".into(), "kokoro-v1".into()],
            ..Default::default()
        };
        let mut said = Vec::new();
        let ready = prepare_models(
            &lemonade,
            || panic!("checked the disk"),
            &mut |line| said.push(line.to_string()),
            &mut Checklist::default(),
        );
        assert!(ready);
        assert!(
            !lemonade
                .calls
                .borrow()
                .iter()
                .any(|c| c.starts_with("pull"))
        );
    }

    #[test]
    fn low_disk_stops_before_the_pull() {
        let lemonade = FakeLemonade {
            installed: vec!["kokoro-v1".into()],
            ..Default::default()
        };
        let (ready, _, checklist) = run(&lemonade, Some(2_000_000_000));
        assert!(!ready);
        assert_eq!(*lemonade.calls.borrow(), ["configure", "installed"]);
        let item = checklist.items().next().unwrap();
        assert!(item.contains("about 3 GB free"), "{item}");
        assert!(item.contains("2.0 GB is free"), "{item}");
    }

    #[test]
    fn an_unknown_free_disk_does_not_stop_the_pull() {
        let lemonade = FakeLemonade::default();
        assert!(run(&lemonade, None).0);
    }

    #[test]
    fn each_failure_becomes_one_checklist_item() {
        for (step, expected) in [
            ("configure", "couldn't configure Lemonade (HTTP 500)"),
            ("installed", "couldn't reach Lemonade through the relay"),
            ("pull Whisper-Small", "Whisper-Small didn't download"),
            ("load kokoro-v1", "couldn't load kokoro-v1"),
            ("warm", "the voice check failed"),
        ] {
            let lemonade = FakeLemonade {
                fail: Some(step),
                ..Default::default()
            };
            let (ready, _, checklist) = run(&lemonade, None);
            assert!(!ready, "{step}");
            let items: Vec<_> = checklist.items().collect();
            assert_eq!(items.len(), 2, "{step}: {items:?}");
            assert!(items[0].contains(expected), "{step}: {}", items[0]);
            assert_eq!(lemonade.calls.borrow().last().unwrap(), step);
        }
    }
}
