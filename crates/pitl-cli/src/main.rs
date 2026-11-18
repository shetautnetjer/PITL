//! pitl: small helper CLI. It never sees or handles secrets.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use pitl_core::protocol::{ClientMessage, DaemonMessage};

const USAGE: &str = "\
pitl: PITL helper

Usage:
  pitl store --name NAME
                     Ask a human, on their phone, for a secret that pitld
                     writes to ~/.config/pitl/secrets/NAME (0600). This
                     command never receives the value.
  pitl unlock        Ask the locked screen to take its password from your
                     phone. The lock screen checks it as if typed; this
                     command never receives it. Then run `pitl list` for
                     the link.
  pitl list          Show pending requests and their phone links
  pitl audit [N]     Show the last N audit records (default 20)
  pitl paths         Show the socket and audit log locations
";

/// Ask pitld to collect a secret from the phone and store it. Prints the link
/// on stderr and the outcome on stdout. Exit 0 only when stored.
fn store(name: &str) -> io::Result<bool> {
    pitl_core::secrets::validate_name(name)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let path = pitl_core::paths::socket_path();
    let s = UnixStream::connect(&path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("cannot reach pitld at {}: {e}", path.display()),
        )
    })?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(15 * 60)))?;
    (&s).write_all(
        &ClientMessage::Store {
            name: name.to_string(),
        }
        .to_line(),
    )?;
    let mut reader = BufReader::new(s);
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "pitld closed the connection",
            ));
        }
        match serde_json::from_str::<DaemonMessage>(&line) {
            Ok(DaemonMessage::Pending {
                url, expires_unix, ..
            }) => eprintln!(
                "pitl: open this on your phone to provide '{name}': {url} (expires {})",
                pitl_core::timefmt::rfc3339(expires_unix)
            ),
            Ok(DaemonMessage::Stored { name, path }) => {
                println!("stored '{name}' at {path}");
                return Ok(true);
            }
            Ok(DaemonMessage::Denied) => {
                println!("denied: nothing was stored");
                return Ok(false);
            }
            Ok(DaemonMessage::Expired) => {
                println!("expired: nothing was stored");
                return Ok(false);
            }
            Ok(DaemonMessage::Error { message }) => {
                println!("refused by pitld: {message}");
                return Ok(false);
            }
            // A store request is never answered with a secret; treat any
            // other reply as a failure without reading further.
            _ => {
                println!("failed: nothing was stored");
                return Ok(false);
            }
        }
    }
}

/// Ask the Omarchy lock screen to request its password through PITL. The
/// lock runs pitl-askpass itself and checks the answer with its own PAM
/// stack, so this only starts the question. Exit 0 when a request is out.
fn unlock() -> io::Result<bool> {
    let out = std::process::Command::new("omarchy")
        .args(["shell", "lock", "phoneUnlock"])
        .output()
        .map_err(|e| io::Error::new(e.kind(), format!("cannot run `omarchy shell`: {e}")))?;
    let answer = String::from_utf8_lossy(&out.stdout).trim().to_string();
    match answer.as_str() {
        "ok" | "pending" => {
            println!("asked: the lock screen is waiting for your phone (see `pitl list` for the link)");
            Ok(true)
        }
        "not-locked" => {
            println!("the screen is not locked");
            Ok(false)
        }
        _ => {
            println!(
                "the lock screen did not take the request (it answered {answer:?}): \
                 this Omarchy shell may not have `lock phoneUnlock` yet"
            );
            Ok(false)
        }
    }
}

fn list() -> io::Result<()> {
    let path = pitl_core::paths::socket_path();
    let mut s = UnixStream::connect(&path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("cannot reach pitld at {}: {e}", path.display()),
        )
    })?;
    s.write_all(&ClientMessage::List.to_line())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line)?;
    match serde_json::from_str::<DaemonMessage>(&line) {
        Ok(DaemonMessage::List { pending }) if pending.is_empty() => {
            println!("No pending requests.")
        }
        Ok(DaemonMessage::List { pending }) => {
            for p in pending {
                println!(
                    "{}  {} s left\n  command:    {}\n  started by: {}\n  link:       {}\n",
                    p["id"].as_str().unwrap_or("?"),
                    p["seconds_left"],
                    p["info"]["command"].as_str().unwrap_or("?"),
                    p["info"]["requester"].as_str().unwrap_or("?"),
                    p["url"].as_str().unwrap_or("?"),
                );
            }
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected reply from pitld",
            ))
        }
    }
    Ok(())
}

fn audit(n: usize) -> io::Result<()> {
    let path = pitl_core::paths::audit_path();
    let text = std::fs::read_to_string(&path)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
    let lines: Vec<&str> = text.lines().collect();
    for l in &lines[lines.len().saturating_sub(n)..] {
        println!("{l}");
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("store") => {
            let name = match (args.get(1).map(String::as_str), args.get(2)) {
                (Some("--name"), Some(n)) => n.clone(),
                _ => {
                    print!("{USAGE}");
                    return ExitCode::from(2);
                }
            };
            return match store(&name) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => ExitCode::FAILURE,
                Err(e) => {
                    eprintln!("pitl: {e}");
                    ExitCode::FAILURE
                }
            };
        }
        Some("unlock") => {
            return match unlock() {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => ExitCode::FAILURE,
                Err(e) => {
                    eprintln!("pitl: {e}");
                    ExitCode::FAILURE
                }
            };
        }
        Some("list") => list(),
        Some("audit") => audit(args.get(1).and_then(|n| n.parse().ok()).unwrap_or(20)),
        Some("paths") => {
            println!("socket: {}", pitl_core::paths::socket_path().display());
            println!("audit:  {}", pitl_core::paths::audit_path().display());
            println!("secrets: {}", pitl_core::secrets::default_dir().display());
            Ok(())
        }
        _ => {
            print!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pitl: {e}");
            ExitCode::FAILURE
        }
    }
}
