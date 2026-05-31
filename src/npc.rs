use crate::world::Dimension;
use ratatui::style::Color;
use serde::Deserialize;
use std::sync::OnceLock;

const NPCS_JSON: &str = include_str!("../assets/npcs.json");

#[derive(Clone, Debug, Deserialize)]
pub struct Npc {
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    #[serde(default = "default_glyph")]
    pub glyph: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub dialogue: Vec<String>,
    /// Which plane this NPC lives in. Defaults to the Surface for
    /// back-compat with the original village NPCs.
    #[serde(default)]
    pub dim: Dimension,
}

fn default_glyph() -> String {
    "@".to_string()
}
fn default_color() -> String {
    "white".to_string()
}

impl Npc {
    pub fn render_char(&self) -> char {
        self.glyph.chars().next().unwrap_or('@')
    }

    pub fn render_color(&self) -> Color {
        match self.color.as_str() {
            "red" => Color::Red,
            "green" => Color::Green,
            "yellow" => Color::Yellow,
            "blue" => Color::Blue,
            "magenta" => Color::Magenta,
            "cyan" => Color::Cyan,
            "gray" => Color::Gray,
            "darkgray" => Color::DarkGray,
            "lightgreen" => Color::LightGreen,
            "lightblue" => Color::LightBlue,
            "lightyellow" => Color::LightYellow,
            "lightred" => Color::LightRed,
            "lightcyan" => Color::LightCyan,
            "lightmagenta" => Color::LightMagenta,
            _ => Color::White,
        }
    }
}

static NPC_CACHE: OnceLock<Vec<Npc>> = OnceLock::new();

pub fn npcs() -> &'static [Npc] {
    NPC_CACHE.get_or_init(|| {
        let raw: Vec<serde_json::Value> = serde_json::from_str(NPCS_JSON)
            .expect("assets/npcs.json failed to parse");
        raw.into_iter()
            .filter(|v| v.get("id").and_then(|n| n.as_str()).is_some())
            .map(|v| serde_json::from_value(v).expect("npc entry malformed"))
            .collect()
    })
}

/// Dim-aware lookup. Surface NPCs only show on Surface, mines NPCs only
/// in mines, etc.
pub fn npc_at_dim(x: i32, y: i32, dim: Dimension) -> Option<&'static Npc> {
    npcs().iter().find(|n| n.x == x && n.y == y && n.dim == dim)
}
