//! Cooking recipes. JSON-driven (assets/recipes.json) so dish names,
//! ingredient mixes, and stat effects can be rewritten without touching
//! source. The effect string reuses the buffs format from `buffs.rs`
//! (e.g. "price_mult:0.01", "wait_mult:-0.02", "luck:0.05") so cooking
//! plugs into the same persistent-buff plumbing as fish-borne effects.

use serde::Deserialize;
use std::sync::OnceLock;

const RECIPES_JSON: &str = include_str!("../assets/recipes.json");

#[derive(Clone, Debug, Deserialize)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    /// (fish_name, qty)
    pub ingredients: Vec<(String, u32)>,
    /// Flat stamina restored on cook.
    #[serde(default)]
    pub stamina: i32,
    /// Optional persistent buff string (see `buffs::apply_effect`).
    #[serde(default)]
    pub effect: Option<String>,
    /// Cooking level required to unlock this recipe in the cookbook.
    /// Defaults to 1.
    #[serde(default = "default_min_level")]
    pub min_cooking_level: u32,
    /// User-rewritable flavor text.
    #[serde(default)]
    pub description: String,
}

fn default_min_level() -> u32 {
    1
}

static RECIPES: OnceLock<Vec<Recipe>> = OnceLock::new();

pub fn recipes() -> &'static [Recipe] {
    RECIPES.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(RECIPES_JSON)
            .expect("assets/recipes.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("recipe entry malformed"))
            .collect()
    })
}

/// Returns true if `basket` contains at least every (fish, qty) in
/// `recipe.ingredients`. Compares names case-insensitively.
pub fn can_cook(recipe: &Recipe, basket: &[&'static crate::fish::FishDef]) -> bool {
    for (name, qty) in &recipe.ingredients {
        let have = basket
            .iter()
            .filter(|f| f.name.eq_ignore_ascii_case(name))
            .count() as u32;
        if have < *qty {
            return false;
        }
    }
    true
}
