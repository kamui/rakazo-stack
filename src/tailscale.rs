//! The stack's Tailscale device after `tailscale` starts (spec §7 step 10): which name
//! Tailscale assigned, whether it can serve HTTPS, and the Checklist items when it can't.

use std::time::Duration;

use serde::Deserialize;

use crate::access::is_hostname;

pub const SERVICE: &str = "tailscale";
/// Past the 60 s that the container's own start-up gives `tailscale up`.
pub const ATTEMPTS: u32 = 45;
pub const PAUSE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joined {
    /// `<device>.<tailnet>.ts.net`, without the trailing dot.
    pub dns_name: String,
    /// MagicDNS and HTTPS certificates are on, so Serve can get a certificate.
    pub https_ready: bool,
}

impl Joined {
    pub fn device_name(&self) -> &str {
        device_label(&self.dns_name)
    }
}

pub fn device_label(dns_name: &str) -> &str {
    dns_name.split('.').next().unwrap_or(dns_name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotJoined {
    NoAnswer,
    /// `tailscaled`'s backend state, such as `NeedsLogin`.
    State(String),
}

/// The fields of `tailscale status --json` that setup reads.
#[derive(Deserialize)]
struct Status {
    #[serde(rename = "BackendState")]
    backend_state: String,
    #[serde(rename = "Self")]
    node: Option<Node>,
    #[serde(rename = "CertDomains")]
    cert_domains: Option<Vec<String>>,
    #[serde(rename = "CurrentTailnet")]
    tailnet: Option<Tailnet>,
}

#[derive(Deserialize)]
struct Node {
    #[serde(rename = "DNSName")]
    dns_name: String,
}

#[derive(Deserialize)]
struct Tailnet {
    #[serde(rename = "MagicDNSEnabled")]
    magic_dns: bool,
}

fn read_status(json: &str) -> Result<Joined, NotJoined> {
    let status: Status = serde_json::from_str(json).map_err(|_| NotJoined::NoAnswer)?;
    if status.backend_state != "Running" {
        return Err(NotJoined::State(status.backend_state));
    }
    let dns_name = status
        .node
        .map(|node| node.dns_name.trim_end_matches('.').to_ascii_lowercase())
        .filter(|name| name.contains('.') && is_hostname(name))
        .ok_or_else(|| NotJoined::State("Running without a DNS name".to_string()))?;
    let https_ready = status.tailnet.is_some_and(|tailnet| tailnet.magic_dns)
        && status
            .cert_domains
            .is_some_and(|domains| !domains.is_empty());
    Ok(Joined {
        dns_name,
        https_ready,
    })
}

/// Whether Tailscale has answered the requested name: with it, or with a numbered
/// variant because another device has it.
fn is_settled(assigned: &str, requested: &str, previous: Option<&str>) -> bool {
    let numbered = assigned
        .strip_prefix(requested)
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    // Right after a rename the node still reports the old name, which may look numbered.
    assigned == requested || (numbered && previous != Some(assigned))
}

/// Polls until the device is up under the requested name. A rename takes Tailscale a
/// moment; when the attempts run out, whatever name the device reports is the answer,
/// since a name set in the admin console wins over the requested one.
pub fn wait_for_join(
    requested: &str,
    previous: Option<&str>,
    attempts: u32,
    mut status: impl FnMut() -> Option<String>,
    mut pause: impl FnMut(),
) -> Result<Joined, NotJoined> {
    let mut last = Err(NotJoined::NoAnswer);
    for attempt in 0..attempts {
        if attempt > 0 {
            pause();
        }
        let Some(json) = status() else { continue };
        last = read_status(&json);
        if let Ok(joined) = &last
            && is_settled(joined.device_name(), requested, previous)
        {
            break;
        }
    }
    last
}

/// `compose` is what the Self-hoster types: `docker compose`, or `docker.exe compose`.
pub fn not_joined_item(failure: &NotJoined, compose: &str) -> String {
    match failure {
        NotJoined::State(state) if state == "NeedsLogin" => format!(
            "Tailscale hasn't joined your tailnet. The auth key may be expired, used up or revoked: create a new one that isn't ephemeral and run `rakazo-stack setup` again. From ~/rakazo-stack, `{compose} logs {SERVICE}` shows what Tailscale reported."
        ),
        NotJoined::State(state) if state == "NeedsMachineAuth" => "Tailscale is waiting for approval: approve the new device on the Machines page of the Tailscale admin console, then run `rakazo-stack setup` again.".to_string(),
        NotJoined::State(state) => start_failed_item(&format!("its state is {state}"), compose),
        NotJoined::NoAnswer => start_failed_item("it didn't answer", compose),
    }
}

pub fn start_failed_item(reason: &str, compose: &str) -> String {
    format!(
        "Tailscale didn't come up: {reason}. From ~/rakazo-stack, `{compose} logs {SERVICE}` shows why; then run `rakazo-stack setup` again."
    )
}

pub fn unresolved_item(reason: &str) -> String {
    format!(
        "Tailscale wasn't set up, because its latest version couldn't be looked up: {reason}. Run `rakazo-stack setup` again."
    )
}

pub fn not_removed_item(compose: &str) -> String {
    format!(
        "The Tailscale container couldn't be removed, so the Instance may still be reachable on your tailnet. Run `{compose} --profile {SERVICE} rm --stop --force {SERVICE}` from ~/rakazo-stack."
    )
}

pub fn no_https_item(url: &str, compose: &str) -> String {
    format!(
        "{url} doesn't answer yet, because your tailnet has MagicDNS or HTTPS certificates off. Enable both, then run `{compose} restart {SERVICE}` from ~/rakazo-stack."
    )
}

pub fn console_item(device_name: &str) -> String {
    format!(
        "Tailscale: in the admin console, enable MagicDNS and HTTPS certificates (device names then appear in public Certificate Transparency logs), and turn off key expiry for the device `{device_name}`."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// The shape `tailscale status --json --peers=false` has in v1.104.
    fn running(name: &str) -> String {
        format!(
            r#"{{"Version":"1.104.7","BackendState":"Running","Self":{{"HostName":"rakazo","DNSName":"{name}.example.ts.net.","Online":true}},"CertDomains":["{name}.example.ts.net"],"CurrentTailnet":{{"MagicDNSSuffix":"example.ts.net","MagicDNSEnabled":true}},"Peer":null}}"#
        )
    }

    const NEEDS_LOGIN: &str = r#"{"BackendState":"NeedsLogin","Self":{"DNSName":""},"CertDomains":null,"CurrentTailnet":null}"#;

    /// Runs the wait over recorded replies, the last of which repeats.
    fn wait(
        requested: &str,
        previous: Option<&str>,
        replies: &[Option<String>],
    ) -> (Result<Joined, NotJoined>, u32) {
        let mut queue: VecDeque<_> = replies.iter().cloned().collect();
        let mut calls = 0;
        let result = wait_for_join(
            requested,
            previous,
            5,
            || {
                calls += 1;
                if queue.len() > 1 {
                    queue.pop_front().unwrap()
                } else {
                    queue.front().cloned().flatten()
                }
            },
            || {},
        );
        (result, calls)
    }

    #[test]
    fn reads_the_assigned_name_and_https() {
        let joined = read_status(&running("rakazo")).unwrap();
        assert_eq!(joined.dns_name, "rakazo.example.ts.net");
        assert_eq!(joined.device_name(), "rakazo");
        assert!(joined.https_ready);

        let no_certs = running("rakazo").replace(r#"["rakazo.example.ts.net"]"#, "null");
        assert!(!read_status(&no_certs).unwrap().https_ready);
        let no_magic_dns =
            running("rakazo").replace(r#""MagicDNSEnabled":true"#, r#""MagicDNSEnabled":false"#);
        assert!(!read_status(&no_magic_dns).unwrap().https_ready);
    }

    #[test]
    fn a_node_that_isnt_up_has_no_name() {
        assert_eq!(
            read_status(NEEDS_LOGIN),
            Err(NotJoined::State("NeedsLogin".into()))
        );
        assert_eq!(read_status("not json"), Err(NotJoined::NoAnswer));
        for bad in ["", "rakazo", "rakazo.example.ts.net\\nSIGNUPS_ENABLED=true"] {
            let reply = format!(r#"{{"BackendState":"Running","Self":{{"DNSName":"{bad}"}}}}"#);
            assert!(read_status(&reply).is_err(), "{bad}");
        }
    }

    #[test]
    fn waits_for_the_node_to_come_up() {
        let (result, calls) = wait(
            "rakazo",
            None,
            &[None, Some(NEEDS_LOGIN.into()), Some(running("rakazo"))],
        );
        assert_eq!(result.unwrap().device_name(), "rakazo");
        assert_eq!(calls, 3);
    }

    #[test]
    fn a_clash_ends_with_the_assigned_name() {
        let (result, calls) = wait("rakazo", None, &[Some(running("rakazo-1"))]);
        assert_eq!(result.unwrap().dns_name, "rakazo-1.example.ts.net");
        assert_eq!(calls, 1);
    }

    #[test]
    fn waits_for_a_rename_to_arrive() {
        let (result, calls) = wait(
            "other",
            Some("rakazo"),
            &[Some(running("rakazo")), Some(running("other"))],
        );
        assert_eq!(result.unwrap().device_name(), "other");
        assert_eq!(calls, 2);

        let (result, calls) = wait(
            "rakazo",
            Some("rakazo-1"),
            &[Some(running("rakazo-1")), Some(running("rakazo"))],
        );
        assert_eq!(result.unwrap().device_name(), "rakazo");
        assert_eq!(calls, 2);
    }

    #[test]
    fn a_name_set_in_the_admin_console_wins_after_the_wait() {
        let (result, calls) = wait("rakazo", Some("rakazo"), &[Some(running("renamed"))]);
        assert_eq!(result.unwrap().device_name(), "renamed");
        assert_eq!(calls, 5);
    }

    #[test]
    fn reports_the_last_state_when_it_never_comes_up() {
        let (result, _) = wait("rakazo", None, &[Some(NEEDS_LOGIN.into()), None]);
        assert_eq!(result, Err(NotJoined::State("NeedsLogin".into())));
        let (result, _) = wait("rakazo", None, &[None]);
        assert_eq!(result, Err(NotJoined::NoAnswer));
    }

    #[test]
    fn failures_say_what_to_do() {
        let compose = "docker.exe compose";
        let item = not_joined_item(&NotJoined::State("NeedsLogin".into()), compose);
        assert!(item.contains("create a new one"), "{item}");
        assert!(
            item.contains("`docker.exe compose logs tailscale`"),
            "{item}"
        );
        let item = not_joined_item(&NotJoined::State("NeedsMachineAuth".into()), compose);
        assert!(item.contains("approve the new device"), "{item}");
        let item = not_joined_item(&NotJoined::NoAnswer, compose);
        assert!(item.contains("didn't answer"), "{item}");
        assert!(console_item("rakazo-1").contains("key expiry for the device `rakazo-1`"));
    }
}
