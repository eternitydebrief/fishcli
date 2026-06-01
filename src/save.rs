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

#[derive(Default, Serialize, Deserialize)]
pub struct SaveData {
    pub name: String,
    pub player_x: i32,
    pub player_y: i32,
    pub valu: u64,
    pub inventory: Vec<String>,
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
}

fn default_stamina() -> f32 {
    100.0
}

/// Saves live in ./saves/ relative to the current working directory so they
/// can be easily copied/exported. Each save is written to its own ISO 8601
/// timestamped file. Retention is intentionally tight (no savescumming):
///   * primary `./saves/` keeps the latest save + 3 backups = 4 files max.
///   * mirrored `./saves/redundancy/` keeps the 3 most recent files as a
///     belt-and-suspenders second copy.
/// Old files beyond those windows are pruned on every write.
const SAVE_DIR: &str = "saves";
const REDUNDANCY_DIR: &str = "redundancy";
const KEEP_PRIMARY: usize = 4;
const KEEP_REDUNDANCY: usize = 3;

fn save_dir() -> PathBuf {
    PathBuf::from(SAVE_DIR)
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
    // pre-timestamping single-file save in the project dir
    v.push(PathBuf::from(SAVE_DIR).join("save.dat"));
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
