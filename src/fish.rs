#[derive(Clone, Copy, Debug)]
pub struct FishDef {
    pub name: &'static str,
    pub description: &'static str,
    pub rarity: f32,
    pub difficulty: u8,
}

impl FishDef {
    pub const fn new(
        name: &'static str,
        description: &'static str,
        rarity: f32,
        difficulty: u8,
    ) -> Self {
        Self {
            name,
            description,
            rarity,
            difficulty,
        }
    }

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

pub fn pick_fish(rng: &mut u32) -> &'static FishDef {
    let total: f32 = crate::fishlist::FISH.iter().map(|f| f.rarity).sum();
    let r = next_rand_f32(rng) * total;
    let mut acc = 0.0;
    for f in crate::fishlist::FISH {
        acc += f.rarity;
        if r <= acc {
            return f;
        }
    }
    &crate::fishlist::FISH[crate::fishlist::FISH.len() - 1]
}

pub fn next_rand_f32(s: &mut u32) -> f32 {
    let mut x = *s;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *s = x;
    (x as f32) / (u32::MAX as f32)
}
