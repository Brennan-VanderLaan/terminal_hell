use crate::arena::Arena;
use crate::camera::{Camera, MipLevel};
use crate::fb::{Framebuffer, Pixel};
use crate::input::Input;
use crate::sprite;

pub struct Player {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub hp: i32,
    pub speed: f32,
    pub aim_x: f32,
    pub aim_y: f32,
    pub firing: bool,
    /// 0..100. Drains from Carcosa exposure, Hastur marks, and Sign flashes.
    /// Regens from kills in good form (for now, any kill nudges).
    pub sanity: f32,
    /// Absorbs incoming damage before HP. Capped at `max_armor()`.
    /// Replenished by ArmorPlate pickups.
    pub armor: i32,
    /// Turret-kit inventory — each increments when picked up, decrements
    /// on a deploy action. Separate from weapons so loot stays clean.
    pub turret_kits: u8,
    /// Run-persistent perks picked up from miniboss drops + wave
    /// milestones. Most modifier hooks read this vec directly.
    pub perks: Vec<crate::perk::Perk>,
    /// Rampage chain counter — number of kills within the sliding
    /// 4s window. At 3+ the player gets the damage buff.
    pub rampage_stack: u8,
    /// Seconds remaining on the Rampage kill-chain window. Reset to
    /// 4.0 on kill; counts down to 0; at 0 the stack resets.
    pub rampage_window: f32,
    /// Seconds remaining on the Rampage damage buff (8s at arm).
    pub rampage_buff_ttl: f32,
    /// Adrenaline speed-burst timer — 0.6s on damage taken, decays.
    pub adrenaline_ttl: f32,
    /// Blood-pool regen throttle — internal cooldown so Bloodhound
    /// heal doesn't apply every tick.
    pub bloodhound_tick: f32,
    /// Equipped traversal verb — Dash, Blink, Grapple, etc. One slot;
    /// a new traversal pickup replaces the old one.
    pub traversal: Option<crate::traversal::TraversalVerb>,
    pub traversal_cooldown: f32,
    /// Dash/slide i-frame timer — while > 0 the player takes no
    /// damage. Lit by Dash + Slide uses.
    pub iframe_ttl: f32,
    /// Locked-motion timer for Slide (and any future forced-motion
    /// verbs). While > 0, movement ignores player input and moves
    /// at `locked_motion_vec` speed.
    pub locked_motion_ttl: f32,
    pub locked_motion_vec: (f32, f32),
    /// Phase wall-pass charges left. While > 0 the next wall the
    /// player walks into is ignored for collision.
    pub phase_charges: u8,
    /// Seconds of blood-spatter emission still queued after the last
    /// hit. Game tick drains this and emits puffs on the cadence
    /// below.
    pub bleed_ttl: f32,
    /// Sub-timer between blood puffs while `bleed_ttl > 0`. Short
    /// for heavy hits, long for scratches. Reset by the game tick
    /// every time a puff fires.
    pub bleed_cadence: f32,
    /// Per-puff particle-count tier — 1 for light scratch, 2 for
    /// solid hits, 3 for "you're in trouble" wounds.
    pub bleed_intensity: u8,
}

pub const PLAYER_MAX_HP: i32 = 100;
pub const PLAYER_BASE_MAX_ARMOR: i32 = 100;

impl Player {
    pub fn new(id: u32, x: f32, y: f32) -> Self {
        Self {
            id,
            x,
            y,
            hp: PLAYER_MAX_HP,
            speed: 38.0,
            aim_x: 1.0,
            aim_y: 0.0,
            firing: false,
            sanity: 100.0,
            armor: 0,
            turret_kits: 0,
            perks: Vec::new(),
            rampage_stack: 0,
            rampage_window: 0.0,
            rampage_buff_ttl: 0.0,
            adrenaline_ttl: 0.0,
            bloodhound_tick: 0.0,
            traversal: None,
            traversal_cooldown: 0.0,
            iframe_ttl: 0.0,
            locked_motion_ttl: 0.0,
            locked_motion_vec: (0.0, 0.0),
            phase_charges: 0,
            bleed_ttl: 0.0,
            bleed_cadence: 0.0,
            bleed_intensity: 0,
        }
    }

    pub fn has_perk(&self, p: crate::perk::Perk) -> bool {
        self.perks.contains(&p)
    }

