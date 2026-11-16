//! pitld: holds pending requests in memory, answers askpass over a 0600 unix
//! socket, and serves the phone page over HTTP (meant to sit behind Tailscale
//! Serve, or on the tailnet address).

pub mod audit;
pub mod socket;
pub mod web;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pitl_core::Store;

/// Longest a request may wait for an answer.
pub const MAX_TTL: Duration = Duration::from_secs(10 * 60);
pub const DEFAULT_TTL: Duration = Duration::from_secs(3 * 60);

#[derive(Debug, Clone)]
pub struct Config {
    pub socket_path: PathBuf,
    pub listen: SocketAddr,
    /// Base of the links sent to the phone, for example
    /// `https://machine.tailnet.ts.net`. No trailing slash.
    pub public_url: String,
    pub audit_path: PathBuf,
    pub ttl: Duration,
    pub machine: String,
    /// True when the HTTP listener is only reachable through Tailscale Serve,
    /// so its `Tailscale-User-*` identity headers can be trusted.
    pub behind_serve: bool,
}

pub struct App {
    pub cfg: Config,
    pub store: Store,
    pub audit: audit::AuditLog,
}

impl App {
    pub fn new(cfg: Config) -> std::io::Result<Arc<Self>> {
        let audit = audit::AuditLog::open(&cfg.audit_path)?;
        let store = Store::new(cfg.ttl);
        Ok(Arc::new(Self { cfg, store, audit }))
    }

    pub fn link(&self, id: &str) -> String {
        format!("{}/r/{}", self.cfg.public_url.trim_end_matches('/'), id)
    }
}
