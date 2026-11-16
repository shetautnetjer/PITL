//! The askpass <-> pitld protocol over the user's unix socket.
//!
//! Framing is newline-terminated JSON, one message per line, with one
//! exception: after a `{"status":"secret"}` line the daemon writes the secret
//! itself as raw bytes ending in `\n`, so it never sits inside a JSON buffer.
//!
//! ```text
//! askpass -> pitld   {"op":"ask","prompt":"[sudo] password for u: "}
//! pitld  -> askpass  {"status":"pending","id":"<32 hex>","url":"https://.../r/<id>","expires_unix":...}
//! pitld  -> askpass  {"status":"secret"}\n<secret bytes>\n      (once, then close)
//!                or  {"status":"denied"} | {"status":"expired"} | {"status":"canceled"} | {"status":"failed"}
//!
//! pitl   -> pitld    {"op":"store","name":"op-service-account"}
//! pitld  -> pitl     {"status":"pending",...} then {"status":"stored","name":...,"path":...}
//!                    (or denied/expired/canceled/failed; the secret is never sent)
//!
//! pitl   -> pitld    {"op":"list"}
//! pitld  -> pitl     {"status":"list","pending":[RequestView...]}
//! ```

use serde::{Deserialize, Serialize};

/// Longest message line accepted from a client, in bytes.
pub const MAX_LINE: usize = 8 * 1024;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ClientMessage {
    /// From askpass: a program wants a secret. `prompt` is whatever sudo
    /// passed to askpass; the daemon shows it only as unverified text.
    Ask { prompt: String },
    /// From the CLI: list pending requests.
    List,
    /// From `pitl store`: ask the human for a secret to be written to a file
    /// by pitld. The secret is never sent back on the socket.
    Store { name: String },
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DaemonMessage {
    Pending {
        id: String,
        url: String,
        expires_unix: u64,
    },
    /// The next line on the socket is the secret.
    Secret,
    /// A store request finished: pitld wrote the secret to `path`.
    Stored {
        name: String,
        path: String,
    },
    Denied,
    Expired,
    Canceled,
    Failed,
    List {
        pending: Vec<serde_json::Value>,
    },
    Error {
        message: String,
    },
}

impl DaemonMessage {
    pub fn to_line(&self) -> Vec<u8> {
        let mut v = serde_json::to_vec(self).expect("serializable");
        v.push(b'\n');
        v
    }
}

impl ClientMessage {
    pub fn to_line(&self) -> Vec<u8> {
        let mut v = serde_json::to_vec(self).expect("serializable");
        v.push(b'\n');
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format() {
        let ask = ClientMessage::Ask { prompt: "p".into() };
        assert_eq!(
            String::from_utf8(ask.to_line()).unwrap(),
            "{\"op\":\"ask\",\"prompt\":\"p\"}\n"
        );
        assert_eq!(
            String::from_utf8(DaemonMessage::Secret.to_line()).unwrap(),
            "{\"status\":\"secret\"}\n"
        );
        let parsed: ClientMessage = serde_json::from_str("{\"op\":\"list\"}").unwrap();
        assert_eq!(parsed, ClientMessage::List);
    }
}
