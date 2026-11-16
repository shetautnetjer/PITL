//! pitld: the PITL daemon.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use pitl_daemon::{socket, web, App, Config, DEFAULT_TTL, MAX_TTL};

const USAGE: &str = "\
pitld: PITL daemon (password in the loop)

Usage: pitld [options]

Options:
  --public-url URL   Base of links sent to the phone (for example the
                     Tailscale Serve address). Default: http://<listen>
  --listen ADDR      HTTP listen address. Default: 127.0.0.1:8787
  --behind-serve     The listener is reachable only through Tailscale Serve,
                     so trust its Tailscale-User-* identity headers
  --socket PATH      Unix socket. Default: $XDG_RUNTIME_DIR/pitl/pitld.sock
  --audit PATH       Audit log. Default: ~/.local/state/pitl/audit.jsonl
  --ttl-secs N       How long a request waits, in seconds (max 600). Default: 180
  -h, --help         Show this help
";

fn parse() -> Result<Config, String> {
    let mut listen: SocketAddr = "127.0.0.1:8787".parse().unwrap();
    let mut public_url: Option<String> = None;
    let mut socket_path = pitl_core::paths::socket_path();
    let mut audit_path = pitl_core::paths::audit_path();
    let mut ttl = DEFAULT_TTL;
    let mut behind_serve = false;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--listen" => {
                listen = val("--listen")?
                    .parse()
                    .map_err(|e| format!("--listen: {e}"))?
            }
            "--public-url" => public_url = Some(val("--public-url")?),
            "--socket" => socket_path = PathBuf::from(val("--socket")?),
            "--audit" => audit_path = PathBuf::from(val("--audit")?),
            "--ttl-secs" => {
                let n: u64 = val("--ttl-secs")?
                    .parse()
                    .map_err(|e| format!("--ttl-secs: {e}"))?;
                ttl = Duration::from_secs(n);
                if ttl.is_zero() || ttl > MAX_TTL {
                    return Err("--ttl-secs must be between 1 and 600".into());
                }
            }
            "--behind-serve" => behind_serve = true,
            other => return Err(format!("unknown option: {other}")),
        }
    }
    let public_url = public_url.unwrap_or_else(|| format!("http://{listen}"));
    Ok(Config {
        socket_path,
        listen,
        public_url: public_url.trim_end_matches('/').to_string(),
        audit_path,
        ttl,
        machine: pitl_core::proc_info::hostname(),
        behind_serve,
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    let cfg = match parse() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("pitld: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let app = match App::new(cfg.clone()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "pitld: cannot open audit log {}: {e}",
                cfg.audit_path.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let unix = match socket::bind(&cfg.socket_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("pitld: cannot bind {}: {e}", cfg.socket_path.display());
            return ExitCode::FAILURE;
        }
    };
    let tcp = match tokio::net::TcpListener::bind(cfg.listen).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("pitld: cannot listen on {}: {e}", cfg.listen);
            return ExitCode::FAILURE;
        }
    };

    eprintln!(
        "pitld {}: socket {}, page {} (links {}/r/...), requests wait {} s, audit {}",
        env!("CARGO_PKG_VERSION"),
        cfg.socket_path.display(),
        cfg.listen,
        cfg.public_url,
        cfg.ttl.as_secs(),
        cfg.audit_path.display()
    );

    tokio::spawn(socket::serve(unix, app.clone()));

    let shutdown = async {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = async { match term.as_mut() { Some(t) => { t.recv().await; } None => std::future::pending::<()>().await } } => {}
        }
    };
    let result = axum::serve(tcp, web::router(app))
        .with_graceful_shutdown(shutdown)
        .await;
    let _ = std::fs::remove_file(&cfg.socket_path);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pitld: http server error: {e}");
            ExitCode::FAILURE
        }
    }
}
