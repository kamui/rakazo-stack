use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    /// A Linux program inside WSL 2, driving Windows' Docker Desktop.
    Wsl,
    MacOs,
}

impl Platform {
    /// How a Backup's manifest names the platform it was made on.
    pub fn id(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Wsl => "wsl",
            Self::MacOs => "macos",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [Self::Linux, Self::Wsl, Self::MacOs]
            .into_iter()
            .find(|platform| platform.id() == id)
    }

    pub fn detect() -> Self {
        if cfg!(target_os = "macos") {
            return Self::MacOs;
        }
        let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
        if is_wsl_kernel(&release) {
            Self::Wsl
        } else {
            Self::Linux
        }
    }
}

fn is_wsl_kernel(release: &str) -> bool {
    release.to_ascii_lowercase().contains("microsoft")
}

/// WSL registers `WSLInterop` (or `WSLInterop-late` under systemd) when it can run
/// Windows programs such as `docker.exe`.
pub fn wsl_interop_enabled() -> bool {
    ["WSLInterop", "WSLInterop-late"].iter().any(|name| {
        let path = Path::new("/proc/sys/fs/binfmt_misc").join(name);
        std::fs::read_to_string(path)
            .map(|text| text.lines().next() == Some("enabled"))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::{Platform, is_wsl_kernel};

    #[test]
    fn ids_round_trip() {
        for platform in [Platform::Linux, Platform::Wsl, Platform::MacOs] {
            assert_eq!(Platform::from_id(platform.id()), Some(platform));
        }
        assert_eq!(Platform::from_id("windows"), None);
    }

    #[test]
    fn recognises_wsl_kernels() {
        assert!(is_wsl_kernel("6.6.87.2-microsoft-standard-WSL2\n"));
        assert!(!is_wsl_kernel("6.8.0-48-generic\n"));
    }
}
