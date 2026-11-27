//! Test-only overrides, so a test Instance never collides with a real one on the same
//! machine. Not documented for Self-hosters: real use always gets the spec's fixed values.

const PROJECT_VAR: &str = "RAKAZO_STACK_TEST_PROJECT";
const WEB_PORT_VAR: &str = "RAKAZO_STACK_TEST_WEB_PORT";
const RAKAZO_FLOOR_VAR: &str = "RAKAZO_STACK_TEST_RAKAZO_FLOOR";
const NEXT_RAKAZO_VAR: &str = "RAKAZO_STACK_TEST_NEXT_RAKAZO";

/// The Compose project name (spec §5: `rakazo-stack`).
pub fn compose_project() -> String {
    std::env::var(PROJECT_VAR)
        .ok()
        .filter(|name| is_valid_project(name))
        .unwrap_or_else(|| "rakazo-stack".to_string())
}

/// Rakazo's web port (upstream's `RAKAZO_WEB_PORT` default).
pub fn web_port() -> u16 {
    std::env::var(WEB_PORT_VAR)
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(5173)
}

/// A lower Rakazo floor, so a test can install an older release and update it (spec §19).
pub fn rakazo_floor() -> Option<String> {
    std::env::var(RAKAZO_FLOOR_VAR)
        .ok()
        .filter(|floor| crate::version::Version::parse(floor).is_some())
}

/// An unpublished Rakazo release, `<tag>=<compose file>`: `update` runs the latest
/// release's images under `<tag>`, with that compose file in place of upstream's, so a
/// test can make the next release move Postgres or fail to start (spec §19).
pub fn next_rakazo() -> Option<(String, std::path::PathBuf)> {
    let value = std::env::var(NEXT_RAKAZO_VAR).ok()?;
    let (tag, file) = value.split_once('=')?;
    crate::version::Version::parse(tag)?;
    Some((tag.to_string(), file.into()))
}

fn is_valid_project(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}
