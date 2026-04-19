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
}

#[derive(Debug, Clone, Deserialize)]
pub struct BrandDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub spawn_pool: Vec<String>,
    pub miniboss: String,
    pub scaling: BrandScaling,
    #[serde(default)]
    pub spawn_weights: HashMap<String, u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BrandScaling {
    pub base_rushers: u32,
    pub rushers_per_wave: u32,
    pub pinkie_start_wave: u32,
    pub pinkie_base: u32,
    pub pinkie_scale_denom: u32,
    pub charger_start_wave: u32,
    pub charger_base: u32,
    pub charger_scale_denom: u32,
    pub revenant_start_wave: u32,
    pub revenant_base: u32,
    pub revenant_scale_denom: u32,
}

#[derive(Debug, Clone)]
pub struct ContentDb {
    pub archetypes: HashMap<Archetype, ArchetypeStats>,
    pub brands: HashMap<String, BrandDef>,
    pub default_brand: String,
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
        let default_brand = brands
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "fps_arena".into());

        Ok(Self { archetypes, brands, default_brand })
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
    match name {
        "rusher" => Ok(Archetype::Rusher),
        "pinkie" => Ok(Archetype::Pinkie),
        "charger" => Ok(Archetype::Charger),
        "revenant" => Ok(Archetype::Revenant),
        "miniboss" => Ok(Archetype::Miniboss),
        other => Err(anyhow!("unknown archetype `{other}`")),
    }
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
