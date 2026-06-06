//! Save persistence with AES-GCM encryption.
//!
//! HONEST CAVEAT: this is not "secure" cryptography in the academic sense.
//! Anyone reading this source can find the SECRET below, derive the same
//! key, and decrypt/re-encrypt saves. There is no way around this for a
//! single-player game where the local binary must be able to write saves
//! (it has to hold the signing key). Asymmetric crypto doesn't help -
//! the game still has to sign locally.
//!
//! What this DOES achieve:
//!   - Save file is opaque base64; you can't open it in vim and bump valu.
//!   - Modifying a single byte fails GCM authentication and the save
//!     refuses to load.
//!   - To cheat, an attacker needs to: read this source, find SECRET,
//!     compute the derived key for their save's name, AES-GCM-decrypt,
//!     edit JSON, re-encrypt with valid auth tag, base64-encode. Doable
//!     but tedious enough to filter out 99% of casual editors.

use crate::buffs::Buffs;
use crate::item::Item;
use crate::rod::OwnedRods;
use crate::stats::{Skills, Stats};
use crate::world::Dimension;
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

// Forward-compat rule (see CLAUDE.md "save-file evolution"):
//   * every new field added here MUST be `#[serde(default)]` (or
//     `#[serde(default = "fn")]` for non-`Default` types) so old saves
//     that predate the field still load.
//   * never rename or remove an existing field without bumping the wire
//     version in `decrypt_opaque`. obsolete fields stay on the struct
//     (prefix with `_`) and just stop being read.
//   * same rule applies recursively to nested types (Stats, Skills,
//     OwnedRods, BaitStock, EquippedTackle, etc.).
#[derive(Default, Serialize, Deserialize)]
pub struct SaveData {
    pub name: String,
    pub player_x: i32,
    pub player_y: i32,
    pub valu: u64,
    pub inventory: Vec<String>,
    /// Fossilized catches, parallel storage to `inventory`. Each entry is
    /// a fish name that resolves to a fossil-tagged FishDef on load.
    #[serde(default)]
    pub fossils: Vec<String>,
    pub caught: Vec<bool>,
    pub world_seed: u32,
    pub rng_state: u32,
    pub play_time_secs: u64,
    pub lifetime_valu_earned: u64,
    #[serde(default)]
    pub quest_progress: Vec<(String, u32)>,
    #[serde(default)]
    pub quest_done: Vec<String>,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub pinned_quest: Option<String>,
    /// Legacy single-dimension seen-cells (used for save backcompat — old
    /// saves only ever explored the Surface). New saves use `seen_by_dim`.
    #[serde(default)]
    pub seen_cells: Vec<(i32, i32)>,
    /// Per-dimension fog of war. Tuples of (dim, x, y) so serde can store
    /// the dim enum inline without needing a HashMap codec.
    #[serde(default)]
    pub seen_by_dim: Vec<(Dimension, i32, i32)>,
    #[serde(default)]
    pub stats: Stats,
    #[serde(default)]
    pub skills: Skills,
    #[serde(default)]
    pub rods: OwnedRods,
    /// First-catch location per fish index, parallel to `caught`.
    /// `None` = never caught (or caught before this field existed).
    /// Tuple is (biome label, water type).
    #[serde(default)]
    pub caught_at: Vec<Option<(String, String)>>,
    #[serde(default)]
    pub caught_context: Vec<Option<(String, String, String)>>,
    #[serde(default)]
    pub buffs: Buffs,
    #[serde(default)]
    pub skill_tree: crate::skill_tree::SkillTree,
    #[serde(default)]
    pub has_boat: bool,
    #[serde(default)]
    pub has_pickaxe: bool,
    #[serde(default)]
    pub dim: Dimension,
    /// Vein cooldown snapshot: (dim, x, y, charges_used, ready_at_unix_secs).
    #[serde(default)]
    pub veins: Vec<(Dimension, i32, i32, u8, u64)>,
    /// Per-species mastery (catches), parallel to `caught`.
    #[serde(default)]
    pub mastery: Vec<u32>,
    #[serde(default)]
    pub mastery_milestones: u32,
    #[serde(default)]
    pub achievements: crate::achievements::AchievementProgress,
    #[serde(default)]
    pub visited_mines: bool,
    #[serde(default)]
    pub visited_atlantis: bool,
    #[serde(default)]
    pub visited_inferno: bool,
    #[serde(default)]
    pub tackle: crate::tackle::EquippedTackle,
    #[serde(default)]
    pub bait: crate::bait::BaitStock,
    #[serde(default)]
    pub daily_day_id: String,
    #[serde(default)]
    pub daily_progress: u32,
    #[serde(default)]
    pub daily_completed: bool,
    #[serde(default)]
    pub daily_bonus_points: u32,
    #[serde(default)]
    pub challenge_progress: std::collections::BTreeMap<String, u32>,
    #[serde(default)]
    pub challenge_done: Vec<String>,
    #[serde(default)]
    pub challenge_bonus_points: u32,
    #[serde(default)]
    pub streak_species: Option<String>,
    #[serde(default)]
    pub streak_count: u32,
    #[serde(default)]
    pub mining_boost_until: u64,
    #[serde(default = "default_stamina")]
    pub stamina: f32,
    #[serde(default)]
    pub settings: crate::app::Settings,
    #[serde(default)]
    pub bounty: Option<crate::procedural_quests::ProceduralQuest>,
    #[serde(default)]
    pub tutorial_step: u32,
    #[serde(default)]
    pub gear: crate::gear::EquippedGear,
    #[serde(default)]
    pub ingots: std::collections::BTreeMap<String, u32>,
    /// Last in-game month-id (`year * MONTHS_PER_YEAR + month`) the cape
    /// paid out. Drives the "1st of every month" cape stipend.
    #[serde(default)]
    pub last_cape_payout_month: u64,
    /// Per-day counters for merchant caps. Reset whenever
    /// `last_market_day` changes.
    #[serde(default)]
    pub fish_sold_today: u32,
    #[serde(default)]
    pub ore_sold_today: u32,
    #[serde(default)]
    pub last_market_day: u64,
    #[serde(default)]
    pub hull_tier: u32,
    #[serde(default)]
    pub crew_hunger: u32,
    #[serde(default)]
    pub biofuel: u32,
    #[serde(default)]
    pub wood: u32,
    #[serde(default)]
    pub cooking_mastery: Vec<u32>,
    #[serde(default)]
    pub fishdex_milestones_granted: u32,
    #[serde(default)]
    pub cookbook_milestones_granted: u32,
    /// Chopped-tree map: anchor (x, y) -> respawn unix-secs. Persisted so
    /// a clearing the player just cut stays cleared across a reload.
    #[serde(default)]
    pub chopped_trees: Vec<(i32, i32, u64)>,
    /// Per-bug-species mastery (catches), parallel to `bugs::defs()`.
    /// APPEND-ONLY: never reorder `assets/bugs.json`.
    #[serde(default)]
    pub bugs_caught: Vec<u32>,
    /// True once the Bug Net has been given to the player. Granted by NPC
    /// quest reward (wired in a later commit).
    #[serde(default)]
    pub has_bug_net: bool,
    /// Cells whose bug was already caught today. Discarded on load if the
    /// stored `bugs_picked_day_id` is older than the current game day.
    #[serde(default)]
    pub bugs_picked: Vec<(crate::world::Dimension, i32, i32)>,
    #[serde(default)]
    pub bugs_picked_day_id: u64,
    /// Cells where a soil patch was dug today. Shares the day rollover with
    /// `bugs_picked`.
    #[serde(default)]
    pub soil_dug: Vec<(crate::world::Dimension, i32, i32)>,
    /// Cells whose forageable object (rock, tree, cactus, flower, pebble)
    /// has been searched today. Shares the day rollover.
    /// Deprecated as of the 30-minute forage cooldown — kept for save
    /// backward-compat but no longer written or read by the engine.
    #[serde(default)]
    pub foraged: Vec<(crate::world::Dimension, i32, i32)>,
    /// Wall-clock cooldown per foraged cell: (dim, x, y, ready_at_unix_secs).
    /// 30 minutes per harvest. Survives day rollover.
    #[serde(default)]
    pub foraged_cooldowns: Vec<(crate::world::Dimension, i32, i32, u64)>,
    /// Scales: a token currency dropped at low odds per catch. Spendable
    /// on small permanent stat bumps via the `:scales` menu, cap of 1000
    /// tokens per axis.
    #[serde(default)]
    pub scales: u64,
    /// Tokens spent per stat axis. Caps at 1000 per key. Each unit grants
    /// +0.05% on the named axis (rare_chance / catch_speed / valu_mult /
    /// xp_mult / bite_speed).
    #[serde(default)]
    pub scales_spent: std::collections::BTreeMap<String, u32>,
    /// Times the player has prestiged. Each prestige resets the skill
    /// tree allocations and grants a permanent +5% global xp_mult.
    #[serde(default)]
    pub prestige_count: u32,
    /// Landmark cape IDs unlocked so far. Checked against current snapshot
    /// every ~1s; missing ones whose criteria now hold fire and grant
    /// their reward.
    #[serde(default)]
    pub landmarks_unlocked: Vec<String>,
    /// Per-species shiny catch counts, parallel to `caught`. Append-only;
    /// zero-extended on load.
    #[serde(default)]
    pub shiny_per_species: Vec<u32>,
    /// True once the Shiny Charm has been auto-granted (at 1000 lifetime
    /// shinies). Persists so reloading doesn't re-trigger the milestone.
    #[serde(default)]
    pub has_shiny_charm: bool,
}

