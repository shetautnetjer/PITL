//! End-to-end tests: a real unix socket client playing askpass, and the real
//! HTTP router playing the phone.

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use pitl_daemon::{socket, web, App, Config};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tower::ServiceExt;

const SECRET: &str = "correct-horse-battery-staple-7731";

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
        };
        let app = App::new(cfg).unwrap();
        let listener = socket::bind(&app.cfg.socket_path).unwrap();
        tokio::spawn(socket::serve(listener, app.clone()));
        Self { app, dir }
    }

    /// Connect like askpass, ask, and return the stream plus the request id.
    async fn ask(&self) -> (BufReader<UnixStream>, String) {
        let mut s = UnixStream::connect(&self.app.cfg.socket_path)
            .await
            .unwrap();
        s.write_all(b"{\"op\":\"ask\",\"prompt\":\"[sudo] password for test: \"}\n")
            .await
            .unwrap();
        let mut r = BufReader::new(s);
        let mut line = String::new();
        r.read_line(&mut line).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["status"], "pending", "got {line}");
        let id = v["id"].as_str().unwrap().to_string();
        assert_eq!(id.len(), 32);
        assert_eq!(v["url"], format!("https://pitl.test/r/{id}"));
        (r, id)
    }

    async fn http(&self, method: &str, uri: &str, body: &str) -> (StatusCode, String) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("tailscale-user-login", "owner@example.com")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = web::router(self.app.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    async fn approve(&self, id: &str) -> StatusCode {
        self.http(
            "POST",
            &format!("/r/{id}/approve"),
            &format!("password={SECRET}"),
        )
        .await
        .0
    }

    fn audit(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("state/audit.jsonl")).unwrap_or_default()
    }

    fn state(&self, id: &str) -> &'static str {
        self.app
            .store
            .view(pitl_core::RequestId::parse(id).unwrap())
            .unwrap()
            .state
    }
}

async fn read_status(r: &mut BufReader<UnixStream>) -> String {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), r.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap();
    v["status"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn approve_delivers_secret_once() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let (mut r, id) = h.ask().await;

    let (status, page) = h.http("GET", &format!("/r/{id}"), "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("test-machine"));
    assert!(page.contains("type=\"password\""));

    assert_eq!(h.approve(&id).await, StatusCode::OK);
    assert_eq!(read_status(&mut r).await, "secret");
    let mut secret = String::new();
    r.read_line(&mut secret).await.unwrap();
    assert_eq!(secret, format!("{SECRET}\n"));
    // Nothing after the secret; the daemon closed the connection.
    let mut rest = Vec::new();
    r.read_to_end(&mut rest).await.unwrap();
    assert!(rest.is_empty());

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(h.state(&id), "consumed");
}

#[tokio::test]
async fn replay_and_double_consume_are_refused() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let (mut r, id) = h.ask().await;
    assert_eq!(h.approve(&id).await, StatusCode::OK);
    assert_eq!(read_status(&mut r).await, "secret");
    let mut secret = String::new();
    r.read_line(&mut secret).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Same link again: refused, nothing stored.
    assert_eq!(h.approve(&id).await, StatusCode::CONFLICT);
    let (status, page) = h.http("POST", &format!("/r/{id}/deny"), "").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(!page.contains(SECRET));
    // The page now says it was used, and offers no form.
    let (status, page) = h.http("GET", &format!("/r/{id}"), "").await;
    assert_eq!(status, StatusCode::GONE);
    assert!(!page.contains("type=\"password\""));
    // A direct second consume yields no secret.
    let id = pitl_core::RequestId::parse(&id).unwrap();
    assert!(matches!(
        h.app.store.consume(id),
        pitl_core::ConsumeOutcome::Finished(pitl_core::State::Consumed)
    ));
}

