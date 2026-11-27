//! Setup's answers (spec §7, "Questions"), from one of two front ends: the wizard, which
//! asks on a terminal, and the no-questions mode, which reads environment variables and
//! `settings.json`. Later slices add their questions to both.

use std::fmt;
use std::io::{BufRead, IsTerminal, Write};

use crate::access::{AccessError, AccessMode, AuthKey, AuthKeyState};
use crate::model_provider::{Choice, PROVIDER_VAR, Provider};
use crate::secrets::{PastedKey, Secrets};

pub const NO_QUESTIONS_OPTION: &str = "--no-questions";
pub const OWNER_EMAIL_VAR: &str = "RAKAZO_STACK_OWNER_EMAIL";
pub const OWNER_PASSWORD_VAR: &str = "RAKAZO_STACK_OWNER_PASSWORD";
pub const ACCESS_VAR: &str = "RAKAZO_STACK_ACCESS";
pub const DEVICE_NAME_VAR: &str = "RAKAZO_STACK_TAILSCALE_DEVICE_NAME";
pub const AUTH_KEY_VAR: &str = "RAKAZO_STACK_TS_AUTHKEY";
pub const INSTANCE_URL_VAR: &str = "RAKAZO_STACK_INSTANCE_URL";

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
    UnknownProvider,
    InvalidKey(Provider),
    InvalidSavedKey(Provider),
    Access(AccessError),
}

impl From<AccessError> for InputError {
    fn from(err: AccessError) -> Self {
        Self::Access(err)
    }
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
            Self::UnknownProvider => write!(
                f,
                "the Model provider is openrouter, openai, gemini, anthropic or none (set {PROVIDER_VAR} with {NO_QUESTIONS_OPTION})."
            ),
            Self::InvalidKey(provider) => write!(
                f,
                "the {} key has a space, quote or other character no key has.",
                provider.name()
            ),
            Self::InvalidSavedKey(provider) => write!(
                f,
                "the {} line in secrets.env isn't a usable key. Correct or remove it.",
                provider.key_name()
            ),
            Self::Access(err) => err.fmt(f),
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
    pub model_provider: Choice,
    /// The chosen provider's key: a new one, or else the saved one. `None` when it was
    /// left blank, or the choice is "none".
    pub model_key: Option<PastedKey>,
}

/// One of setup's two front ends. Both produce the same [`Answers`].
pub trait FrontEnd {
    fn owner_email(&mut self, recorded: Option<&str>) -> Result<String, InputError>;
    /// The password at step 2. The wizard asks only on a first run, which needs it.
    fn password_now(&mut self, first_run: bool) -> Result<Option<Password>, InputError>;
    /// The password when a database-only setting will be filled in and none was given.
    fn password_for_fill_in(&mut self) -> Result<Option<Password>, InputError>;
    fn model_provider(&mut self, recorded: Option<Choice>) -> Result<Choice, InputError>;
    /// A new key for `provider`; `None` keeps the saved one, if there is one.
    fn model_key(
        &mut self,
        provider: Provider,
        saved: bool,
    ) -> Result<Option<PastedKey>, InputError>;
    /// "Check again" (true) or "Continue without it", when ZDR only is off.
    fn check_zdr_again(&mut self) -> bool;
    /// "… was removed in Rakazo. Add it back? [y/N]"
    fn add_back(&mut self, question: &str) -> bool;
    fn access(&mut self, recorded: AccessMode) -> Result<String, InputError>;
    /// `current` is the recorded name, or the default.
    fn device_name(&mut self, current: &str) -> Result<String, InputError>;
    /// A newly given auth key; `None` keeps what the Instance has.
    fn auth_key(&mut self, state: AuthKeyState) -> Result<Option<AuthKey>, InputError>;
    fn instance_url(&mut self, recorded: Option<&str>) -> Result<String, InputError>;
    /// Whether an Instance URL arrives whether or not it's asked for.
    fn offers_instance_url(&self) -> bool;
    /// Shows the warning and says whether to switch the Instance URL.
    fn confirm_switch(&mut self, warning: &str) -> bool;
}