    /// Max armor including Survivor perk bonus.
    pub fn max_armor(&self) -> i32 {
        if self.has_perk(crate::perk::Perk::Survivor) {
            PLAYER_BASE_MAX_ARMOR + 50
        } else {
            PLAYER_BASE_MAX_ARMOR
        }
    }

    /// Fire-rate multiplier applied to weapon cooldowns. Currently
    /// only QuickHands bumps it.
    pub fn fire_rate_mul(&self) -> f32 {
        if self.has_perk(crate::perk::Perk::QuickHands) {
            1.25
        } else {
            1.0
        }
    }

    /// Outgoing damage multiplier — Rampage stacks here when the
    /// kill-chain buff is live.
    pub fn outgoing_damage_mul(&self) -> f32 {
        if self.has_perk(crate::perk::Perk::Rampage) && self.rampage_buff_ttl > 0.0 {
            1.30
        } else {
            1.0
        }
    }

    /// Movement speed multiplier — Adrenaline fires a short burst
    /// when damage is taken.
    pub fn speed_mul(&self) -> f32 {
        if self.has_perk(crate::perk::Perk::Adrenaline) && self.adrenaline_ttl > 0.0 {
            1.60
        } else {
            1.0
        }
    }

    /// Apply raw damage with armor-first soak + LastStand resist. Armor
    /// absorbs up to 60% of the incoming hit (the rest still chips
    /// HP) so armor delays but doesn't trivialize pressure.
    pub fn take_damage(&mut self, raw: i32) {
        if raw <= 0 || self.is_iframed() {
            return;
        }
        // LastStand: when low-HP, cut incoming damage by 40%.
        let mut dmg = raw;
        // Slide's final 60% of duration grants 30% damage resist.
        if self.locked_motion_ttl > 0.0 {
            dmg = ((dmg as f32) * 0.7).round() as i32;
        }
        if self.has_perk(crate::perk::Perk::LastStand) && self.hp <= PLAYER_MAX_HP / 4 {
            dmg = ((dmg as f32) * 0.6).round() as i32;
        }
        let armor_share = ((dmg as f32) * 0.6).round() as i32;
        let hp_share = dmg - armor_share;
        let absorbed = armor_share.min(self.armor);
        self.armor -= absorbed;
        let leftover = armor_share - absorbed;
        self.hp -= hp_share + leftover;
        if self.hp < 0 {
            self.hp = 0;
        }
        // Adrenaline: any incoming damage triggers a brief speed
        // burst — rewards aggressive reposition under pressure.
        if self.has_perk(crate::perk::Perk::Adrenaline) {
            self.adrenaline_ttl = 0.6;
        }
        // Start a bleed emission that lasts a few seconds; severity
        // scales with the raw hit, not the post-armor residue — a
        // big shotgun blast should gush even if plate ate most of
        // the damage. Caps at 6s so sustained small pings don't
        // stack into a permanent blood fountain.
        let bleed_add = 1.2 + (raw as f32) * 0.08;
        self.bleed_ttl = (self.bleed_ttl + bleed_add).min(6.0);
        let new_intensity = if raw >= 25 {
            3
        } else if raw >= 10 {
            2
        } else {
            1
        };
        self.bleed_intensity = self.bleed_intensity.max(new_intensity);
        // Kick an immediate puff so the first frame post-hit shows
        // something even if the cadence hadn't rolled over yet.
        self.bleed_cadence = 0.0;
    }

    /// Drive the bleed cadence. Returns Some(intensity) on the frames
    /// where the game should emit a blood puff at the player's
    /// position. Heavier hits fire more frequent puffs; when `bleed_ttl`
    /// runs out, the intensity decays back to 0.
    pub fn tick_bleed(&mut self, dt: f32) -> Option<u8> {
        if self.bleed_ttl <= 0.0 {
            self.bleed_intensity = 0;
            self.bleed_cadence = 0.0;
            return None;
        }
        self.bleed_ttl = (self.bleed_ttl - dt).max(0.0);
        self.bleed_cadence -= dt;
        if self.bleed_cadence > 0.0 {
            return None;
        }
        // Cadence: heavier intensity fires more often. Light bleeds
        // puff every ~0.35s; "in trouble" bleeds puff every ~0.15s.
        self.bleed_cadence = match self.bleed_intensity {
            3 => 0.15,
            2 => 0.25,
            _ => 0.35,
        };
        Some(self.bleed_intensity)
    }

