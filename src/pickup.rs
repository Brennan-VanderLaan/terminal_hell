//! Ground pickups: weapons + consumables + deployables. Picked up by
//! pressing E when within reach. Each `PickupKind` has its own
//! consume effect — weapons swap into the active slot, medkits heal,
//! armor plates refill the damage buffer, sanity doses refill
//! sanity, turret kits increment the deploy inventory.
//!
//! Weapon pickups carry the rolled primitive slots from the loot
//! system; consumables carry a numeric amount; turret kits are a
//! one-shot use.

use crate::camera::Camera;
use crate::fb::{Framebuffer, Pixel};
use crate::primitive::{Primitive, Rarity};
use crate::sprite;
use crate::weapon::{FireMode, Weapon};
use serde::{Deserialize, Serialize};

const DEFAULT_TTL: f32 = 45.0;

/// Which kind of pickup this is — drives consume logic + rendering.
/// Serialized over the wire so clients can tag their view identically.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PickupKind {
    /// Weapon drop with a rarity + primitive loadout + fire mode.
    Weapon {
        rarity: Rarity,
        primitives: Vec<Primitive>,
        #[serde(default)]
        mode: FireMode,
    },
    /// Heals HP on pickup.
    MedKit { heal: i32 },
    /// Restores sanity.
    SanityDose { restore: f32 },
    /// Refills armor buffer.
    ArmorPlate { absorb: i32 },
    /// Inventory-grant: bumps the player's turret_kits counter.
    /// Deploy the turret with a separate key (future).
    TurretKit,
    /// Run-persistent passive modifier. Grants the perk to the
    /// picker; stacks with existing perks.
    Perk { perk: crate::perk::Perk },
    /// Replaces the player's current traversal verb.
    Traversal { verb: crate::traversal::TraversalVerb },
}

impl PickupKind {
    pub fn to_byte(&self) -> u8 {
        match self {
            PickupKind::Weapon { .. } => 0,
            PickupKind::MedKit { .. } => 1,
            PickupKind::SanityDose { .. } => 2,
            PickupKind::ArmorPlate { .. } => 3,
            PickupKind::TurretKit => 4,
            PickupKind::Perk { .. } => 5,
            PickupKind::Traversal { .. } => 6,
        }
    }

    /// Display halo color per kind — gives each pickup an at-a-
    /// glance readable silhouette on the ground.
    pub fn halo_color(&self) -> Pixel {
        match self {
            PickupKind::Weapon { rarity, .. } => rarity.color(),
            PickupKind::MedKit { .. } => Pixel::rgb(255, 80, 110),
            PickupKind::SanityDose { .. } => Pixel::rgb(120, 200, 255),
            PickupKind::ArmorPlate { .. } => Pixel::rgb(180, 180, 210),
            PickupKind::TurretKit => Pixel::rgb(255, 200, 60),
            PickupKind::Perk { perk } => perk.color(),
            PickupKind::Traversal { verb } => verb.color(),
        }
    }

    /// Short on-ground label drawn above the pickup. Keeps players
    /// from having to memorize the 2×2 sprite palette for each
    /// consumable — a glance is enough.
    pub fn label(&self) -> String {
        match self {
            PickupKind::Weapon { rarity, primitives, mode } => {
                let prims: String = primitives.iter().map(|p| p.glyph()).collect();
                if prims.is_empty() {
                    format!("{:?} · {}", rarity, mode.label())
                } else {
                    format!("{:?} · {} [{}]", rarity, mode.label(), prims)
                }
            }
            PickupKind::MedKit { heal } => format!("medkit +{heal}"),
            PickupKind::SanityDose { restore } => format!("sanity +{}", restore.round() as i32),
            PickupKind::ArmorPlate { absorb } => format!("armor +{absorb}"),
            PickupKind::TurretKit => "turret kit".to_string(),
            PickupKind::Perk { perk } => format!("perk · {}", perk.name()),
            PickupKind::Traversal { verb } => format!("verb · {}", verb.name()),
        }
    }

    /// "Equipment" — weapons + deployables need an explicit E press
    /// so the player doesn't accidentally swap their carefully
    /// rolled rare loadout by jogging past the wrong crate. Buffs
    /// and consumables auto-pick on collision.
    pub fn requires_interact(&self) -> bool {
        matches!(
            self,
            PickupKind::Weapon { .. } | PickupKind::TurretKit
        )
    }

