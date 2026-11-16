//! The request state machine.
//!
//! ```text
//! Created ──► Pending ──► Authorized ──► Consumed
//!                │            │
//!                ├──► Denied  └──► Failed (delivery to askpass failed)
//!                ├──► Expired
//!                └──► Canceled (askpass went away)
//! ```
//!
//! Every transition happens under one mutex, so "authorize" and "consume" are
//! each atomic: a request can be consumed at most once, and only by the
//! connection that created it. The secret lives only in `Authorized`, is
//! moved out by `consume`, and is zeroized when dropped.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::id::RequestId;

/// Terminal requests are kept this long so the phone page can say what
/// happened, then forgotten.
pub const KEEP_FINISHED: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Created,
    Pending,
    Authorized,
    Consumed,
    Denied,
    Expired,
    Canceled,
    Failed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Created => "created",
            State::Pending => "pending",
            State::Authorized => "authorized",
            State::Consumed => "consumed",
            State::Denied => "denied",
            State::Expired => "expired",
            State::Canceled => "canceled",
            State::Failed => "failed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            State::Consumed | State::Denied | State::Expired | State::Canceled | State::Failed
        )
    }
}

/// What the human is shown. Every field is gathered by the daemon itself
/// (from the kernel and `/proc`), never taken from text the agent supplied.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RequestInfo {
    pub machine: String,
    pub user: String,
    pub uid: u32,
    /// The program that asked for authorization (usually `sudo`), with its arguments.
    pub command: String,
    /// The process that started that program (for example a shell or an agent).
    pub requester: String,
    pub requester_pid: u32,
    /// The prompt text sudo passed to askpass. Shown, but labelled as unverified.
    /// Empty for store requests.
    pub prompt: String,
    /// What is being asked for.
    pub kind: RequestKind,
}

/// The kinds of request pitld handles.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestKind {
    /// A password handed once to a waiting `SUDO_ASKPASS` helper.
    Sudo,
    /// A secret written by pitld to a 0600 file, never sent back to anyone.
    Store {
        /// Validated name, see `crate::secrets::validate_name`.
        name: String,
        /// The file it will be written to.
        path: String,
    },
}

impl RequestKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RequestKind::Sudo => "sudo",
            RequestKind::Store { .. } => "store",
        }
    }
}

pub struct Request {
    pub id: RequestId,
    pub info: RequestInfo,
    pub created_unix: u64,
    pub expires_unix: u64,
    expires_at: Instant,
    state: State,
    finished_at: Option<Instant>,
    secret: Option<Zeroizing<String>>,
    wake: Option<Waker>,
}

impl Request {
    pub fn state(&self) -> State {
        self.state
    }
}

/// Fires once when the request leaves `Pending` (answered, expired, canceled).
pub type WakeReceiver = tokio::sync::oneshot::Receiver<()>;
type Waker = tokio::sync::oneshot::Sender<()>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionError {
    NotFound,
    /// The request is not in a state that allows this (already answered,
    /// expired, canceled, consumed ...). Carries the current state.
    WrongState(State),
}

/// Result of the delivering connection asking for its answer.
#[derive(Debug)]
pub enum ConsumeOutcome {
    /// The secret, exactly once. The request is now `Consumed`.
    Secret(Zeroizing<String>),
    /// Still waiting for the human.
    StillPending,
    /// Finished without a secret (denied, expired, canceled, failed).
    Finished(State),
    NotFound,
}

/// A snapshot for listing and rendering. Never contains the secret.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RequestView {
    pub id: String,
    pub state: &'static str,
    pub info: RequestInfo,
    pub created_unix: u64,
    pub expires_unix: u64,
    pub seconds_left: u64,
}