    /// Called on every enemy kill credited to this player. Handles
    /// CorpsePulse heal + Rampage kill-chain arming.
    pub fn note_kill(&mut self) {
        if self.has_perk(crate::perk::Perk::CorpsePulse) {
            self.hp = (self.hp + 4).min(PLAYER_MAX_HP);
        }
        if self.has_perk(crate::perk::Perk::Rampage) {
            self.rampage_window = 4.0;
            self.rampage_stack = self.rampage_stack.saturating_add(1);
            if self.rampage_stack >= 3 {
                self.rampage_buff_ttl = 8.0;
            }
        }
    }

    /// Called at the start of each new wave. Refreshes Survivor's
    /// +10 armor on arrival.
    pub fn on_wave_start(&mut self) {
        if self.has_perk(crate::perk::Perk::Survivor) {
            self.armor = (self.armor + 10).min(self.max_armor());
        }
    }

    /// Tick perk-internal timers. Call from the main player update
    /// loop.
    pub fn tick_perk_timers(&mut self, dt: f32) {
        if self.rampage_window > 0.0 {
            self.rampage_window = (self.rampage_window - dt).max(0.0);
            if self.rampage_window <= 0.0 {
                self.rampage_stack = 0;
            }
        }
        if self.rampage_buff_ttl > 0.0 {
            self.rampage_buff_ttl = (self.rampage_buff_ttl - dt).max(0.0);
        }
        if self.adrenaline_ttl > 0.0 {
            self.adrenaline_ttl = (self.adrenaline_ttl - dt).max(0.0);
        }
        if self.bloodhound_tick > 0.0 {
            self.bloodhound_tick = (self.bloodhound_tick - dt).max(0.0);
        }
        if self.traversal_cooldown > 0.0 {
            self.traversal_cooldown = (self.traversal_cooldown - dt).max(0.0);
        }
        if self.iframe_ttl > 0.0 {
            self.iframe_ttl = (self.iframe_ttl - dt).max(0.0);
        }
        if self.locked_motion_ttl > 0.0 {
            self.locked_motion_ttl = (self.locked_motion_ttl - dt).max(0.0);
        }
    }

    pub fn is_iframed(&self) -> bool {
        self.iframe_ttl > 0.0
    }

    pub fn apply_input_local(&mut self, input: &Input, arena: &Arena, dt: f32) {
        let (dx, dy) = input.move_vec();
        self.step(dx, dy, arena, dt);
    }

    pub fn apply_remote_input(
        &mut self,
        move_x: f32,
        move_y: f32,
        aim_x: f32,
        aim_y: f32,
        firing: bool,
        arena: &Arena,
        dt: f32,
    ) {
        self.aim_x = aim_x;
        self.aim_y = aim_y;
        self.firing = firing;
        self.step(move_x, move_y, arena, dt);
    }

    fn step(&mut self, dx: f32, dy: f32, arena: &Arena, dt: f32) {
        let speed = self.speed * self.speed_mul();
        // Locked motion (Slide): overrides input direction, moves at
        // 1.8× speed along the committed vector.
        let (mx, my, speed_mul) = if self.locked_motion_ttl > 0.0 {
            (self.locked_motion_vec.0, self.locked_motion_vec.1, 1.8)
        } else {
            (dx, dy, 1.0)
        };
        let step_x = mx * speed * speed_mul * dt;
        let step_y = my * speed * speed_mul * dt;
        let target_x = self.x + step_x;
        let allow_wall = self.phase_charges > 0;
        if allow_wall || !collides(arena, target_x, self.y) {
            self.x = target_x;
            if allow_wall && collides(arena, target_x, self.y) {
                // Consumed one phase charge to walk through a wall.
                self.phase_charges = self.phase_charges.saturating_sub(1);
            }
        }
        let target_y = self.y + step_y;
        let allow_wall_y = self.phase_charges > 0;
        if allow_wall_y || !collides(arena, self.x, target_y) {
            self.y = target_y;
            if allow_wall_y && collides(arena, self.x, target_y) {
                self.phase_charges = self.phase_charges.saturating_sub(1);
            }
        }
    }

