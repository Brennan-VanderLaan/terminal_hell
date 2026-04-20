//! Scripted player behaviors. Each tick the runner translates a
//! `PlayerScript` + current `Game` state into a `PlayerInput`. Pure
//! functions — no state beyond the wall-clock scenario time, so the
//! same script always produces the same inputs for the same sim.

use crate::bench::scenario::ScriptedPlayer;
use crate::game::{Game, PlayerInput};

/// Declarative behavior for a scripted player during a scenario.
/// Deliberately small — we want repeatable, describable inputs the
/// telemetry can be attributed to, not emergent play.
#[derive(Clone, Copy, Debug)]
pub enum PlayerScript {
    /// No input. Player sits at spawn, doesn't aim, doesn't fire.
    /// The canonical "what does the sim cost with nothing happening
    /// on the player side" baseline.
    Stationary,
    /// No movement, but aim at the nearest enemy and hold fire.
    /// Measures pure firing-loop + projectile cost.
    ShootNearest,
    /// Circle-strafe a fixed world point at `radius` tiles, aiming
    /// outward while firing. Movement + firing + rotating sightlines
    /// — closest to "real play" of the scripts.
    CircleStrafe {
        center: (f32, f32),
        radius: f32,
        /// Angular rate, radians per second.
        rate: f32,
    },
    /// Stationary with auto-deploy cadence: every `deploy_secs`
    /// consume one turret kit. Intended for the "ring of turrets"
    /// scenarios — the player never moves, the turrets do the work.
    HoldAndDeploy {
        /// How often to fire a `DeployTurret` action while kits
        /// remain. Realistic cadence — the sim respects the runtime
        /// deploy-kit flow, not a magic spawn.
        deploy_secs: f32,
    },
}

impl PlayerScript {
    /// Convert the script's intent for the current tick into a
    /// `PlayerInput`. Returns `None` when the script makes no input
    /// change (kept for API symmetry — the runner always assigns
    /// something).
    pub fn tick(
        self,
        elapsed_secs: f32,
        player_pos: (f32, f32),
        game: &Game,
    ) -> PlayerInput {
        match self {
            PlayerScript::Stationary => PlayerInput {
                move_x: 0.0,
                move_y: 0.0,
                aim_x: 1.0,
                aim_y: 0.0,
                firing: false,
            },
            PlayerScript::ShootNearest => {
                let (ax, ay) = aim_at_nearest(player_pos, game).unwrap_or((1.0, 0.0));
                PlayerInput {
                    move_x: 0.0,
                    move_y: 0.0,
                    aim_x: ax,
                    aim_y: ay,
                    firing: true,
                }
            }
            PlayerScript::CircleStrafe { center, radius, rate } => {
                let angle = elapsed_secs * rate;
                let target = (
                    center.0 + angle.cos() * radius,
                    center.1 + angle.sin() * radius,
                );
                let dx = target.0 - player_pos.0;
                let dy = target.1 - player_pos.1;
                let len = (dx * dx + dy * dy).sqrt().max(0.001);
                let (ax, ay) = aim_at_nearest(player_pos, game).unwrap_or((dx / len, dy / len));
                PlayerInput {
                    move_x: dx / len,
                    move_y: dy / len,
                    aim_x: ax,
                    aim_y: ay,
                    firing: true,
                }
            }
            PlayerScript::HoldAndDeploy { .. } => PlayerInput {
                // Hold-and-deploy's *input* is "stand and shoot."
                // The turret-deploy action is driven by the runner
                // outside PlayerInput (it's a reliable-ordered one-
                // shot event, not an axis/button state).
                move_x: 0.0,
                move_y: 0.0,
                aim_x: 1.0,
                aim_y: 0.0,
                firing: true,
            },
        }
    }
}

/// Unit vector from `pos` toward the nearest living enemy. `None`
/// when the world has no enemies.
fn aim_at_nearest(pos: (f32, f32), game: &Game) -> Option<(f32, f32)> {
    let mut best: Option<(f32, f32, f32)> = None; // (dx, dy, d²)
    for e in &game.enemies {
        if e.hp <= 0 {
            continue;
        }
        let dx = e.x - pos.0;
        let dy = e.y - pos.1;
        let d2 = dx * dx + dy * dy;
        if best.map_or(true, |(_, _, bd)| d2 < bd) {
            best = Some((dx, dy, d2));
        }
    }
    let (dx, dy, d2) = best?;
    let len = d2.sqrt().max(0.001);
    Some((dx / len, dy / len))
}

/// Per-tick deploy-kit scheduler for `HoldAndDeploy`. Returns true
/// when the runner should fire a DeployTurret on this tick.
pub fn should_deploy_turret(
    sp: &ScriptedPlayer,
    last_deploy_secs: f32,
    elapsed_secs: f32,
) -> bool {
    match sp.script {
        PlayerScript::HoldAndDeploy { deploy_secs } => {
            elapsed_secs - last_deploy_secs >= deploy_secs
        }
        _ => false,
    }
}
