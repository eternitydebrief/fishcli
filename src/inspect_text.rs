//! Inspect-text lookup. All player-visible tile / furniture descriptions
//! live in `assets/inspect.json` so the user can rewrite them without
//! touching source. Keys: "tile:<TileVariant>", "furn:<Furn>".

use std::collections::HashMap;
use std::sync::OnceLock;

const INSPECT_JSON: &str = include_str!("../assets/inspect.json");

static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();

fn map() -> &'static HashMap<String, String> {
    MAP.get_or_init(|| {
        let raw: HashMap<String, serde_json::Value> = serde_json::from_str(INSPECT_JSON)
            .expect("assets/inspect.json failed to parse");
        raw.into_iter()
            .filter_map(|(k, v)| {
                if k.starts_with('_') {
                    return None;
                }
                v.as_str().map(|s| (k, s.to_string()))
            })
            .collect()
    })
}

pub fn get(key: &str) -> &'static str {
    map()
        .get(key)
        .map(|s| s.as_str())
        .unwrap_or("(no description)")
}