    /// One-glyph label drawn at the center of the pickup at the
    /// Close/Hero mip so players can tell consumables apart.
    pub fn glyph_color(&self) -> Pixel {
        match self {
            PickupKind::Weapon { .. } => Pixel::rgb(20, 20, 30),
            PickupKind::MedKit { .. } => Pixel::rgb(255, 255, 255),
            PickupKind::SanityDose { .. } => Pixel::rgb(255, 255, 255),
            PickupKind::ArmorPlate { .. } => Pixel::rgb(60, 60, 80),
            PickupKind::TurretKit => Pixel::rgb(50, 30, 10),
            PickupKind::Perk { .. } => Pixel::rgb(20, 20, 35),
            PickupKind::Traversal { .. } => Pixel::rgb(30, 30, 60),
        }
    }
}

pub struct Pickup {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub kind: PickupKind,
    pub ttl: f32,
    pub flash_phase: f32,
}

impl Pickup {
    pub fn new_weapon(id: u32, x: f32, y: f32, rarity: Rarity, primitives: Vec<Primitive>) -> Self {
        Self::new_weapon_with_mode(id, x, y, rarity, primitives, FireMode::default())
    }

    pub fn new_weapon_with_mode(
        id: u32,
        x: f32,
        y: f32,
        rarity: Rarity,
        primitives: Vec<Primitive>,
        mode: FireMode,
    ) -> Self {
        Self {
            id,
            x,
            y,
            kind: PickupKind::Weapon { rarity, primitives, mode },
            ttl: DEFAULT_TTL,
            flash_phase: 0.0,
        }
    }

    pub fn new_kind(id: u32, x: f32, y: f32, kind: PickupKind) -> Self {
        Self { id, x, y, kind, ttl: DEFAULT_TTL, flash_phase: 0.0 }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        self.ttl -= dt;
        self.flash_phase = (self.flash_phase + dt * 4.0) % 1.0;
        self.ttl > 0.0
    }

    /// Build a `Weapon` runtime instance from this pickup if it's a
    /// weapon kind. Panics on non-weapon pickups — callers should
    /// check `kind` first.
    pub fn into_weapon(&self) -> Option<Weapon> {
        match &self.kind {
            PickupKind::Weapon { rarity, primitives, mode } => {
                Some(Weapon::with_slots(*rarity, primitives.clone()).with_mode(*mode))
            }
            _ => None,
        }
    }

    pub fn pickup_radius() -> f32 {
        6.0
    }

