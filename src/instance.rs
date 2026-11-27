use std::fs::{self, File, TryLockError};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The Instance folder, fixed at `~/rakazo-stack` (spec §6).
pub struct InstanceFolder {
    path: PathBuf,
}

impl InstanceFolder {
    pub fn for_home(home: &Path) -> Self {
        Self {
            path: home.join("rakazo-stack"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn has_settings(&self) -> bool {
        self.file("settings.json").exists()
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    /// Reads a file in the folder; `None` when it doesn't exist.
    pub fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        match fs::read(self.file(name)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Writes a file in the folder through a temporary file, so a crash never leaves it
    /// half-written, and sets its mode to 600 (spec §6: modes are re-applied every run).
    pub fn write(&self, name: &str, contents: &[u8]) -> io::Result<()> {
        let target = self.file(name);
        let temp = self.file(&format!(".{name}.tmp"));
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&temp)?;
            file.write_all(contents)?;
            file.sync_all()?;
        }
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
        fs::rename(temp, target)
    }

    /// The folder at 700 and every file in it at 600, on every run (spec §6).
    pub fn reapply_modes(&self) -> io::Result<()> {
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o700))?;
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o600))?;
            }
        }
        Ok(())
    }

    /// Takes the lock that `setup`, `update`, `backup` and `restore` share. It is held
    /// until the returned guard drops, and released by the OS if the process dies.
    pub fn lock(&self) -> Result<InstanceLock, LockError> {
        fs::create_dir_all(&self.path).map_err(LockError::Io)?;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o700))
            .map_err(LockError::Io)?;
        let file = File::create(self.path.join(".lock")).map_err(LockError::Io)?;
        match file.try_lock() {
            Ok(()) => Ok(InstanceLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(LockError::Held),
            Err(TryLockError::Error(err)) => Err(LockError::Io(err)),
        }
    }
}

pub struct InstanceLock {
    _file: File,
}

#[derive(Debug)]
pub enum LockError {
    Held,
    Io(io::Error),
}

#[cfg(test)]
mod tests {
    use super::{InstanceFolder, LockError};

    #[test]
    fn a_second_lock_is_refused_until_the_first_drops() {
        let home = tempfile::tempdir().unwrap();
        let folder = InstanceFolder::for_home(home.path());
        let first = folder.lock().unwrap();
        assert!(matches!(folder.lock(), Err(LockError::Held)));
        drop(first);
        assert!(folder.lock().is_ok());
    }

    #[test]
    fn writes_files_with_mode_600() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let folder = InstanceFolder::for_home(home.path());
        let _lock = folder.lock().unwrap();
        folder.write("secrets.env", b"A=1\n").unwrap();
        folder.write("secrets.env", b"A=2\n").unwrap();
        assert_eq!(folder.read("secrets.env").unwrap().unwrap(), b"A=2\n");
        let mode = std::fs::metadata(folder.file("secrets.env"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(folder.read("missing").unwrap(), None);
    }
}