fn default_stamina() -> f32 {
    100.0
}

/// Saves live under the platform data dir (`$XDG_DATA_HOME/fishcli/saves`
/// on Linux, e.g. `~/.local/share/fishcli/saves`) so the binary is FHS-
/// installable and runnable from any cwd. Each save is written to its own
/// ISO 8601 timestamped file. Retention is intentionally tight (no
/// savescumming):
///   * primary keeps the latest save + 3 backups = 4 files max.
///   * mirrored `redundancy/` keeps the 3 most recent files as a
///     belt-and-suspenders second copy.
/// Old files beyond those windows are pruned on every write. If
/// `dirs::data_dir()` is unresolvable (no HOME), we fall back to a
/// cwd-relative `./saves/` so dev iteration from inside the repo still
/// works.
const SAVE_DIR: &str = "saves";
const REDUNDANCY_DIR: &str = "redundancy";
const KEEP_PRIMARY: usize = 4;
const KEEP_REDUNDANCY: usize = 3;

fn save_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("fishcli").join(SAVE_DIR))
        .unwrap_or_else(|| PathBuf::from(SAVE_DIR))
}

fn redundancy_dir() -> PathBuf {
    save_dir().join(REDUNDANCY_DIR)
}

fn make_timestamped_path() -> PathBuf {
    // ISO 8601 with colons swapped for hyphens (windows filesystems hate :)
    let now = chrono::Utc::now();
    let stamp = now.format("%Y-%m-%dT%H-%M-%S-%3f");
    save_dir().join(format!("save-{stamp}.dat"))
}

