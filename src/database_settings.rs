//! Database-only settings in Rakazo (spec §7): detect what exists, decide what to fill,
//! fill it through Rakazo's own code paths, and record what was filled. The model
//! connection and memory plug in here later.

use serde::{Deserialize, Serialize};

use crate::answers::{FrontEnd, OWNER_PASSWORD_VAR, Password};
use crate::checklist::Checklist;
use crate::lemonade::{RELAY_ORIGIN, RELAY_URL, SPEECH_MODEL, TRANSCRIPTION_MODEL};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseSetting {
    Owner,
    /// The Owner's voice credential, pointing at the relay.
    Voice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Fill,
    /// Setup filled it before and it's gone: someone removed it in Rakazo.
    RemovedInRakazo,
    Leave,
}

pub fn decide(present: bool, filled_before: bool) -> Decision {
    match (present, filled_before) {
        (true, _) => Decision::Leave,
        (false, false) => Decision::Fill,
        (false, true) => Decision::RemovedInRakazo,
    }
}

/// A signed-in session. Its token is a bearer credential: it is never printed, and
/// signing out consumes it.
pub struct Session {
    pub(crate) token: String,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Session(hidden)")
    }
}

/// The running Instance's Rakazo, as the fill-in sequence uses it.
pub trait Rakazo {
    /// Read-only: a verified user with this email and a password (`credential`) account.
    fn owner_exists(&self, email: &str) -> Result<bool, String>;
    /// Read-only: any voice credential for the user with this email.
    fn voice_exists(&self, email: &str) -> Result<bool, String>;
    fn bootstrap_owner(&self, email: &str, password: &Password) -> Result<(), String>;
    fn sign_in(&self, email: &str, password: &Password) -> Result<Session, String>;
    /// `POST /rpc/voice/connect` with the relay, `kokoro-v1` and `Whisper-Small`.
    fn connect_voice(&self, session: &Session) -> Result<(), String>;
    fn sign_out(&self, session: Session) -> Result<(), String>;
}

/// Steps 2–12 of the sequence. Every failure becomes a Checklist item; none stops setup.
/// `voice_ready` says the voice models are there, so voice is a setting to fill.
/// `filled` is what `settings.json` records as filled by setup, and gains what this run fills.
pub fn fill_in(
    rakazo: &dyn Rakazo,
    front: &mut dyn FrontEnd,
    email: &str,
    password: Option<Password>,
    voice_ready: bool,
    filled: &mut Vec<DatabaseSetting>,
    checklist: &mut Checklist,
) {
    let owner_present = match rakazo.owner_exists(email) {
        Ok(present) => present,
        Err(err) => {
            checklist.add(format!(
                "Setup couldn't check Rakazo's database for the Owner account: {err}. Run `rakazo-stack setup` again."
            ));
            return;
        }
    };
    let fill_owner = wanted(
        front,
        decide(owner_present, filled.contains(&DatabaseSetting::Owner)),
        &format!("The Owner account {email} was removed in Rakazo. Add it back?"),
    );
    if !owner_present && !fill_owner {
        return;
    }
    // A new Owner account has no voice yet, whatever an earlier one had.
    let voice = if !voice_ready {
        Decision::Leave
    } else if fill_owner {
        Decision::Fill
    } else {
        match rakazo.voice_exists(email) {
            Ok(present) => decide(present, filled.contains(&DatabaseSetting::Voice)),
            Err(err) => {
                checklist.add(voice_not_connected(&format!(
                    "setup couldn't check Rakazo's database for it ({err})"
                )));
                Decision::Leave
            }
        }
    };
    let fill_voice = wanted(
        front,
        voice,
        "Your voice settings were removed in Rakazo. Add them back?",
    );
    if !fill_owner && !fill_voice {
        return;
    }

    let not_filled = |reason: &str, checklist: &mut Checklist| {
        if fill_owner {
            checklist.add(owner_not_created(reason));
        }
        if fill_voice {
            checklist.add(voice_not_connected(reason));
        }
    };
    let password = match password.map_or_else(|| front.password_for_fill_in(), |p| Ok(Some(p))) {
        Ok(Some(password)) => password,
        Ok(None) => {
            not_filled(
                &format!("no Owner password was given (set {OWNER_PASSWORD_VAR})"),
                checklist,
            );
            return;
        }
        Err(err) => {
            not_filled(&err.to_string(), checklist);
            return;
        }
    };
    if fill_owner {
        if let Err(err) = rakazo.bootstrap_owner(email, &password) {
            not_filled(&err, checklist);
            return;
        }
        record(filled, DatabaseSetting::Owner);
    }

    let session = match rakazo.sign_in(email, &password) {
        Ok(session) => session,
        Err(err) => {
            if fill_owner {
                checklist.add(format!(
                    "Setup created the Owner account {email} but couldn't sign in with it: {err}. Sign in to Rakazo to check the email and password."
                ));
            }
            if fill_voice {
                checklist.add(voice_not_connected(&format!(
                    "setup couldn't sign in as the Owner ({err})"
                )));
            }
            return;
        }
    };
    if fill_voice {
        match rakazo.connect_voice(&session) {
            Ok(()) => {
                record(filled, DatabaseSetting::Voice);
                checklist.add(voice_consent());
            }
            Err(err) => checklist.add(voice_not_connected(&err)),
        }
    }
    if let Err(err) = rakazo.sign_out(session) {
        checklist.add(format!(
            "Setup couldn't sign out of Rakazo: {err}. Its session ends on its own; the Owner can also sign out other sessions in Rakazo's settings."
        ));
    }
}

