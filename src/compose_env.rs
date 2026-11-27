//! Compose takes a shell variable over the same name in `.env` when it interpolates the
//! compose files, so an exported `OPENROUTER_API_KEY` or `COMPOSE_PROFILES` would quietly
//! replace what setup wrote (spec §5). These are the names to keep out of Compose's
//! environment.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// `.env`'s own names, every name the compose files it lists interpolate, and every
/// `COMPOSE_*` setting.
pub fn shadowing(folder: &Path, environment: impl Iterator<Item = String>) -> BTreeSet<String> {
    let env = fs::read_to_string(folder.join(".env")).unwrap_or_default();
    let mut names: BTreeSet<String> = env_keys(&env).collect();
    let compose_files = env_keys_and_values(&env)
        .find(|(key, _)| *key == "COMPOSE_FILE")
        .map(|(_, value)| value.split(':').map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    for file in compose_files {
        if let Ok(text) = fs::read_to_string(folder.join(file)) {
            names.extend(interpolated(&text));
        }
    }
    names.extend(environment.filter(|name| name.starts_with("COMPOSE_")));
    names
}

fn env_keys_and_values(env: &str) -> impl Iterator<Item = (&str, &str)> {
    env.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim(), value.trim()))
}

fn env_keys(env: &str) -> impl Iterator<Item = String> {
    env_keys_and_values(env).map(|(key, _)| key.to_string())
}

/// Names written `$NAME` or `${NAME…}`; `$$` is a literal dollar sign.
fn interpolated(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut names = BTreeSet::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            i += 1;
            continue;
        }
        match bytes.get(i + 1) {
            Some(b'$') => i += 2,
            Some(b'{') => {
                let name = name_at(&text[i + 2..]);
                i += 2 + name.len();
                if !name.is_empty() {
                    names.insert(name.to_string());
                }
            }
            _ => {
                let name = name_at(&text[i + 1..]);
                i += 1 + name.len();
                if !name.is_empty() {
                    names.insert(name.to_string());
                }
            }
        }
    }
    names
}

fn name_at(text: &str) -> &str {
    let end = text
        .char_indices()
        .find(|(n, c)| !(c.is_ascii_alphanumeric() || *c == '_') || (*n == 0 && c.is_ascii_digit()))
        .map_or(text.len(), |(n, _)| n);
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_interpolated_names_and_skips_literal_dollars() {
        let compose = "image: ${RAKAZO_IMAGE}:${RAKAZO_IMAGE_TAG:-latest}\n\
                       ports: [\"${RAKAZO_WEB_BIND:-127.0.0.1}:${RAKAZO_WEB_PORT:-5173}:5173\"]\n\
                       key: $OPENAI_API_KEY\n\
                       serve: $${TS_CERT_DOMAIN} and $$5 and $1\n";
        assert_eq!(
            interpolated(compose).into_iter().collect::<Vec<_>>(),
            [
                "OPENAI_API_KEY",
                "RAKAZO_IMAGE",
                "RAKAZO_IMAGE_TAG",
                "RAKAZO_WEB_BIND",
                "RAKAZO_WEB_PORT"
            ]
        );
    }

    #[test]
    fn collects_env_names_compose_file_names_and_compose_settings() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".env"),
            "# generated\nCOMPOSE_FILE=a.yml:b.yaml\nPOSTGRES_PASSWORD=placeholder\n",
        )
        .unwrap();
        fs::write(dir.path().join("a.yml"), "x: ${OPENAI_API_KEY:-}\n").unwrap();
        fs::write(dir.path().join("b.yaml"), "y: ${LEMONADE_UPSTREAM}\n").unwrap();
        let environment = ["PATH", "COMPOSE_PROFILES", "HOME"].map(String::from);
        let names = shadowing(dir.path(), environment.into_iter());
        assert_eq!(
            names.into_iter().collect::<Vec<_>>(),
            [
                "COMPOSE_FILE",
                "COMPOSE_PROFILES",
                "LEMONADE_UPSTREAM",
                "OPENAI_API_KEY",
                "POSTGRES_PASSWORD"
            ]
        );
    }
}
