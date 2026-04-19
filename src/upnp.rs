//! Best-effort UPnP port-forwarding on serve.
//!
//! Many consumer routers support IGD / UPnP and will happily open a port
//! on request. When it works, friends can paste the share code and play —
//! no manual router config. When it doesn't, we print a clear error +
//! point at Tailscale as the fallback.
//!
//! The lease duration we request is 2h — long enough to cover most play
//! sessions, short enough that stale mappings clean themselves up.

use igd::{PortMappingProtocol, SearchOptions};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

const LEASE_SECS: u32 = 7200;

pub struct ForwardReport {
    pub game_opened: bool,
    pub http_opened: bool,
    pub error: Option<String>,
}

pub fn try_open(local_ip: Ipv4Addr, game_port: u16, http_port: u16) -> ForwardReport {
    let opts = SearchOptions {
        timeout: Some(Duration::from_secs(3)),
        ..Default::default()
    };
    let gateway = match igd::search_gateway(opts) {
        Ok(g) => g,
        Err(e) => {
            return ForwardReport {
                game_opened: false,
                http_opened: false,
                error: Some(format!("no IGD gateway found: {e}")),
            };
        }
    };

    let game_addr = SocketAddrV4::new(local_ip, game_port);
    let http_addr = SocketAddrV4::new(local_ip, http_port);

    let game_opened = gateway
        .add_port(
            PortMappingProtocol::UDP,
            game_port,
            game_addr,
            LEASE_SECS,
            "terminal_hell game",
        )
        .is_ok();
    let http_opened = gateway
        .add_port(
            PortMappingProtocol::TCP,
            http_port,
            http_addr,
            LEASE_SECS,
            "terminal_hell install",
        )
        .is_ok();

    ForwardReport {
        game_opened,
        http_opened,
        error: if !game_opened && !http_opened {
            Some("gateway found but add_port refused (router may disable UPnP)".into())
        } else {
            None
        },
    }
}
