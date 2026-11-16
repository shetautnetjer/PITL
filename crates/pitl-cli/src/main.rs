//! pitl: small helper CLI. It never sees or handles secrets.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use pitl_core::protocol::{ClientMessage, DaemonMessage};

const USAGE: &str = "\
pitl: PITL helper

Usage:
  pitl list          Show pending requests and their phone links
  pitl audit [N]     Show the last N audit records (default 20)
  pitl paths         Show the socket and audit log locations
";

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
        Some("list") => list(),
        Some("audit") => audit(args.get(1).and_then(|n| n.parse().ok()).unwrap_or(20)),
        Some("paths") => {
            println!("socket: {}", pitl_core::paths::socket_path().display());
            println!("audit:  {}", pitl_core::paths::audit_path().display());
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
