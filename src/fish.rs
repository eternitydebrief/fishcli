use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct FishDef {
    pub name: String,
    pub description: String,
    pub rarity: f32,
    pub difficulty: u8,
}

impl Default for FishDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            rarity: 0.0,
            difficulty: 1,
        }
    }
}

impl FishDef {
    fn t(&self) -> f32 {
        ((self.difficulty as f32 - 1.0) / 9.0).clamp(0.0, 1.0)
    }

    pub fn rect_h(&self) -> f32 {
        7.0 - self.t() * 4.0
    }

    pub fn fish_speed(&self) -> f32 {
        0.25 + self.t() * 0.55
    }

    pub fn target_change_ticks(&self) -> u32 {
        50 - (self.t() * 30.0) as u32
    }
}

pub fn pick_fish<'a>(rng: &mut u32, fish: &'a [FishDef]) -> &'a FishDef {
    let total: f32 = fish.iter().map(|f| f.rarity).sum();
    if total <= 0.0 || fish.is_empty() {
        // shouldn't happen but degrade gracefully
        return &fish[0];
    }
    let r = next_rand_f32(rng) * total;
    let mut acc = 0.0;
    for f in fish {
        acc += f.rarity;
        if r <= acc {
            return f;
        }
    }
    &fish[fish.len() - 1]
}

pub fn next_rand_f32(s: &mut u32) -> f32 {
    let mut x = *s;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *s = x;
    (x as f32) / (u32::MAX as f32)
}
