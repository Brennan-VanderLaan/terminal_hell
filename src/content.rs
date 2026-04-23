//! Content-pack loader.
//!
//! The core pack is embedded in the binary via `include_dir!` and parsed at
//! startup. External packs in a sibling `content/` directory on disk can
//! override / extend core in future milestones (not wired for v0.3).
//!
//! Archetype *identity* still lives in the Rust `enum Archetype` — this pack
//! system only data-drives per-archetype stats and brand definitions. A
//! deeper refactor to content-driven archetypes arrives with audio + primitives.

use anyhow::{Context, Result, anyhow};
use include_dir::{Dir, include_dir};
use serde::Deserialize;
use std::collections::HashMap;

use crate::behavior::BehaviorToml;
use crate::enemy::Archetype;
use crate::perk::Perk;
use crate::primitive::Primitive;
use crate::substance::{SubstanceId, SubstanceRegistry, SubstanceToml};
use crate::traversal::TraversalVerb;
use crate::weapon::FireMode;

static CORE_PACK: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/content/core");

#[derive(Debug, Clone, Deserialize)]
pub struct ArchetypeStats {
    pub hp: i32,
    pub speed: f32,
    pub touch_damage: i32,
    pub hit_radius: f32,
    #[serde(default)]
    pub reach: Option<f32>,
    #[serde(default)]
    pub preferred_distance: Option<f32>,
    #[serde(default)]
    pub ranged_damage: Option<i32>,
    /// Optional ASCII-art file under `content/core/art/` — if present,
    /// overrides the hardcoded Rust sprite builder for this archetype.
    #[serde(default)]
    pub art: Option<String>,
    /// Single-char palette keys → hex colors, used with `art`.
    #[serde(default)]
    pub palette: Option<HashMap<String, String>>,
    /// Archetype tags — feeds into the interaction framework for
    /// body-on-contact / body-on-death reactions.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Name of a Reaction registered in the InteractionBook,
    /// triggered when an instance of this archetype dies.
    #[serde(default)]
    pub body_on_death: Option<String>,
    /// Gore profile name: "standard", "heavy", or "extreme". Controls
    /// particle count, palette breadth, pool trail size, and wall
    /// splatter behavior on death.
    #[serde(default)]
    pub gore_profile: Option<String>,
    /// Primitive ids this archetype thematically drops. Used when a
    /// weapon rolls off this archetype's corpse — entries are sampled
    /// with replacement so a Rare 3-slot weapon can stack the same
    /// primitive. Empty/absent means fall back to the global pool.
    #[serde(default)]
    pub signature_primitives: Option<Vec<String>>,
    /// Fire-mode label for weapons this archetype drops. One of
    /// pulse / auto / burst / pump / rail. Absent → random mode per
    /// roll, preserving variety when the archetype is intentionally
    /// generic (rusher, swarmling).
    #[serde(default)]
    pub signature_fire_mode: Option<String>,
    /// Optional [`BehaviorToml`] override block. Merges over the
    /// archetype's default registered BehaviorSet at Game construction
    /// time. Absent → the archetype uses its `BehaviorRegistry::
    /// with_builtins` default. B1 wiring — the sim doesn't consult the
    /// resulting BehaviorSet at dispatch yet; B2+ starts routing
    /// through it.
    #[serde(default)]
    pub behavior: Option<BehaviorToml>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BrandDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub spawn_pool: Vec<String>,
    pub miniboss: String,
    #[serde(default)]
    pub spawn_weights: HashMap<String, u32>,
    /// Legacy fine-grained scaling — ignored by the current director (which
    /// uses global budget + brand-count bonus), but still parsed so older
    /// brand files remain loadable.
    #[serde(default)]
    pub scaling: Option<BrandScaling>,
    /// Traversal verb id this brand's pickups default to. If unset,
    /// traversal drops roll from the full verb pool. Brand flavor:
    /// grounded mil-sim uses "slide", arcane brands use "blink",
    /// Flood uses "phase", Deep Rock uses "grapple", etc.
    #[serde(default)]
    pub signature_traversal: Option<String>,
    /// Perk-id pool this brand biases toward. When a perk is dropped
    /// in this brand, we try to roll one from this list first; if the
    /// player already has all of them, fall back to any unowned perk.
    #[serde(default)]
    pub signature_perks: Option<Vec<String>>,
    /// Legacy per-archetype sprite overrides. Entry format:
    /// `archetype_snake_name → art_filename.art`, paired with a
    /// matching `[sprite_palettes.<arch>]` table. Deprecated in favor
    /// of `[units.*]` BrandUnit entries which carry their own sprite
    /// + palette alongside stat + behavior overrides. Kept working
    /// so un-converted brands continue to load.
    #[serde(default)]
    pub sprite_overrides: HashMap<String, String>,
    #[serde(default)]
    pub sprite_palettes: HashMap<String, HashMap<String, String>>,
    /// Brand-authored unit roster. Keys are unit ids (e.g.
    /// "pistol_scav", "combine_soldier") that the brand's
    /// `spawn_pool` + `spawn_weights` reference. Each entry merges
    /// stat + behavior + sprite overrides onto a backing sim
    /// archetype, so one brand can ship multiple distinct units
    /// that share an archetype's AI loop (e.g. three flavors of
    /// Rusher in a single brand, each with its own hp/speed/fire
    /// profile).
    ///
    /// See also: `BrandUnit`. Brands that don't declare `[units.*]`
    /// tables still work — their `spawn_pool` is read as raw
    /// archetype snake names and the resolver returns synthetic
    /// ResolvedUnits using archetype defaults.
    #[serde(default)]
    pub units: HashMap<String, BrandUnit>,
}

