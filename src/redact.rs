//! What every log the program writes replaces (spec §6): each secret by its name, and
//! personal values by placeholders.

use std::path::Path;

use crate::access::DEFAULT_DEVICE_NAME;
use crate::secrets::Secrets;
use crate::settings::Settings;

/// Shorter values are no key, and replacing them would garble the log.
const SHORTEST_SECRET: usize = 4;

const OWNER_EMAIL: &str = "<Owner email>";
const INSTANCE_URL: &str = "<Instance URL>";
const INSTANCE_HOST: &str = "<Instance URL host>";
const TS_NET_NAME: &str = "<ts.net name>";
const DEVICE_NAME: &str = "<Tailscale device name>";
const TAILSCALE_IP: &str = "<Tailscale IP>";
/// Tailscale's IPv6 range, `fd7a:115c:a1e0::/48`.
const TAILSCALE_IPV6_PREFIX: &str = "fd7a:115c:a1e0:";

#[derive(Debug, Default, Clone)]
pub struct Redactions {
    /// Each value with its placeholder.
    secrets: Vec<(String, String)>,
    personal: Vec<(String, &'static str)>,
    device_names: Vec<String>,
    homes: Vec<String>,
}

impl Redactions {
    /// The folder's `secrets.env` and `settings.json`, and the home folder it sits in.
    pub fn for_folder(folder: &Path) -> Self {
        let mut redactions = Self::default();
        redactions.learn_folder(folder);
        redactions
    }

    pub fn learn_folder(&mut self, folder: &Path) {
        if let Ok(text) = std::fs::read_to_string(folder.join("secrets.env")) {
            self.secrets_file(&text);
        }
        if let Ok(text) = std::fs::read_to_string(folder.join("settings.json")) {
            self.settings_file(&text);
        }
        if let Some(home) = folder.parent() {
            self.home(home);
        }
    }

    /// A secret replaced by `<name>`. A value seen before under another name takes this one.
    pub fn secret(&mut self, name: &str, value: &str) {
        if value.len() < SHORTEST_SECRET {
            return;
        }
        let placeholder = format!("<{name}>");
        match self.secrets.iter_mut().find(|(known, _)| known == value) {
            Some(entry) => entry.1 = placeholder,
            None => self.secrets.push((value.to_string(), placeholder)),
        }
    }

    /// A secret whose name isn't known yet; `secrets.env` names it once it is saved there.
    pub fn unnamed_secret(&mut self, label: &str, value: &str) {
        if !self.secrets.iter().any(|(known, _)| known == value) {
            self.secret(label, value);
        }
    }

    pub fn secrets_file(&mut self, text: &str) {
        for (name, value) in Secrets::parse(text).iter() {
            self.secret(name, value);
        }
    }

    pub fn settings_file(&mut self, text: &str) {
        if let Ok(settings) = Settings::parse(text) {
            self.settings(&settings);
        }
    }

    pub fn settings(&mut self, settings: &Settings) {
        if let Some(email) = &settings.owner_email {
            self.personal(email, OWNER_EMAIL);
        }
        if let Some(url) = &settings.bring_your_own_url {
            self.instance_url(url);
        }
        // The host itself is any `ts.net` name, which `apply` replaces wherever it is.
        if let Some(name) = &settings.tailscale_dns_name {
            self.personal(&format!("https://{name}"), INSTANCE_URL);
        }
        if let Some(name) = &settings.tailscale_device_name {
            self.device_name(name);
        }
    }

    pub fn instance_url(&mut self, url: &str) {
        let url = url.trim_end_matches('/');
        self.personal(url, INSTANCE_URL);
        if let Some((_, host)) = url.split_once("://") {
            let host = host.split('/').next().unwrap_or(host);
            self.personal(host, INSTANCE_HOST);
            if let Some((name, _port)) = host.rsplit_once(':') {
                self.personal(name, INSTANCE_HOST);
            }
        }
    }

    /// The program's default name, and the `-1` Tailscale adds to it on a clash, are no
    /// one's own, and replacing `rakazo` would garble every line naming Rakazo.
    pub fn device_name(&mut self, name: &str) {
        let default = name.strip_prefix(DEFAULT_DEVICE_NAME).is_some_and(|rest| {
            rest.is_empty()
                || rest
                    .strip_prefix('-')
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        });
        let name = name.to_ascii_lowercase();
        if !default && !name.is_empty() && !self.device_names.contains(&name) {
            self.device_names.push(name);
        }
    }

    pub fn home(&mut self, home: &Path) {
        let home = home.to_string_lossy().trim_end_matches('/').to_string();
        if home.len() > 1 && !self.homes.contains(&home) {
            // Docker Desktop names WSL paths with backslashes: `\\wsl.localhost\<distro>\home\…`.
            self.homes.push(home.replace('/', "\\"));
            self.homes.push(home);
        }
    }

    fn personal(&mut self, value: &str, placeholder: &'static str) {
        let value = value.trim();
        if !value.is_empty() && !self.personal.iter().any(|(known, _)| known == value) {
            self.personal.push((value.to_string(), placeholder));
        }
    }

