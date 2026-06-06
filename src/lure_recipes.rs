#![allow(dead_code)]
//! Lure-crafting recipes used by the Bait Bench scene. Recipes consume
//! existing bait IDs (worms, bugs, processed-fish chunks) plus valu and
//! produce a higher-tier bait that would otherwise have to be shop-bought.

use serde::Deserialize;
use std::sync::OnceLock;

const RECIPES_JSON: &str = include_str!("../assets/lure_recipes.json");

#[derive(Clone, Debug, Deserialize)]
pub struct LureInput {
    pub bait_id: String,
    pub count: u32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LureRecipe {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub valu_cost: u64,
    pub inputs: Vec<LureInput>,
    pub output_bait_id: String,
    #[serde(default = "default_one")]
    pub output_count: u32,
}

fn default_one() -> u32 {
    1
}

static CACHE: OnceLock<Vec<LureRecipe>> = OnceLock::new();

pub fn recipes() -> &'static [LureRecipe] {
    CACHE.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(RECIPES_JSON)
            .expect("assets/lure_recipes.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("lure recipe malformed"))
            .collect()
    })
}