fn list_dat_files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("dat"))
        .collect();
    files.sort();
    files
}

fn list_saves() -> Vec<PathBuf> {
    list_dat_files(&save_dir())
}

fn prune_dir(dir: &PathBuf, keep: usize) {
    let files = list_dat_files(dir);
    if files.len() <= keep {
        return;
    }
    for p in &files[..files.len() - keep] {
        let _ = std::fs::remove_file(p);
    }
}

/// Mirror the most recent save into the redundancy directory and prune the
/// mirror to KEEP_REDUNDANCY files. The mirror is a straight byte-copy so
/// it carries the same opaque-encrypted payload as the primary.
fn mirror_redundancy(latest: &PathBuf) {
    let dir = redundancy_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Some(name) = latest.file_name() {
        let dst = dir.join(name);
        // skip if the file already exists (same timestamp = same byte content)
        if !dst.exists() {
            let _ = std::fs::copy(latest, &dst);
        }
    }
    prune_dir(&dir, KEEP_REDUNDANCY);
}

fn legacy_save_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(dir) = dirs::data_dir() {
        let base = dir.join("fishcli");
        v.push(base.join("save.dat"));
        v.push(base.join("save.json"));
    }
    // pre-XDG saves directory in the project cwd. Includes both the
    // pre-timestamping single-file form and any timestamped dev saves,
    // so launching the installed binary from the repo (or anywhere with
    // a stale ./saves/) auto-imports them once into XDG.
    let cwd_dir = PathBuf::from(SAVE_DIR);
    v.push(cwd_dir.join("save.dat"));
    v.extend(list_dat_files(&cwd_dir));
    v
}

/// Baked-in secret. Yes, anyone reading the source has it. See module docs.
const SECRET: &[u8] = b"fishcli-v1.salt//7c2e9d8a-do-not-edit-saves-please";

fn derive_key(name: &str) -> Key<Aes256Gcm> {
    let mut hasher = Sha256::new();
    hasher.update(SECRET);
    hasher.update(b":");
    hasher.update(name.as_bytes());
    let hash = hasher.finalize();
    *Key::<Aes256Gcm>::from_slice(&hash)
}

// Custom opaque binary format. No JSON, no labels, no recognizable
// fields. An XOR mask derived from SECRET is applied over the whole
// file, so even the structure isn't visible without knowing the secret.
//
// On-wire layout (XORed):
//   [0..4]   magic = MAGIC (after XOR)
//   [4..5]   version (1)
//   [5..6]   name length (u8)
//   [6..6+n] name bytes (utf-8)
//   [..+12]  AES-GCM nonce
//   [..]     AES-GCM ciphertext + 16-byte auth tag

