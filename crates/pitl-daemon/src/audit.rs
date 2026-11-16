//! Append-only JSONL audit log, mode 0600. Only `AuditRecord`s are written,
//! and that type has no field for a secret.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::Mutex;

use pitl_core::audit::AuditRecord;

pub struct AuditLog {
    file: Mutex<File>,
}

impl AuditLog {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(dir)?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
        // Tighten a file that already existed with looser permissions.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    pub fn write(&self, record: &AuditRecord<'_>) {
        let line = record.to_line();
        let mut f = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = f.write_all(&line).and_then(|_| f.flush()) {
            eprintln!("pitld: audit write failed: {e}");
        }
    }
}