/// Per-brand authored unit. Combines a backing sim-mechanic
/// `archetype` with per-unit overrides for stats, behavior, sprite,
/// and ranged-fire cadence. Resolved at spawn time via
/// [`ContentDb::resolve_unit`] into a [`ResolvedUnit`].
#[derive(Debug, Clone, Deserialize)]
pub struct BrandUnit {
    /// Which sim archetype backs this unit. Must be a valid
    /// `Archetype::from_name` result; validated at content load.
    pub archetype: String,
    /// HUD-facing name. Used in the kill feed + brand strip. When
    /// absent, the unit id is humanized (e.g. `"pistol_scav"` →
    /// `"Pistol Scav"`).
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub hp: Option<i32>,
    #[serde(default)]
    pub speed: Option<f32>,
    #[serde(default)]
    pub touch_damage: Option<i32>,
    #[serde(default)]
    pub hit_radius: Option<f32>,
    #[serde(default)]
    pub reach: Option<f32>,
    #[serde(default)]
    pub preferred_distance: Option<f32>,
    #[serde(default)]
    pub ranged_damage: Option<i32>,
    /// Archetype TOML tags override. When Some, replaces the
    /// archetype's tag list entirely — this is what the
    /// interaction-matrix keys off of, so merging would be
    /// semantically weird (a unit that adds "armored" on top of
    /// "biological" is authored by repeating both).
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub body_on_death: Option<String>,
    #[serde(default)]
    pub gore_profile: Option<String>,
    #[serde(default)]
    pub signature_primitives: Option<Vec<String>>,
    #[serde(default)]
    pub signature_fire_mode: Option<String>,
    /// Sprite art filename (relative to `content/core/art/<brand_id>/`
    /// or `content/core/art/`).
    #[serde(default)]
    pub sprite: Option<String>,
    #[serde(default)]
    pub palette: Option<HashMap<String, String>>,
    /// Optional [`BehaviorToml`] override. Merges over the archetype's
    /// registered default (same semantics as the archetype-level
    /// [stats].behavior block at B1's apply_toml — fields set here
    /// override; unset fields inherit).
    #[serde(default)]
    pub behavior: Option<BehaviorToml>,
    /// Optional per-unit ranged-fire cadence override. Swaps the
    /// default Enemy::ranged_profile() result at spawn time. Lets
    /// Half-Life's Combine Soldier fire 4-round bursts while
    /// Tarkov's Scav Sniper uses the deliberate single-shot sniper
    /// cadence, even though both are backed by the Marksman
    /// archetype.
    #[serde(default)]
    pub ranged_profile: Option<RangedProfileToml>,
}

/// TOML schema for a ranged-fire cadence override. Resolves to an
/// `enemy::RangedProfile` at spawn time.
#[derive(Debug, Clone, Deserialize)]
pub struct RangedProfileToml {
    pub tell: f32,
    pub shot_cooldown: f32,
    /// 1 = single-shot rhythm; >1 = burst-fire with `reload` lockout
    /// after the mag empties.
    #[serde(default = "ranged_default_mag_size")]
    pub mag_size: u16,
    #[serde(default)]
    pub reload: f32,
}

fn ranged_default_mag_size() -> u16 {
    1
}

#[derive(Debug, Clone, Deserialize)]
pub struct BrandScaling {
    #[serde(default)]
    pub _legacy_unused: Option<u32>,
}

