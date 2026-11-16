//! PITL core: the request state machine, the askpass socket protocol, audit
//! records and small Linux helpers. No I/O beyond reading `/proc` and
//! `/dev/urandom`.

pub mod audit;
pub mod id;
pub mod paths;
pub mod proc_info;
pub mod protocol;
pub mod secrets;
pub mod state;
pub mod timefmt;

pub use id::RequestId;
pub use state::{
    ConsumeOutcome, Request, RequestInfo, RequestKind, RequestView, State, Store, TransitionError,
    WakeReceiver,
};
