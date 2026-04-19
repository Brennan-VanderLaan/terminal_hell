pub mod client;
pub mod proto;
pub mod server;

pub use client::run_connect;
pub use server::run_serve;

const PROTOCOL_ID: u64 = 0x5445524D_48454C4C; // "TERMHELL"
const DEFAULT_PORT: u16 = 4646;

pub fn default_port() -> u16 {
    DEFAULT_PORT
}