pub struct ContentDb {
    pub archetypes: HashMap<Archetype, ArchetypeStats>,
    pub brands: HashMap<String, BrandDef>,
    pub default_brand: String,
    pub substances: SubstanceRegistry,
    /// Stable ids for the core substances so gameplay code doesn't
    /// have to look them up by name every tick.
    pub blood_pool_id: SubstanceId,
    pub scorch_id: SubstanceId,
    /// Fire substance id. Cached so Ignite's paint path + the
    /// standing-hazard pass + fire propagation don't have to lookup
    /// by name every tick.
    pub fire_id: SubstanceId,
    /// Optional ASCII-art sprites keyed by archetype. When present,
    /// sprite.rs uses these instead of the hardcoded builders.
    pub archetype_sprites: HashMap<Archetype, crate::sprite::Sprite>,
    /// Legacy brand-specific sprite overrides: `(brand_id, archetype)
    /// → Sprite`. Authored via each brand's `sprite_overrides` +
    /// `sprite_palettes` TOML sections. Deprecated in favor of per-
    /// unit sprites in `brand_unit_sprites`; still consulted as a
    /// fallback for un-converted brands.
    pub brand_sprites: HashMap<(String, Archetype), crate::sprite::Sprite>,
    /// Per-unit sprite cache: `(brand_id, unit_id) → Sprite`. Built
    /// from each BrandUnit's `sprite` + `palette` at content load.
    /// Consulted first by the sprite lookup path — an enemy carrying
    /// a unit_id gets its unit-specific art regardless of archetype
    /// and independent of how many distinct units a brand hangs off
    /// the same backing archetype.
    pub brand_unit_sprites: HashMap<(String, String), crate::sprite::Sprite>,
}

/// Resolved spawn-time unit definition. Merges archetype defaults
/// with per-unit overrides; the callers (wave director, convert
/// swap, etc.) use this to construct an `Enemy` + annotate it with
/// the unit_id + brand_id that keep sprite/display lookups coherent
/// across the enemy's lifetime (and past death, for the corpse).
#[derive(Debug, Clone)]
pub struct ResolvedUnit {
    pub archetype: Archetype,
    /// `Some(tag)` when this came from a brand `[units.<id>]` entry.
    /// `None` for the legacy archetype-id-in-spawn-pool path.
    pub unit_id: Option<crate::tag::Tag>,
    pub brand_id: Option<crate::tag::Tag>,
    pub stats: ArchetypeStats,
    pub display_name: String,
    pub behavior: Option<crate::behavior::BehaviorToml>,
    pub ranged_profile: Option<RangedProfileToml>,
}

impl std::fmt::Debug for ContentDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContentDb")
            .field("archetypes", &self.archetypes.len())
            .field("brands", &self.brands.len())
            .field("default_brand", &self.default_brand)
            .field("substances", &self.substances.len())
            .finish()
    }
}

impl Clone for ContentDb {
    fn clone(&self) -> Self {
        // ContentDb is cloned rarely — usually we pass &ContentDb. The
        // substance registry isn't Clone-friendly (would require cloning
        // leaked strings); this impl just panics since no current caller
        // actually clones content. If someone starts, we fix it then.
        panic!("ContentDb is not cloneable; pass &ContentDb instead")
    }
}