    /// Fire the equipped traversal verb. Returns true if the verb
    /// fired this tick (for feedback / cooldown logging).
    pub fn activate_traversal(&mut self, arena: &Arena) -> bool {
        if self.traversal_cooldown > 0.0 {
            return false;
        }
        let Some(verb) = self.traversal else { return false };
        match verb {
            crate::traversal::TraversalVerb::Dash => {
                // Burst in aim direction ~7 tiles with i-frames.
                let target_x = self.x + self.aim_x * 7.0;
                let target_y = self.y + self.aim_y * 7.0;
                if !collides(arena, target_x, self.y) {
                    self.x = target_x;
                }
                if !collides(arena, self.x, target_y) {
                    self.y = target_y;
                }
                self.iframe_ttl = 0.35;
            }
            crate::traversal::TraversalVerb::Blink => {
                // Teleport 9 tiles forward — passes through walls.
                self.x += self.aim_x * 9.0;
                self.y += self.aim_y * 9.0;
                self.sanity = (self.sanity - 6.0).max(0.0);
            }
            crate::traversal::TraversalVerb::Grapple => {
                // Find the first wall along the aim vector up to 16
                // tiles out, snap to just in front of it.
                let from = (self.x, self.y);
                let to = (self.x + self.aim_x * 16.0, self.y + self.aim_y * 16.0);
                if let Some(hit) = arena.raycast_wall(from, to) {
                    self.x = hit.point.0 + hit.normal.0 as f32 * 0.5;
                    self.y = hit.point.1 + hit.normal.1 as f32 * 0.5;
                } else {
                    // No wall to hook — full pull to end point.
                    self.x = to.0;
                    self.y = to.1;
                }
            }
            crate::traversal::TraversalVerb::Slide => {
                self.locked_motion_ttl = 0.6;
                self.locked_motion_vec = (self.aim_x, self.aim_y);
            }
            crate::traversal::TraversalVerb::Phase => {
                self.phase_charges = 2;
                self.sanity = (self.sanity - 4.0).max(0.0);
            }
        }
        self.traversal_cooldown = verb.cooldown();
        true
    }

    pub fn render(
        &self,
        fb: &mut Framebuffer,
        camera: &Camera,
        is_self: bool,
        marked: bool,
    ) {
        let (sx, sy) = camera.world_to_screen((self.x, self.y));
        let center = (sx.round() as i32, sy.round() as i32);
        let self_color = Pixel::rgb(80, 255, 220);
        let other_color = Pixel::rgb(140, 255, 100);
        let tint = if is_self { self_color } else { other_color };

        let mip = camera.mip_level();
        if mip.shows_sprite() {
            let body = sprite::player_body();
            body.blit_scaled(fb, center, camera.zoom);
            let hi = matches!(mip, MipLevel::Hero);
            sprite::render_player_barrel(
                fb,
                sx,
                sy,
                self.aim_x,
                self.aim_y,
                camera.zoom,
                hi,
            );
            // Muzzle flash only at Close/Hero tiers — keeps the Normal tier
            // reading clean and matches where the barrel detail lives.
            if self.firing && mip.shows_overlay() {
                let intensity = if hi { 1.0 } else { 0.7 };
                sprite::render_muzzle_flash(
                    fb,
                    sx,
                    sy,
                    self.aim_x,
                    self.aim_y,
                    camera.zoom,
                    intensity,
                );
            }
            if marked {
                sprite::render_hastur_mark(fb, center.0, center.1);
            }
        } else if matches!(mip, MipLevel::Blob) {
            sprite::render_blob(fb, center, tint);
            if marked {
                sprite::render_hastur_mark(fb, center.0, center.1);
            }
        } else {
            sprite::render_dot(fb, center, tint);
            // Even at max zoom-out, marked survivors keep a tiny crown
            // so the team can locate them on the overview.
            if marked && center.1 >= 2 {
                fb.set(center.0 as u16, (center.1 - 2) as u16, Pixel::rgb(255, 220, 80));
            }
        }
    }
}

fn collides(arena: &Arena, x: f32, y: f32) -> bool {
    // 1×2 collider in pixel space — movement still uses the old tile grid for
    // wall collision; sprite size is visual-only.
    let tx = x.floor() as i32;
    let ty0 = y.floor() as i32;
    let ty1 = ty0 + 1;
    arena.is_wall(tx, ty0) || arena.is_wall(tx, ty1)
}