    pub fn render(&self, fb: &mut Framebuffer, camera: &Camera) {
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let cx = sx.round() as i32;
        let cy = sy.round() as i32;
        let rarity = self.kind.halo_color();
        let mip = camera.mip_level();

        if mip.shows_sprite() {
            let scale = (camera.zoom * 0.5).max(1.0);
            let pulse = 0.6 + 0.4 * (self.flash_phase * std::f32::consts::TAU).sin().abs();
            let halo = Pixel::rgb(
                (rarity.r as f32 * pulse) as u8,
                (rarity.g as f32 * pulse) as u8,
                (rarity.b as f32 * pulse) as u8,
            );
            for dy in -2..=2_i32 {
                for dx in -2..=2_i32 {
                    if dx.abs() == 2 || dy.abs() == 2 {
                        stamp(fb, cx, cy, dx, dy, scale, halo);
                    }
                }
            }

            // Inner pattern per kind — weapons show slot colors, heals
            // show a cross, armor shows plate segments, sanity shows a
            // pulse ring, turret kit shows a box.
            match &self.kind {
                PickupKind::Weapon { rarity: r, primitives, .. } => {
                    let slot_colors: Vec<Pixel> =
                        primitives.iter().map(|p| p.color()).collect();
                    let mut i = 0;
                    for dy in -1..=1_i32 {
                        for dx in -1..=1_i32 {
                            let color = if slot_colors.is_empty() {
                                r.color()
                            } else {
                                slot_colors[i % slot_colors.len()]
                            };
                            stamp(fb, cx, cy, dx, dy, scale, color);
                            i += 1;
                        }
                    }
                }
                PickupKind::MedKit { .. } => {
                    let cross = Pixel::rgb(255, 255, 255);
                    stamp(fb, cx, cy, 0, -1, scale, cross);
                    stamp(fb, cx, cy, 0, 0, scale, cross);
                    stamp(fb, cx, cy, 0, 1, scale, cross);
                    stamp(fb, cx, cy, -1, 0, scale, cross);
                    stamp(fb, cx, cy, 1, 0, scale, cross);
                }
                PickupKind::SanityDose { .. } => {
                    // Concentric cyan ring.
                    let inner = Pixel::rgb(200, 240, 255);
                    stamp(fb, cx, cy, 0, 0, scale, inner);
                    for &(dx, dy) in &[(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        stamp(fb, cx, cy, dx, dy, scale, rarity);
                    }
                }
                PickupKind::ArmorPlate { .. } => {
                    // Plate segments.
                    let plate = Pixel::rgb(220, 220, 240);
                    let seam = Pixel::rgb(80, 80, 110);
                    for dx in -1..=1_i32 {
                        stamp(fb, cx, cy, dx, -1, scale, plate);
                        stamp(fb, cx, cy, dx, 0, scale, seam);
                        stamp(fb, cx, cy, dx, 1, scale, plate);
                    }
                }
                PickupKind::TurretKit => {
                    // Crated turret — solid square with an antenna.
                    let body = Pixel::rgb(255, 220, 100);
                    let dark = Pixel::rgb(90, 60, 10);
                    for dy in -1..=1_i32 {
                        for dx in -1..=1_i32 {
                            stamp(fb, cx, cy, dx, dy, scale, body);
                        }
                    }
                    stamp(fb, cx, cy, 0, -2, scale, dark);
                }
                PickupKind::Perk { perk } => {
                    // Diamond halo pulsing in the perk's signature
                    // color + a bright glyph pip at center. Reads
                    // as "special / important."
                    let c = perk.color();
                    let bright = Pixel::rgb(
                        (c.r as u16 + 60).min(255) as u8,
                        (c.g as u16 + 60).min(255) as u8,
                        (c.b as u16 + 60).min(255) as u8,
                    );
                    stamp(fb, cx, cy, -1, -1, scale, c);
                    stamp(fb, cx, cy, 1, -1, scale, c);
                    stamp(fb, cx, cy, -1, 1, scale, c);
                    stamp(fb, cx, cy, 1, 1, scale, c);
                    stamp(fb, cx, cy, 0, 0, scale, bright);
                    stamp(fb, cx, cy, 0, -2, scale, c);
                    stamp(fb, cx, cy, 0, 2, scale, c);
                    stamp(fb, cx, cy, -2, 0, scale, c);
                    stamp(fb, cx, cy, 2, 0, scale, c);
                }
                PickupKind::Traversal { verb } => {
                    // Arrow-shaped sprite to evoke movement. Four
                    // arms radiating out from a bright core.
                    let c = verb.color();
                    let bright = Pixel::rgb(
                        (c.r as u16 + 50).min(255) as u8,
                        (c.g as u16 + 50).min(255) as u8,
                        (c.b as u16 + 50).min(255) as u8,
                    );
                    stamp(fb, cx, cy, 0, 0, scale, bright);
                    stamp(fb, cx, cy, -1, 0, scale, c);
                    stamp(fb, cx, cy, 1, 0, scale, c);
                    stamp(fb, cx, cy, 0, -1, scale, c);
                    stamp(fb, cx, cy, 0, 1, scale, c);
                    stamp(fb, cx, cy, -2, 0, scale, c);
                    stamp(fb, cx, cy, 2, 0, scale, c);
                    stamp(fb, cx, cy, 0, -2, scale, c);
                    stamp(fb, cx, cy, 0, 2, scale, c);
                }
            }

            if self.ttl < 8.0 {
                let dim = Pixel::rgb(40, 30, 50);
                if (self.flash_phase * 8.0).floor() as i32 % 2 == 0 {
                    stamp(fb, cx, cy, 0, 0, scale, dim);
                }
            }
        } else if matches!(mip, crate::camera::MipLevel::Blob) {
            sprite::render_blob(fb, (cx, cy), rarity);
        } else {
            sprite::render_dot(fb, (cx, cy), rarity);
        }
    }
}

fn stamp(fb: &mut Framebuffer, cx: i32, cy: i32, dx: i32, dy: i32, scale: f32, color: Pixel) {
    let s = scale.max(1.0);
    let x0 = cx + (dx as f32 * s).round() as i32;
    let y0 = cy + (dy as f32 * s).round() as i32;
    let x1 = x0 + s.round() as i32;
    let y1 = y0 + s.round() as i32;
    for py in y0..y1 {
        for px in x0..x1 {
            if px >= 0 && py >= 0 {
                fb.set(px as u16, py as u16, color);
            }
        }
    }
}
