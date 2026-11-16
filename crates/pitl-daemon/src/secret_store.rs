//! Writing stored secrets to disk: directory 0700, file 0600, written to a
//! temporary file and renamed into place so a reader never sees half a secret.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use pitl_core::secrets::path_for;
use pitl_core::RequestId;

/// Create (or tighten) the secrets directory to 0700.
pub fn ensure_dir(dir: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// Write `value` to `<dir>/<name>` with mode 0600, replacing any older file.
/// Returns the final path. On error nothing is left behind.
pub fn write(dir: &Path, name: &str, value: &[u8]) -> io::Result<PathBuf> {
    let target = path_for(dir, name).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    ensure_dir(dir)?;
    // Real names never start with '.', so the temporary name can't collide.
    let suffix = RequestId::random()?.to_hex();
    let tmp = dir.join(format!(".{name}.tmp-{suffix}"));
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.set_permissions(fs::Permissions::from_mode(0o600))?;
        f.write_all(value)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, &target)?;
        File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map(|()| target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(p: &Path) -> u32 {
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn writes_private_file_and_replaces() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a/secrets");
        let p = write(&dir, "tok", b"one").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one");
        assert_eq!(mode(&p), 0o600);
        assert_eq!(mode(&dir), 0o700);

        // An older, looser file is replaced by a 0600 one.
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        let p2 = write(&dir, "tok", b"two").unwrap();
        assert_eq!(p, p2);
        assert_eq!(fs::read(&p2).unwrap(), b"two");
        assert_eq!(mode(&p2), 0o600);
        // No temporary files left over.
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("tok")]);
    }

    #[test]
    fn tightens_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("s");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        write(&dir, "x", b"v").unwrap();
        assert_eq!(mode(&dir), 0o700);
    }

    #[test]
    fn refuses_bad_names() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(write(tmp.path(), "../escape", b"v").is_err());
        assert!(write(tmp.path(), "a/b", b"v").is_err());
        assert!(!tmp.path().parent().unwrap().join("escape").exists());
    }
}
