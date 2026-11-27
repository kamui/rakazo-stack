//! Setup's answers (spec §7, "Questions"), from one of two front ends: the wizard, which
//! asks on a terminal, and the no-questions mode, which reads environment variables and
//! `settings.json`. Later slices add their questions to both.

use std::fmt;
use std::io::{BufRead, IsTerminal, Write};

pub const NO_QUESTIONS_OPTION: &str = "--no-questions";
pub const OWNER_EMAIL_VAR: &str = "RAKAZO_STACK_OWNER_EMAIL";
pub const OWNER_PASSWORD_VAR: &str = "RAKAZO_STACK_OWNER_PASSWORD";

/// Rakazo's own bounds, counted in UTF-16 code units as its JavaScript does:
/// `bootstrap-owner` refuses fewer than 16 and sign-in refuses more than 128.
const PASSWORD_MIN: usize = 16;
const PASSWORD_MAX: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    NoTerminal,
    NoAnswer,
    MissingEmail,
    InvalidEmail,
    MissingPassword,
    ShortPassword,
    LongPassword,
    PasswordsDiffer,
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTerminal => write!(
                f,
                "setup asks its questions on a terminal, and there isn't one. For a scripted run, use `rakazo-stack setup {NO_QUESTIONS_OPTION}`."
            ),
            Self::NoAnswer => f.write_str("no answer was given."),
            Self::MissingEmail => write!(
                f,
                "no Owner email. Set {OWNER_EMAIL_VAR} for a first run with {NO_QUESTIONS_OPTION}."
            ),
            Self::InvalidEmail => f.write_str("the Owner email isn't a valid address."),
            Self::MissingPassword => write!(
                f,
                "no Owner password. Set {OWNER_PASSWORD_VAR} for a first run with {NO_QUESTIONS_OPTION}."
            ),
            Self::ShortPassword => write!(
                f,
                "the Owner password needs at least {PASSWORD_MIN} characters."
            ),
            Self::LongPassword => write!(
                f,
                "the Owner password can have at most {PASSWORD_MAX} characters."
            ),
            Self::PasswordsDiffer => f.write_str("the two Owner passwords don't match."),
        }
    }
}

/// The Owner password. It is never saved, logged or printed, so it has no `Display`
/// and its `Debug` hides it.
#[derive(Clone, PartialEq, Eq)]
pub struct Password(String);

impl Password {
    pub fn new(text: String) -> Result<Self, InputError> {
        let units = text.encode_utf16().count();
        if units < PASSWORD_MIN {
            Err(InputError::ShortPassword)
        } else if units > PASSWORD_MAX {
            Err(InputError::LongPassword)
        } else {
            Ok(Self(text))
        }
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(hidden)")
    }
}

/// Rakazo's `bootstrap-owner` trims and lowercases the email before it stores it.
pub fn normalize_email(text: &str) -> Result<String, InputError> {
    let email = text.trim().to_lowercase();
    let valid = email
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'))
        && email.matches('@').count() == 1
        && !email.ends_with('.')
        && !email
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '\\'));
    if valid {
        Ok(email)
    } else {
        Err(InputError::InvalidEmail)
    }
}

#[derive(Debug)]
pub struct Answers {
    pub owner_email: String,
    /// Given at step 2: always on a first run, and in no-questions mode whenever its
    /// variable is set. A wizard re-run asks later, only if something will be filled in.
    pub owner_password: Option<Password>,
}

/// One of setup's two front ends. Both produce the same [`Answers`].
pub trait FrontEnd {
    fn owner_email(&mut self, recorded: Option<&str>) -> Result<String, InputError>;
    /// The password at step 2. The wizard asks only on a first run, which needs it.
    fn password_now(&mut self, first_run: bool) -> Result<Option<Password>, InputError>;
    /// The password when a database-only setting will be filled in and none was given.
    fn password_for_fill_in(&mut self) -> Result<Option<Password>, InputError>;
    /// "… was removed in Rakazo. Add it back? [y/N]"
    fn add_back(&mut self, question: &str) -> bool;
}

/// Step 2: stops on invalid input, before anything changes.
pub fn collect(
    front: &mut dyn FrontEnd,
    recorded_email: Option<&str>,
    first_run: bool,
) -> Result<Answers, InputError> {
    let owner_email = normalize_email(&front.owner_email(recorded_email)?)?;
    let owner_password = front.password_now(first_run)?;
    if first_run && owner_password.is_none() {
        return Err(InputError::MissingPassword);
    }
    Ok(Answers {
        owner_email,
        owner_password,
    })
}

/// Each answer comes from its environment variable, then `settings.json`, then the
/// wizard's default. An answer with no default stops the run when neither gives it.
pub struct NoQuestions<F: Fn(&str) -> Option<String>> {
    pub env: F,
}

