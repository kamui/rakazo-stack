//! Components by name, and the Holds `rakazo-stack update` keeps them at (spec §4,
//! "Floors and Holds").

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::resolve::{self, HINDSIGHT_FLOOR};
use crate::settings::Components;
use crate::version::Version;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Component {
    Rakazo,
    Hindsight,
    Lemonade,
    Tailscale,
}

/// Each held Component's version, as `settings.json` records versions.
pub type Holds = BTreeMap<Component, String>;

impl Component {
    pub const ALL: [Self; 4] = [
        Self::Rakazo,
        Self::Hindsight,
        Self::Lemonade,
        Self::Tailscale,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Rakazo => "rakazo",
            Self::Hindsight => "hindsight",
            Self::Lemonade => "lemonade",
            Self::Tailscale => "tailscale",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Rakazo => "Rakazo",
            Self::Hindsight => "Hindsight",
            Self::Lemonade => "Lemonade",
            Self::Tailscale => "Tailscale",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == text.trim())
    }

    pub fn floor(self) -> Option<String> {
        match self {
            Self::Rakazo => Some(resolve::rakazo_floor()),
            Self::Hindsight => Some(HINDSIGHT_FLOOR.to_string()),
            Self::Lemonade | Self::Tailscale => None,
        }
    }

    pub fn below_floor(self, version: &str) -> bool {
        self.floor().is_some_and(|floor| below(version, &floor))
    }

    /// `X.Y.Z`, written as `settings.json` writes this Component's versions: Hindsight's
    /// image tags have no `v`, the others do.
    pub fn normalize(self, version: &str) -> Option<String> {
        let bare = version.trim().strip_prefix('v').unwrap_or(version.trim());
        let parts: Vec<&str> = bare.split('.').collect();
        let numeric = parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
        numeric.then(|| match self {
            Self::Hindsight => bare.to_string(),
            _ => format!("v{bare}"),
        })
    }

    /// The version `settings.json` records for it, if any.
    pub fn recorded(self, components: &Components) -> Option<&str> {
        match self {
            Self::Rakazo => Some(&components.rakazo.version),
            Self::Hindsight => components.hindsight.as_ref().map(|h| h.version.as_str()),
            Self::Lemonade => components.lemonade.as_ref().map(|l| l.version.as_str()),
            Self::Tailscale => components.tailscale.as_ref().map(|t| t.version.as_str()),
        }
    }
}

pub fn below(version: &str, than: &str) -> bool {
    match (Version::parse(version), Version::parse(than)) {
        (Some(version), Some(than)) => version < than,
        _ => true,
    }
}

/// `--hold <component>=<version>`.
pub fn parse_hold(text: &str) -> Result<(Component, String), String> {
    let (name, version) = text
        .split_once('=')
        .ok_or_else(|| format!("`--hold {text}` needs the form <component>=<version>."))?;
    let component = parse_component(name)?;
    let version = component
        .normalize(version)
        .ok_or_else(|| format!("`--hold {text}`: the version must look like 1.2.3 or v1.2.3."))?;
    Ok((component, version))
}

pub fn parse_component(name: &str) -> Result<Component, String> {
    Component::parse(name).ok_or_else(|| {
        let known: Vec<&str> = Component::ALL.iter().map(|c| c.id()).collect();
        format!(
            "`{name}` isn't a Component. Use one of: {}.",
            known.join(", ")
        )
    })
}

/// Applies `--unhold` then `--hold` to the recorded Holds. A Hold below the floor is
/// refused, and so is one below the recorded version: `update` never moves a
/// Component back, because its data has already migrated forward.
pub fn apply(
    holds: &mut Holds,
    add: &[(Component, String)],
    remove: &[Component],
    recorded: &Components,
) -> Result<(), String> {
    for component in remove {
        holds.remove(component);
    }
    for (component, version) in add {
        if let Some(floor) = component.floor().filter(|floor| below(version, floor)) {
            return Err(format!(
                "{} {version} is below the {floor} this rakazo-stack needs, so it can't be held.",
                component.name()
            ));
        }
        if let Some(current) = component
            .recorded(recorded)
            .filter(|current| below(version, current))
        {
            return Err(format!(
                "The Instance runs {} {current}, and update never moves a Component back. Restore an older Backup instead.",
                component.name()
            ));
        }
        holds.insert(*component, version.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{RakazoImages, RecordedImage, RecordedRakazo};

    fn recorded() -> Components {
        Components {
            rakazo: RecordedRakazo {
                version: "v0.16.0".into(),
                images: RakazoImages {
                    app: "sha256:aaa".into(),
                    computer: "sha256:ccc".into(),
                },
            },
            tailscale: None,
            lemonade: Some(RecordedImage {
                version: "v2026.48.3".into(),
                digest: "sha256:lll".into(),
            }),
            hindsight: None,
        }
    }

    #[test]
    fn parses_holds_in_each_components_own_form() {
        assert_eq!(
            parse_hold("rakazo=0.16.0"),
            Ok((Component::Rakazo, "v0.16.0".into()))
        );
        assert_eq!(
            parse_hold("hindsight=v1.3.0"),
            Ok((Component::Hindsight, "1.3.0".into()))
        );
        assert!(
            parse_hold("rakazo")
                .unwrap_err()
                .contains("<component>=<version>")
        );
        assert!(
            parse_hold("postgres=16.0.0")
                .unwrap_err()
                .contains("rakazo, hindsight, lemonade, tailscale")
        );
        for bad in [
            "rakazo=latest",
            "rakazo=v0.16",
            "rakazo=v0.16.0-rc1",
            "rakazo=",
        ] {
            assert!(parse_hold(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn refuses_holds_below_the_floor_or_the_recorded_version() {
        let mut holds = Holds::new();
        let err = apply(
            &mut holds,
            &[(Component::Rakazo, "v0.15.0".into())],
            &[],
            &recorded(),
        )
        .unwrap_err();
        assert!(err.contains("below the v0.16.0"), "{err}");
        let err = apply(
            &mut holds,
            &[(Component::Lemonade, "v2026.47.1".into())],
            &[],
            &recorded(),
        )
        .unwrap_err();
        assert!(err.contains("never moves a Component back"), "{err}");
        assert!(holds.is_empty());
    }

    #[test]
    fn holds_and_unholds() {
        let mut holds = Holds::from([(Component::Tailscale, "v1.104.7".into())]);
        apply(
            &mut holds,
            &[
                (Component::Rakazo, "v0.16.0".into()),
                (Component::Lemonade, "v2026.49.0".into()),
            ],
            &[Component::Tailscale],
            &recorded(),
        )
        .unwrap();
        assert_eq!(
            holds,
            Holds::from([
                (Component::Rakazo, "v0.16.0".into()),
                (Component::Lemonade, "v2026.49.0".into()),
            ])
        );
    }
}
