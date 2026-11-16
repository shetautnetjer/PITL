//! The unix socket askpass and the CLI talk to.

use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use pitl_core::audit::AuditRecord;
use pitl_core::proc_info;
use pitl_core::protocol::{ClientMessage, DaemonMessage, MAX_LINE};
use pitl_core::{ConsumeOutcome, RequestId, RequestInfo, State};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::{UnixListener, UnixStream};
use zeroize::Zeroizing;

use crate::App;

/// Create the socket: parent directory 0700, socket 0600, stale socket removed.
pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

pub async fn serve(listener: UnixListener, app: Arc<App>) {
    let my_uid = pitl_core::paths::current_uid();
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("pitld: accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let app = app.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, app, my_uid).await {
                eprintln!("pitld: connection error: {e}");
            }
        });
    }
}

async fn send(w: &mut OwnedWriteHalf, msg: &DaemonMessage) -> std::io::Result<()> {
    w.write_all(&msg.to_line()).await?;
    w.flush().await
}

async fn handle(stream: UnixStream, app: Arc<App>, my_uid: Option<u32>) -> std::io::Result<()> {
    let cred = stream.peer_cred()?;
    // The socket is 0600 already; refuse anyone else anyway.
    if my_uid.is_some_and(|uid| uid != cred.uid()) {
        return Ok(());
    }
    let peer_pid = cred.pid().and_then(|p| u32::try_from(p).ok());

    let (rd, mut wr) = stream.into_split();
    let mut reader = BufReader::new(rd);
    let mut line = String::new();
    let n = (&mut reader)
        .take(MAX_LINE as u64)
        .read_line(&mut line)
        .await?;
    if n == 0 {
        return Ok(());
    }
    let msg: ClientMessage = match serde_json::from_str(line.trim_end()) {
        Ok(m) => m,
        Err(_) => {
            return send(
                &mut wr,
                &DaemonMessage::Error {
                    message: "bad request".into(),
                },
            )
            .await;
        }
    };

    match msg {
        ClientMessage::List => {
            let pending = app
                .store
                .pending()
                .into_iter()
                .map(|v| {
                    let mut j = serde_json::to_value(&v).unwrap_or_default();
                    j["url"] = serde_json::Value::String(app.link(&v.id));
                    j
                })
                .collect();
            send(&mut wr, &DaemonMessage::List { pending }).await
        }
        ClientMessage::Ask { prompt } => {
            let Some(pid) = peer_pid else {
                return send(
                    &mut wr,
                    &DaemonMessage::Error {
                        message: "no peer pid".into(),
                    },
                )
                .await;
            };
            let info = gather_info(&app, pid, cred.uid(), &prompt);
            ask(app, info, reader, wr).await
        }
    }
}

/// Everything shown to the human comes from here: the kernel's view of who is
/// on the other end of the socket, not from text the requester sent.
fn gather_info(app: &App, askpass_pid: u32, uid: u32, prompt: &str) -> RequestInfo {
    let askpass = proc_info::read_proc(askpass_pid);
    let program = askpass.as_ref().and_then(|a| proc_info::read_proc(a.ppid));
    let requester = program.as_ref().and_then(|p| proc_info::read_proc(p.ppid));
    let mut prompt = proc_info::format_cmdline(prompt.as_bytes());
    prompt.truncate(200);
    RequestInfo {
        machine: app.cfg.machine.clone(),
        user: proc_info::user_name(uid),
        uid,
        command: program
            .as_ref()
            .map(|p| p.cmdline.clone())
            .unwrap_or_else(|| "unknown".into()),
        requester: requester
            .as_ref()
            .map(|r| r.cmdline.clone())
            .unwrap_or_else(|| "unknown".into()),
        requester_pid: requester.map(|r| r.pid).unwrap_or(0),
        prompt,
    }
}

fn audit(app: &App, event: &str, id: RequestId, info: &RequestInfo) {
    app.audit.write(&AuditRecord::new(event, id.to_hex(), info));
}

async fn ask(
    app: Arc<App>,
    info: RequestInfo,
    mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    mut wr: OwnedWriteHalf,
) -> std::io::Result<()> {
    let (id, mut woken) = app.store.create(info.clone())?;
    let url = app.link(&id.to_hex());
    audit(&app, "created", id, &info);
    eprintln!("pitld: request {} from \"{}\": {}", id, info.command, url);

    let expires_unix = app.store.view(id).map(|v| v.expires_unix).unwrap_or(0);
    if send(
        &mut wr,
        &DaemonMessage::Pending {
            id: id.to_hex(),
            url,
            expires_unix,
        },
    )
    .await
    .is_err()
    {
        if app.store.cancel(id).is_ok() {
            audit(&app, "canceled", id, &info);
        }
        return Ok(());
    }

    let deadline = tokio::time::Instant::now() + app.store.ttl() + Duration::from_millis(50);
    let mut probe = [0u8; 1];
    tokio::select! {
        _ = &mut woken => {}
        // askpass sends nothing after its question, so any read result means
        // it went away (or misbehaved). Fail closed.
        _ = reader.read(&mut probe) => {
            if app.store.cancel(id).is_ok() {
                audit(&app, "canceled", id, &info);
            }
            return Ok(());
        }
        _ = tokio::time::sleep_until(deadline) => {}
    }

    match app.store.consume(id) {
        ConsumeOutcome::Secret(secret) => {
            let mut payload = Zeroizing::new(Vec::with_capacity(secret.len() + 1));
            payload.extend_from_slice(secret.as_bytes());
            payload.push(b'\n');
            drop(secret);
            let delivered = async {
                wr.write_all(&DaemonMessage::Secret.to_line()).await?;
                wr.write_all(&payload).await?;
                wr.flush().await?;
                wr.shutdown().await
            }
            .await;
            drop(payload);
            match delivered {
                Ok(()) => audit(&app, "consumed", id, &info),
                Err(_) => {
                    let _ = app.store.mark_failed(id);
                    audit(&app, "failed", id, &info);
                }
            }
        }
        ConsumeOutcome::StillPending => {
            // Woken without an answer can only mean the deadline passed.
            let state = app.store.expire_if_due(id).unwrap_or(State::Expired);
            let state = if state == State::Pending {
                let _ = app.store.mark_failed(id);
                State::Failed
            } else {
                state
            };
            finish(&app, id, &info, state, &mut wr).await;
        }
        ConsumeOutcome::Finished(state) => finish(&app, id, &info, state, &mut wr).await,
        ConsumeOutcome::NotFound => {
            let _ = send(&mut wr, &DaemonMessage::Failed).await;
        }
    }
    Ok(())
}

async fn finish(
    app: &App,
    id: RequestId,
    info: &RequestInfo,
    state: State,
    wr: &mut OwnedWriteHalf,
) {
    let msg = match state {
        State::Denied => DaemonMessage::Denied,
        State::Expired => {
            audit(app, "expired", id, info);
            DaemonMessage::Expired
        }
        State::Canceled => DaemonMessage::Canceled,
        _ => {
            if state == State::Failed {
                audit(app, "failed", id, info);
            }
            DaemonMessage::Failed
        }
    };
    let _ = send(wr, &msg).await;
}
