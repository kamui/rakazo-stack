use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    /// A Linux program inside WSL 2, driving Windows' Docker Desktop.
    Wsl,
    MacOs,
}

impl Platform {
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
    use super::is_wsl_kernel;

    #[test]
    fn recognises_wsl_kernels() {
        assert!(is_wsl_kernel("6.6.87.2-microsoft-standard-WSL2\n"));
        assert!(!is_wsl_kernel("6.8.0-48-generic\n"));
    }
}
