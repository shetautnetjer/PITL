//! Names and locations for secrets stored by `pitl store`.

use std::path::{Path, PathBuf};

/// Longest secret name.
pub const MAX_NAME: usize = 64;

/// A secret name becomes a file name, so it is kept to a safe, boring set:
/// lowercase letters, digits, `-`, `_` and `.`, starting with a letter or
/// digit, at most 64 characters. No slashes, no `..`, no hidden files.
pub fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.len() > MAX_NAME {
        return Err("name must be 1 to 64 characters");
    }
    let first = name.as_bytes()[0];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err("name must start with a lowercase letter or a digit");
    }
    if !name
        .bytes()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_' | b'.'))
    {
        return Err("name may only use a-z, 0-9, '-', '_' and '.'");
    }
    if name.contains("..") {
        return Err("name may not contain '..'");
    }
    Ok(())
}

/// `$XDG_CONFIG_HOME/pitl/secrets`, else `~/.config/pitl/secrets`.
pub fn default_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(p).join("pitl").join("secrets");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("pitl").join("secrets")
}

/// The file a validated name is stored in.
pub fn path_for(dir: &Path, name: &str) -> Result<PathBuf, &'static str> {
    validate_name(name)?;
    Ok(dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_reasonable_names() {
        for n in [
            "op-service-account",
            "github_token",
            "a",
            "k8s.prod-1",
            "0abc",
        ] {
            assert!(validate_name(n).is_ok(), "{n}");
        }
    }

    #[test]
    fn rejects_unsafe_names() {
        for n in [
            "",
            "../x",
            "a/b",
            ".hidden",
            "-flag",
            "UPPER",
            "a..b",
            "sp ace",
            "nul\0",
            "new\nline",
            &"x".repeat(65),
        ] {
            assert!(validate_name(n).is_err(), "{n:?}");
        }
    }

    #[test]
    fn path_stays_in_dir() {
        let dir = Path::new("/tmp/pitl-secrets");
        assert_eq!(path_for(dir, "tok").unwrap(), dir.join("tok"));
        assert!(path_for(dir, "../../etc/passwd").is_err());
    }
}
