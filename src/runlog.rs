//! Setup run logs in the Instance folder's `logs/` (spec §6). They hold what setup
//! printed, never a secret.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct RunLog {
    file: File,
}

impl RunLog {
    pub fn create(folder: &Path) -> io::Result<Self> {
        let dir = folder.join("logs");
        DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(dir.join(format!("setup-{}.log", utc_stamp(seconds))))?;
        Ok(Self { file })
    }

    /// Prints a line and appends it to the log. A failed log write never stops setup.
    pub fn say(&self, line: &str) {
        println!("{line}");
        let _ = writeln!(&self.file, "{line}");
    }
}

/// `YYYYMMDDTHHMMSSZ`, from Howard Hinnant's `civil_from_days`.
fn utc_stamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_utc_times() {
        assert_eq!(utc_stamp(0), "19700101T000000Z");
        assert_eq!(utc_stamp(951_782_400), "20000229T000000Z");
        assert_eq!(utc_stamp(1_795_694_461), "20261126T120101Z");
    }

    #[test]
    fn writes_private_logs() {
        let folder = tempfile::tempdir().unwrap();
        let log = RunLog::create(folder.path()).unwrap();
        log.say("Starting Rakazo…");
        let dir = folder.path().join("logs");
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let entry = fs::read_dir(&dir).unwrap().next().unwrap().unwrap();
        assert_eq!(
            entry.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::read_to_string(entry.path()).unwrap(),
            "Starting Rakazo…\n"
        );
    }
}
