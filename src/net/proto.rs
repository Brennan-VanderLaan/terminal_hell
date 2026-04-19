//! Wire protocol. Messages are bincode-serde encoded. Channels:
//!
//! - `ReliableOrdered` (0): Welcome, TileUpdate, PlayerJoined/Left, RunEnded
//! - `Unreliable` (1): Snapshot (positions), Input, Blast (visual-only)

use crate::primitive::{Primitive, Rarity};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientMsg {
    Input(ClientInput),
    /// One-shot "grab the nearest pickup" event. Sent on the press edge of
    /// the interact key (E) over the reliable-ordered channel.
    Interact,
    /// One-shot "swap to next weapon slot" event. Reliable-ordered.
    CycleWeapon,
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
    /// Seed used to generate this arena. Clients can regenerate locally if
    /// they prefer, though sending the encoded tile state keeps things
    /// robust to generator-version drift across builds.
    pub arena_seed: u64,
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
    pub pickups: Vec<PickupSnap>,
    /// Indexed per-player (by order in `players`): which weapon slot is
    /// active, and a short loadout summary for the HUD.
    pub weapons: Vec<WeaponSnap>,
    /// Active enemy hitscan tracers; clients render these until ttl expires.
    pub hitscans: Vec<HitscanSnap>,
    pub kiosks: Vec<KioskSnap>,
    pub active_brands: Vec<String>,
    /// 0=Breathe, 1=Vote, 2=Stock, 3=Warning, u8::MAX=no intermission.
    pub intermission_phase: u8,
    pub phase_timer: f32,
    pub corruption: f32,
    pub marked_player_id: u32, // 0 == no mark
    pub yellow_signs: Vec<SignSnap>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SignSnap {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub ttl: f32,
    pub ttl_max: f32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct KioskSnap {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub brand_id: String,
    pub brand_name: String,
    pub color: [u8; 3],
    pub votes: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HitscanSnap {
    pub from_x: f32,
    pub from_y: f32,
    pub to_x: f32,
    pub to_y: f32,
    pub ttl: f32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PickupSnap {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub rarity: Rarity,
    pub primitives: Vec<Primitive>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct WeaponSnap {
    pub player_id: u32,
    pub active_slot: u8,
    pub slot0: Option<WeaponLoadout>,
    pub slot1: Option<WeaponLoadout>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WeaponLoadout {
    pub rarity: Rarity,
    pub primitives: Vec<Primitive>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PlayerSnap {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub hp: i32,
    pub aim_x: f32,
    pub aim_y: f32,
    pub sanity: f32,
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
