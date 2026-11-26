use std::fs::{self, File, TryLockError};
use std::io;
use std::os::unix::fs::PermissionsExt;
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
        self.path.join("settings.json").exists()
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
}
