//! Minimal STUN client. Hand-rolled against RFC 5389 — enough to send a
//! Binding Request to a STUN server and parse the XOR-MAPPED-ADDRESS out
//! of the response. Used on `serve` to learn the host's public IPv4 so it
//! can be baked into the share code.
//!
//! No external STUN crate dependency; the protocol is just ~30 bytes of
//! wire format and one XOR.
//!
//! Known limitations:
//! - IPv4 only (XOR-MAPPED-ADDRESS with family = 0x01).
//! - No retransmit — single UDP shot, 3s timeout. Good enough for a one-
//!   time probe on session start; if it fails we fall back to LAN IP.
//! - Doesn't authenticate the response beyond matching the transaction ID.

use anyhow::{Context, Result, anyhow};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

pub const GOOGLE_STUN: &str = "stun.l.google.com:19302";
const MAGIC_COOKIE: u32 = 0x2112_A442;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;
const MAPPED_ADDRESS: u16 = 0x0001;

/// Probe the STUN server for our public IPv4 endpoint. The `bind_port`
/// argument binds the outbound UDP socket to a specific local port so the
/// NAT mapping the server sees is for that port — typically you want this
/// to match the port your game server is listening on, so the reported
/// endpoint matches the address friends will try to reach.
pub fn public_endpoint(server: &str, bind_port: u16, timeout: Duration) -> Result<SocketAddr> {
    let sock = UdpSocket::bind(("0.0.0.0", bind_port))
        .with_context(|| format!("bind UDP :{bind_port} for STUN"))?;
    sock.set_read_timeout(Some(timeout))?;
    sock.set_write_timeout(Some(timeout))?;

    // Build a binding request. Msg length 0 — no attributes.
    let mut txn_id = [0u8; 12];
    getrandom::getrandom(&mut txn_id).context("STUN transaction id")?;
    let mut req = [0u8; 20];
    req[0..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    req[2..4].copy_from_slice(&0u16.to_be_bytes());
    req[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    req[8..20].copy_from_slice(&txn_id);

    // Resolve + send.
    let addrs: Vec<SocketAddr> = server
        .to_socket_addrs_compat()
        .with_context(|| format!("resolve STUN server `{server}`"))?;
    let addr = addrs.into_iter().find(|a| a.is_ipv4()).ok_or_else(|| {
        anyhow!("no IPv4 address for STUN server `{server}`")
    })?;
    sock.send_to(&req, addr)
        .with_context(|| format!("send to {addr}"))?;

    // Read response.
    let mut buf = [0u8; 512];
    let (n, _) = sock.recv_from(&mut buf).context("STUN recv")?;
    if n < 20 {
        return Err(anyhow!("STUN response too short ({n} bytes)"));
    }

    // Validate header.
    let msg_type = u16::from_be_bytes([buf[0], buf[1]]);
    if msg_type != BINDING_SUCCESS {
        return Err(anyhow!("STUN response type 0x{msg_type:04x} != success"));
    }
    let cookie = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    if cookie != MAGIC_COOKIE {
        return Err(anyhow!("STUN magic cookie mismatch"));
    }
    if buf[8..20] != txn_id {
        return Err(anyhow!("STUN transaction id mismatch"));
    }
    let msg_len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    if msg_len + 20 > n {
        return Err(anyhow!("STUN msg_len overruns packet"));
    }

    // Walk attributes looking for XOR-MAPPED-ADDRESS (preferred) or
    // plain MAPPED-ADDRESS (fallback).
    let body = &buf[20..20 + msg_len];
    let mut i = 0usize;
    while i + 4 <= body.len() {
        let atype = u16::from_be_bytes([body[i], body[i + 1]]);
        let alen = u16::from_be_bytes([body[i + 2], body[i + 3]]) as usize;
        let start = i + 4;
        let end = start + alen;
        if end > body.len() {
            break;
        }
        let attr = &body[start..end];

        if atype == XOR_MAPPED_ADDRESS && attr.len() >= 8 && attr[1] == 0x01 {
            let xor_port = u16::from_be_bytes([attr[2], attr[3]]);
            let port = xor_port ^ ((MAGIC_COOKIE >> 16) as u16);
            let xor_ip = u32::from_be_bytes([attr[4], attr[5], attr[6], attr[7]]);
            let ip = Ipv4Addr::from(xor_ip ^ MAGIC_COOKIE);
            return Ok(SocketAddr::new(ip.into(), port));
        }
        if atype == MAPPED_ADDRESS && attr.len() >= 8 && attr[1] == 0x01 {
            let port = u16::from_be_bytes([attr[2], attr[3]]);
            let ip = Ipv4Addr::new(attr[4], attr[5], attr[6], attr[7]);
            return Ok(SocketAddr::new(ip.into(), port));
        }

        // Attributes are padded to 4-byte boundary.
        i = end + ((4 - (alen % 4)) % 4);
    }

    Err(anyhow!("STUN response had no XOR-MAPPED-ADDRESS attribute"))
}

/// Carrier-grade NAT detection: the 100.64.0.0/10 range (RFC 6598) is
/// reserved by IANA specifically for ISPs running CG-NAT. If STUN reports
/// an address in this range, friends can't reach the host directly — they
/// need Tailscale or equivalent tunneling.
pub fn is_cgnat(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && (o[1] & 0xC0) == 0x40
}

// std doesn't expose `to_socket_addrs` on `&str` directly in every path we
// use, so we go through a tiny helper. `str::to_socket_addrs` requires a
// trait import — this wrapper keeps the call site tidy.
trait ToSocketAddrsCompat {
    fn to_socket_addrs_compat(&self) -> Result<Vec<SocketAddr>>;
}

impl ToSocketAddrsCompat for str {
    fn to_socket_addrs_compat(&self) -> Result<Vec<SocketAddr>> {
        use std::net::ToSocketAddrs;
        Ok(self.to_socket_addrs()?.collect())
    }
}
