//! The access mode and the Instance URL that follows from it (spec §11): the answers,
//! their validation, the switch warning and Rakazo's address settings.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::answers::{
    ACCESS_VAR, AUTH_KEY_VAR, FrontEnd, INSTANCE_URL_VAR, InputError, NO_QUESTIONS_OPTION,
};
use crate::secrets::PastedKey;
use crate::settings::Settings;

pub const DEFAULT_DEVICE_NAME: &str = "rakazo";
/// The auth key's name in `secrets.env` and `.env`, which is the container's own.
pub const AUTH_KEY_NAME: &str = "TS_AUTHKEY";
pub const SWITCH_WARNING: &str = "Changing the Instance URL signs everyone out, breaks passkeys made on the old address, makes phones sign in again and leaves old email links stale. MCP servers connected with OAuth may need adding again. No data is lost.";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessMode {
    #[default]
    Localhost,
    Tailscale,
    BringYourOwn,
}

impl AccessMode {
    pub fn parse(text: &str) -> Result<Self, AccessError> {
        match text.trim().to_ascii_lowercase().as_str() {
            "localhost" => Ok(Self::Localhost),
            "tailscale" => Ok(Self::Tailscale),
            "bring-your-own" => Ok(Self::BringYourOwn),
            _ => Err(AccessError::UnknownMode),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Localhost => "localhost",
            Self::Tailscale => "tailscale",
            Self::BringYourOwn => "bring-your-own",
        }
    }
}

/// No message quotes what was given: a value in the wrong variable could be a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessError {
    UnknownMode,
    BadDeviceName,
    MissingAuthKey,
    NotAnAuthKey,
    EphemeralKey,
    MissingUrl,
    BadUrl,
    UrlWithoutBringYourOwn,
    SwitchDeclined,
}

impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMode => {
                f.write_str("the access mode is `localhost`, `tailscale` or `bring-your-own`.")
            }
            Self::BadDeviceName => f.write_str(
                "the Tailscale device name takes up to 63 letters, digits and hyphens, and can't start or end with a hyphen.",
            ),
            Self::MissingAuthKey => write!(
                f,
                "joining your tailnet needs a Tailscale auth key that isn't ephemeral. With {NO_QUESTIONS_OPTION}, set {AUTH_KEY_VAR}."
            ),
            Self::NotAnAuthKey => f.write_str(
                "that isn't a Tailscale auth key. Auth keys start with `tskey-auth-`.",
            ),
            Self::EphemeralKey => f.write_str(
                "that key makes ephemeral devices, which Tailscale removes once they go offline. OAuth client secrets do so too. Create an auth key that isn't ephemeral.",
            ),
            Self::MissingUrl => write!(
                f,
                "no bring-your-own Instance URL. With {NO_QUESTIONS_OPTION}, set {INSTANCE_URL_VAR}."
            ),
            Self::BadUrl => f.write_str(
                "the bring-your-own Instance URL must look like `https://rakazo.example.com`, with an optional port and no path.",
            ),
            Self::UrlWithoutBringYourOwn => write!(
                f,
                "{INSTANCE_URL_VAR} is set, but the access mode isn't `bring-your-own`. A bring-your-own Instance URL can't be combined with Tailscale: set {ACCESS_VAR}=bring-your-own, or unset {INSTANCE_URL_VAR}."
            ),
            Self::SwitchDeclined => {
                f.write_str("the Instance URL stays as it is, and nothing was changed.")
            }
        }
    }
}

/// A single DNS label, lowercased as Tailscale does in device names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceName(String);

