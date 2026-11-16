//! Request ids: 128 random bits from the kernel, shown as 32 hex characters.
//! The id is the capability in the phone link, so it must be unguessable.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};

/// Number of random bytes in an id (128 bits).
pub const ID_BYTES: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId([u8; ID_BYTES]);

impl RequestId {
    /// A new id from `/dev/urandom`.
    pub fn random() -> io::Result<Self> {
        let mut bytes = [0u8; ID_BYTES];
        File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        Ok(Self(bytes))
    }

    pub fn to_hex(self) -> String {
        let mut s = String::with_capacity(ID_BYTES * 2);
        for b in self.0 {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    /// Parse the 32-character lowercase hex form. Anything else is rejected.
    pub fn parse(s: &str) -> Option<Self> {
        if s.len() != ID_BYTES * 2 || !s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {
            return None;
        }
        let mut bytes = [0u8; ID_BYTES];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hex = std::str::from_utf8(chunk).ok()?;
            bytes[i] = u8::from_str_radix(hex, 16).ok()?;
        }
        Some(Self(bytes))
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RequestId({})", self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_uniqueness() {
        let a = RequestId::random().unwrap();
        let b = RequestId::random().unwrap();
        assert_ne!(a, b);
        assert_eq!(a.to_hex().len(), 32);
        assert_eq!(RequestId::parse(&a.to_hex()), Some(a));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(RequestId::parse("").is_none());
        assert!(RequestId::parse("zz").is_none());
        assert!(RequestId::parse(&"A".repeat(32)).is_none());
        assert!(RequestId::parse(&"0".repeat(31)).is_none());
        assert!(RequestId::parse("../../../../etc/passwd0000000000").is_none());
    }
}
