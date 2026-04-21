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
    /// Optional per-archetype sprite overrides. When an enemy spawns
    /// from this brand and renders, the render path checks for a
    /// brand-specific sprite first, then the archetype default, then
    /// the hardcoded builder. Entry format: `archetype_snake_name →
    /// art_filename.art`, with the corresponding palette declared in
    /// `sprite_palettes`. Art files live in
    /// `content/core/art/<brand_id>/`.
    #[serde(default)]
    pub sprite_overrides: HashMap<String, String>,
    /// Palettes for each sprite override — mirrors the archetype-
    /// level `palette` field. Keyed by the same archetype snake
    /// name as `sprite_overrides`. Values are {char → hex} maps.
    #[serde(default)]
    pub sprite_palettes: HashMap<String, HashMap<String, String>>,
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
    /// Brand-specific sprite overrides: `(brand_id, archetype) →
    /// Sprite`. When an enemy spawned from a specific brand renders,
    /// the lookup tries this first; falls back to `archetype_sprites`
    /// and then the hardcoded builder. Authored via each brand's
    /// `sprite_overrides` + `sprite_palettes` TOML sections.
    pub brand_sprites: HashMap<(String, Archetype), crate::sprite::Sprite>,
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
            // Validate archetype references.
            for name in brand.spawn_pool.iter().chain(std::iter::once(&brand.miniboss)) {
                archetype_from_name(name)
                    .with_context(|| format!("brand {} references unknown archetype `{name}`", brand.id))?;
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
