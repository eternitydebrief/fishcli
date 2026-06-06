use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    Fish,
    Fossils,
    Plant,
    Mineral,
    Misc,
}

impl Category {
    pub const fn all() -> &'static [Category] {
        &[
            Category::Fish,
            Category::Fossils,
            Category::Plant,
            Category::Mineral,
            Category::Misc,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Fish => "Fish",
            Category::Fossils => "Fossils",
            Category::Plant => "Plants",
            Category::Mineral => "Minerals",
            Category::Misc => "Misc",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub category: Category,
    pub description: String,
}