impl DeviceName {
    pub fn parse(text: &str) -> Result<Self, AccessError> {
        let name = text.trim().to_ascii_lowercase();
        if is_label(&name) {
            Ok(Self(name))
        } else {
            Err(AccessError::BadDeviceName)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_label(label: &str) -> bool {
    (1..=63).contains(&label.len())
        && label
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !label.starts_with('-')
        && !label.ends_with('-')
}

pub fn is_hostname(host: &str) -> bool {
    host.len() <= 253 && host.split('.').all(is_label)
}

/// A pasted key that is also shaped like a Tailscale auth key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthKey(PastedKey);

impl AuthKey {
    /// Whether an auth key is ephemeral is known only to Tailscale's servers, so this
    /// refuses the forms that say so themselves, and anything that isn't an auth key.
    pub fn parse(text: &str) -> Result<Self, AccessError> {
        let key = text.trim();
        if key.starts_with("tskey-client-") || key.contains("ephemeral=true") {
            return Err(AccessError::EphemeralKey);
        }
        let shaped = key.strip_prefix("tskey-auth-").is_some_and(|rest| {
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
        PastedKey::new(key)
            .filter(|_| shaped)
            .map(Self)
            .ok_or(AccessError::NotAnAuthKey)
    }

    pub fn pasted(&self) -> &PastedKey {
        &self.0
    }
}

/// A bring-your-own Instance URL, kept as a browser writes an origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceUrl {
    origin: String,
    host: String,
}

impl InstanceUrl {
    pub fn parse(text: &str) -> Result<Self, AccessError> {
        let url = text.trim().to_ascii_lowercase();
        let rest = url.strip_prefix("https://").ok_or(AccessError::BadUrl)?;
        let authority = rest.strip_suffix('/').unwrap_or(rest);
        let (host, port) = match authority.split_once(':') {
            Some((host, port)) => {
                let digits = port.chars().all(|c| c.is_ascii_digit());
                let port = port.parse::<u16>().ok().filter(|port| digits && *port != 0);
                (host, Some(port.ok_or(AccessError::BadUrl)?))
            }
            None => (authority, None),
        };
        if !is_hostname(host) {
            return Err(AccessError::BadUrl);
        }
        let origin = match port {
            // Browsers leave the default port out of the `Origin` Rakazo compares with.
            Some(port) if port != 443 => format!("https://{host}:{port}"),
            _ => format!("https://{host}"),
        };
        Ok(Self {
            origin,
            host: host.to_string(),
        })
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    Localhost,
    Tailscale {
        device_name: DeviceName,
        /// `None` once the Instance has joined: its identity is in `tailscale-state`.
        auth_key: Option<AuthKey>,
    },
    BringYourOwn(InstanceUrl),
}

/// What the Instance already has when the auth key is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKeyState {
    Needed,
    Saved,
    Joined,
}

/// Rakazo's four address settings (spec §11).
#[derive(Debug, PartialEq, Eq)]
pub struct Addresses {
    pub better_auth_url: String,
    pub web_origin: String,
    pub api_url: String,
    pub rakazo_host: String,
}

impl Access {
    pub fn mode(&self) -> AccessMode {
        match self {
            Self::Localhost => AccessMode::Localhost,
            Self::Tailscale { .. } => AccessMode::Tailscale,
            Self::BringYourOwn(_) => AccessMode::BringYourOwn,
        }
    }

    /// `tailscale_dns_name` is the name Tailscale assigned; until the Instance has
    /// joined there is none, and the Instance stays on localhost.
    pub fn addresses(&self, tailscale_dns_name: Option<&str>, web_port: u16) -> Addresses {
        let local = format!("http://127.0.0.1:{web_port}");
        let (better_auth_url, origin, host) = match (self, tailscale_dns_name) {
            // `BETTER_AUTH_URL` stays on loopback so that password sign-in works at
            // both addresses: Rakazo trusts no loopback origin otherwise (#24).
            (Self::Tailscale { .. }, Some(name)) => {
                (local, format!("https://{name}"), name.to_string())
            }
            (Self::BringYourOwn(url), _) => {
                (url.origin.clone(), url.origin.clone(), url.host.clone())
            }
            _ => (local.clone(), local, "127.0.0.1".to_string()),
        };
        Addresses {
            better_auth_url,
            web_origin: origin.clone(),
            api_url: origin,
            rakazo_host: host,
        }
    }

    /// Whether going from the recorded answers to these moves the Instance URL.
    pub fn switches_from(&self, recorded: &Settings) -> bool {
        if self.mode() != recorded.access {
            return true;
        }
        match self {
            Self::Localhost => false,
            Self::Tailscale { device_name, .. } => {
                recorded.tailscale_dns_name.is_some()
                    && recorded.tailscale_device_name.as_deref() != Some(device_name.as_str())
            }
            Self::BringYourOwn(url) => recorded.bring_your_own_url.as_deref() != Some(url.origin()),
        }
    }

    /// Records the choice. The other modes' values stay, to pre-fill a later switch back.
    pub fn record(&self, settings: &mut Settings) {
        settings.access = self.mode();
        match self {
            Self::Localhost => {}
            Self::Tailscale { device_name, .. } => {
                settings.tailscale_device_name = Some(device_name.0.clone());
            }
            Self::BringYourOwn(url) => settings.bring_your_own_url = Some(url.origin.clone()),
        }
    }
}

/// The access questions of step 2. Stops on invalid input, or on a declined switch,
/// before anything changes.
pub fn collect(
    front: &mut dyn FrontEnd,
    recorded: Option<&Settings>,
    saved_auth_key: Option<&str>,
) -> Result<Access, InputError> {
    let recorded_mode = recorded.map(|s| s.access).unwrap_or_default();
    let mode = AccessMode::parse(&front.access(recorded_mode)?)?;
    if mode != AccessMode::BringYourOwn && front.offers_instance_url() {
        return Err(AccessError::UrlWithoutBringYourOwn.into());
    }
    let access = match mode {
        AccessMode::Localhost => Access::Localhost,
        AccessMode::Tailscale => {
            let current = recorded.map_or(DEFAULT_DEVICE_NAME, Settings::device_name);
            let device_name = DeviceName::parse(&front.device_name(current)?)?;
            let joined = recorded.is_some_and(|s| s.tailscale_dns_name.is_some());
            let state = match (saved_auth_key, joined) {
                (Some(_), _) => AuthKeyState::Saved,
                (None, true) => AuthKeyState::Joined,
                (None, false) => AuthKeyState::Needed,
            };
            let auth_key = match front.auth_key(state)? {
                Some(key) => Some(key),
                None => saved_auth_key.map(AuthKey::parse).transpose()?,
            };
            if auth_key.is_none() && !joined {
                return Err(AccessError::MissingAuthKey.into());
            }
            Access::Tailscale {
                device_name,
                auth_key,
            }
        }
        AccessMode::BringYourOwn => {
            let recorded_url = recorded.and_then(|s| s.bring_your_own_url.as_deref());
            Access::BringYourOwn(InstanceUrl::parse(&front.instance_url(recorded_url)?)?)
        }
    };
    if recorded.is_some_and(|settings| access.switches_from(settings))
        && !front.confirm_switch(SWITCH_WARNING)
    {
        return Err(AccessError::SwitchDeclined.into());
    }
    Ok(access)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::answers::{DEVICE_NAME_VAR, NoQuestions, Prompt, Wizard};
    use crate::resolve::RakazoRelease;
    use std::collections::VecDeque;

    const KEY: &str = "tskey-auth-kPLACEHOLDER-0000000000";

    fn settings() -> Settings {
        Settings::new(&RakazoRelease {
            tag: "v0.16.0".into(),
            app_digest: "sha256:aaa".into(),
            computer_digest: "sha256:ccc".into(),
        })
    }

    fn joined(name: &str) -> Settings {
        Settings {
            access: AccessMode::Tailscale,
            tailscale_device_name: Some(name.into()),
            tailscale_dns_name: Some(format!("{name}.example.ts.net")),
            ..settings()
        }
    }

    fn tailscale(name: &str) -> Access {
        Access::Tailscale {
            device_name: DeviceName::parse(name).unwrap(),
            auth_key: None,
        }
    }

    fn byo(url: &str) -> Access {
        Access::BringYourOwn(InstanceUrl::parse(url).unwrap())
    }

    fn scripted(pairs: &[(&str, &str)]) -> NoQuestions<impl Fn(&str) -> Option<String>> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        NoQuestions {
            env: move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
            },
        }
    }

    struct Typed {
        lines: VecDeque<&'static str>,
        asked: Vec<String>,
        secrets: Vec<String>,
    }

    impl Prompt for Typed {
        fn line(&mut self, question: &str) -> Option<String> {
            self.asked.push(question.to_string());
            self.lines.pop_front().map(str::to_string)
        }
        fn secret(&mut self, question: &str) -> Option<String> {
            self.secrets.push(question.to_string());
            self.line(question)
        }
    }

    fn wizard(lines: &[&'static str]) -> Wizard<Typed> {
        Wizard {
            prompt: Typed {
                lines: lines.iter().copied().collect(),
                asked: Vec::new(),
                secrets: Vec::new(),
            },
        }
    }

    fn refusal(result: Result<Access, InputError>) -> AccessError {
        match result {
            Err(InputError::Access(err)) => err,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn device_names_are_one_dns_label() {
        assert_eq!(
            DeviceName::parse(" Rakazo-1 ").unwrap().as_str(),
            "rakazo-1"
        );
        assert!(DeviceName::parse(&"a".repeat(63)).is_ok());
        for bad in [
            "",
            "-rakazo",
            "rakazo-",
            "my box",
            "my_box",
            "rakazo.example",
            "rakazö",
            "a\nTS_AUTHKEY=x",
            &"a".repeat(64),
        ] {
            assert_eq!(
                DeviceName::parse(bad),
                Err(AccessError::BadDeviceName),
                "{bad}"
            );
        }
    }

    #[test]
    fn instance_urls_are_https_origins() {
        for (given, origin, host) in [
            (
                "https://rakazo.example.com",
                "https://rakazo.example.com",
                "rakazo.example.com",
            ),
            (
                " HTTPS://Rakazo.Example.com/ ",
                "https://rakazo.example.com",
                "rakazo.example.com",
            ),
            (
                "https://rakazo.example.com:443",
                "https://rakazo.example.com",
                "rakazo.example.com",
            ),
            (
                "https://rakazo.example.com:8443/",
                "https://rakazo.example.com:8443",
                "rakazo.example.com",
            ),
            ("https://192.0.2.7", "https://192.0.2.7", "192.0.2.7"),
        ] {
            let url = InstanceUrl::parse(given).unwrap();
            assert_eq!((url.origin(), url.host.as_str()), (origin, host), "{given}");
        }
        for bad in [
            "",
            "rakazo.example.com",
            "http://rakazo.example.com",
            "https://",
            "https:///",
            "https://rakazo.example.com/app",
            "https://rakazo.example.com//",
            "https://rakazo.example.com?x=1",
            "https://rakazo.example.com#x",
            "https://user@rakazo.example.com",
            "https://rakazo example.com",
            "https://rakazo.example.com:",
            "https://rakazo.example.com:0",
            "https://rakazo.example.com:+80",
            "https://rakazo.example.com:70000",
            "https://rakazo..example.com",
            "https://rakazo.example.com.",
            "https://[2001:db8::1]",
            "https://rakazo.example.com\nSIGNUPS_ENABLED=true",
        ] {
            assert_eq!(InstanceUrl::parse(bad), Err(AccessError::BadUrl), "{bad:?}");
        }
    }

    #[test]
    fn only_a_plain_auth_key_is_taken() {
        assert_eq!(
            AuthKey::parse(&format!(" {KEY} "))
                .unwrap()
                .pasted()
                .expose(),
            KEY
        );
        for ephemeral in [
            "tskey-client-kPLACEHOLDER-0000000000",
            "tskey-auth-kPLACEHOLDER-0000000000?ephemeral=true",
        ] {
            assert_eq!(AuthKey::parse(ephemeral), Err(AccessError::EphemeralKey));
        }
        for bad in [
            "tskey-auth-",
            "tskey-api-kPLACEHOLDER-0000000000",
            "kPLACEHOLDER",
            "tskey-auth-kPLACEHOLDER 0000000000",
            "tskey-auth-kPLACEHOLDER\nSIGNUPS_ENABLED=true",
            "tskey-auth-kPLACEHOLDER$HOME",
        ] {
            assert_eq!(
                AuthKey::parse(bad),
                Err(AccessError::NotAnAuthKey),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn the_auth_key_never_shows() {
        let access = Access::Tailscale {
            device_name: DeviceName::parse("rakazo").unwrap(),
            auth_key: Some(AuthKey::parse(KEY).unwrap()),
        };
        let debug = format!("{access:?}");
        assert!(debug.contains("PastedKey(hidden)"), "{debug}");
        assert!(!debug.contains("PLACEHOLDER"), "{debug}");

        for pasted in [
            "tskey-auth-kPLACEHOLDER!",
            "tskey-client-kPLACEHOLDER",
            "kPLACEHOLDER",
        ] {
            let refused = refusal(collect(
                &mut scripted(&[(ACCESS_VAR, "tailscale"), (AUTH_KEY_VAR, pasted)]),
                None,
                None,
            ));
            let message = format!("{refused} {refused:?}");
            assert!(!message.contains("PLACEHOLDER"), "{message}");
        }
        for var in [ACCESS_VAR, DEVICE_NAME_VAR] {
            let refused = refusal(collect(
                &mut scripted(&[(ACCESS_VAR, "tailscale"), (var, "tskey-auth-kPLACEHOLDER!")]),
                None,
                None,
            ));
            assert!(!refused.to_string().contains("PLACEHOLDER"), "{refused}");
        }
    }

    #[test]
    fn address_settings_follow_the_access_mode() {
        let local = "http://127.0.0.1:5173";
        let on = |access: &Access, dns: Option<&str>| {
            let a = access.addresses(dns, 5173);
            [a.better_auth_url, a.web_origin, a.api_url, a.rakazo_host]
        };
        let localhost = [local, local, local, "127.0.0.1"];
        assert_eq!(on(&Access::Localhost, None), localhost);
        assert_eq!(
            on(&Access::Localhost, Some("rakazo.example.ts.net")),
            localhost,
            "a switched-off tailnet name is ignored"
        );
        assert_eq!(on(&tailscale("rakazo"), None), localhost, "not joined yet");
        assert_eq!(
            on(&tailscale("rakazo"), Some("rakazo-1.example.ts.net")),
            [
                local,
                "https://rakazo-1.example.ts.net",
                "https://rakazo-1.example.ts.net",
                "rakazo-1.example.ts.net"
            ]
        );
        let url = "https://rakazo.example.com:8443";
        assert_eq!(
            on(&byo(url), Some("rakazo.example.ts.net")),
            [url, url, url, "rakazo.example.com"]
        );
    }

    #[test]
    fn a_switch_is_a_change_of_instance_url() {
        let local = settings();
        assert!(!Access::Localhost.switches_from(&local));
        assert!(tailscale("rakazo").switches_from(&local));
        assert!(byo("https://rakazo.example.com").switches_from(&local));

        let on = joined("rakazo-1");
        assert!(Access::Localhost.switches_from(&on));
        assert!(!tailscale("rakazo-1").switches_from(&on));
        assert!(tailscale("other").switches_from(&on));

        let never_joined = Settings {
            tailscale_dns_name: None,
            ..joined("rakazo")
        };
        assert!(
            !tailscale("other").switches_from(&never_joined),
            "its Instance URL is still localhost"
        );
        assert!(Access::Localhost.switches_from(&never_joined));

        let mut own = settings();
        byo("https://rakazo.example.com").record(&mut own);
        assert!(!byo("https://rakazo.example.com:443/").switches_from(&own));
        assert!(byo("https://other.example.com").switches_from(&own));
    }

    #[test]
    fn a_first_scripted_run_defaults_to_localhost() {
        let access = collect(&mut scripted(&[]), None, None).unwrap();
        assert_eq!(access, Access::Localhost);
    }

    #[test]
    fn a_scripted_tailscale_join_needs_a_key() {
        let mut front = scripted(&[(ACCESS_VAR, "tailscale"), (AUTH_KEY_VAR, KEY)]);
        let Access::Tailscale {
            device_name,
            auth_key,
        } = collect(&mut front, None, None).unwrap()
        else {
            panic!("not Tailscale");
        };
        assert_eq!(device_name.as_str(), "rakazo");
        assert_eq!(auth_key.unwrap().pasted().expose(), KEY);

        let mut no_key = scripted(&[(ACCESS_VAR, "tailscale")]);
        assert_eq!(
            refusal(collect(&mut no_key, None, None)),
            AccessError::MissingAuthKey
        );
        assert_eq!(
            refusal(collect(&mut no_key, Some(&settings()), None)),
            AccessError::MissingAuthKey,
            "turning it on for the first time is a first join too"
        );
    }

    #[test]
    fn a_rerun_keeps_the_saved_key_and_a_joined_instance_needs_none() {
        let never_joined = Settings {
            tailscale_dns_name: None,
            ..joined("rakazo")
        };
        let Access::Tailscale { auth_key, .. } =
            collect(&mut scripted(&[]), Some(&never_joined), Some(KEY)).unwrap()
        else {
            panic!("not Tailscale");
        };
        assert_eq!(auth_key.unwrap().pasted().expose(), KEY);

        let access = collect(&mut scripted(&[]), Some(&joined("rakazo-1")), None).unwrap();
        assert_eq!(access, tailscale("rakazo-1"), "the assigned name pre-fills");

        let mut off = joined("rakazo-1");
        off.access = AccessMode::Localhost;
        let mut on_again = scripted(&[(ACCESS_VAR, "tailscale")]);
        assert_eq!(
            collect(&mut on_again, Some(&off), None).unwrap(),
            tailscale("rakazo-1")
        );
    }

    #[test]
    fn a_scripted_bring_your_own_url_is_checked() {
        let mut front = scripted(&[
            (ACCESS_VAR, "bring-your-own"),
            (INSTANCE_URL_VAR, "https://rakazo.example.com"),
        ]);
        assert_eq!(
            collect(&mut front, None, None).unwrap(),
            byo("https://rakazo.example.com")
        );
        let mut bad = scripted(&[
            (ACCESS_VAR, "bring-your-own"),
            (INSTANCE_URL_VAR, "http://rakazo.example.com"),
        ]);
        assert_eq!(refusal(collect(&mut bad, None, None)), AccessError::BadUrl);
        let mut missing = scripted(&[(ACCESS_VAR, "bring-your-own")]);
        assert_eq!(
            refusal(collect(&mut missing, None, None)),
            AccessError::MissingUrl
        );
        let mut unknown = scripted(&[(ACCESS_VAR, "lan")]);
        assert_eq!(
            refusal(collect(&mut unknown, None, None)),
            AccessError::UnknownMode
        );
    }

    #[test]
    fn a_url_with_tailscale_is_refused() {
        let mut both = scripted(&[
            (ACCESS_VAR, "tailscale"),
            (AUTH_KEY_VAR, KEY),
            (INSTANCE_URL_VAR, "https://rakazo.example.com"),
        ]);
        assert_eq!(
            refusal(collect(&mut both, None, None)),
            AccessError::UrlWithoutBringYourOwn
        );
        let mut recorded_tailscale = scripted(&[(INSTANCE_URL_VAR, "https://rakazo.example.com")]);
        assert_eq!(
            refusal(collect(
                &mut recorded_tailscale,
                Some(&joined("rakazo")),
                None
            )),
            AccessError::UrlWithoutBringYourOwn
        );
    }

    #[test]
    fn a_scripted_rerun_switches_without_asking() {
        let mut front = scripted(&[(ACCESS_VAR, "localhost")]);
        assert_eq!(
            collect(&mut front, Some(&joined("rakazo")), None).unwrap(),
            Access::Localhost
        );
    }

    #[test]
    fn the_wizard_asks_for_the_key_without_echo() {
        let mut front = wizard(&["tailscale", "", KEY]);
        let Access::Tailscale { auth_key, .. } = collect(&mut front, None, None).unwrap() else {
            panic!("not Tailscale");
        };
        assert_eq!(auth_key.unwrap().pasted().expose(), KEY);
        assert_eq!(
            front.prompt.asked,
            [
                "Access: localhost, tailscale, or bring-your-own (advanced, unsupported) [localhost]: ",
                "Tailscale device name [rakazo]: ",
                "Tailscale auth key (not ephemeral): ",
            ]
        );
        assert_eq!(
            front.prompt.secrets,
            ["Tailscale auth key (not ephemeral): "]
        );
    }

    #[test]
    fn a_wizard_rerun_prefills_every_access_answer() {
        let mut front = wizard(&["", "", ""]);
        let access = collect(&mut front, Some(&joined("rakazo-1")), None).unwrap();
        assert_eq!(access, tailscale("rakazo-1"));
        assert!(front.prompt.asked[1].ends_with("[rakazo-1]: "));
        assert!(front.prompt.asked[2].contains("has joined; press Enter"));

        let mut saved = wizard(&["", "", ""]);
        let never_joined = Settings {
            tailscale_dns_name: None,
            ..joined("rakazo")
        };
        collect(&mut saved, Some(&never_joined), Some(KEY)).unwrap();
        assert_eq!(
            saved.prompt.asked[2],
            "Tailscale auth key: set, press Enter to keep, or paste a new one: "
        );
        assert!(saved.prompt.asked.iter().all(|q| !q.contains(KEY)));

        let mut own = settings();
        byo("https://rakazo.example.com").record(&mut own);
        let mut front = wizard(&["", ""]);
        assert_eq!(
            collect(&mut front, Some(&own), None).unwrap(),
            byo("https://rakazo.example.com")
        );
        assert_eq!(
            front.prompt.asked[1],
            "Instance URL [https://rakazo.example.com]: "
        );
    }

    #[test]
    fn the_wizard_switches_only_on_yes() {
        for (recorded, lines) in [
            (settings(), vec!["tailscale", "", KEY]),
            (joined("rakazo"), vec!["localhost"]),
            (
                settings(),
                vec!["bring-your-own", "https://rakazo.example.com"],
            ),
        ] {
            let mut declined = wizard(&lines);
            declined.prompt.lines.push_back("");
            assert_eq!(
                refusal(collect(&mut declined, Some(&recorded), None)),
                AccessError::SwitchDeclined
            );
            let question = declined.prompt.asked.last().unwrap();
            assert!(question.starts_with(SWITCH_WARNING), "{question}");
            assert!(question.ends_with("Switch? [y/N] "), "{question}");

            let mut accepted = wizard(&lines);
            accepted.prompt.lines.push_back("y");
            collect(&mut accepted, Some(&recorded), None).unwrap();
        }
    }

    #[test]
    fn a_first_run_has_nothing_to_switch_from() {
        let mut front = wizard(&["bring-your-own", "https://rakazo.example.com"]);
        collect(&mut front, None, None).unwrap();
        assert_eq!(front.prompt.asked.len(), 2);
    }
}