impl ContentDb {
    pub fn load_core() -> Result<Self> {
        let archetypes_text = read_embedded("archetypes.toml")?;
        let raw: HashMap<String, ArchetypeStats> =
            toml::from_str(&archetypes_text).context("parse archetypes.toml")?;
        let mut archetypes = HashMap::new();
        for (name, stats) in raw {
            let arch = archetype_from_name(&name)
                .with_context(|| format!("unknown archetype `{name}` in archetypes.toml"))?;
            archetypes.insert(arch, stats);
        }
        require_arch(&archetypes, Archetype::Rusher)?;
        require_arch(&archetypes, Archetype::Pinkie)?;
        require_arch(&archetypes, Archetype::Charger)?;
        require_arch(&archetypes, Archetype::Revenant)?;
        require_arch(&archetypes, Archetype::Miniboss)?;

        // Log every loaded archetype name so missing rows are
        // immediately visible in the backtick log console. Each new
        // archetype should show up here the first run after adding
        // it to the TOML.
        let mut loaded_names: Vec<&str> = raw_names(&archetypes);
        loaded_names.sort();
        tracing::info!(
            count = archetypes.len(),
            archetypes = ?loaded_names,
            "archetype stats loaded"
        );

        let mut brands = HashMap::new();
        let brand_dir = CORE_PACK
            .get_dir("brands")
            .context("core pack missing brands/ directory")?;
        for file in brand_dir.files() {
            if file.path().extension().map(|e| e != "toml").unwrap_or(true) {
                continue;
            }
            let text = file.contents_utf8().context("brand not UTF-8")?;
            let brand: BrandDef = toml::from_str(text)
                .with_context(|| format!("parse {}", file.path().display()))?;
            // Validate spawn_pool + miniboss references. Each entry
            // must resolve to EITHER a brand-unit id (a key in
            // `brand.units`) OR a raw archetype snake name.
            // ContentDb::resolve_unit handles both at spawn time;
            // we mirror that precedence here at load so typos fail
            // loud.
            for name in brand.spawn_pool.iter().chain(std::iter::once(&brand.miniboss)) {
                let ok = brand.units.contains_key(name)
                    || Archetype::from_name(name).is_some();
                if !ok {
                    return Err(anyhow!(
                        "brand {} references unknown unit/archetype `{name}`",
                        brand.id
                    ));
                }
            }
            // Validate BrandUnit archetype references so a typo in a
            // brand roster is caught at load, not at spawn.
            for (unit_id, unit) in &brand.units {
                if Archetype::from_name(&unit.archetype).is_none() {
                    return Err(anyhow!(
                        "brand {} unit `{unit_id}` references unknown archetype `{}`",
                        brand.id,
                        unit.archetype
                    ));
                }
            }
            brands.insert(brand.id.clone(), brand);
        }
        if brands.is_empty() {
            return Err(anyhow!("core pack ships no brands"));
        }

        // Substances — data-driven ground decals. Always-present cores
        // (blood_pool, scorch) are required so gameplay can reference
        // them by stable id.
        let mut substances = SubstanceRegistry::new();
        if let Some(subs_dir) = CORE_PACK.get_dir("substances") {
            for file in subs_dir.files() {
                if file.path().extension().map(|e| e != "toml").unwrap_or(true) {
                    continue;
                }
                let text = file.contents_utf8().context("substance not UTF-8")?;
                let toml_def: SubstanceToml = toml::from_str(text)
                    .with_context(|| format!("parse {}", file.path().display()))?;
                substances.insert(toml_def)?;
            }
        }
        let blood_pool_id = substances
            .by_name("blood_pool")
            .ok_or_else(|| anyhow!("substance `blood_pool` required in content/core/substances/"))?;
        let scorch_id = substances
            .by_name("scorch")
            .ok_or_else(|| anyhow!("substance `scorch` required in content/core/substances/"))?;
        // Apply gameplay behaviors (emitters, damage, flammability)
        // now that every substance is loaded. Behaviors keyed by
        // substance name so content packs opt in by naming matching
        // a known key in `apply_default_behaviors`.
        substances.apply_default_behaviors();
        let fire_id = substances
            .by_name("fire")
            .ok_or_else(|| anyhow!("substance `fire` required in content/core/substances/"))?;
        // Prefer `tarkov_scavs` as the opening brand — grounded,
        // legible fire-team flavor matches the design spec's "start
        // recognizable" pillar. Fall back alphabetically if it's
        // missing (e.g., a reduced content pack).
        let default_brand = if brands.contains_key("tarkov_scavs") {
            "tarkov_scavs".to_string()
        } else if brands.contains_key("doom_inferno") {
            "doom_inferno".to_string()
        } else {
            let mut keys: Vec<&String> = brands.keys().collect();
            keys.sort();
            keys.first()
                .cloned()
                .cloned()
                .unwrap_or_else(|| "tarkov_scavs".into())
        };

        // Load any ASCII-art sprites referenced by archetype TOML. If
        // the archetype has both `art` + `palette` we parse the art
        // file into a `Sprite` and cache it; otherwise nothing changes
        // and the legacy hardcoded builder stays in use.
        let mut archetype_sprites = HashMap::new();
        let art_dir = CORE_PACK.get_dir("art");
        for (arch, stats) in &archetypes {
            let (Some(art_name), Some(pal_map)) = (stats.art.as_ref(), stats.palette.as_ref())
            else {
                continue;
            };
            let Some(dir) = art_dir else { continue };
            let Some(file) = dir.get_file(art_name).or_else(|| {
                dir.get_file(format!("art/{art_name}"))
            }) else {
                tracing::warn!(archetype = ?arch, art = %art_name, "art file missing");
                continue;
            };
            let text = file.contents_utf8().context("art not UTF-8")?;
            let palette = crate::art::palette_from_toml(pal_map)
                .with_context(|| format!("palette for {arch:?}"))?;
            let sprite = crate::art::parse_art(text, &palette)
                .with_context(|| format!("art for {arch:?}"))?;
            archetype_sprites.insert(*arch, sprite);
        }

        // Brand-level sprite overrides. Each brand can declare a
        // `[sprite_overrides]` map of archetype → art filename plus
        // `[sprite_palettes.<arch>]` palette tables. When an enemy
        // spawns from that brand and renders, the lookup tries this
        // first. Art files live under `content/core/art/<brand_id>/`
        // by convention, but any filename inside the art/ dir works.
        let mut brand_sprites: HashMap<(String, Archetype), crate::sprite::Sprite> =
            HashMap::new();
        for (brand_id, brand) in &brands {
            if brand.sprite_overrides.is_empty() {
                continue;
            }
            let Some(dir) = art_dir else { continue };
            for (arch_name, art_filename) in &brand.sprite_overrides {
                let Some(arch) = Archetype::from_name(arch_name) else {
                    tracing::warn!(
                        brand = %brand_id,
                        archetype = %arch_name,
                        "sprite_overrides references unknown archetype"
                    );
                    continue;
                };
                // `include_dir`'s `get_entry` recurses but matches
                // against each entry's FULL embed-root-relative path,
                // so lookups must include the `art/` prefix even
                // though we already have an `art_dir` handle — the
                // handle is just a walk-starting-point, not a path
                // rebase. Try the brand subdirectory first, then a
                // loose filename at `art/<file>` as a fallback.
                let sub = format!("art/{brand_id}/{art_filename}");
                let file = dir
                    .get_file(&sub)
                    .or_else(|| dir.get_file(format!("art/{art_filename}")));
                let Some(file) = file else {
                    tracing::warn!(
                        brand = %brand_id,
                        archetype = %arch_name,
                        art = %art_filename,
                        "sprite_overrides art file missing"
                    );
                    continue;
                };
                let Some(pal_map) = brand.sprite_palettes.get(arch_name) else {
                    tracing::warn!(
                        brand = %brand_id,
                        archetype = %arch_name,
                        "sprite_overrides missing matching sprite_palettes entry"
                    );
                    continue;
                };
                let text = match file.contents_utf8() {
                    Some(t) => t,
                    None => {
                        tracing::warn!(
                            brand = %brand_id,
                            archetype = %arch_name,
                            "sprite_overrides art file not UTF-8"
                        );
                        continue;
                    }
                };
                let palette = match crate::art::palette_from_toml(pal_map) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(
                            brand = %brand_id,
                            archetype = %arch_name,
                            %e,
                            "sprite_palettes parse failed"
                        );
                        continue;
                    }
                };
                let sprite = match crate::art::parse_art(text, &palette) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(
                            brand = %brand_id,
                            archetype = %arch_name,
                            %e,
                            "sprite_overrides art parse failed"
                        );
                        continue;
                    }
                };
                brand_sprites.insert((brand_id.clone(), arch), sprite);
            }
        }

        // BrandUnit sprite loading. Each unit's optional `sprite` +
        // `palette` fields produce a per-`(brand_id, unit_id)` sprite
        // entry that's consulted first in the render path. Missing
        // art / palette / parse failures are warn-only — the unit
        // still works, just falls through to the archetype default
        // at render time.
        let mut brand_unit_sprites: HashMap<(String, String), crate::sprite::Sprite> =
            HashMap::new();
        for (brand_id, brand) in &brands {
            let Some(dir) = art_dir else { continue };
            for (unit_id, unit) in &brand.units {
                let (Some(art_filename), Some(pal_map)) =
                    (unit.sprite.as_ref(), unit.palette.as_ref())
                else {
                    continue;
                };
                let sub = format!("art/{brand_id}/{art_filename}");
                let Some(file) = dir
                    .get_file(&sub)
                    .or_else(|| dir.get_file(format!("art/{art_filename}")))
                else {
                    tracing::warn!(
                        brand = %brand_id,
                        unit = %unit_id,
                        art = %art_filename,
                        "BrandUnit art file missing"
                    );
                    continue;
                };
                let text = match file.contents_utf8() {
                    Some(t) => t,
                    None => {
                        tracing::warn!(
                            brand = %brand_id,
                            unit = %unit_id,
                            "BrandUnit art file not UTF-8"
                        );
                        continue;
                    }
                };
                let palette = match crate::art::palette_from_toml(pal_map) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(
                            brand = %brand_id, unit = %unit_id, %e,
                            "BrandUnit palette parse failed"
                        );
                        continue;
                    }
                };
                let sprite = match crate::art::parse_art(text, &palette) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(
                            brand = %brand_id, unit = %unit_id, %e,
                            "BrandUnit art parse failed"
                        );
                        continue;
                    }
                };
                brand_unit_sprites.insert((brand_id.clone(), unit_id.clone()), sprite);
            }
        }

        Ok(Self {
            archetypes,
            brands,
            default_brand,
            substances,
            blood_pool_id,
            scorch_id,
            fire_id,
            archetype_sprites,
            brand_sprites,
            brand_unit_sprites,
        })
    }

    /// Resolve a `spawn_pool` id from a given brand into a
    /// `ResolvedUnit` carrying the merged stats + behavior + sprite
    /// hooks needed to spawn an Enemy. The id is tried first as a
    /// brand-unit id (a key in `brand.units`); if that misses, fall
    /// back to treating the id as a raw archetype snake name
    /// (the pre-BrandUnit content authoring shape).
    ///
    /// Returns None when the id names neither a brand unit nor a
    /// known archetype.
    pub fn resolve_unit(&self, brand_id: &str, id: &str) -> Option<ResolvedUnit> {
        let brand_tag = crate::tag::Tag::new(brand_id);
        // Path 1 — id is a brand-unit id.
        if let Some(brand) = self.brands.get(brand_id) {
            if let Some(unit) = brand.units.get(id) {
                let archetype = Archetype::from_name(&unit.archetype)?;
                let base = self.stats(archetype).clone();
                let stats = ArchetypeStats {
                    hp: unit.hp.unwrap_or(base.hp),
                    speed: unit.speed.unwrap_or(base.speed),
                    touch_damage: unit.touch_damage.unwrap_or(base.touch_damage),
                    hit_radius: unit.hit_radius.unwrap_or(base.hit_radius),
                    reach: unit.reach.or(base.reach),
                    preferred_distance: unit.preferred_distance.or(base.preferred_distance),
                    ranged_damage: unit.ranged_damage.or(base.ranged_damage),
                    art: base.art.clone(),
                    palette: base.palette.clone(),
                    tags: unit.tags.clone().or(base.tags.clone()),
                    body_on_death: unit.body_on_death.clone().or(base.body_on_death.clone()),
                    gore_profile: unit.gore_profile.clone().or(base.gore_profile.clone()),
                    signature_primitives: unit
                        .signature_primitives
                        .clone()
                        .or(base.signature_primitives.clone()),
                    signature_fire_mode: unit
                        .signature_fire_mode
                        .clone()
                        .or(base.signature_fire_mode.clone()),
                    behavior: base.behavior.clone(),
                };
                let display_name = unit
                    .display_name
                    .clone()
                    .unwrap_or_else(|| humanize_snake(id));
                return Some(ResolvedUnit {
                    archetype,
                    unit_id: Some(crate::tag::Tag::new(id)),
                    brand_id: Some(brand_tag),
                    stats,
                    display_name,
                    behavior: unit.behavior.clone(),
                    ranged_profile: unit.ranged_profile.clone(),
                });
            }
        }
        // Path 2 — legacy: id is an archetype snake name directly in
        // the spawn_pool (every brand pre-BrandUnit).
        let archetype = Archetype::from_name(id)?;
        let stats = self.stats(archetype).clone();
        let display_name = self.enemy_display_name(archetype, Some(brand_id));
        Some(ResolvedUnit {
            archetype,
            unit_id: None,
            brand_id: Some(brand_tag),
            stats,
            display_name,
            behavior: None,
            ranged_profile: None,
        })
    }

    pub fn stats(&self, archetype: Archetype) -> &ArchetypeStats {
        if let Some(s) = self.archetypes.get(&archetype) {
            return s;
        }
        // Fallback: warn loudly + return Rusher stats so a missing
        // TOML entry for a newly-added archetype doesn't crash the
        // run. Check the log console for the warning to identify
        // which archetype needs its TOML row.
        tracing::error!(
            archetype = ?archetype,
            loaded_count = self.archetypes.len(),
            "stats() requested for archetype not in TOML; falling back to Rusher"
        );
        self.archetypes
            .get(&Archetype::Rusher)
            .expect("rusher stats missing from archetypes.toml — unrecoverable")
    }

    pub fn brand(&self, id: &str) -> Option<&BrandDef> {
        self.brands.get(id)
    }

    pub fn active_brand(&self) -> &BrandDef {
        self.brand(&self.default_brand).expect("default brand validated at load")
    }

    /// Human-readable name for an enemy. When the enemy's brand has
    /// a `sprite_overrides` entry for this archetype, we use the art
    /// filename's stem as the flavor name (dead_space's `leaper =
    /// "slasher.art"` → `"Slasher"`). Otherwise the bare archetype
    /// name ("Charger", "Zergling"). No parenthetical brand suffix —
    /// the flavor name already carries the theme, and when there's
    /// no flavor the archetype is its own answer.
    pub fn enemy_display_name(
        &self,
        archetype: Archetype,
        brand_id: Option<&str>,
    ) -> String {
        self.enemy_display_name_with_unit(archetype, brand_id, None)
    }

    /// BrandUnit-aware display name. Lookup order:
    ///   1. BrandUnit's explicit `display_name` (e.g. "Combine Soldier").
    ///   2. Humanized unit id (e.g. "pistol_scav" → "Pistol Scav").
    ///   3. Legacy `sprite_overrides` filename-stem fallback.
    ///   4. Archetype Debug name.
    /// Used by the kill feed + brand strip so a player sees
    /// "Combine Soldier" or "Pistol Scav" in chat, not "Marksman"
    /// or "Rusher".
    pub fn enemy_display_name_with_unit(
        &self,
        archetype: Archetype,
        brand_id: Option<&str>,
        unit_id: Option<&str>,
    ) -> String {
        if let (Some(bid), Some(uid)) = (brand_id, unit_id) {
            if let Some(brand) = self.brands.get(bid) {
                if let Some(unit) = brand.units.get(uid) {
                    if let Some(name) = unit.display_name.as_deref() {
                        return name.to_string();
                    }
                    return humanize_snake(uid);
                }
            }
        }
        let arch_snake = archetype.to_name();
        if let Some(bid) = brand_id {
            if let Some(brand) = self.brands.get(bid) {
                if let Some(art) = brand.sprite_overrides.get(arch_snake) {
                    let stem = std::path::Path::new(art)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(art);
                    return humanize_snake(stem);
                }
            }
        }
        format!("{archetype:?}")
    }

    /// Resolved signature primitive pool for an archetype — skips any
    /// TOML entries that don't parse as a known primitive id.
    pub fn archetype_signature_primitives(&self, archetype: Archetype) -> Vec<Primitive> {
        let Some(stats) = self.archetypes.get(&archetype) else {
            return Vec::new();
        };
        let Some(names) = stats.signature_primitives.as_ref() else {
            return Vec::new();
        };
        names
            .iter()
            .filter_map(|n| Primitive::from_name(n))
            .collect()
    }

    /// Resolved signature fire-mode for an archetype. `None` means
    /// "random / whatever the roller picks."
    pub fn archetype_fire_mode(&self, archetype: Archetype) -> Option<FireMode> {
        self.archetypes
            .get(&archetype)
            .and_then(|s| s.signature_fire_mode.as_deref())
            .and_then(FireMode::from_name)
    }

    /// Resolved signature traversal verb for a brand. `None` means
    /// the brand does not prescribe a verb; drops should roll freely.
    pub fn brand_traversal(&self, brand_id: &str) -> Option<TraversalVerb> {
        self.brand(brand_id)
            .and_then(|b| b.signature_traversal.as_deref())
            .and_then(TraversalVerb::from_name)
    }

    /// Resolved signature perk pool for a brand. Empty vec means no
    /// brand bias; perk drops roll from the full unowned set.
    pub fn brand_perks(&self, brand_id: &str) -> Vec<Perk> {
        let Some(brand) = self.brand(brand_id) else {
            return Vec::new();
        };
        let Some(names) = brand.signature_perks.as_ref() else {
            return Vec::new();
        };
        names.iter().filter_map(|n| Perk::from_name(n)).collect()
    }
}

