//! `secrets.env`: generated secrets and pasted keys, the source of truth for both
//! (spec §6). A secret is generated once and never regenerated.

use std::io::Read;

/// Rakazo's six generated secrets (spec §6) and how many random bytes each gets,
/// written as hex. Postgres passwords stay hex because they're interpolated into a URL.
const RAKAZO_SECRETS: [(&str, usize); 6] = [
    ("POSTGRES_PASSWORD", 16),
    ("POSTGRES_APP_PASSWORD", 16),
    ("BETTER_AUTH_SECRET", 32),
    ("ENCRYPTION_KEY", 32),
    ("SCREEN_PROXY_SECRET", 32),
    ("SANDBOX_SUPERVISOR_TOKEN", 32),
];

const HEADER: &str =
    "# rakazo-stack secrets. Back this file up with the Instance; never share it.\n";

#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secrets {
    entries: Vec<(String, String)>,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.entries.iter().map(|(k, _)| k.as_str()).collect();
        write!(f, "Secrets({names:?}, values hidden)")
    }
}

/// A key the Self-hoster pasted. It is never printed or logged, so it has no `Display`
/// and its `Debug` hides it.
#[derive(Clone, PartialEq, Eq)]
pub struct PastedKey(String);

impl PastedKey {
    /// Surrounding whitespace is dropped. `None` for a blank key, and for characters no
    /// key has, which would change the meaning of its `.env` line.
    pub fn new(text: &str) -> Option<Self> {
        let key = text.trim();
        let usable = !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_graphic() && !"\"'`\\$#".contains(c));
        usable.then(|| Self(key.to_string()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for PastedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PastedKey(hidden)")
    }
}

impl Secrets {
    pub fn parse(text: &str) -> Self {
        let entries = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.to_string()))
            .collect();
        Self { entries }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Sets a pasted key in place, or removes it.
    pub fn set_pasted(&mut self, name: &str, key: Option<&PastedKey>) {
        let at = self.entries.iter().position(|(k, _)| k == name);
        match (at, key) {
            (Some(at), Some(key)) => self.entries[at].1 = key.expose().to_string(),
            (None, Some(key)) => self
                .entries
                .push((name.to_string(), key.expose().to_string())),
            (_, None) => self.entries.retain(|(k, _)| k != name),
        }
    }

    /// Adds every missing Rakazo secret; existing values are kept as they are.
    pub fn fill_missing(&mut self, mut random_hex: impl FnMut(usize) -> String) {
        for (key, bytes) in RAKAZO_SECRETS {
            if self.get(key).is_none() {
                self.entries.push((key.to_string(), random_hex(bytes)));
            }
        }
    }

    pub fn rakazo(&self) -> impl Iterator<Item = (&'static str, &str)> {
        RAKAZO_SECRETS
            .iter()
            .filter_map(|(key, _)| self.get(key).map(|value| (*key, value)))
    }

    pub fn to_text(&self) -> String {
        let mut text = HEADER.to_string();
        for (key, value) in &self.entries {
            text.push_str(&format!("{key}={value}\n"));
        }
        text
    }
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .expect("read /dev/urandom");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_only_missing_secrets_and_round_trips() {
        let mut secrets = Secrets::parse("# comment\nENCRYPTION_KEY=keepme\n");
        let mut calls = 0;
        secrets.fill_missing(|n| {
            calls += 1;
            "x".repeat(n * 2)
        });
        assert_eq!(calls, 5);
        assert_eq!(secrets.get("ENCRYPTION_KEY"), Some("keepme"));
        assert_eq!(secrets.get("POSTGRES_PASSWORD").map(str::len), Some(32));
        assert_eq!(secrets.get("BETTER_AUTH_SECRET").map(str::len), Some(64));
        let again = Secrets::parse(&secrets.to_text());
        assert_eq!(again, secrets);
        let mut unchanged = again.clone();
        unchanged.fill_missing(|_| panic!("nothing is missing"));
        assert_eq!(unchanged, again);
    }

    #[test]
    fn pasted_keys_are_trimmed_checked_and_never_shown() {
        let key = PastedKey::new("  placeholder.key-1_2:3/4+5=  ").unwrap();
        assert_eq!(key.expose(), "placeholder.key-1_2:3/4+5=");
        assert_eq!(format!("{key:?}"), "PastedKey(hidden)");
        for bad in [
            "", "  ", "a b", "a\nB=c", "$HOME", "a#b", "\"a\"", "a'b", "a\\b", "é",
        ] {
            assert!(PastedKey::new(bad).is_none(), "{bad:?}");
        }
        let mut secrets = Secrets::default();
        secrets.set_pasted("OPENROUTER_API_KEY", Some(&key));
        assert!(!format!("{secrets:?}").contains(key.expose()));
        assert_eq!(Secrets::parse(&secrets.to_text()), secrets);
    }

    #[test]
    fn random_hex_has_the_requested_length() {
        let a = random_hex(32);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, random_hex(32));
    }
}
