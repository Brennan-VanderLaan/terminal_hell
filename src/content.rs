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
use crate::substance::{SubstanceId, SubstanceRegistry, SubstanceToml};

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
    /// Optional ASCII-art sprites keyed by archetype. When present,
    /// sprite.rs uses these instead of the hardcoded builders.
    pub archetype_sprites: HashMap<Archetype, crate::sprite::Sprite>,
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
        // Prefer tactical as the opening brand so every run starts grounded
        // per the design spec. Fall back to whatever's available.
        let default_brand = if brands.contains_key("tactical") {
            "tactical".to_string()
        } else if brands.contains_key("fps_arena") {
            "fps_arena".to_string()
        } else {
            brands.keys().next().cloned().unwrap_or_else(|| "fps_arena".into())
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

        Ok(Self {
            archetypes,
            brands,
            default_brand,
            substances,
            blood_pool_id,
            scorch_id,
            archetype_sprites,
        })
    }

    pub fn stats(&self, archetype: Archetype) -> &ArchetypeStats {
        self.archetypes
            .get(&archetype)
            .expect("stats table validated at load")
    }

    pub fn brand(&self, id: &str) -> Option<&BrandDef> {
        self.brands.get(id)
    }

    pub fn active_brand(&self) -> &BrandDef {
        self.brand(&self.default_brand).expect("default brand validated at load")
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

fn read_embedded(path: &str) -> Result<String> {
    let file = CORE_PACK
        .get_file(path)
        .with_context(|| format!("core pack missing {path}"))?;
    Ok(file
        .contents_utf8()
        .with_context(|| format!("{path} not UTF-8"))?
        .to_string())
}
