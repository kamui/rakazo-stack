use std::fs::{self, File, TryLockError};
use std::io::{self, Write};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, RawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::runlog;

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
        let text = || String::from_utf8_lossy(contents);
        match name {
            "secrets.env" => runlog::learn(|r| r.secrets_file(&text())),
            "settings.json" => runlog::learn(|r| r.settings_file(&text())),
            _ => {}
        }
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

    /// Writes a file of a build folder, such as `relay/src/relay.ts`. Docker's `COPY` keeps
    /// modes, and the image's user isn't the Self-hoster, so these are world-readable;
    /// the folder's own 700 still keeps other accounts out.
    pub fn write_build_file(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        let target = self.path.join(path);
        let mut dir = self.path.clone();
        for part in Path::new(path)
            .parent()
            .into_iter()
            .flat_map(Path::components)
        {
            dir.push(part);
            match fs::create_dir(&dir) {
                Err(err) if err.kind() != io::ErrorKind::AlreadyExists => return Err(err),
                _ => fs::set_permissions(&dir, fs::Permissions::from_mode(0o755))?,
            }
        }
        fs::write(&target, contents)?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644))
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
        take(file)
    }

    /// Takes over the lock an older program held, from the descriptor it left open
    /// across `exec` (spec §14 step 2). `fd` is the number it passed.
    pub fn adopt_lock(&self, fd: &str) -> Result<InstanceLock, LockError> {
        let fd: RawFd = fd
            .parse()
            .ok()
            .filter(|fd| *fd > 2)
            .ok_or(LockError::HandOver)?;
        // SAFETY: only asks whether the number is an open descriptor; a closed one fails.
        rustix::io::fcntl_getfd(unsafe { BorrowedFd::borrow_raw(fd) })
            .map_err(|_| LockError::HandOver)?;
        // SAFETY: it is open, and the older program left it to this one, so nothing else
        // in this process owns it.
        let file = unsafe { File::from_raw_fd(fd) };
        let lock_file = fs::metadata(self.path.join(".lock")).map_err(LockError::Io)?;
        let open = file.metadata().map_err(LockError::Io)?;
        if (open.dev(), open.ino()) != (lock_file.dev(), lock_file.ino()) {
            let _ = file.into_raw_fd();
            return Err(LockError::HandOver);
        }
        rustix::io::fcntl_setfd(&file, rustix::io::FdFlags::CLOEXEC)
            .map_err(|err| LockError::Io(err.into()))?;
        take(file)
    }
}

/// `flock` belongs to the open file, so the descriptor an older program held it through
/// already has it, and taking it again succeeds.
fn take(file: File) -> Result<InstanceLock, LockError> {
    match file.try_lock() {
        Ok(()) => Ok(InstanceLock { file }),
        Err(TryLockError::WouldBlock) => Err(LockError::Held),
        Err(TryLockError::Error(err)) => Err(LockError::Io(err)),
    }
}

pub struct InstanceLock {
    file: File,
}

impl InstanceLock {
    /// Leaves the lock's descriptor open across `exec`, so the program started next holds
    /// the lock with no gap, and returns its number.
    pub fn keep_across_exec(&self) -> io::Result<RawFd> {
        rustix::io::fcntl_setfd(&self.file, rustix::io::FdFlags::empty())?;
        Ok(self.file.as_raw_fd())
    }
}

#[derive(Debug)]
pub enum LockError {
    Held,
    /// The descriptor passed for a hand-over isn't the Instance's lock.
    HandOver,
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
    fn a_handed_over_descriptor_keeps_the_lock() {
        use std::os::fd::{AsRawFd, IntoRawFd};
        let home = tempfile::tempdir().unwrap();
        let folder = InstanceFolder::for_home(home.path());
        let first = folder.lock().unwrap();
        let fd = first.keep_across_exec().unwrap();
        let flags = rustix::io::fcntl_getfd(&first.file).unwrap();
        assert!(!flags.contains(rustix::io::FdFlags::CLOEXEC));
        // What exec leaves the next program: a descriptor of the same open file.
        let inherited = rustix::io::dup(&first.file).unwrap().into_raw_fd();
        assert_ne!(inherited, fd);

        let adopted = folder.adopt_lock(&inherited.to_string()).unwrap();
        drop(first);
        assert!(matches!(folder.lock(), Err(LockError::Held)));
        let flags = rustix::io::fcntl_getfd(&adopted.file).unwrap();
        assert!(flags.contains(rustix::io::FdFlags::CLOEXEC));
        drop(adopted);
        assert!(folder.lock().is_ok());

        let other = std::fs::File::open(home.path()).unwrap();
        for fd in [
            other.as_raw_fd().to_string(),
            "1".into(),
            "999999".into(),
            "x".into(),
        ] {
            assert!(
                matches!(folder.adopt_lock(&fd), Err(LockError::HandOver)),
                "{fd}"
            );
        }
        assert!(other.metadata().is_ok(), "a refused descriptor stays open");
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

    #[test]
    fn writes_build_folders_readable_by_the_images_user() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let folder = InstanceFolder::for_home(home.path());
        let _lock = folder.lock().unwrap();
        folder
            .write_build_file("relay/src/relay.ts", b"one")
            .unwrap();
        folder
            .write_build_file("relay/src/relay.ts", b"two")
            .unwrap();
        let mode = |path: &str| {
            std::fs::metadata(folder.file(path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode("relay"), 0o755);
        assert_eq!(mode("relay/src"), 0o755);
        assert_eq!(mode("relay/src/relay.ts"), 0o644);
        assert_eq!(folder.read("relay/src/relay.ts").unwrap().unwrap(), b"two");
    }
}
