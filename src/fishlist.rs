use crate::fish::FishDef;
use std::sync::OnceLock;

const FISH_JSON: &str = include_str!("../assets/fish.json");

static FISH_CACHE: OnceLock<Vec<FishDef>> = OnceLock::new();

pub fn fish() -> &'static [FishDef] {
    FISH_CACHE.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(FISH_JSON)
            .expect("assets/fish.json failed to parse");
        raw.into_iter()
            .filter(|v| {
                // skip _comment entries that have no name field
                v.get("name").and_then(|n| n.as_str()).is_some()
            })
            .map(|v| serde_json::from_value(v).expect("fish entry malformed"))
            .collect()
    })
}
