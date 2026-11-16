//! The phone page. Everything rendered comes from pitld's own record of the
//! request. The page never echoes back what the human typed.

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Path, Request, State as AxState};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use pitl_core::audit::AuditRecord;
use pitl_core::state::RequestView;
use pitl_core::timefmt::rfc3339;
use pitl_core::{RequestId, RequestKind, State, TransitionError};
use serde::Deserialize;
use zeroize::{Zeroize, Zeroizing};

use crate::App;

const CSS: &str = include_str!("../static/pitl.css");
const JS: &str = include_str!("../static/pitl.js");

/// Largest form body accepted (the password plus field name).
pub const MAX_BODY: usize = 4096;

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/r/{id}", get(show))
        .route("/r/{id}/approve", post(approve))
        .route("/r/{id}/deny", post(deny))
        .route(
            "/static/pitl.css",
            get(|| async { ([(header::CONTENT_TYPE, "text/css")], CSS) }),
        )
        .route(
            "/static/pitl.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], JS) }),
        )
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn(security_headers))
        .with_state(app)
}

async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    for (k, v) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        (
            "content-security-policy",
            "default-src 'none'; style-src 'self'; script-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
        ),
    ] {
        h.insert(k, HeaderValue::from_static(v));
    }
    res
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<meta name=\"robots\" content=\"noindex\">\
<title>{}</title><link rel=\"stylesheet\" href=\"/static/pitl.css\"></head>\
<body><main>{}</main><script src=\"/static/pitl.js\"></script></body></html>",
        esc(title),
        body
    )
}

fn message(status: StatusCode, title: &str, text: &str) -> Response {
    let body = format!("<h1>{}</h1><p class=\"note\">{}</p>", esc(title), esc(text));
    (status, Html(page(title, &body))).into_response()
}

async fn index() -> Response {
    message(
        StatusCode::OK,
        "PITL",
        "PITL is running. Open the one-time link you were sent.",
    )
}

fn details(v: &RequestView) -> String {
    let i = &v.info;
    let what = match &i.kind {
        RequestKind::Sudo => String::new(),
        RequestKind::Store { name, path } => format!(
            "<dt>Secret name</dt><dd><code>{}</code></dd>\
<dt>Will be saved to</dt><dd><code>{}</code> (file readable only by {})</dd>",
            esc(name),
            esc(path),
            esc(&i.user),
        ),
    };
    let prompt = match i.kind {
        RequestKind::Sudo => format!(
            "<dt>Prompt text</dt><dd class=\"unverified\">{} <small>(supplied by the program, not verified)</small></dd>",
            esc(&i.prompt)
        ),
        RequestKind::Store { .. } => String::new(),
    };
    format!(
        "<dl>\
<dt>Machine</dt><dd>{}</dd>\
<dt>User</dt><dd>{} (uid {})</dd>\
{what}\
<dt>Command</dt><dd><code>{}</code></dd>\
<dt>Started by</dt><dd><code>{}</code> (pid {})</dd>\
<dt>Expires</dt><dd>{} <span id=\"left\" data-expires=\"{}\">({} s left)</span></dd>\
{prompt}\
</dl>",
        esc(&i.machine),
        esc(&i.user),
        i.uid,
        esc(&i.command),
        esc(&i.requester),
        i.requester_pid,
        esc(&rfc3339(v.expires_unix)),
        v.expires_unix,
        v.seconds_left,
    )
}

fn parse_id(id: &str) -> Option<RequestId> {
    RequestId::parse(id)
}

async fn show(AxState(app): AxState<Arc<App>>, Path(id): Path<String>) -> Response {
    let Some(id) = parse_id(&id) else {
        return not_found();
    };
    let Some(v) = app.store.view(id) else {
        return not_found();
    };
    if v.state != State::Pending.as_str() {
        return finished_page(&v);
    }
    let hex = v.id.clone();
    let (heading, label, button, note) = match &v.info.kind {
        RequestKind::Sudo => (
            "Password request".to_string(),
            format!("Password for {}", esc(&v.info.user)),
            "Approve and send",
            "The password goes from this page to the waiting program on that machine, once. It is not shown to the agent and not logged.",
        ),
        RequestKind::Store { name, .. } => (
            "Store a secret".to_string(),
            format!("Value for <code>{}</code>", esc(name)),
            "Store on this machine",
            "The value goes from this page straight into that file, once. It is not shown to the agent, not sent back to the program that asked, and not logged. Programs running as this user can read the file.",
        ),
    };
    let body = format!(
        "<h1>{heading}</h1>\
<p class=\"warn\">Only answer if you started this, or you expected it. If anything looks wrong, deny.</p>\
{}\
<form method=\"post\" action=\"/r/{hex}/approve\" autocomplete=\"off\">\
<label for=\"pw\">{label}</label>\
<input id=\"pw\" name=\"password\" type=\"password\" autocomplete=\"off\" autocapitalize=\"off\" spellcheck=\"false\" required>\
<button class=\"approve\" type=\"submit\">{button}</button></form>\
<form method=\"post\" action=\"/r/{hex}/deny\"><button class=\"deny\" type=\"submit\">Deny</button></form>\
<p class=\"note\">{note}</p>",
        details(&v),
    );
    Html(page("PITL request", &body)).into_response()
}