    pub fn apply(&self, text: &str) -> String {
        let mut text = text.to_string();
        let mut secrets: Vec<_> = self.secrets.iter().collect();
        secrets.sort_by_key(|(value, _)| std::cmp::Reverse(value.len()));
        for (value, placeholder) in secrets {
            text = text.replace(value.as_str(), placeholder);
        }
        let mut personal: Vec<_> = self.personal.iter().collect();
        personal.sort_by_key(|(value, _)| std::cmp::Reverse(value.len()));
        for (value, placeholder) in personal {
            let value = value.to_ascii_lowercase();
            let found = |lower: &str, at| lower[at..].starts_with(&value).then_some(value.len());
            text = replace_matches(&text, found, placeholder);
        }
        text = replace_matches(&text, ts_net_name, TS_NET_NAME);
        for name in &self.device_names {
            text = replace_matches(&text, |lower, at| whole_word(lower, at, name), DEVICE_NAME);
        }
        text = replace_matches(&text, tailscale_ipv4, TAILSCALE_IP);
        text = replace_matches(&text, tailscale_ipv6, TAILSCALE_IP);
        for home in &self.homes {
            let home = home.to_ascii_lowercase();
            text = replace_matches(&text, |lower, at| whole_path(lower, at, &home), "~");
        }
        text
    }
}

/// Replaces each match `found` reports: the length of the match starting at a byte of the
/// ASCII-lowercased text. Lowercasing keeps every byte where it was.
fn replace_matches(
    text: &str,
    found: impl Fn(&str, usize) -> Option<usize>,
    placeholder: &str,
) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut at = 0;
    while at < text.len() {
        if text.is_char_boundary(at)
            && let Some(len) = found(&lower, at).filter(|len| *len > 0)
        {
            out.push_str(&text[copied..at]);
            out.push_str(placeholder);
            at += len;
            copied = at;
        } else {
            at += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

fn byte_before(text: &str, at: usize) -> Option<u8> {
    at.checked_sub(1).map(|i| text.as_bytes()[i])
}

fn whole_word(lower: &str, at: usize, word: &str) -> Option<usize> {
    let end = at + word.len();
    (lower[at..].starts_with(word)
        && !byte_before(lower, at).is_some_and(is_name_byte)
        && !lower.as_bytes().get(end).copied().is_some_and(is_name_byte))
    .then_some(word.len())
}

/// A home folder, but not a longer one that starts the same, such as `/home/someone2`.
fn whole_path(text: &str, at: usize, home: &str) -> Option<usize> {
    let end = at + home.len();
    (text.get(at..end) == Some(home)
        && !text
            .as_bytes()
            .get(end)
            .is_some_and(|b| is_name_byte(*b) || *b == b'.'))
    .then_some(home.len())
}

/// A whole `<name>.ts.net` host name.
fn ts_net_name(lower: &str, at: usize) -> Option<usize> {
    let bytes = lower.as_bytes();
    if byte_before(lower, at).is_some_and(|b| is_name_byte(b) || b == b'.') {
        return None;
    }
    let len = bytes[at..]
        .iter()
        .take_while(|b| is_name_byte(**b) || **b == b'.')
        .count();
    let host = &lower[at..at + len];
    let name = host.strip_suffix(".ts.net")?;
    (!name.is_empty() && !name.starts_with('.')).then_some(len)
}

/// An address in `100.64.0.0/10`, Tailscale's IPv4 range.
fn tailscale_ipv4(lower: &str, at: usize) -> Option<usize> {
    let bytes = lower.as_bytes();
    if byte_before(lower, at).is_some_and(|b| b.is_ascii_alphanumeric() || b == b'.') {
        return None;
    }
    let len = bytes[at..]
        .iter()
        .take_while(|b| b.is_ascii_digit() || **b == b'.')
        .count();
    let address = lower[at..at + len].trim_end_matches('.');
    if bytes
        .get(at + address.len())
        .is_some_and(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    let octets: Vec<u8> = address
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    (octets.len() == 4 && octets[0] == 100 && (64..128).contains(&octets[1]))
        .then_some(address.len())
}

fn tailscale_ipv6(lower: &str, at: usize) -> Option<usize> {
    if !lower[at..].starts_with(TAILSCALE_IPV6_PREFIX)
        || byte_before(lower, at).is_some_and(|b| b.is_ascii_hexdigit() || b == b':')
    {
        return None;
    }
    Some(
        lower.as_bytes()[at..]
            .iter()
            .take_while(|b| b.is_ascii_hexdigit() || **b == b':')
            .count(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRETS: &str = "# rakazo-stack secrets.\n\
        POSTGRES_PASSWORD=0123456789abcdef0123456789abcdef\n\
        BETTER_AUTH_SECRET=fedcba9876543210fedcba9876543210fedcba9876543210\n\
        LEMONADE_API_KEY=aaaabbbbccccddddeeeeffff00001111\n\
        OPENROUTER_API_KEY=sk-or-v1-PLACEHOLDER0000000000000000\n\
        COMPOSIO_API_KEY=ak_PLACEHOLDER000000\n\
        TS_AUTHKEY=tskey-auth-kPLACEHOLDER-0000000000\n";

    fn settings() -> Settings {
        let mut settings = Settings::parse(
            r#"{"schema":1,"program_version":"0.1.0","components":{"rakazo":{"version":"v0.16.0","images":{"app":"sha256:a","computer":"sha256:c"}}}}"#,
        )
        .unwrap();
        settings.owner_email = Some("owner@example.com".into());
        settings.tailscale_device_name = Some("den-pc".into());
        settings.tailscale_dns_name = Some("den-pc.tail0000.ts.net".into());
        settings.bring_your_own_url = Some("https://chat.example.com:8443".into());
        settings
    }

    fn redactions() -> Redactions {
        let mut redactions = Redactions::default();
        redactions.secrets_file(SECRETS);
        redactions.settings(&settings());
        redactions.home(Path::new("/home/Someone"));
        redactions
    }

    #[test]
    fn every_secret_and_personal_value_is_replaced() {
        let secret_values: Vec<(&str, &str)> = SECRETS
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect();
        let personal = [
            "owner@example.com",
            "Owner@Example.COM",
            "den-pc",
            "den-pc.tail0000.ts.net",
            "https://den-pc.tail0000.ts.net",
            "https://chat.example.com:8443",
            "chat.example.com",
            "100.101.102.103",
            "fd7a:115c:a1e0::1234:abcd",
            "/home/Someone",
            "\\\\wsl.localhost\\Ubuntu\\home\\someone",
        ];
        let mut text = String::new();
        for (name, value) in &secret_values {
            text.push_str(&format!(
                "{name} is {value}; in a URL postgres://rakazo:{value}@postgres\n"
            ));
        }
        for value in personal {
            text.push_str(&format!("value: {value}/rakazo-stack, again ({value})\n"));
        }
        let redacted = redactions().apply(&text);
        for (name, value) in secret_values {
            assert!(!redacted.contains(value), "{value} in\n{redacted}");
            assert!(
                redacted.contains(&format!("{name} is <{name}>")),
                "{redacted}"
            );
        }
        for value in personal {
            let lower = redacted.to_ascii_lowercase();
            for piece in [
                "owner@example.com",
                "den-pc",
                "tail0000",
                "chat.example.com",
                "100.101",
                "fd7a:115c",
                "someone",
            ] {
                assert!(!lower.contains(piece), "{piece} of {value} in\n{redacted}");
            }
        }
        for placeholder in [
            OWNER_EMAIL,
            INSTANCE_URL,
            INSTANCE_HOST,
            DEVICE_NAME,
            TAILSCALE_IP,
            "~/rakazo-stack",
        ] {
            assert!(
                redacted.contains(placeholder),
                "{placeholder} in\n{redacted}"
            );
        }
    }

    #[test]
    fn what_is_no_ones_own_stays() {
        let mut redactions = redactions();
        redactions.device_name("rakazo");
        redactions.device_name("rakazo-1");
        let text = "Pulling rakazo-stack-api and ghcr.io/example/rakazo:v0.16.0 from 10.0.0.2, \
                    100.200.1.1 and 1.100.64.1; /home/someone2/x; http://127.0.0.1:5173; \
                    the den-pcs; LEMONADE_API_KEY is set";
        assert_eq!(redactions.apply(text), text);
    }

    #[test]
    fn any_ts_net_name_and_the_default_device_name() {
        let mut redactions = Redactions::default();
        redactions.device_name("rakazo");
        assert_eq!(
            redactions.apply("joined as rakazo-2.tail1234.ts.net (rakazo)"),
            "joined as <ts.net name> (rakazo)"
        );
    }

    #[test]
    fn replacing_twice_changes_nothing_more() {
        let redactions = redactions();
        let once = redactions.apply(&format!(
            "{SECRETS} owner@example.com /home/someone 100.64.0.1"
        ));
        assert_eq!(redactions.apply(&once), once);
    }

    #[test]
    fn a_name_from_secrets_env_replaces_a_generic_label() {
        let mut redactions = Redactions::default();
        redactions.unnamed_secret("pasted key", "sk-or-v1-PLACEHOLDER");
        assert_eq!(redactions.apply("k=sk-or-v1-PLACEHOLDER"), "k=<pasted key>");
        redactions.secret("OPENROUTER_API_KEY", "sk-or-v1-PLACEHOLDER");
        redactions.unnamed_secret("pasted key", "sk-or-v1-PLACEHOLDER");
        assert_eq!(
            redactions.apply("k=sk-or-v1-PLACEHOLDER"),
            "k=<OPENROUTER_API_KEY>"
        );
        redactions.secret("TOO_SHORT", "abc");
        assert_eq!(redactions.apply("abc"), "abc");
    }
}
