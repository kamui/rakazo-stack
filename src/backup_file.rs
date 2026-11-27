//! The Backup file (spec §15): a tar archive of the Backup's parts, encrypted with the
//! Backup passphrase in the `age` format, so the `age` and `tar` CLIs can open it by hand.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::iter;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use age::secrecy::SecretString;
use age::{Decryptor, Encryptor, Identity, Recipient, scrypt, x25519};

pub const EXTENSION: &str = "tar.age";

/// The Backup passphrase. It is never saved, logged or printed, so it has no `Display`
/// and its `Debug` hides it.
#[derive(Clone, PartialEq, Eq)]
pub struct Passphrase(String);

impl Passphrase {
    pub fn new(text: String) -> Option<Self> {
        (!text.is_empty()).then_some(Self(text))
    }

    fn secret(&self) -> SecretString {
        SecretString::from(self.0.clone())
    }
}

impl fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Passphrase(hidden)")
    }
}

/// One file in the Backup's tar archive. A Backup holds them in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    Manifest,
    Settings,
    Secrets,
    RakazoDump,
    HindsightDump,
    Appdata,
    TailscaleState,
}

impl Part {
    const ALL: [Self; 7] = [
        Self::Manifest,
        Self::Settings,
        Self::Secrets,
        Self::RakazoDump,
        Self::HindsightDump,
        Self::Appdata,
        Self::TailscaleState,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Manifest => "manifest.json",
            Self::Settings => "settings.json",
            Self::Secrets => "secrets.env",
            Self::RakazoDump => "rakazo.dump",
            Self::HindsightDump => "hindsight.dump",
            Self::Appdata => "appdata.tar",
            Self::TailscaleState => "tailscale-state.tar",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|part| part.name() == name)
    }

    /// Hindsight's dump and `tailscale-state` are there only when their volumes exist.
    fn required(self) -> bool {
        !matches!(self, Self::HindsightDump | Self::TailscaleState)
    }
}

/// Parts wait here until the Backup file is written, because a tar header needs each
/// part's size before its bytes. They are encrypted to a key that only this process
/// holds, so a run that dies leaves nothing readable, even on an `--output` drive.
pub struct Spool {
    dir: tempfile::TempDir,
    identity: x25519::Identity,
    parts: Vec<(Part, PathBuf, u64)>,
}

impl Spool {
    pub fn new_in(folder: &Path) -> io::Result<Self> {
        Ok(Self {
            dir: tempfile::Builder::new()
                .prefix(".rakazo-stack-backup-")
                .tempdir_in(folder)?,
            identity: x25519::Identity::generate(),
            parts: Vec::new(),
        })
    }

    /// Writes one part through `fill`, which returns once it has written all of it.
    pub fn add(
        &mut self,
        part: Part,
        fill: impl FnOnce(&mut dyn Write) -> Result<(), String>,
    ) -> Result<(), String> {
        let path = self.dir.path().join(part.name());
        let recipient = self.identity.to_public();
        let write = || -> Result<u64, String> {
            let file = BufWriter::new(File::create(&path).map_err(|err| err.to_string())?);
            let encryptor = Encryptor::with_recipients(iter::once(&recipient as &dyn Recipient))
                .map_err(|err| err.to_string())?;
            let mut counted = Counted {
                inner: encryptor.wrap_output(file).map_err(|err| err.to_string())?,
                bytes: 0,
            };
            fill(&mut counted)?;
            counted
                .inner
                .finish()
                .and_then(|mut file| file.flush())
                .map_err(|err| err.to_string())?;
            Ok(counted.bytes)
        };
        let bytes = write()?;
        self.parts.push((part, path, bytes));
        Ok(())
    }

    pub fn has(&self, part: Part) -> bool {
        self.parts.iter().any(|(p, ..)| *p == part)
    }

    fn open(&self, path: &Path) -> io::Result<impl Read + use<'_>> {
        let decryptor =
            Decryptor::new_buffered(BufReader::new(File::open(path)?)).map_err(io::Error::other)?;
        decryptor
            .decrypt(iter::once(&self.identity as &dyn Identity))
            .map_err(io::Error::other)
    }
}

