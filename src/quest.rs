use serde::Deserialize;
use std::sync::OnceLock;

const QUESTS_JSON: &str = include_str!("../assets/quests.json");

#[derive(Clone, Debug, Deserialize)]
pub struct Objective {
    pub kind: String,
    pub target: String,
    #[serde(default = "one")]
    pub count: u32,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Debug, Deserialize)]
pub struct Reward {
    #[serde(default)]
    pub valu: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct QuestDef {
    pub id: String,
    pub title: String,
    pub description: String,
    pub objective: Objective,
    #[serde(default)]
    pub reward: Reward,
}

impl Default for Reward {
    fn default() -> Self {
        Self { valu: 0 }
    }
}

static QUEST_CACHE: OnceLock<Vec<QuestDef>> = OnceLock::new();

pub fn quests() -> &'static [QuestDef] {
    QUEST_CACHE.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(QUESTS_JSON)
            .expect("assets/quests.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("quest entry malformed"))
            .collect()
    })
}