/// Step 2: stops on invalid input, before anything changes.
pub fn collect(
    front: &mut dyn FrontEnd,
    recorded_email: Option<&str>,
    recorded_provider: Option<Choice>,
    secrets: &Secrets,
    first_run: bool,
) -> Result<Answers, InputError> {
    let owner_email = normalize_email(&front.owner_email(recorded_email)?)?;
    let owner_password = front.password_now(first_run)?;
    if first_run && owner_password.is_none() {
        return Err(InputError::MissingPassword);
    }
    let model_provider = front.model_provider(recorded_provider)?;
    let model_key = match model_provider.0 {
        Some(provider) => model_key(front, provider, secrets)?,
        None => None,
    };
    Ok(Answers {
        owner_email,
        owner_password,
        model_provider,
        model_key,
    })
}

fn model_key(
    front: &mut dyn FrontEnd,
    provider: Provider,
    secrets: &Secrets,
) -> Result<Option<PastedKey>, InputError> {
    let saved = secrets
        .get(provider.key_name())
        .filter(|key| !key.is_empty());
    if let Some(key) = front.model_key(provider, saved.is_some())? {
        return Ok(Some(key));
    }
    saved
        .map(|key| PastedKey::new(key).ok_or(InputError::InvalidSavedKey(provider)))
        .transpose()
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

    fn model_provider(&mut self, recorded: Option<Choice>) -> Result<Choice, InputError> {
        match self.var(PROVIDER_VAR) {
            Some(text) => Choice::parse(&text).ok_or(InputError::UnknownProvider),
            None => Ok(recorded.unwrap_or_default()),
        }
    }

    fn model_key(
        &mut self,
        provider: Provider,
        _saved: bool,
    ) -> Result<Option<PastedKey>, InputError> {
        self.var(&format!("RAKAZO_STACK_{}", provider.key_name()))
            .map(|text| PastedKey::new(&text).ok_or(InputError::InvalidKey(provider)))
            .transpose()
    }

    fn check_zdr_again(&mut self) -> bool {
        false
    }

    fn add_back(&mut self, _question: &str) -> bool {
        false
    }

    fn access(&mut self, recorded: AccessMode) -> Result<String, InputError> {
        Ok(self
            .var(ACCESS_VAR)
            .unwrap_or_else(|| recorded.as_str().to_string()))
    }

    fn device_name(&mut self, current: &str) -> Result<String, InputError> {
        Ok(self
            .var(DEVICE_NAME_VAR)
            .unwrap_or_else(|| current.to_string()))
    }

    fn auth_key(&mut self, _state: AuthKeyState) -> Result<Option<AuthKey>, InputError> {
        let key = self.var(AUTH_KEY_VAR);
        Ok(key.map(|key| AuthKey::parse(&key)).transpose()?)
    }

    fn instance_url(&mut self, recorded: Option<&str>) -> Result<String, InputError> {
        self.var(INSTANCE_URL_VAR)
            .or_else(|| recorded.map(str::to_string))
            .ok_or_else(|| AccessError::MissingUrl.into())
    }

    fn offers_instance_url(&self) -> bool {
        self.var(INSTANCE_URL_VAR).is_some()
    }

    /// A scripted change of choice is already deliberate, so it only warns.
    fn confirm_switch(&mut self, warning: &str) -> bool {
        println!("{warning}");
        true
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
    /// An answer where Enter takes what's shown in brackets.
    fn prefilled(&mut self, question: &str, current: &str) -> Result<String, InputError> {
        let answer = self
            .prompt
            .line(&format!("{question} [{current}]: "))
            .ok_or(InputError::NoAnswer)?;
        Ok(match answer.trim() {
            "" => current.to_string(),
            answer => answer.to_string(),
        })
    }

    fn yes(&mut self, question: &str) -> bool {
        self.prompt
            .line(&format!("{question} [y/N] "))
            .is_some_and(|answer| matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
    }

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

    fn model_provider(&mut self, recorded: Option<Choice>) -> Result<Choice, InputError> {
        let default = recorded.unwrap_or_default();
        let question = format!(
            "Model provider (openrouter, openai, gemini, anthropic or none) [{}]: ",
            default.id()
        );
        let answer = self.prompt.line(&question).ok_or(InputError::NoAnswer)?;
        match answer.trim() {
            "" => Ok(default),
            answer => Choice::parse(answer).ok_or(InputError::UnknownProvider),
        }
    }

    fn model_key(
        &mut self,
        provider: Provider,
        saved: bool,
    ) -> Result<Option<PastedKey>, InputError> {
        let name = provider.name();
        let billing = if provider == Provider::Gemini {
            ", which must be billing-enabled"
        } else {
            ""
        };
        let question = if saved {
            format!("{name} API key{billing}: set, press Enter to keep, or paste a new one: ")
        } else {
            format!("{name} API key{billing} (blank for none): ")
        };
        let answer = self.prompt.secret(&question).ok_or(InputError::NoAnswer)?;
        if answer.trim().is_empty() {
            return Ok(None);
        }
        PastedKey::new(&answer)
            .map(Some)
            .ok_or(InputError::InvalidKey(provider))
    }

    fn check_zdr_again(&mut self) -> bool {
        self.prompt
            .line("Check again (c), or continue without it (Enter)? ")
            .is_some_and(|answer| matches!(answer.trim(), "c" | "C"))
    }

    fn add_back(&mut self, question: &str) -> bool {
        self.yes(question)
    }

    fn access(&mut self, recorded: AccessMode) -> Result<String, InputError> {
        self.prefilled(
            "Access: localhost, tailscale, or bring-your-own (advanced, unsupported)",
            recorded.as_str(),
        )
    }

    fn device_name(&mut self, current: &str) -> Result<String, InputError> {
        self.prefilled("Tailscale device name", current)
    }

    fn auth_key(&mut self, state: AuthKeyState) -> Result<Option<AuthKey>, InputError> {
        let question = match state {
            AuthKeyState::Needed => "Tailscale auth key (not ephemeral): ",
            AuthKeyState::Saved => {
                "Tailscale auth key: set, press Enter to keep, or paste a new one: "
            }
            AuthKeyState::Joined => {
                "Tailscale auth key (this Instance has joined; press Enter, or paste a new one to join again): "
            }
        };
        let answer = self.prompt.secret(question).ok_or(InputError::NoAnswer)?;
        match answer.trim() {
            "" => Ok(None),
            key => Ok(Some(AuthKey::parse(key)?)),
        }
    }

    fn instance_url(&mut self, recorded: Option<&str>) -> Result<String, InputError> {
        match recorded {
            Some(url) => self.prefilled("Instance URL", url),
            None => self
                .prompt
                .line("Instance URL (https://…): ")
                .ok_or(InputError::NoAnswer),
        }
    }

    fn offers_instance_url(&self) -> bool {
        false
    }

    fn confirm_switch(&mut self, warning: &str) -> bool {
        self.yes(&format!("{warning}\nSwitch?"))
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

    /// Reads byte by byte with signals off, so Ctrl-C can't kill the program while the
    /// terminal's echo is off.
    fn secret(&mut self, question: &str) -> Option<String> {
        use rustix::termios::{
            LocalModes, OptionalActions, SpecialCodeIndex, tcgetattr, tcsetattr,
        };
        let stdin = std::io::stdin();
        let saved = tcgetattr(&stdin).ok()?;
        let mut quiet = saved.clone();
        quiet
            .local_modes
            .remove(LocalModes::ECHO | LocalModes::ICANON | LocalModes::ISIG);
        quiet.special_codes[SpecialCodeIndex::VMIN] = 1;
        quiet.special_codes[SpecialCodeIndex::VTIME] = 0;
        tcsetattr(&stdin, OptionalActions::Now, &quiet).ok()?;
        print!("{question}");
        let _ = std::io::stdout().flush();
        let typed = read_secret(stdin.lock());
        let _ = tcsetattr(&stdin, OptionalActions::Now, &saved);
        println!();
        match typed {
            Typed::Line(line) => Some(line),
            Typed::End => None,
            Typed::Interrupted => std::process::exit(130),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Typed {
    Line(String),
    End,
    Interrupted,
}

const CTRL_C: u8 = 0x03;
const CTRL_D: u8 = 0x04;
const BACKSPACE: u8 = 0x08;
const DELETE: u8 = 0x7f;

/// Line editing for a terminal in non-canonical mode, which no longer does it itself.
fn read_secret(input: impl BufRead) -> Typed {
    let mut line = Vec::new();
    for byte in input.bytes() {
        match byte {
            Err(_) => return Typed::End,
            Ok(CTRL_C) => return Typed::Interrupted,
            Ok(CTRL_D) if line.is_empty() => return Typed::End,
            Ok(b'\r' | b'\n') => return Typed::Line(String::from_utf8_lossy(&line).into_owned()),
            Ok(BACKSPACE | DELETE) => while line.pop().is_some_and(|b| b & 0xc0 == 0x80) {},
            Ok(byte) => line.push(byte),
        }
    }
    if line.is_empty() {
        Typed::End
    } else {
        Typed::Line(String::from_utf8_lossy(&line).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const GOOD: &str = "correct-horse-battery";

    #[test]
    fn reads_a_secret_with_line_editing_and_stops_on_ctrl_c() {
        assert_eq!(read_secret(&b"secret\r"[..]), Typed::Line("secret".into()));
        assert_eq!(
            read_secret(&b"sex\x7fcret\n"[..]),
            Typed::Line("secret".into())
        );
        assert_eq!(
            read_secret("caf\u{e9}\x7fe\r".as_bytes()),
            Typed::Line("cafe".into())
        );
        assert_eq!(read_secret(&b"\x7f\x7fok\r"[..]), Typed::Line("ok".into()));
        assert_eq!(read_secret(&b"half\x03typed\r"[..]), Typed::Interrupted);
        assert_eq!(read_secret(&b"\x04"[..]), Typed::End);
        assert_eq!(read_secret(&b""[..]), Typed::End);
    }

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

    /// `collect` with nothing recorded about the Model provider.
    fn collect_owner(
        front: &mut dyn FrontEnd,
        recorded_email: Option<&str>,
        first_run: bool,
    ) -> Result<Answers, InputError> {
        collect(front, recorded_email, None, &Secrets::default(), first_run)
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
        let answers = collect_owner(&mut front, None, true).unwrap();
        assert_eq!(answers.owner_email, "owner@example.com");
        assert_eq!(answers.owner_password.unwrap().expose(), GOOD);
    }

    #[test]
    fn a_first_scripted_run_stops_without_an_email_or_with_a_bad_password() {
        let mut no_email = NoQuestions {
            env: env(&[(OWNER_PASSWORD_VAR, GOOD)]),
        };
        assert_eq!(
            collect_owner(&mut no_email, None, true).unwrap_err(),
            InputError::MissingEmail
        );
        let mut no_password = NoQuestions {
            env: env(&[(OWNER_EMAIL_VAR, "owner@example.com")]),
        };
        assert_eq!(
            collect_owner(&mut no_password, None, true).unwrap_err(),
            InputError::MissingPassword
        );
        let mut short = NoQuestions {
            env: env(&[
                (OWNER_EMAIL_VAR, "owner@example.com"),
                (OWNER_PASSWORD_VAR, "fifteen-chars!!"),
            ]),
        };
        assert_eq!(
            collect_owner(&mut short, None, true).unwrap_err(),
            InputError::ShortPassword
        );
    }

    #[test]
    fn a_scripted_rerun_uses_settings_and_needs_no_password() {
        let mut front = NoQuestions { env: env(&[]) };
        let answers = collect_owner(&mut front, Some("owner@example.com"), false).unwrap();
        assert_eq!(answers.owner_email, "owner@example.com");
        assert!(answers.owner_password.is_none());
        assert!(front.password_for_fill_in().unwrap().is_none());
        assert!(!front.add_back("Add it back?"));

        let mut changed = NoQuestions {
            env: env(&[(OWNER_EMAIL_VAR, "new@example.com")]),
        };
        let answers = collect_owner(&mut changed, Some("owner@example.com"), false).unwrap();
        assert_eq!(answers.owner_email, "new@example.com");
    }

    #[test]
    fn the_wizard_asks_twice_on_a_first_run() {
        let mut wizard = Wizard {
            prompt: Scripted::new(&["owner@example.com", GOOD, GOOD, "", ""]),
        };
        let answers = collect_owner(&mut wizard, None, true).unwrap();
        assert_eq!(answers.owner_password.unwrap().expose(), GOOD);
        assert_eq!(wizard.prompt.asked.len(), 5);

        let mut differ = Wizard {
            prompt: Scripted::new(&["owner@example.com", GOOD, "correct-horse-battery-staple"]),
        };
        assert_eq!(
            collect_owner(&mut differ, None, true).unwrap_err(),
            InputError::PasswordsDiffer
        );
        let mut short = Wizard {
            prompt: Scripted::new(&["owner@example.com", "fifteen-chars!!"]),
        };
        assert_eq!(
            collect_owner(&mut short, None, true).unwrap_err(),
            InputError::ShortPassword
        );
    }

    #[test]
    fn a_wizard_rerun_prefills_the_email_and_asks_no_password() {
        let mut wizard = Wizard {
            prompt: Scripted::new(&["", "", ""]),
        };
        let answers = collect_owner(&mut wizard, Some("owner@example.com"), false).unwrap();
        assert_eq!(answers.owner_email, "owner@example.com");
        assert!(answers.owner_password.is_none());
        assert_eq!(
            wizard.prompt.asked,
            [
                "Owner email [owner@example.com]: ",
                "Model provider (openrouter, openai, gemini, anthropic or none) [openrouter]: ",
                "OpenRouter API key (blank for none): ",
            ]
        );
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

    const SAVED: &str = "ANTHROPIC_API_KEY=placeholder-saved\n";

    fn anthropic() -> Choice {
        Choice(Some(Provider::Anthropic))
    }

    fn model(
        front: &mut dyn FrontEnd,
        recorded: Option<Choice>,
        secrets: &str,
    ) -> Result<(Choice, Option<String>), InputError> {
        let answers = collect(
            front,
            Some("owner@example.com"),
            recorded,
            &Secrets::parse(secrets),
            false,
        )?;
        let key = answers.model_key.map(|key| key.expose().to_string());
        Ok((answers.model_provider, key))
    }

    #[test]
    fn a_scripted_run_takes_the_provider_and_its_key_from_the_environment_first() {
        let mut nothing = NoQuestions { env: env(&[]) };
        assert_eq!(
            model(&mut nothing, None, ""),
            Ok((Choice::default(), None)),
            "OpenRouter is the default, and a blank key is allowed"
        );
        assert_eq!(
            model(&mut nothing, Some(anthropic()), SAVED),
            Ok((anthropic(), Some("placeholder-saved".into())))
        );

        let mut given = NoQuestions {
            env: env(&[
                (PROVIDER_VAR, "anthropic"),
                ("RAKAZO_STACK_ANTHROPIC_API_KEY", "placeholder-new"),
                ("RAKAZO_STACK_OPENROUTER_API_KEY", "placeholder-other"),
            ]),
        };
        assert_eq!(
            model(&mut given, Some(Choice(None)), SAVED),
            Ok((anthropic(), Some("placeholder-new".into())))
        );

        let mut none = NoQuestions {
            env: env(&[(PROVIDER_VAR, "none")]),
        };
        assert_eq!(
            model(&mut none, Some(anthropic()), SAVED),
            Ok((Choice(None), None))
        );
    }

    #[test]
    fn a_bad_provider_or_key_is_invalid_input_that_is_never_quoted() {
        let mut unknown = NoQuestions {
            env: env(&[(PROVIDER_VAR, "placeholder-pasted-here")]),
        };
        let err = model(&mut unknown, None, "").unwrap_err();
        assert_eq!(err, InputError::UnknownProvider);
        assert!(!err.to_string().contains("placeholder-pasted-here"));

        let mut spaced = NoQuestions {
            env: env(&[("RAKAZO_STACK_OPENROUTER_API_KEY", "placeholder with spaces")]),
        };
        let err = model(&mut spaced, None, "").unwrap_err();
        assert_eq!(err, InputError::InvalidKey(Provider::OpenRouter));
        assert!(!err.to_string().contains("placeholder"));

        let mut nothing = NoQuestions { env: env(&[]) };
        assert_eq!(
            model(&mut nothing, None, "OPENROUTER_API_KEY=\"quoted\"\n"),
            Err(InputError::InvalidSavedKey(Provider::OpenRouter))
        );
    }

    #[test]
    fn the_wizard_keeps_a_saved_key_on_enter_and_takes_a_pasted_one() {
        let mut keep = Wizard {
            prompt: Scripted::new(&["", "", ""]),
        };
        assert_eq!(
            model(&mut keep, Some(anthropic()), SAVED),
            Ok((anthropic(), Some("placeholder-saved".into())))
        );
        assert_eq!(
            keep.prompt.asked[2],
            "Anthropic API key: set, press Enter to keep, or paste a new one: "
        );

        let mut paste = Wizard {
            prompt: Scripted::new(&["", "gemini", " placeholder-new "]),
        };
        assert_eq!(
            model(&mut paste, Some(anthropic()), SAVED),
            Ok((
                Choice(Some(Provider::Gemini)),
                Some("placeholder-new".into())
            ))
        );
        assert!(paste.prompt.asked[2].contains("billing-enabled"));

        let mut none = Wizard {
            prompt: Scripted::new(&["", "none"]),
        };
        assert_eq!(
            model(&mut none, Some(anthropic()), SAVED),
            Ok((Choice(None), None))
        );
        assert_eq!(none.prompt.asked.len(), 2, "no key question");

        let mut typo = Wizard {
            prompt: Scripted::new(&["", "open router"]),
        };
        assert_eq!(model(&mut typo, None, ""), Err(InputError::UnknownProvider));
    }
}
