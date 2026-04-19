//! Session share codes.
//!
//! Format: `TH01` prefix + 40 base32 chars encoding 25 bytes:
//!
//! ```text
//! [schema:1] [ip:4] [game_port:2] [http_port:2] [token:16] = 25 bytes
//! ```
//!
//! The base32 alphabet is RFC 4648 (A-Z + 2-7), no padding. No character in
//! the encoded form is a shell metacharacter, so codes are safe to paste
//! into bash / zsh / PowerShell as plain args.
//!
//! The **token** is a 128-bit random value generated per `serve` session.
//! It acts as:
//! - An authentication secret — the host validates it on every incoming
//!   netcode user_data payload, rejecting peers who lack the token.
//! - The HMAC key for binary signing (phase 3), so MITM can't substitute
//!   a different binary during install.
//!
//! The token expires when the host process ends — a fresh `serve` rolls a
//! new one. Leaked codes become useless as soon as the host restarts.

use anyhow::{Context, Result, anyhow};
use std::net::Ipv4Addr;

pub const MAGIC: &str = "TH01";
pub const SCHEMA_V1: u8 = 1;

/// Total raw payload size, excluding the `TH01` prefix.
pub const PAYLOAD_BYTES: usize = 25;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareCode {
    pub schema: u8,
    pub ip: Ipv4Addr,
    pub game_port: u16,
    pub http_port: u16,
    pub token: [u8; 16],
}

impl ShareCode {
    pub fn new(ip: Ipv4Addr, game_port: u16, http_port: u16, token: [u8; 16]) -> Self {
        Self { schema: SCHEMA_V1, ip, game_port, http_port, token }
    }

    pub fn encode(&self) -> String {
        let mut buf = [0u8; PAYLOAD_BYTES];
        buf[0] = self.schema;
        buf[1..5].copy_from_slice(&self.ip.octets());
        buf[5..7].copy_from_slice(&self.game_port.to_be_bytes());
        buf[7..9].copy_from_slice(&self.http_port.to_be_bytes());
        buf[9..25].copy_from_slice(&self.token);
        let b32 =
            base32::encode(base32::Alphabet::Rfc4648 { padding: false }, &buf);
        format!("{MAGIC}{b32}")
    }

    pub fn decode(s: &str) -> Result<Self> {
        let rest = s
            .strip_prefix(MAGIC)
            .ok_or_else(|| anyhow!("share code must start with `{MAGIC}`"))?;
        let bytes = base32::decode(base32::Alphabet::Rfc4648 { padding: false }, rest)
            .ok_or_else(|| anyhow!("share code base32 decode failed"))?;
        if bytes.len() != PAYLOAD_BYTES {
            return Err(anyhow!(
                "share code payload is {} bytes, expected {}",
                bytes.len(),
                PAYLOAD_BYTES
            ));
        }
        let schema = bytes[0];
        if schema != SCHEMA_V1 {
            return Err(anyhow!("unsupported share-code schema v{schema}"));
        }
        let ip = Ipv4Addr::new(bytes[1], bytes[2], bytes[3], bytes[4]);
        let game_port = u16::from_be_bytes([bytes[5], bytes[6]]);
        let http_port = u16::from_be_bytes([bytes[7], bytes[8]]);
        let mut token = [0u8; 16];
        token.copy_from_slice(&bytes[9..25]);
        Ok(Self { schema, ip, game_port, http_port, token })
    }

    /// Bash / zsh / PowerShell all accept `terminal_hell connect TH01...`
    /// verbatim. Kept as a helper in case we want to format this differently
    /// once auto-install lands (phase 3).
    pub fn connect_command(&self) -> String {
        format!("terminal_hell connect {}", self.encode())
    }
}

/// Generate a fresh 128-bit session token using the OS CSPRNG.
pub fn new_token() -> Result<[u8; 16]> {
    let mut t = [0u8; 16];
    getrandom::getrandom(&mut t).context("OS randomness")?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let code = ShareCode::new(
            Ipv4Addr::new(203, 0, 113, 7),
            4646,
            4647,
            [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
             0xee, 0xff, 0x00],
        );
        let s = code.encode();
        assert!(s.starts_with(MAGIC));
        let decoded = ShareCode::decode(&s).unwrap();
        assert_eq!(decoded, code);
    }

    #[test]
    fn rejects_bad_magic() {
        assert!(ShareCode::decode("XX01ABCD").is_err());
    }
}