struct Counted<W> {
    inner: W,
    bytes: u64,
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.bytes += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Writes the Backup file at `path`, which must not exist yet, with mode 600: the
/// `small` parts from memory and the rest from the spool.
pub fn write(
    path: &Path,
    passphrase: &Passphrase,
    small: &[(Part, &[u8])],
    spool: &Spool,
    mtime: u64,
) -> io::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    let encrypted =
        Encryptor::with_user_passphrase(passphrase.secret()).wrap_output(BufWriter::new(file))?;
    let mut archive = tar::Builder::new(encrypted);
    for part in Part::ALL {
        if let Some((_, bytes)) = small.iter().find(|(p, _)| *p == part) {
            append(&mut archive, part, bytes.len() as u64, *bytes, mtime)?;
        } else if let Some((_, spooled, size)) = spool.parts.iter().find(|(p, ..)| *p == part) {
            append(&mut archive, part, *size, spool.open(spooled)?, mtime)?;
        }
    }
    let file = archive.into_inner()?.finish()?;
    file.into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .sync_all()
}

fn append<W: Write>(
    archive: &mut tar::Builder<W>,
    part: Part,
    size: u64,
    data: impl Read,
    mtime: u64,
) -> io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_path(part.name())?;
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(size);
    header.set_mode(0o600);
    header.set_mtime(mtime);
    header.set_cksum();
    let mut counted = data.take(size);
    archive.append(&header, &mut counted)?;
    if counted.limit() == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{} was shorter than its {size} bytes",
            part.name()
        )))
    }
}

