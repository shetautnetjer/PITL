//! End-to-end tests for store requests (`pitl store --name ...`): the value
//! goes from the phone page into a 0600 file, and never back to the client,
//! the page, or the audit log.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use pitl_daemon::{socket, web, App, Config};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tower::ServiceExt;

const TOKEN: &str = "ops_TESTTOKEN-not-real-4f1c9a7e2b";

struct Harness {
    app: Arc<App>,
    dir: tempfile::TempDir,
}

impl Harness {
    async fn start(ttl: Duration) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            socket_path: dir.path().join("run/pitld.sock"),
            listen: "127.0.0.1:0".parse().unwrap(),
            public_url: "https://pitl.test".into(),
            audit_path: dir.path().join("state/audit.jsonl"),
            ttl,
            machine: "test-machine".into(),
            behind_serve: true,
            secrets_dir: dir.path().join("config/pitl/secrets"),
        };
        let app = App::new(cfg).unwrap();
        let listener = socket::bind(&app.cfg.socket_path).unwrap();
        tokio::spawn(socket::serve(listener, app.clone()));
        Self { app, dir }
    }

    fn secret_path(&self, name: &str) -> PathBuf {
        self.app.cfg.secrets_dir.join(name)
    }

    /// Connect like `pitl store --name <name>`.
    async fn store(&self, name: &str) -> (BufReader<UnixStream>, String) {
        let mut s = UnixStream::connect(&self.app.cfg.socket_path)
            .await
            .unwrap();
        s.write_all(format!("{{\"op\":\"store\",\"name\":\"{name}\"}}\n").as_bytes())
            .await
            .unwrap();
        let mut r = BufReader::new(s);
        let mut line = String::new();
        r.read_line(&mut line).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["status"], "pending", "got {line}");
        (r, v["id"].as_str().unwrap().to_string())
    }

    async fn http(&self, method: &str, uri: &str, body: &str) -> (StatusCode, String) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("tailscale-user-login", "owner@example.com")
            .header("x-forwarded-for", "100.64.0.9")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = web::router(self.app.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn approve(&self, id: &str, value: &str) -> (StatusCode, String) {
        self.http(
            "POST",
            &format!("/r/{id}/approve"),
            &format!("password={value}"),
        )
        .await
    }

    fn audit(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("state/audit.jsonl")).unwrap_or_default()
    }
}

/// Everything the client receives after the pending line, until close.
async fn rest(r: &mut BufReader<UnixStream>) -> String {
    let mut out = String::new();
    tokio::time::timeout(Duration::from_secs(5), r.read_to_string(&mut out))
        .await
        .unwrap()
        .unwrap();
    out
}

fn mode(p: &std::path::Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn approved_value_lands_in_private_file_only() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let (mut r, id) = h.store("op-service-account").await;

    // The page names the secret and where it goes, and asks for the value.
    let (status, page) = h.http("GET", &format!("/r/{id}"), "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Store a secret"));
    assert!(page.contains("op-service-account"));
    assert!(page.contains(&h.secret_path("op-service-account").display().to_string()));

    let (status, page) = h.approve(&id, TOKEN).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!page.contains(TOKEN), "page echoed the value");

    // The client hears only that it was stored, and where.
    let reply = rest(&mut r).await;
    assert!(!reply.contains(TOKEN), "value sent back to client");
    let v: serde_json::Value = serde_json::from_str(reply.trim()).unwrap();
    assert_eq!(v["status"], "stored");
    assert_eq!(v["name"], "op-service-account");
    assert_eq!(reply.lines().count(), 1);

    // The file holds exactly the value, privately.
    let file = h.secret_path("op-service-account");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), TOKEN);
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(&h.app.cfg.secrets_dir), 0o700);

    // The audit log has name, time, device and outcome, never the value.
    let audit = h.audit();
    assert!(!audit.contains(TOKEN), "value leaked into audit log");
    for event in ["created", "authorized", "stored"] {
        assert!(audit.contains(&format!("\"event\":\"{event}\"")), "{audit}");
    }
    for line in audit.lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["kind"], "store");
        assert_eq!(v["secret_name"], "op-service-account");
        assert!(v["time"].as_str().unwrap().ends_with('Z'));
    }
    assert!(audit.contains("100.64.0.9"));
}

#[tokio::test]
async fn link_cannot_be_reused_and_page_hides_value() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let (mut r, id) = h.store("tok").await;
    assert_eq!(h.approve(&id, TOKEN).await.0, StatusCode::OK);
    rest(&mut r).await;

    let (status, _) = h.approve(&id, "replacement-value").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        std::fs::read_to_string(h.secret_path("tok")).unwrap(),
        TOKEN
    );

    let (status, page) = h.http("GET", &format!("/r/{id}"), "").await;
    assert_eq!(status, StatusCode::GONE);
    assert!(page.contains("stored"));
    assert!(!page.contains(TOKEN));
    assert!(!page.contains("type=\"password\""));
}

#[tokio::test]
async fn deny_writes_nothing() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let (mut r, id) = h.store("tok").await;
    let (status, _) = h.http("POST", &format!("/r/{id}/deny"), "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(rest(&mut r).await.contains("\"denied\""));
    assert!(!h.secret_path("tok").exists());
    assert_eq!(h.approve(&id, TOKEN).await.0, StatusCode::CONFLICT);
    assert!(!h.secret_path("tok").exists());
}

#[tokio::test]
async fn expiry_writes_nothing() {
    let h = Harness::start(Duration::from_millis(300)).await;
    let (mut r, id) = h.store("tok").await;
    assert!(rest(&mut r).await.contains("\"expired\""));
    assert_eq!(h.approve(&id, TOKEN).await.0, StatusCode::CONFLICT);
    assert!(!h.secret_path("tok").exists());
    assert!(h.audit().contains("\"event\":\"expired\""));
}

#[tokio::test]
async fn disconnect_writes_nothing() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let (r, id) = h.store("tok").await;
    drop(r);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(h.approve(&id, TOKEN).await.0, StatusCode::CONFLICT);
    assert!(!h.secret_path("tok").exists());
}

#[tokio::test]
async fn unsafe_names_are_refused_before_asking() {
    let h = Harness::start(Duration::from_secs(60)).await;
    for name in ["../escape", "a/b", ".hidden", "UPPER", ""] {
        let mut s = UnixStream::connect(&h.app.cfg.socket_path).await.unwrap();
        let msg = serde_json::json!({"op": "store", "name": name}).to_string() + "\n";
        s.write_all(msg.as_bytes()).await.unwrap();
        let mut r = BufReader::new(s);
        let mut line = String::new();
        r.read_line(&mut line).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["status"], "error", "{name:?} -> {line}");
    }
    assert!(h.app.store.pending().is_empty());
    assert!(!h.dir.path().join("config/pitl/escape").exists());
}

#[tokio::test]
async fn replacing_keeps_file_private() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let (mut r, id) = h.store("tok").await;
    h.approve(&id, "first-value").await;
    rest(&mut r).await;
    let file = h.secret_path("tok");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

    let (mut r, id) = h.store("tok").await;
    h.approve(&id, TOKEN).await;
    assert!(rest(&mut r).await.contains("\"stored\""));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), TOKEN);
    assert_eq!(mode(&file), 0o600);
}
