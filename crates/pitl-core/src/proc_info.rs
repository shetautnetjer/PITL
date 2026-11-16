//! What the daemon learns about a requester, from the kernel rather than from
//! anything the requester says.
//!
//! The daemon gets the askpass process id from the socket's peer credentials
//! (SO_PEERCRED). Its parent is the program that wants the secret (sudo), and
//! that program's parent is whoever ran it. In compatibility mode all of these
//! run as the same user, so a hostile process of that user can still dress
//! itself up; see docs/THREAT-MODEL.md.

use std::fs;

/// Longest command line shown, in characters.
pub const MAX_CMDLINE: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSummary {
    pub pid: u32,
    pub ppid: u32,
    /// Command line with arguments separated by spaces, or `[comm]` when the
    /// command line is unreadable or empty.
    pub cmdline: String,
}

/// Parent pid from the contents of `/proc/<pid>/stat`. The process name may
/// contain spaces and parentheses, so parse after the last `)`.
pub fn parse_stat_ppid(stat: &str) -> Option<u32> {
    let after = &stat[stat.rfind(')')? + 1..];
    let mut fields = after.split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse().ok()
}

/// Real uid from the contents of `/proc/<pid>/status`.
pub fn parse_status_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|uid| uid.parse().ok())
}

/// A NUL-separated `/proc/<pid>/cmdline` as one readable line.
pub fn format_cmdline(raw: &[u8]) -> String {
    let parts: Vec<String> = raw
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    let mut s = parts.join(" ");
    if s.chars().count() > MAX_CMDLINE {
        s = s.chars().take(MAX_CMDLINE).collect::<String>() + "…";
    }
    // Control characters could be used to fake text on the page or in logs.
    s.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

pub fn read_proc(pid: u32) -> Option<ProcSummary> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let ppid = parse_stat_ppid(&stat)?;
    let raw = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let mut cmdline = format_cmdline(&raw);
    if cmdline.is_empty() {
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        cmdline = format!("[{}]", format_cmdline(comm.trim().as_bytes()));
    }
    Some(ProcSummary { pid, ppid, cmdline })
}

/// User name for a uid from `/etc/passwd`, or the number.
pub fn user_name(uid: u32) -> String {
    let passwd = fs::read_to_string("/etc/passwd").unwrap_or_default();
    passwd
        .lines()
        .find_map(|line| {
            let mut f = line.split(':');
            let name = f.next()?;
            let _pw = f.next()?;
            (f.next()?.parse::<u32>().ok()? == uid).then(|| name.to_string())
        })
        .unwrap_or_else(|| uid.to_string())
}

/// This machine's host name.
pub fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .or_else(|_| fs::read_to_string("/etc/hostname"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_with_tricky_name() {
        let stat = "1234 (evil) name) S 99 1234 1234 0 -1";
        assert_eq!(parse_stat_ppid(stat), Some(99));
    }

    #[test]
    fn status_uid() {
        assert_eq!(
            parse_status_uid("Name:\tx\nUid:\t1000\t1000\t1000\t1000\n"),
            Some(1000)
        );
    }

    #[test]
    fn cmdline_formatting() {
        assert_eq!(
            format_cmdline(b"sudo\0-A\0-k\0/usr/bin/id\0"),
            "sudo -A -k /usr/bin/id"
        );
        assert_eq!(format_cmdline(b"a\x1b[2Jb\0"), "a?[2Jb");
        assert!(format_cmdline(&vec![b'x'; 2000]).chars().count() <= MAX_CMDLINE + 1);
    }

    #[test]
    fn reads_self() {
        let me = std::process::id();
        let p = read_proc(me).unwrap();
        assert_eq!(p.pid, me);
        assert!(!p.cmdline.is_empty());
    }
}
