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

use crate::item::Item;
use crate::stats::{Skills, Stats};
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
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
    #[serde(default)]
    pub seen_cells: Vec<(i32, i32)>,
    #[serde(default)]
    pub stats: Stats,
    #[serde(default)]
    pub skills: Skills,
}

fn save_path() -> Option<PathBuf> {
    let dir = dirs::data_dir()?.join("fishcli");
    Some(dir.join("save.dat"))
}

fn legacy_save_path() -> Option<PathBuf> {
    let dir = dirs::data_dir()?.join("fishcli");
    Some(dir.join("save.json"))
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

#[derive(Serialize, Deserialize)]
struct Envelope {
    v: u8,
    /// Player name in cleartext so we can derive the key on load.
    name: String,
    nonce: String,
    body: String,
}

pub fn save_to_disk(data: &SaveData) -> Result<()> {
    let path = save_path().ok_or_else(|| anyhow!("could not resolve data dir"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec(data)?;
    let key = derive_key(&data.name);
    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher
        .encrypt(&nonce, json.as_ref())
        .map_err(|e| anyhow!("encrypt failed: {e}"))?;
    let env = Envelope {
        v: 1,
        name: data.name.clone(),
        nonce: B64.encode(nonce.as_slice()),
        body: B64.encode(ct),
    };
    let serialized = serde_json::to_vec_pretty(&env)?;
    std::fs::write(path, serialized)?;
    Ok(())
}

pub fn load_from_disk() -> Option<SaveData> {
    let path = save_path()?;
    if let Ok(bytes) = std::fs::read(&path) {
        if let Some(d) = decrypt_envelope(&bytes) {
            return Some(d);
        }
    }
    // legacy plaintext json - one-time migration for older installs
    let lp = legacy_save_path()?;
    if let Ok(json) = std::fs::read_to_string(&lp) {
        if let Ok(d) = serde_json::from_str::<SaveData>(&json) {
            // re-save in encrypted form, leave the old file alone for the
            // user to delete manually
            let _ = save_to_disk(&d);
            return Some(d);
        }
    }
    None
}

fn decrypt_envelope(bytes: &[u8]) -> Option<SaveData> {
    let env: Envelope = serde_json::from_slice(bytes).ok()?;
    if env.v != 1 {
        return None;
    }
    let key = derive_key(&env.name);
    let cipher = Aes256Gcm::new(&key);
    let nonce_bytes = B64.decode(env.nonce.as_bytes()).ok()?;
    if nonce_bytes.len() != 12 {
        return None;
    }
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = B64.decode(env.body.as_bytes()).ok()?;
    let pt = cipher.decrypt(nonce, ct.as_ref()).ok()?;
    serde_json::from_slice(&pt).ok()
}