const MAGIC: [u8; 4] = [0xF1, 0x5C, 0xCC, 0x11];

fn xor_mask(secret_extra: u8, len: usize) -> Vec<u8> {
    // stream of bytes from SHA256(SECRET || counter || secret_extra)
    let mut out = Vec::with_capacity(len);
    let mut counter: u32 = 0;
    while out.len() < len {
        let mut h = Sha256::new();
        h.update(SECRET);
        h.update(&counter.to_le_bytes());
        h.update([secret_extra]);
        let block = h.finalize();
        for &b in block.iter() {
            if out.len() >= len {
                break;
            }
            out.push(b);
        }
        counter = counter.wrapping_add(1);
    }
    out
}

fn xor_in_place(buf: &mut [u8], mask: &[u8]) {
    for (b, m) in buf.iter_mut().zip(mask.iter().cycle()) {
        *b ^= m;
    }
}

pub fn save_to_disk(data: &SaveData) -> Result<()> {
    std::fs::create_dir_all(save_dir())?;
    let path = make_timestamped_path();
    let json = serde_json::to_vec(data)?;
    let key = derive_key(&data.name);
    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher
        .encrypt(&nonce, json.as_ref())
        .map_err(|e| anyhow!("encrypt failed: {e}"))?;

    let name_bytes = data.name.as_bytes();
    if name_bytes.len() > 255 {
        return Err(anyhow!("name too long"));
    }
    let mut buf = Vec::with_capacity(4 + 1 + 1 + name_bytes.len() + 12 + ct.len());
    buf.extend_from_slice(&MAGIC);
    buf.push(1); // version
    buf.push(name_bytes.len() as u8);
    buf.extend_from_slice(name_bytes);
    buf.extend_from_slice(nonce.as_slice());
    buf.extend_from_slice(&ct);

    let mask = xor_mask(0x7E, buf.len());
    xor_in_place(&mut buf, &mask);

    std::fs::write(&path, buf)?;
    prune_dir(&save_dir(), KEEP_PRIMARY);
    mirror_redundancy(&path);
    Ok(())
}

/// Returns a list of (filename, byte size) for every save file on disk,
/// primaries followed by redundancy entries. Names are short for log display.
pub fn list_saves_meta() -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for p in list_saves() {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        out.push((name, bytes));
    }
    for p in list_dat_files(&redundancy_dir()) {
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        out.push((format!("redundancy/{name}"), bytes));
    }
    out
}

pub fn load_from_disk() -> Option<SaveData> {
    // primary saves: newest first; fall back to redundancy if every primary
    // is corrupt or missing.
    let mut candidates = list_saves();
    candidates.reverse();
    let mut backup = list_dat_files(&redundancy_dir());
    backup.reverse();
    candidates.extend(backup);
    for p in &candidates {
        if let Ok(bytes) = std::fs::read(p) {
            if let Some(d) = decrypt_opaque(&bytes) {
                return Some(d);
            }
        }
    }
    for lp in legacy_save_paths() {
        if let Ok(bytes) = std::fs::read(&lp) {
            if let Some(d) = decrypt_opaque(&bytes) {
                let _ = save_to_disk(&d);
                return Some(d);
            }
            if let Ok(json) = std::str::from_utf8(&bytes) {
                if let Ok(d) = serde_json::from_str::<SaveData>(json) {
                    let _ = save_to_disk(&d);
                    return Some(d);
                }
            }
        }
    }
    None
}

fn decrypt_opaque(bytes: &[u8]) -> Option<SaveData> {
    if bytes.len() < 4 + 1 + 1 + 12 + 16 {
        return None;
    }
    let mut buf = bytes.to_vec();
    let mask = xor_mask(0x7E, buf.len());
    xor_in_place(&mut buf, &mask);

    if buf[0..4] != MAGIC {
        return None;
    }
    if buf[4] != 1 {
        return None;
    }
    let name_len = buf[5] as usize;
    if buf.len() < 6 + name_len + 12 + 16 {
        return None;
    }
    let name = std::str::from_utf8(&buf[6..6 + name_len]).ok()?.to_string();
    let nonce_start = 6 + name_len;
    let nonce_end = nonce_start + 12;
    let nonce = Nonce::from_slice(&buf[nonce_start..nonce_end]);
    let ct = &buf[nonce_end..];

    let key = derive_key(&name);
    let cipher = Aes256Gcm::new(&key);
    let pt = cipher.decrypt(nonce, ct).ok()?;
    serde_json::from_slice(&pt).ok()
}