pub struct Store {
    ttl: Duration,
    inner: Mutex<HashMap<RequestId, Request>>,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Store {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<RequestId, Request>> {
        // A poisoned lock means a panic mid-transition; keep serving rather
        // than wedge every pending request. Transitions are single writes.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Create a request and move it to `Pending`. Returns the id and a
    /// receiver that fires when a human answers.
    pub fn create(&self, info: RequestInfo) -> std::io::Result<(RequestId, WakeReceiver)> {
        let id = RequestId::random()?;
        let now = Instant::now();
        let created_unix = unix_now();
        let (waker, rx) = tokio::sync::oneshot::channel();
        let mut req = Request {
            id,
            info,
            created_unix,
            expires_unix: created_unix + self.ttl.as_secs(),
            expires_at: now + self.ttl,
            state: State::Created,
            finished_at: None,
            secret: None,
            wake: Some(waker),
        };
        // Created -> Pending happens before anyone else can see the request.
        req.state = State::Pending;
        let mut map = self.lock();
        map.retain(|_, r| {
            r.finished_at
                .map_or(true, |t| now.duration_since(t) < KEEP_FINISHED)
        });
        map.insert(id, req);
        Ok((id, rx))
    }

    /// Expire the request if its time is up. Call with the lock held.
    fn settle_expiry(req: &mut Request, now: Instant) {
        if matches!(req.state, State::Pending | State::Authorized) && now >= req.expires_at {
            req.state = State::Expired;
            req.secret = None;
            req.finished_at = Some(now);
            if let Some(w) = req.wake.take() {
                let _ = w.send(());
            }
        }
    }

    fn finish(req: &mut Request, state: State) {
        req.state = state;
        req.secret = None;
        req.finished_at = Some(Instant::now());
        if let Some(w) = req.wake.take() {
            let _ = w.send(());
        }
    }

    /// The human entered the secret. Pending -> Authorized.
    pub fn authorize(
        &self,
        id: RequestId,
        secret: Zeroizing<String>,
    ) -> Result<(), TransitionError> {
        let mut map = self.lock();
        let req = map.get_mut(&id).ok_or(TransitionError::NotFound)?;
        Self::settle_expiry(req, Instant::now());
        if req.state != State::Pending {
            return Err(TransitionError::WrongState(req.state));
        }
        req.state = State::Authorized;
        req.secret = Some(secret);
        if let Some(w) = req.wake.take() {
            let _ = w.send(());
        }
        Ok(())
    }

    /// The human said no. Pending -> Denied.
    pub fn deny(&self, id: RequestId) -> Result<(), TransitionError> {
        self.end_pending(id, State::Denied)
    }

    /// The asking connection went away. Pending/Authorized -> Canceled.
    pub fn cancel(&self, id: RequestId) -> Result<(), TransitionError> {
        let mut map = self.lock();
        let req = map.get_mut(&id).ok_or(TransitionError::NotFound)?;
        Self::settle_expiry(req, Instant::now());
        match req.state {
            State::Pending | State::Authorized => {
                Self::finish(req, State::Canceled);
                Ok(())
            }
            s => Err(TransitionError::WrongState(s)),
        }
    }

    /// Delivery of a consumed secret failed. Consumed -> Failed.
    pub fn mark_failed(&self, id: RequestId) -> Result<(), TransitionError> {
        let mut map = self.lock();
        let req = map.get_mut(&id).ok_or(TransitionError::NotFound)?;
        match req.state {
            State::Consumed | State::Pending | State::Authorized => {
                Self::finish(req, State::Failed);
                Ok(())
            }
            s => Err(TransitionError::WrongState(s)),
        }
    }

    /// Expire the request now if its time is up. Returns the current state.
    pub fn expire_if_due(&self, id: RequestId) -> Option<State> {
        let mut map = self.lock();
        let req = map.get_mut(&id)?;
        Self::settle_expiry(req, Instant::now());
        Some(req.state)
    }

    fn end_pending(&self, id: RequestId, to: State) -> Result<(), TransitionError> {
        let mut map = self.lock();
        let req = map.get_mut(&id).ok_or(TransitionError::NotFound)?;
        Self::settle_expiry(req, Instant::now());
        if req.state != State::Pending {
            return Err(TransitionError::WrongState(req.state));
        }
        Self::finish(req, to);
        Ok(())
    }

    /// The one atomic consume: Authorized -> Consumed, moving the secret out.
    /// A second call returns `Finished(Consumed)`, never the secret again.
    pub fn consume(&self, id: RequestId) -> ConsumeOutcome {
        let mut map = self.lock();
        let Some(req) = map.get_mut(&id) else {
            return ConsumeOutcome::NotFound;
        };
        Self::settle_expiry(req, Instant::now());
        match req.state {
            State::Authorized => match req.secret.take() {
                Some(secret) => {
                    req.state = State::Consumed;
                    req.finished_at = Some(Instant::now());
                    ConsumeOutcome::Secret(secret)
                }
                None => {
                    Self::finish(req, State::Failed);
                    ConsumeOutcome::Finished(State::Failed)
                }
            },
            State::Pending | State::Created => ConsumeOutcome::StillPending,
            s => ConsumeOutcome::Finished(s),
        }
    }

    pub fn view(&self, id: RequestId) -> Option<RequestView> {
        let mut map = self.lock();
        let req = map.get_mut(&id)?;
        Self::settle_expiry(req, Instant::now());
        Some(Self::to_view(req))
    }

    pub fn pending(&self) -> Vec<RequestView> {
        let now = Instant::now();
        let mut map = self.lock();
        let mut out: Vec<RequestView> = map
            .values_mut()
            .filter_map(|req| {
                Self::settle_expiry(req, now);
                (req.state == State::Pending).then(|| Self::to_view(req))
            })
            .collect();
        out.sort_by_key(|v| v.created_unix);
        out
    }

    fn to_view(req: &Request) -> RequestView {
        let left = req
            .expires_at
            .saturating_duration_since(Instant::now())
            .as_secs();
        RequestView {
            id: req.id.to_hex(),
            state: req.state.as_str(),
            info: req.info.clone(),
            created_unix: req.created_unix,
            expires_unix: req.expires_unix,
            seconds_left: if req.state.is_terminal() { 0 } else { left },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> RequestInfo {
        RequestInfo {
            machine: "m".into(),
            user: "u".into(),
            uid: 1000,
            command: "sudo -A -k /usr/bin/id".into(),
            requester: "bash".into(),
            requester_pid: 1,
            prompt: "[sudo] password for u: ".into(),
            kind: RequestKind::Sudo,
        }
    }

    fn secret(s: &str) -> Zeroizing<String> {
        Zeroizing::new(s.to_string())
    }

    #[test]
    fn approve_then_consume_once() {
        let store = Store::new(Duration::from_secs(60));
        let (id, mut rx) = store.create(info()).unwrap();
        assert_eq!(store.view(id).unwrap().state, "pending");
        store.authorize(id, secret("pw")).unwrap();
        rx.try_recv().expect("waker fired");
        match store.consume(id) {
            ConsumeOutcome::Secret(s) => assert_eq!(s.as_str(), "pw"),
            other => panic!("expected secret, got {other:?}"),
        }
        assert!(matches!(
            store.consume(id),
            ConsumeOutcome::Finished(State::Consumed)
        ));
        assert_eq!(
            store.authorize(id, secret("again")),
            Err(TransitionError::WrongState(State::Consumed))
        );
    }

    #[test]
    fn deny_is_final() {
        let store = Store::new(Duration::from_secs(60));
        let (id, _rx) = store.create(info()).unwrap();
        store.deny(id).unwrap();
        assert!(matches!(
            store.consume(id),
            ConsumeOutcome::Finished(State::Denied)
        ));
        assert_eq!(
            store.authorize(id, secret("pw")),
            Err(TransitionError::WrongState(State::Denied))
        );
        assert_eq!(
            store.deny(id),
            Err(TransitionError::WrongState(State::Denied))
        );
    }

    #[test]
    fn expiry_blocks_approval_and_drops_secret() {
        let store = Store::new(Duration::from_millis(50));
        let (id, _rx) = store.create(info()).unwrap();
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(
            store.authorize(id, secret("pw")),
            Err(TransitionError::WrongState(State::Expired))
        );
        assert!(matches!(
            store.consume(id),
            ConsumeOutcome::Finished(State::Expired)
        ));
        assert!(store.pending().is_empty());
    }

    #[test]
    fn authorized_but_unconsumed_secret_expires() {
        let store = Store::new(Duration::from_millis(50));
        let (id, _rx) = store.create(info()).unwrap();
        store.authorize(id, secret("pw")).unwrap();
        std::thread::sleep(Duration::from_millis(80));
        assert!(matches!(
            store.consume(id),
            ConsumeOutcome::Finished(State::Expired)
        ));
    }

    #[test]
    fn cancel_fails_closed() {
        let store = Store::new(Duration::from_secs(60));
        let (id, _rx) = store.create(info()).unwrap();
        store.cancel(id).unwrap();
        assert_eq!(
            store.authorize(id, secret("pw")),
            Err(TransitionError::WrongState(State::Canceled))
        );
        assert!(matches!(
            store.consume(id),
            ConsumeOutcome::Finished(State::Canceled)
        ));
    }

    #[test]
    fn cancel_after_authorize_drops_secret() {
        let store = Store::new(Duration::from_secs(60));
        let (id, _rx) = store.create(info()).unwrap();
        store.authorize(id, secret("pw")).unwrap();
        store.cancel(id).unwrap();
        assert!(matches!(
            store.consume(id),
            ConsumeOutcome::Finished(State::Canceled)
        ));
    }

    #[test]
    fn unknown_id() {
        let store = Store::new(Duration::from_secs(60));
        let id = RequestId::random().unwrap();
        assert_eq!(store.deny(id), Err(TransitionError::NotFound));
        assert!(matches!(store.consume(id), ConsumeOutcome::NotFound));
    }

    #[test]
    fn concurrent_consume_yields_secret_once() {
        use std::sync::Arc;
        let store = Arc::new(Store::new(Duration::from_secs(60)));
        let (id, _rx) = store.create(info()).unwrap();
        store.authorize(id, secret("pw")).unwrap();
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let s = store.clone();
                std::thread::spawn(move || matches!(s.consume(id), ConsumeOutcome::Secret(_)))
            })
            .collect();
        let wins = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|w| *w)
            .count();
        assert_eq!(wins, 1);
    }
}