fn finished_page(v: &RequestView) -> Response {
    let text = match v.state {
        "consumed" => match v.info.kind {
            RequestKind::Sudo => "This request was answered and used. The link no longer works.",
            RequestKind::Store { .. } => "The secret was stored. The link no longer works.",
        },
        "authorized" => "Answered. Delivering it now.",
        "denied" => "This request was denied.",
        "expired" => "This request expired before it was answered.",
        "canceled" => "The program that asked stopped waiting. Nothing was sent.",
        _ => "This request failed. Nothing more will be sent.",
    };
    let body = format!(
        "<h1>Request {}</h1><p class=\"note\">{}</p>{}",
        esc(v.state),
        esc(text),
        details(v)
    );
    (StatusCode::GONE, Html(page("PITL request", &body))).into_response()
}

fn not_found() -> Response {
    message(
        StatusCode::NOT_FOUND,
        "Not found",
        "This link is not valid, or it is too old.",
    )
}

fn conflict(err: TransitionError) -> Response {
    match err {
        TransitionError::NotFound => not_found(),
        TransitionError::WrongState(s) => message(
            StatusCode::CONFLICT,
            "Already finished",
            &format!(
                "This request is {} and can't be answered now. Nothing was sent.",
                s.as_str()
            ),
        ),
    }
}

/// Who answered. Tailscale Serve sets `Tailscale-User-Login`; only trusted
/// when pitld is configured as reachable through Serve alone.
fn answered_by(app: &App, headers: &HeaderMap) -> (Option<String>, Option<String>) {
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.chars().take(200).collect::<String>())
    };
    let login = get("tailscale-user-login");
    let device = get("x-forwarded-for");
    if app.cfg.behind_serve {
        (login, device)
    } else {
        (
            login.map(|l| format!("unverified:{l}")),
            device.map(|d| format!("unverified:{d}")),
        )
    }
}

fn audit_answer(app: &App, event: &str, id: RequestId, headers: &HeaderMap) {
    if let Some(v) = app.store.view(id) {
        let (by, device) = answered_by(app, headers);
        let mut rec = AuditRecord::new(event, v.id.clone(), &v.info);
        rec.answered_by = by;
        rec.device = device;
        app.audit.write(&rec);
    }
}

#[derive(Deserialize)]
pub struct ApproveForm {
    password: String,
}

impl Drop for ApproveForm {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

async fn approve(
    AxState(app): AxState<Arc<App>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Form(mut form): Form<ApproveForm>,
) -> Response {
    let Some(id) = parse_id(&id) else {
        return not_found();
    };
    let secret = Zeroizing::new(std::mem::take(&mut form.password));
    drop(form);
    // A newline or NUL would end or corrupt the line askpass hands to sudo.
    if secret.is_empty() || secret.bytes().any(|b| b == b'\n' || b == b'\r' || b == 0) {
        return message(
            StatusCode::BAD_REQUEST,
            "Not sent",
            "The value was empty or had a line break in it.",
        );
    }
    match app.store.authorize(id, secret) {
        Ok(()) => {
            audit_answer(&app, "authorized", id, &headers);
            let store = app
                .store
                .view(id)
                .is_some_and(|v| matches!(v.info.kind, RequestKind::Store { .. }));
            let text = if store {
                "The secret was sent to pitld to be written to its file. You can close this page."
            } else {
                "The password was sent to the waiting program. You can close this page."
            };
            message(StatusCode::OK, "Sent", text)
        }
        Err(e) => conflict(e),
    }
}

async fn deny(
    AxState(app): AxState<Arc<App>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(id) = parse_id(&id) else {
        return not_found();
    };
    match app.store.deny(id) {
        Ok(()) => {
            audit_answer(&app, "denied", id, &headers);
            message(
                StatusCode::OK,
                "Denied",
                "Nothing was sent. The program was told no.",
            )
        }
        Err(e) => conflict(e),
    }
}