impl<F: Fn(&str) -> Option<String>> NoQuestions<F> {
    fn var(&self, name: &str) -> Option<String> {
        (self.env)(name).filter(|value| !value.is_empty())
    }
}

impl<F: Fn(&str) -> Option<String>> FrontEnd for NoQuestions<F> {
    fn owner_email(&mut self, recorded: Option<&str>) -> Result<String, InputError> {
        self.var(OWNER_EMAIL_VAR)
            .or_else(|| recorded.map(str::to_string))
            .ok_or(InputError::MissingEmail)
    }

    fn password_now(&mut self, _first_run: bool) -> Result<Option<Password>, InputError> {
        self.var(OWNER_PASSWORD_VAR).map(Password::new).transpose()
    }

    fn password_for_fill_in(&mut self) -> Result<Option<Password>, InputError> {
        Ok(None)
    }

    fn add_back(&mut self, _question: &str) -> bool {
        false
    }
}

/// How the wizard reads a line. `secret` doesn't echo.
pub trait Prompt {
    fn line(&mut self, question: &str) -> Option<String>;
    fn secret(&mut self, question: &str) -> Option<String>;
}

pub struct Wizard<P: Prompt> {
    pub prompt: P,
}

impl<P: Prompt> Wizard<P> {
    fn new_password(&mut self) -> Result<Password, InputError> {
        let question = format!("Owner password (at least {PASSWORD_MIN} characters): ");
        let first = self.prompt.secret(&question).ok_or(InputError::NoAnswer)?;
        let password = Password::new(first)?;
        let again = self
            .prompt
            .secret("Owner password again: ")
            .ok_or(InputError::NoAnswer)?;
        if again == password.expose() {
            Ok(password)
        } else {
            Err(InputError::PasswordsDiffer)
        }
    }
}

impl<P: Prompt> FrontEnd for Wizard<P> {
    fn owner_email(&mut self, recorded: Option<&str>) -> Result<String, InputError> {
        let question = match recorded {
            Some(email) => format!("Owner email [{email}]: "),
            None => "Owner email: ".to_string(),
        };
        let answer = self.prompt.line(&question).ok_or(InputError::NoAnswer)?;
        match (answer.trim(), recorded) {
            ("", Some(email)) => Ok(email.to_string()),
            (answer, _) => Ok(answer.to_string()),
        }
    }

    fn password_now(&mut self, first_run: bool) -> Result<Option<Password>, InputError> {
        if first_run {
            self.new_password().map(Some)
        } else {
            Ok(None)
        }
    }

    fn password_for_fill_in(&mut self) -> Result<Option<Password>, InputError> {
        self.new_password().map(Some)
    }

    fn add_back(&mut self, question: &str) -> bool {
        self.prompt
            .line(&format!("{question} [y/N] "))
            .is_some_and(|answer| matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
    }
}

/// The wizard's prompt on the process's own terminal.
pub struct Terminal;

impl Terminal {
    pub fn attached() -> Result<Self, InputError> {
        if std::io::stdin().is_terminal() {
            Ok(Self)
        } else {
            Err(InputError::NoTerminal)
        }
    }

    fn read_line(question: &str) -> Option<String> {
        print!("{question}");
        std::io::stdout().flush().ok()?;
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\r', '\n']).to_string()),
        }
    }
}

impl Prompt for Terminal {
    fn line(&mut self, question: &str) -> Option<String> {
        Self::read_line(question)
    }

    fn secret(&mut self, question: &str) -> Option<String> {
        use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};
        let stdin = std::io::stdin();
        let saved = tcgetattr(&stdin).ok()?;
        let mut quiet = saved.clone();
        quiet.local_modes.remove(LocalModes::ECHO);
        tcsetattr(&stdin, OptionalActions::Now, &quiet).ok()?;
        let answer = Self::read_line(question);
        let _ = tcsetattr(&stdin, OptionalActions::Now, &saved);
        println!();
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const GOOD: &str = "correct-horse-battery";

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    struct Scripted {
        lines: VecDeque<&'static str>,
        asked: Vec<String>,
    }

    impl Scripted {
        fn new(lines: &[&'static str]) -> Self {
            Self {
                lines: lines.iter().copied().collect(),
                asked: Vec::new(),
            }
        }
    }

    impl Prompt for Scripted {
        fn line(&mut self, question: &str) -> Option<String> {
            self.asked.push(question.to_string());
            self.lines.pop_front().map(str::to_string)
        }
        fn secret(&mut self, question: &str) -> Option<String> {
            self.line(question)
        }
    }