/// `some_snake_id` → `"Some Snake Id"`. Used to surface brand IDs
/// (`tarkov_scavs`) and art-file stems (`greater_wisp`) as display
/// names in the kill feed / death report.
fn humanize_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut cap_next = true;
    for ch in s.chars() {
        if ch == '_' || ch == '-' {
            out.push(' ');
            cap_next = true;
        } else if cap_next {
            for c in ch.to_uppercase() {
                out.push(c);
            }
            cap_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn archetype_from_name(name: &str) -> Result<Archetype> {
    Archetype::from_name(name).ok_or_else(|| anyhow!("unknown archetype `{name}`"))
}

fn require_arch(db: &HashMap<Archetype, ArchetypeStats>, a: Archetype) -> Result<()> {
    if !db.contains_key(&a) {
        Err(anyhow!("archetypes.toml missing required variant {a:?}"))
    } else {
        Ok(())
    }
}

/// Short, human-readable labels for loaded archetypes — used in the
/// startup log line so the user can eyeball which TOML sections made
/// it into the runtime map.
fn raw_names(db: &HashMap<Archetype, ArchetypeStats>) -> Vec<&'static str> {
    db.keys()
        .map(|a| match a {
            Archetype::Rusher => "rusher",
            Archetype::Pinkie => "pinkie",
            Archetype::Charger => "charger",
            Archetype::Revenant => "revenant",
            Archetype::Marksman => "marksman",
            Archetype::Pmc => "pmc",
            Archetype::Swarmling => "swarmling",
            Archetype::Orb => "orb",
            Archetype::Miniboss => "miniboss",
            Archetype::Eater => "eater",
            Archetype::Breacher => "breacher",
            Archetype::Rocketeer => "rocketeer",
            Archetype::Leaper => "leaper",
            Archetype::Sapper => "sapper",
            Archetype::Juggernaut => "juggernaut",
            Archetype::Howler => "howler",
            Archetype::Sentinel => "sentinel",
            Archetype::Splitter => "splitter",
            Archetype::Killa => "killa",
            Archetype::Zergling => "zergling",
            Archetype::Floodling => "floodling",
            Archetype::Flood => "flood",
            Archetype::FloodCarrier => "flood_carrier",
            Archetype::PlayerTurret => "player_turret",
            Archetype::Phaser => "phaser",
            Archetype::Headcrab => "headcrab",
            Archetype::Zombie => "zombie",
            Archetype::Rat => "rat",
            Archetype::DireRat => "dire_rat",
        })
        .collect()
}

fn read_embedded(path: &str) -> Result<String> {
    let file = CORE_PACK
        .get_file(path)
        .with_context(|| format!("core pack missing {path}"))?;
    Ok(file
        .contents_utf8()
        .with_context(|| format!("{path} not UTF-8"))?
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BrandUnit TOML round-trips through serde with all the fields
    /// the brand-rewrite pass uses.
    #[test]
    fn brand_unit_toml_parses() {
        let src = r#"
            archetype = "marksman"
            display_name = "Combine Soldier"
            hp = 90
            speed = 13.0
            ranged_damage = 6

            [ranged_profile]
            tell = 0.25
            shot_cooldown = 0.12
            mag_size = 4
            reload = 1.8
        "#;
        let u: BrandUnit = toml::from_str(src).unwrap();
        assert_eq!(u.archetype, "marksman");
        assert_eq!(u.display_name.as_deref(), Some("Combine Soldier"));
        assert_eq!(u.hp, Some(90));
        assert_eq!(u.speed, Some(13.0));
        assert_eq!(u.ranged_damage, Some(6));
        let rp = u.ranged_profile.expect("ranged_profile should parse");
        assert_eq!(rp.tell, 0.25);
        assert_eq!(rp.mag_size, 4);
    }

    /// Resolve Half-Life's combine_soldier — a real brand unit that
    /// ships in core content. Merged stats pull the unit's HP/speed/
    /// damage override, preserve archetype defaults for unset fields,
    /// and carry the ranged_profile for burst-fire dispatch.
    #[test]
    fn resolve_unit_merges_half_life_combine_soldier() {
        let db = ContentDb::load_core().expect("core content loads");
        let r = db
            .resolve_unit("half_life", "combine_soldier")
            .expect("half_life/combine_soldier resolves");
        assert_eq!(r.archetype, Archetype::Marksman);
        assert_eq!(r.stats.hp, 90);
        assert_eq!(r.stats.speed, 13.0);
        assert_eq!(r.stats.ranged_damage, Some(6));
        assert_eq!(r.display_name, "Combine Soldier");
        assert_eq!(r.unit_id.map(|t| t.as_str()), Some("combine_soldier"));
        assert_eq!(r.brand_id.map(|t| t.as_str()), Some("half_life"));
        let rp = r.ranged_profile.expect("combine_soldier has burst profile");
        assert_eq!(rp.mag_size, 4);
    }

    /// Tarkov's scav_sniper + Half-Life's combine_soldier both use
    /// the Marksman archetype but resolve to very different stats +
    /// ranged profiles. This is the showcase assertion: same sim
    /// archetype, two distinctly-authored units with independent
    /// brand identities.
    #[test]
    fn marksman_differs_across_brands() {
        let db = ContentDb::load_core().expect("core content loads");
        let combine = db
            .resolve_unit("half_life", "combine_soldier")
            .expect("combine_soldier");
        let scav = db
            .resolve_unit("tarkov_scavs", "scav_sniper")
            .expect("scav_sniper");
        // Same backing archetype.
        assert_eq!(combine.archetype, scav.archetype);
        assert_eq!(combine.archetype, Archetype::Marksman);
        // Distinctly-tuned stats.
        assert_ne!(combine.stats.hp, scav.stats.hp);
        assert_ne!(combine.stats.speed, scav.stats.speed);
        // Distinctly-tuned fire cadence — combine bursts, scav snipes.
        let c_rp = combine.ranged_profile.as_ref().unwrap();
        let s_rp = scav.ranged_profile.as_ref().unwrap();
        assert!(c_rp.mag_size > 1, "combine_soldier should burst-fire");
        assert_eq!(s_rp.mag_size, 1, "scav_sniper should be single-shot");
        assert!(c_rp.shot_cooldown < s_rp.shot_cooldown);
    }

    /// Legacy fallback: a brand that ships no `[units.*]` tables
    /// still resolves via the archetype-snake-name path. This is the
    /// backward-compat promise for un-converted brands.
    #[test]
    fn resolve_unit_falls_back_to_archetype_name() {
        let db = ContentDb::load_core().expect("core content loads");
        // doom_inferno hasn't been converted to BrandUnit yet —
        // spawn_pool still uses archetype names.
        let r = db
            .resolve_unit("doom_inferno", "rusher")
            .expect("legacy archetype id resolves");
        assert_eq!(r.archetype, Archetype::Rusher);
        assert!(r.unit_id.is_none(), "legacy fallback has no unit_id");
    }

    /// Unknown ids return None — both for unknown unit ids AND
    /// unknown archetype names.
    #[test]
    fn resolve_unit_rejects_unknown_ids() {
        let db = ContentDb::load_core().expect("core content loads");
        assert!(db.resolve_unit("half_life", "not_a_unit").is_none());
        assert!(db.resolve_unit("half_life", "not_an_archetype").is_none());
    }
}