fn wanted(front: &mut dyn FrontEnd, decision: Decision, question: &str) -> bool {
    match decision {
        Decision::Fill => true,
        Decision::RemovedInRakazo => front.add_back(question),
        Decision::Leave => false,
    }
}

fn record(filled: &mut Vec<DatabaseSetting>, setting: DatabaseSetting) {
    if !filled.contains(&setting) {
        filled.push(setting);
    }
}

fn owner_not_created(reason: &str) -> String {
    format!(
        "The Owner account wasn't created: {reason}. Run `rakazo-stack setup` again; if it keeps failing, create it as Rakazo's docs describe under \"Installations without email\"."
    )
}

/// Rakazo's web app asks each person once before it first sends audio to a voice server
/// (spec §9). It is the person's own answer, so setup only says what the question means.
fn voice_consent() -> String {
    format!(
        "The first time you use voice, Rakazo asks \"Share voice data with Self-hosted ({RELAY_ORIGIN})?\". Choose Allow: that server is this Instance's own Lemonade, so the audio stays on this machine."
    )
}

fn voice_not_connected(reason: &str) -> String {
    format!(
        "Voice wasn't connected for the Owner: {reason}. Run `rakazo-stack setup` again, or in Rakazo open Settings → Voice, pick Self-hosted, and enter the Server URL {RELAY_URL}, the Speech model {SPEECH_MODEL} and the Transcription model {TRANSCRIPTION_MODEL}."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{AccessMode, AuthKey, AuthKeyState};
    use crate::answers::{InputError, Prompt, Wizard};
    use crate::model_provider::{Choice, Provider};
    use crate::secrets::PastedKey;
    use std::cell::RefCell;

    const EMAIL: &str = "owner@example.com";

    fn password() -> Password {
        Password::new("correct-horse-battery".into()).unwrap()
    }

    #[derive(Default)]
    struct FakeRakazo {
        owner: RefCell<bool>,
        voice: RefCell<bool>,
        bootstrap_error: Option<String>,
        sign_in_error: Option<String>,
        connect_error: Option<String>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl Rakazo for FakeRakazo {
        fn owner_exists(&self, _email: &str) -> Result<bool, String> {
            self.calls.borrow_mut().push("detect");
            Ok(*self.owner.borrow())
        }
        fn voice_exists(&self, _email: &str) -> Result<bool, String> {
            self.calls.borrow_mut().push("detect voice");
            Ok(*self.voice.borrow())
        }
        fn connect_voice(&self, session: &Session) -> Result<(), String> {
            assert_eq!(session.token, "placeholder-token");
            self.calls.borrow_mut().push("connect voice");
            match &self.connect_error {
                Some(err) => Err(err.clone()),
                None => {
                    *self.voice.borrow_mut() = true;
                    Ok(())
                }
            }
        }
        fn bootstrap_owner(&self, _email: &str, _password: &Password) -> Result<(), String> {
            self.calls.borrow_mut().push("bootstrap-owner");
            match &self.bootstrap_error {
                Some(err) => Err(err.clone()),
                None => {
                    *self.owner.borrow_mut() = true;
                    Ok(())
                }
            }
        }
        fn sign_in(&self, _email: &str, _password: &Password) -> Result<Session, String> {
            self.calls.borrow_mut().push("sign-in");
            match &self.sign_in_error {
                Some(err) => Err(err.clone()),
                None => Ok(Session {
                    token: "placeholder-token".into(),
                }),
            }
        }
        fn sign_out(&self, session: Session) -> Result<(), String> {
            assert_eq!(session.token, "placeholder-token");
            self.calls.borrow_mut().push("sign-out");
            Ok(())
        }
    }

    /// A front end that must not be asked anything.
    struct Silent;

    impl FrontEnd for Silent {
        fn owner_email(&mut self, _: Option<&str>) -> Result<String, InputError> {
            panic!("asked for the email")
        }
        fn password_now(&mut self, _: bool) -> Result<Option<Password>, InputError> {
            panic!("asked for the password")
        }
        fn password_for_fill_in(&mut self) -> Result<Option<Password>, InputError> {
            panic!("asked for the password")
        }
        fn model_provider(&mut self, _: Option<Choice>) -> Result<Choice, InputError> {
            panic!("asked for the Model provider")
        }
        fn model_key(&mut self, _: Provider, _: bool) -> Result<Option<PastedKey>, InputError> {
            panic!("asked for a key")
        }
        fn check_zdr_again(&mut self) -> bool {
            panic!("asked about ZDR only")
        }
        fn add_back(&mut self, _: &str) -> bool {
            panic!("asked to add back")
        }
        fn access(&mut self, _: AccessMode) -> Result<String, InputError> {
            panic!("asked for the access mode")
        }
        fn device_name(&mut self, _: &str) -> Result<String, InputError> {
            panic!("asked for the device name")
        }
        fn auth_key(&mut self, _: AuthKeyState) -> Result<Option<AuthKey>, InputError> {
            panic!("asked for the auth key")
        }
        fn instance_url(&mut self, _: Option<&str>) -> Result<String, InputError> {
            panic!("asked for the Instance URL")
        }
        fn offers_instance_url(&self) -> bool {
            panic!("asked about the Instance URL")
        }
        fn confirm_switch(&mut self, _: &str) -> bool {
            panic!("asked to switch")
        }
    }

    struct Answers(Vec<&'static str>);

    impl Prompt for Answers {
        fn line(&mut self, _: &str) -> Option<String> {
            (!self.0.is_empty()).then(|| self.0.remove(0).to_string())
        }
        fn secret(&mut self, question: &str) -> Option<String> {
            self.line(question)
        }
    }

    #[test]
    fn decides_from_presence_and_the_record() {
        assert_eq!(decide(true, true), Decision::Leave);
        assert_eq!(decide(true, false), Decision::Leave);
        assert_eq!(decide(false, false), Decision::Fill);
        assert_eq!(decide(false, true), Decision::RemovedInRakazo);
    }

    #[test]
    fn a_first_run_creates_the_owner_signs_in_and_out_and_records_it() {
        let rakazo = FakeRakazo::default();
        let mut filled = Vec::new();
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            Some(password()),
            false,
            &mut filled,
            &mut checklist,
        );
        assert_eq!(
            *rakazo.calls.borrow(),
            ["detect", "bootstrap-owner", "sign-in", "sign-out"]
        );
        assert_eq!(filled, [DatabaseSetting::Owner]);
        assert_eq!(checklist.items().count(), 1, "only the standing item");
    }

    #[test]
    fn an_owner_already_there_needs_no_password_and_no_bootstrap() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner];
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            None,
            false,
            &mut filled,
            &mut checklist,
        );
        assert_eq!(*rakazo.calls.borrow(), ["detect"]);
        assert_eq!(filled, [DatabaseSetting::Owner]);
    }

    #[test]
    fn a_removed_owner_comes_back_only_when_the_wizard_says_yes() {
        let rakazo = FakeRakazo::default();
        let mut filled = vec![DatabaseSetting::Owner];
        let mut checklist = Checklist::default();
        let mut no = Wizard {
            prompt: Answers(vec![""]),
        };
        fill_in(
            &rakazo,
            &mut no,
            EMAIL,
            None,
            false,
            &mut filled,
            &mut checklist,
        );
        assert_eq!(*rakazo.calls.borrow(), ["detect"]);

        let mut yes = Wizard {
            prompt: Answers(vec!["y", "correct-horse-battery", "correct-horse-battery"]),
        };
        fill_in(
            &rakazo,
            &mut yes,
            EMAIL,
            None,
            false,
            &mut filled,
            &mut checklist,
        );
        assert_eq!(
            *rakazo.calls.borrow(),
            ["detect", "detect", "bootstrap-owner", "sign-in", "sign-out"]
        );
    }

    #[test]
    fn a_scripted_run_without_a_password_adds_an_item() {
        let rakazo = FakeRakazo::default();
        let mut filled = Vec::new();
        let mut checklist = Checklist::default();
        let mut front = crate::answers::NoQuestions {
            env: |_: &str| None,
        };
        fill_in(
            &rakazo,
            &mut front,
            EMAIL,
            None,
            false,
            &mut filled,
            &mut checklist,
        );
        assert_eq!(*rakazo.calls.borrow(), ["detect"]);
        assert!(filled.is_empty());
        assert!(
            checklist
                .items()
                .next()
                .unwrap()
                .contains(OWNER_PASSWORD_VAR)
        );
    }

    #[test]
    fn failures_become_checklist_items_and_setup_carries_on() {
        let rakazo = FakeRakazo {
            bootstrap_error: Some("Existing account password does not match".into()),
            ..Default::default()
        };
        let mut filled = Vec::new();
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            Some(password()),
            false,
            &mut filled,
            &mut checklist,
        );
        assert!(filled.is_empty());
        assert!(
            checklist
                .items()
                .next()
                .unwrap()
                .contains("Existing account password does not match")
        );

        let rakazo = FakeRakazo {
            sign_in_error: Some("Invalid email or password".into()),
            ..Default::default()
        };
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            Some(password()),
            false,
            &mut filled,
            &mut checklist,
        );
        assert_eq!(filled, [DatabaseSetting::Owner]);
        assert!(
            checklist
                .items()
                .next()
                .unwrap()
                .contains("Invalid email or password")
        );
    }

    fn run(
        rakazo: &FakeRakazo,
        front: &mut dyn FrontEnd,
        password: Option<Password>,
        filled: &mut Vec<DatabaseSetting>,
    ) -> Vec<String> {
        let mut checklist = Checklist::default();
        fill_in(rakazo, front, EMAIL, password, true, filled, &mut checklist);
        checklist.items().map(str::to_string).collect()
    }

    #[test]
    fn a_first_run_connects_voice_in_the_same_session() {
        let rakazo = FakeRakazo::default();
        let mut filled = Vec::new();
        let items = run(&rakazo, &mut Silent, Some(password()), &mut filled);
        assert_eq!(
            *rakazo.calls.borrow(),
            [
                "detect",
                "bootstrap-owner",
                "sign-in",
                "connect voice",
                "sign-out"
            ]
        );
        assert_eq!(filled, [DatabaseSetting::Owner, DatabaseSetting::Voice]);
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(
            items[0].contains("Share voice data with Self-hosted (http://lemonade-relay:13306)?")
        );
    }

    #[test]
    fn a_rerun_with_voice_present_leaves_it_alone() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            voice: RefCell::new(true),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner, DatabaseSetting::Voice];
        run(&rakazo, &mut Silent, None, &mut filled);
        assert_eq!(*rakazo.calls.borrow(), ["detect", "detect voice"]);
    }

    #[test]
    fn voice_never_set_is_filled_for_an_existing_owner() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner];
        run(&rakazo, &mut Silent, Some(password()), &mut filled);
        assert_eq!(
            *rakazo.calls.borrow(),
            [
                "detect",
                "detect voice",
                "sign-in",
                "connect voice",
                "sign-out"
            ]
        );
        assert_eq!(filled, [DatabaseSetting::Owner, DatabaseSetting::Voice]);
    }

    #[test]
    fn removed_voice_asks_the_wizard_and_stays_removed_without_questions() {
        let removed = || FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let both = || vec![DatabaseSetting::Owner, DatabaseSetting::Voice];

        let rakazo = removed();
        let mut front = crate::answers::NoQuestions {
            env: |_: &str| Some("correct-horse-battery".to_string()),
        };
        let items = run(&rakazo, &mut front, Some(password()), &mut both());
        assert_eq!(*rakazo.calls.borrow(), ["detect", "detect voice"]);
        assert_eq!(items.len(), 1, "{items:?}");

        let rakazo = removed();
        let mut no = Wizard {
            prompt: Answers(vec!["n"]),
        };
        run(&rakazo, &mut no, None, &mut both());
        assert_eq!(*rakazo.calls.borrow(), ["detect", "detect voice"]);

        let rakazo = removed();
        let mut yes = Wizard {
            prompt: Answers(vec!["y", "correct-horse-battery", "correct-horse-battery"]),
        };
        run(&rakazo, &mut yes, None, &mut both());
        assert_eq!(
            *rakazo.calls.borrow(),
            [
                "detect",
                "detect voice",
                "sign-in",
                "connect voice",
                "sign-out"
            ]
        );
    }

    #[test]
    fn voice_not_ready_is_neither_detected_nor_filled() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner];
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            None,
            false,
            &mut filled,
            &mut checklist,
        );
        assert_eq!(*rakazo.calls.borrow(), ["detect"]);
    }

    #[test]
    fn a_failed_voice_connect_becomes_an_item_with_the_ui_steps_and_still_signs_out() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            connect_error: Some("Couldn't reach the speech server (HTTP 400)".into()),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner];
        let items = run(&rakazo, &mut Silent, Some(password()), &mut filled);
        assert_eq!(rakazo.calls.borrow().last().unwrap(), &"sign-out");
        assert_eq!(filled, [DatabaseSetting::Owner]);
        assert!(items[0].contains("Couldn't reach the speech server (HTTP 400)"));
        assert!(items[0].contains("http://lemonade-relay:13306/api/v1"));
        assert!(items[0].contains("Whisper-Small"));
    }

    #[test]
    fn voice_without_a_password_says_to_run_again_with_it() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let mut front = crate::answers::NoQuestions {
            env: |_: &str| None,
        };
        let items = run(&rakazo, &mut front, None, &mut vec![DatabaseSetting::Owner]);
        assert!(items[0].starts_with("Voice wasn't connected"), "{items:?}");
        assert!(items[0].contains(OWNER_PASSWORD_VAR));
    }
}
