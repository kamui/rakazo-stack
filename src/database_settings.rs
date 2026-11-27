//! Database-only settings in Rakazo (spec §7): detect what exists, decide what to fill,
//! fill it through Rakazo's own code paths, and record what was filled. The Owner is the
//! first setting; voice, the model connection and memory plug in here later.

use serde::{Deserialize, Serialize};

use crate::answers::{FrontEnd, OWNER_PASSWORD_VAR, Password};
use crate::checklist::Checklist;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseSetting {
    Owner,
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
    fn bootstrap_owner(&self, email: &str, password: &Password) -> Result<(), String>;
    fn sign_in(&self, email: &str, password: &Password) -> Result<Session, String>;
    fn sign_out(&self, session: Session) -> Result<(), String>;
}

/// Steps 2–12 of the sequence. Every failure becomes a Checklist item; none stops setup.
/// `filled` is what `settings.json` records as filled by setup, and gains what this run fills.
pub fn fill_in(
    rakazo: &dyn Rakazo,
    front: &mut dyn FrontEnd,
    email: &str,
    password: Option<Password>,
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
    let fill_owner = match decide(owner_present, filled.contains(&DatabaseSetting::Owner)) {
        Decision::Fill => true,
        Decision::RemovedInRakazo => front.add_back(&format!(
            "The Owner account {email} was removed in Rakazo. Add it back?"
        )),
        Decision::Leave => false,
    };
    if !fill_owner {
        return;
    }

    let password = match password.map_or_else(|| front.password_for_fill_in(), |p| Ok(Some(p))) {
        Ok(Some(password)) => password,
        Ok(None) => {
            checklist.add(owner_not_created(&format!(
                "no Owner password was given (set {OWNER_PASSWORD_VAR})"
            )));
            return;
        }
        Err(err) => {
            checklist.add(owner_not_created(&err.to_string()));
            return;
        }
    };
    if let Err(err) = rakazo.bootstrap_owner(email, &password) {
        checklist.add(owner_not_created(&err));
        return;
    }
    if !filled.contains(&DatabaseSetting::Owner) {
        filled.push(DatabaseSetting::Owner);
    }

    match rakazo.sign_in(email, &password) {
        Ok(session) => {
            if let Err(err) = rakazo.sign_out(session) {
                checklist.add(format!(
                    "Setup couldn't sign out of Rakazo: {err}. Its session ends on its own; the Owner can also sign out other sessions in Rakazo's settings."
                ));
            }
        }
        Err(err) => checklist.add(format!(
            "Setup created the Owner account {email} but couldn't sign in with it: {err}. Sign in to Rakazo to check the email and password."
        )),
    }
}

fn owner_not_created(reason: &str) -> String {
    format!(
        "The Owner account wasn't created: {reason}. Run `rakazo-stack setup` again; if it keeps failing, create it as Rakazo's docs describe under \"Installations without email\"."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
        bootstrap_error: Option<String>,
        sign_in_error: Option<String>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl Rakazo for FakeRakazo {
        fn owner_exists(&self, _email: &str) -> Result<bool, String> {
            self.calls.borrow_mut().push("detect");
            Ok(*self.owner.borrow())
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
        fill_in(&rakazo, &mut no, EMAIL, None, &mut filled, &mut checklist);
        assert_eq!(*rakazo.calls.borrow(), ["detect"]);

        let mut yes = Wizard {
            prompt: Answers(vec!["y", "correct-horse-battery", "correct-horse-battery"]),
        };
        fill_in(&rakazo, &mut yes, EMAIL, None, &mut filled, &mut checklist);
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
}
