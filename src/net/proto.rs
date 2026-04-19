//! Wire protocol. Messages are bincode-serde encoded. Channels:
//!
//! - `ReliableOrdered` (0): Welcome, TileUpdate, PlayerJoined/Left, RunEnded
//! - `Unreliable` (1): Snapshot (positions), Input, Blast (visual-only)

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientMsg {
    Input(ClientInput),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default)]
pub struct ClientInput {
    pub move_x: f32,
    pub move_y: f32,
    pub aim_x: f32,
    pub aim_y: f32,
    pub firing: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerMsg {
    Welcome(Welcome),
    Snapshot(Snapshot),
    TileUpdate { x: i32, y: i32, kind: u8, hp: u8 },
    Blast(Blast),
    PlayerJoined { id: u32 },
    PlayerLeft { id: u32 },
    RunEnded { wave: u32, kills: u32, elapsed_secs: f32 },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Welcome {
    pub your_id: u32,
    pub arena_w: u16,
    pub arena_h: u16,
    pub arena_tiles: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct Blast {
    pub x: f32,
    pub y: f32,
    pub color: [u8; 3],
    pub seed: u64,
    pub intensity: u8,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Snapshot {
    pub wave: u32,
    pub kills: u32,
    pub alive: bool,
    pub elapsed_secs: f32,
    pub players: Vec<PlayerSnap>,
    pub enemies: Vec<EnemySnap>,
    pub projectiles: Vec<ProjSnap>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PlayerSnap {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub hp: i32,
    pub aim_x: f32,
    pub aim_y: f32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EnemySnap {
    pub x: f32,
    pub y: f32,
    pub hp: i32,
    pub kind: u8,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ProjSnap {
    pub x: f32,
    pub y: f32,
}

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    bincode::serde::encode_to_vec(msg, bincode::config::standard())
        .expect("bincode encode")
}

pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Option<T> {
    bincode::serde::borrow_decode_from_slice(bytes, bincode::config::standard())
        .ok()
        .map(|(v, _)| v)
}
