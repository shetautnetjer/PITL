//! Audit records: one JSON object per line. Metadata only. There is no field
//! that can hold a secret, so a secret cannot be logged by accident through
//! this type.

use serde::Serialize;

use crate::state::RequestInfo;

#[derive(Debug, Serialize)]
pub struct AuditRecord<'a> {
    /// RFC 3339 UTC time.
    pub time: String,
    /// sudo | store
    pub kind: &'static str,
    /// For store requests: the secret's name (never its value).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_name: Option<&'a str>,
    /// created | authorized | consumed | stored | denied | expired | canceled | failed
    pub event: &'a str,
    pub id: String,
    pub machine: &'a str,
    pub user: &'a str,
    pub command: &'a str,
    pub requester: &'a str,
    pub requester_pid: u32,
    /// Who answered, when known: the Tailscale login of the phone (from
    /// Tailscale Serve's identity headers) and/or the remote address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

impl<'a> AuditRecord<'a> {
    pub fn new(event: &'a str, id: String, info: &'a RequestInfo) -> Self {
        Self {
            time: crate::timefmt::rfc3339_now(),
            kind: info.kind.as_str(),
            secret_name: match &info.kind {
                crate::state::RequestKind::Store { name, .. } => Some(name.as_str()),
                crate::state::RequestKind::Sudo => None,
            },
            event,
            id,
            machine: &info.machine,
            user: &info.user,
            command: &info.command,
            requester: &info.requester,
            requester_pid: info.requester_pid,
            answered_by: None,
            device: None,
        }
    }

    pub fn to_line(&self) -> Vec<u8> {
        let mut v = serde_json::to_vec(self).expect("serializable");
        v.push(b'\n');
        v
    }
}