#[tokio::test]
async fn deny_sends_no_secret() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let (mut r, id) = h.ask().await;
    let (status, _) = h.http("POST", &format!("/r/{id}/deny"), "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read_status(&mut r).await, "denied");
    let mut rest = Vec::new();
    r.read_to_end(&mut rest).await.unwrap();
    assert!(rest.is_empty());
    assert_eq!(h.approve(&id).await, StatusCode::CONFLICT);
    assert_eq!(h.state(&id), "denied");
}

#[tokio::test]
async fn expiry_fails_closed() {
    let h = Harness::start(Duration::from_millis(300)).await;
    let (mut r, id) = h.ask().await;
    assert_eq!(read_status(&mut r).await, "expired");
    assert_eq!(h.approve(&id).await, StatusCode::CONFLICT);
    assert_eq!(h.state(&id), "expired");
    assert!(h.audit().contains("\"event\":\"expired\""));
}

#[tokio::test]
async fn disconnect_cancels_and_blocks_later_approval() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let (r, id) = h.ask().await;
    drop(r);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(h.state(&id), "canceled");
    assert_eq!(h.approve(&id).await, StatusCode::CONFLICT);
    assert!(h.audit().contains("\"event\":\"canceled\""));
}

#[tokio::test]
async fn audit_has_metadata_but_never_the_secret() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let (mut r, id) = h.ask().await;
    assert_eq!(h.approve(&id).await, StatusCode::OK);
    assert_eq!(read_status(&mut r).await, "secret");
    let mut secret = String::new();
    r.read_line(&mut secret).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let audit = h.audit();
    assert!(!audit.contains(SECRET), "secret leaked into audit log");
    for event in ["created", "authorized", "consumed"] {
        assert!(
            audit.contains(&format!("\"event\":\"{event}\"")),
            "missing {event} in {audit}"
        );
    }
    assert!(audit.contains("owner@example.com"));
    for line in audit.lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["id"], id);
        assert!(v["time"].as_str().unwrap().ends_with('Z'));
    }
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(h.dir.path().join("state/audit.jsonl"))
            .unwrap()
            .permissions(),
    );
    assert_eq!(mode & 0o777, 0o600);
}

#[tokio::test]
async fn socket_is_private_and_bad_input_is_rejected() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(&h.app.cfg.socket_path)
            .unwrap()
            .permissions(),
    );
    assert_eq!(mode & 0o777, 0o600);

    let (status, _) = h.http("GET", "/r/not-a-valid-id", "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = h.http("GET", &format!("/r/{}", "0".repeat(32)), "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_r, id) = h.ask().await;
    let (status, _) = h
        .http("POST", &format!("/r/{id}/approve"), "password=a%0Ab")
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = h
        .http("POST", &format!("/r/{id}/approve"), "password=")
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(h.state(&id), "pending");
    let big = format!("password={}", "x".repeat(10_000));
    let (status, _) = h.http("POST", &format!("/r/{id}/approve"), &big).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn page_escapes_and_sets_security_headers() {
    let h = Harness::start(Duration::from_secs(60)).await;
    let mut s = UnixStream::connect(&h.app.cfg.socket_path).await.unwrap();
    s.write_all(b"{\"op\":\"ask\",\"prompt\":\"<script>alert(1)</script>\"}\n")
        .await
        .unwrap();
    let mut r = BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line).await.unwrap();
    let id = serde_json::from_str::<serde_json::Value>(&line).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let req = Request::builder()
        .uri(format!("/r/{id}"))
        .body(Body::empty())
        .unwrap();
    let res = web::router(h.app.clone()).oneshot(req).await.unwrap();
    assert_eq!(res.headers()["cache-control"], "no-store");
    assert_eq!(res.headers()["referrer-policy"], "no-referrer");
    assert!(res.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("default-src 'none'"));
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    let page = String::from_utf8_lossy(&bytes);
    assert!(!page.contains("<script>alert(1)"));
    assert!(page.contains("&lt;script&gt;"));
}
