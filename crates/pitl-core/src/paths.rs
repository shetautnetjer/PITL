//! Default locations. The socket lives in the user's runtime directory
//! (`/run/user/<uid>`), which only that user can enter.

use std::path::PathBuf;

/// The real uid of this process, read from `/proc/self/status`.
pub fn current_uid() -> Option<u32> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    crate::proc_info::parse_status_uid(&status)
}

/// `$PITL_SOCKET`, else `$XDG_RUNTIME_DIR/pitl/pitld.sock`, else
/// `/run/user/<uid>/pitl/pitld.sock`.
pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("PITL_SOCKET") {
        return PathBuf::from(p);
    }
    runtime_dir().join("pitl").join("pitld.sock")
}

fn runtime_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(p);
    }
    match current_uid() {
        Some(uid) => PathBuf::from(format!("/run/user/{uid}")),
        None => std::env::temp_dir(),
    }
}

/// `$XDG_STATE_HOME/pitl`, else `~/.local/state/pitl`.
pub fn state_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(p).join("pitl");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local").join("state").join("pitl")
}

pub fn audit_path() -> PathBuf {
    state_dir().join("audit.jsonl")
}
