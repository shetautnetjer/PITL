//! pitl-askpass: a `SUDO_ASKPASS` helper.
//!
//! sudo runs it with the prompt as the first argument and reads the password
//! from its stdout. This program asks pitld instead of a keyboard, tells the
//! caller (on stderr) where the human should answer, waits, and prints the
//! answer to stdout only. Anything other than an approved answer exits 1 with
//! nothing on stdout, which sudo treats as a failed password.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use pitl_core::protocol::{ClientMessage, DaemonMessage, MAX_LINE};
use zeroize::Zeroizing;

/// Upper bound on how long to wait for pitld, beyond any request's lifetime.
const WAIT_LIMIT_SECS: u64 = 15 * 60;

/// Read one line without buffering past it, so no extra bytes are copied.
fn read_line(s: &mut UnixStream, limit: usize) -> io::Result<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(Vec::new());
    let mut byte = [0u8; 1];
    loop {
        match s.read(&mut byte)? {
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "pitld closed the connection",
                ))
            }
            _ if byte[0] == b'\n' => return Ok(out),
            _ => {
                if out.len() >= limit {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
                }
                out.push(byte[0]);
            }
        }
    }
}

fn run() -> io::Result<bool> {
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let path = pitl_core::paths::socket_path();
    let mut s = UnixStream::connect(&path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("cannot reach pitld at {}: {e}", path.display()),
        )
    })?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(WAIT_LIMIT_SECS)))?;
    s.write_all(&ClientMessage::Ask { prompt }.to_line())?;

    loop {
        let line = read_line(&mut s, MAX_LINE)?;
        let msg: DaemonMessage = serde_json::from_slice(&line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        match msg {
            DaemonMessage::Pending {
                url, expires_unix, ..
            } => {
                eprintln!(
                    "pitl: waiting for a human to answer on their phone: {url} (expires {})",
                    pitl_core::timefmt::rfc3339(expires_unix)
                );
            }
            DaemonMessage::Secret => {
                let mut secret = read_line(&mut s, MAX_LINE)?;
                secret.push(b'\n');
                let mut out = io::stdout().lock();
                out.write_all(&secret)?;
                out.flush()?;
                return Ok(true);
            }
            DaemonMessage::Denied => {
                eprintln!("pitl: the request was denied");
                return Ok(false);
            }
            DaemonMessage::Expired => {
                eprintln!("pitl: the request expired without an answer");
                return Ok(false);
            }
            DaemonMessage::Canceled | DaemonMessage::Failed => {
                eprintln!("pitl: the request did not complete");
                return Ok(false);
            }
            DaemonMessage::Error { message } => {
                eprintln!("pitl: pitld refused the request: {message}");
                return Ok(false);
            }
            DaemonMessage::List { .. } => return Ok(false),
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("pitl-askpass: {e}");
            ExitCode::FAILURE
        }
    }
}