    #[test]
    fn passwords_need_16_to_128_utf16_units() {
        assert_eq!(
            Password::new("a".repeat(15)).unwrap_err(),
            InputError::ShortPassword
        );
        assert!(Password::new("a".repeat(16)).is_ok());
        assert!(Password::new("a".repeat(128)).is_ok());
        assert_eq!(
            Password::new("a".repeat(129)).unwrap_err(),
            InputError::LongPassword
        );
        assert!(
            Password::new("🔑".repeat(8)).is_ok(),
            "8 astral chars are 16 units"
        );
        assert_eq!(
            format!("{:?}", Password::new(GOOD.into()).unwrap()),
            "Password(hidden)"
        );
    }

    #[test]
    fn emails_are_normalized_like_bootstrap_owner() {
        assert_eq!(
            normalize_email("  Owner@Example.COM ").unwrap(),
            "owner@example.com"
        );
        for bad in [
            "",
            "owner",
            "@example.com",
            "owner@example",
            "a@b@example.com",
            "o wner@example.com",
            "o'wner@example.com",
        ] {
            assert_eq!(normalize_email(bad), Err(InputError::InvalidEmail), "{bad}");
        }
    }

    #[test]
    fn a_first_scripted_run_takes_its_answers_from_the_environment() {
        let mut front = NoQuestions {
            env: env(&[
                (OWNER_EMAIL_VAR, "owner@example.com"),
                (OWNER_PASSWORD_VAR, GOOD),
            ]),
        };
        let answers = collect(&mut front, None, true).unwrap();
        assert_eq!(answers.owner_email, "owner@example.com");
        assert_eq!(answers.owner_password.unwrap().expose(), GOOD);
    }

    #[test]
    fn a_first_scripted_run_stops_without_an_email_or_with_a_bad_password() {
        let mut no_email = NoQuestions {
            env: env(&[(OWNER_PASSWORD_VAR, GOOD)]),
        };
        assert_eq!(
            collect(&mut no_email, None, true).unwrap_err(),
            InputError::MissingEmail
        );
        let mut no_password = NoQuestions {
            env: env(&[(OWNER_EMAIL_VAR, "owner@example.com")]),
        };
        assert_eq!(
            collect(&mut no_password, None, true).unwrap_err(),
            InputError::MissingPassword
        );
        let mut short = NoQuestions {
            env: env(&[
                (OWNER_EMAIL_VAR, "owner@example.com"),
                (OWNER_PASSWORD_VAR, "fifteen-chars!!"),
            ]),
        };
        assert_eq!(
            collect(&mut short, None, true).unwrap_err(),
            InputError::ShortPassword
        );
    }

    #[test]
    fn a_scripted_rerun_uses_settings_and_needs_no_password() {
        let mut front = NoQuestions { env: env(&[]) };
        let answers = collect(&mut front, Some("owner@example.com"), false).unwrap();
        assert_eq!(answers.owner_email, "owner@example.com");
        assert!(answers.owner_password.is_none());
        assert!(front.password_for_fill_in().unwrap().is_none());
        assert!(!front.add_back("Add it back?"));

        let mut changed = NoQuestions {
            env: env(&[(OWNER_EMAIL_VAR, "new@example.com")]),
        };
        let answers = collect(&mut changed, Some("owner@example.com"), false).unwrap();
        assert_eq!(answers.owner_email, "new@example.com");
    }

    #[test]
    fn the_wizard_asks_twice_on_a_first_run() {
        let mut wizard = Wizard {
            prompt: Scripted::new(&["owner@example.com", GOOD, GOOD]),
        };
        let answers = collect(&mut wizard, None, true).unwrap();
        assert_eq!(answers.owner_password.unwrap().expose(), GOOD);
        assert_eq!(wizard.prompt.asked.len(), 3);

        let mut differ = Wizard {
            prompt: Scripted::new(&["owner@example.com", GOOD, "correct-horse-battery-staple"]),
        };
        assert_eq!(
            collect(&mut differ, None, true).unwrap_err(),
            InputError::PasswordsDiffer
        );
        let mut short = Wizard {
            prompt: Scripted::new(&["owner@example.com", "fifteen-chars!!"]),
        };
        assert_eq!(
            collect(&mut short, None, true).unwrap_err(),
            InputError::ShortPassword
        );
    }

    #[test]
    fn a_wizard_rerun_prefills_the_email_and_asks_no_password() {
        let mut wizard = Wizard {
            prompt: Scripted::new(&[""]),
        };
        let answers = collect(&mut wizard, Some("owner@example.com"), false).unwrap();
        assert_eq!(answers.owner_email, "owner@example.com");
        assert!(answers.owner_password.is_none());
        assert_eq!(wizard.prompt.asked, ["Owner email [owner@example.com]: "]);
    }

    #[test]
    fn the_wizard_adds_back_only_on_yes() {
        let mut wizard = Wizard {
            prompt: Scripted::new(&["", "y"]),
        };
        assert!(!wizard.add_back("Add it back?"));
        assert!(wizard.add_back("Add it back?"));
        assert_eq!(wizard.prompt.asked[0], "Add it back? [y/N] ");
    }
}