/// Decrypts the Backup file and hands each part to `each`, in order. Fails when the
/// passphrase doesn't open it, a part is unknown, repeated or missing, or the file was
/// cut short.
pub fn check(
    path: &Path,
    passphrase: &Passphrase,
    mut each: impl FnMut(Part, &mut dyn Read) -> Result<(), String>,
) -> Result<Vec<Part>, String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let decryptor = Decryptor::new_buffered(BufReader::new(file))
        .map_err(|err| format!("it isn't an age file: {err}"))?;
    let mut identity = scrypt::Identity::new(passphrase.secret());
    identity.set_max_work_factor(30);
    let plain = decryptor
        .decrypt(iter::once(&identity as &dyn Identity))
        .map_err(|err| format!("the passphrase doesn't open it: {err}"))?;
    let mut archive = tar::Archive::new(plain);
    let mut seen: Vec<Part> = Vec::new();
    for entry in archive.entries().map_err(|err| err.to_string())? {
        let mut entry = entry.map_err(|err| err.to_string())?;
        let name = entry
            .path()
            .map_err(|err| err.to_string())?
            .to_string_lossy()
            .into_owned();
        let part = Part::from_name(&name)
            .filter(|_| entry.header().entry_type() == tar::EntryType::Regular)
            .ok_or_else(|| format!("it holds {name:?}, which no Backup has"))?;
        if seen.last().is_some_and(|last| *last >= part) {
            return Err(format!("{name} is out of place"));
        }
        each(part, &mut entry).map_err(|err| format!("{name}: {err}"))?;
        io::copy(&mut entry, &mut io::sink()).map_err(|err| format!("{name}: {err}"))?;
        seen.push(part);
    }
    // Reading to the end checks the last chunk's tag, which catches a cut-off file.
    io::copy(&mut archive.into_inner(), &mut io::sink()).map_err(|err| err.to_string())?;
    if let Some(missing) = Part::ALL
        .into_iter()
        .find(|part| part.required() && !seen.contains(part))
    {
        return Err(format!("it has no {}", missing.name()));
    }
    Ok(seen)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passphrase() -> Passphrase {
        Passphrase::new("correct horse battery staple".into()).unwrap()
    }

    fn spool_with(dir: &Path, parts: &[(Part, &[u8])]) -> Spool {
        let mut spool = Spool::new_in(dir).unwrap();
        for (part, bytes) in parts {
            spool
                .add(*part, |out| out.write_all(bytes).map_err(|e| e.to_string()))
                .unwrap();
        }
        spool
    }

    fn read_all(path: &Path, passphrase: &Passphrase) -> Result<Vec<(Part, Vec<u8>)>, String> {
        let mut parts = Vec::new();
        check(path, passphrase, |part, data| {
            let mut bytes = Vec::new();
            data.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            parts.push((part, bytes));
            Ok(())
        })?;
        Ok(parts)
    }

    #[test]
    fn writes_every_part_in_order_and_reads_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let spool = spool_with(
            dir.path(),
            &[
                (Part::Appdata, b"appdata archive"),
                (Part::RakazoDump, b"rakazo dump"),
                (Part::HindsightDump, &[7; 70_000]),
            ],
        );
        assert_eq!(spool.parts[2].2, 70_000);
        assert!(!spool.has(Part::TailscaleState));
        let path = dir.path().join("backup.tar.age");
        let small: [(Part, &[u8]); 3] = [
            (Part::Secrets, b"A=1\n"),
            (Part::Manifest, b"{}"),
            (Part::Settings, b"{\"schema\":1}"),
        ];
        write(&path, &passphrase(), &small, &spool, 1_795_694_461).unwrap();

        let parts = read_all(&path, &passphrase()).unwrap();
        let names: Vec<_> = parts.iter().map(|(part, _)| part.name()).collect();
        assert_eq!(
            names,
            [
                "manifest.json",
                "settings.json",
                "secrets.env",
                "rakazo.dump",
                "hindsight.dump",
                "appdata.tar"
            ]
        );
        assert_eq!(parts[2].1, b"A=1\n");
        assert_eq!(parts[4].1, vec![7; 70_000]);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(write(&path, &passphrase(), &small, &spool, 0).is_err());
    }

    #[test]
    fn spooled_parts_are_unreadable_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let secret = b"a home with a browser profile";
        let spool = spool_with(dir.path(), &[(Part::Appdata, secret)]);
        let on_disk = std::fs::read(&spool.parts[0].1).unwrap();
        assert!(on_disk.starts_with(b"age-encryption.org/v1\n"));
        assert!(!on_disk.windows(secret.len()).any(|w| w == secret));
        let spool_dir = spool.dir.path().to_path_buf();
        drop(spool);
        assert!(!spool_dir.exists());
    }

    #[test]
    fn the_check_refuses_a_wrong_passphrase_a_cut_file_and_a_missing_part() {
        let dir = tempfile::tempdir().unwrap();
        let spool = spool_with(
            dir.path(),
            &[
                (Part::RakazoDump, &[1; 200_000]),
                (Part::Appdata, b"appdata"),
            ],
        );
        let small: [(Part, &[u8]); 3] = [
            (Part::Manifest, b"{}"),
            (Part::Settings, b"{}"),
            (Part::Secrets, b"A=1\n"),
        ];
        let path = dir.path().join("whole.tar.age");
        write(&path, &passphrase(), &small, &spool, 0).unwrap();

        let wrong = Passphrase::new("not the passphrase".into()).unwrap();
        let err = read_all(&path, &wrong).unwrap_err();
        assert!(err.starts_with("the passphrase doesn't open it"), "{err}");

        let bytes = std::fs::read(&path).unwrap();
        let cut = dir.path().join("cut.tar.age");
        std::fs::write(&cut, &bytes[..bytes.len() - 100]).unwrap();
        assert!(read_all(&cut, &passphrase()).is_err());

        let without_appdata = spool_with(dir.path(), &[(Part::RakazoDump, b"dump")]);
        let partial = dir.path().join("partial.tar.age");
        write(&partial, &passphrase(), &small, &without_appdata, 0).unwrap();
        assert_eq!(
            read_all(&partial, &passphrase()).unwrap_err(),
            "it has no appdata.tar"
        );
    }

    #[test]
    fn a_part_that_fails_its_check_fails_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let spool = spool_with(
            dir.path(),
            &[(Part::RakazoDump, b"dump"), (Part::Appdata, b"appdata")],
        );
        let small: [(Part, &[u8]); 3] = [
            (Part::Manifest, b"{}"),
            (Part::Settings, b"{}"),
            (Part::Secrets, b""),
        ];
        let path = dir.path().join("backup.tar.age");
        write(&path, &passphrase(), &small, &spool, 0).unwrap();
        let err = check(&path, &passphrase(), |part, _| match part {
            Part::RakazoDump => Err("pg_restore --list exited with 1".into()),
            _ => Ok(()),
        })
        .unwrap_err();
        assert_eq!(err, "rakazo.dump: pg_restore --list exited with 1");
    }

    #[test]
    fn a_failed_fill_keeps_the_part_out() {
        let dir = tempfile::tempdir().unwrap();
        let mut spool = Spool::new_in(dir.path()).unwrap();
        let err = spool
            .add(Part::RakazoDump, |out| {
                out.write_all(b"half").unwrap();
                Err("pg_dump exited with 1".into())
            })
            .unwrap_err();
        assert_eq!(err, "pg_dump exited with 1");
        assert!(!spool.has(Part::RakazoDump));
    }

    #[test]
    fn hides_the_passphrase() {
        assert_eq!(format!("{:?}", passphrase()), "Passphrase(hidden)");
        assert_eq!(Passphrase::new(String::new()), None);
    }
}
