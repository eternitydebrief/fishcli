# fishcli — instructions for claude

standing instructions the user has given for this project. read before working.

## workflow
- progressive commits only. no monolithic dumps. one small, functional change per commit.
- both remotes (codeberg `origin`, github `github`) must stay in sync. ask before pushing if a remote is missing.
- never run `git config` in this repo. git identity is managed declaratively in `~/nix-config`.

## game design
- stardew-style fishing minigame: vertical bar, player-controlled rectangle, fish moves inside, keep fish in rectangle to fill catch meter. rectangle shrinks and fish gets faster with difficulty.
- pseudo-graphics in the dwarf-fortress vein.
- overworld is a tile map with proper houses built from `#` characters (walls). characters (player, npcs) are single `@` glyphs.
- progression: rod-shop (hundreds of rod upgrades) + fishing-school (technique upgrades, e.g. permanently +1 to rectangle height, faster bites, etc.).

## stack
- rust, ratatui + crossterm.
- declarative dev shell via `flake.nix`.

## save-file evolution (forward-compat)
this is a continuously-updating game. **older saves must keep loading after new versions add fields.** the convention is non-negotiable; players should never need to restart on an update.

- every new field on `SaveData` (or on any struct nested inside it — `Stats`, `Skills`, `OwnedRods`, `BaitStock`, `EquippedTackle`, etc.) gets `#[serde(default)]`. for non-`Default` types use `#[serde(default = "fn_name")]` with an explicit default fn.
- never rename or remove an existing field without a wire-version bump. rename = old saves silently lose data; removal = same. if a field becomes obsolete, leave it on the struct (mark unused with a leading underscore) and stop reading from it.
- when adding a new dim/biome/item/quest, the `caught`/`mastery`/etc. parallel vectors are length-checked and zero-extended on load — don't reorder existing entries in `assets/*.json`, only append.
- breaking wire-layout changes (new header field, new crypto envelope) bump the version byte at `save.rs:246`-ish and add a v1→v2 conversion in `decrypt_opaque`. the loader already rejects unknown versions cleanly, so a partial rollout doesn't corrupt files.
- there's a reminder comment block above the `SaveData` struct in `src/save.rs` — keep it in sync if the rule evolves.

## building
- the user iterates from this CLI, so build speed matters.
- **always run `nix develop` first** if your shell doesn't already have `cargo`/`rustc` on PATH. the flake's dev shell provides cargo, rustc, rustfmt, clippy, rust-analyzer, mold, sccache, and clang. sccache (the rustc wrapper) is auto-configured by the flake's `shellHook` — it caches rustc artifacts to `~/.cache/sccache` so swapping branches or `cargo clean` barely costs anything.
- default `cargo build --release` is ~3s incrementally (LTO off, codegen-units=16, incremental on).
- **never use `--profile ship`** unless the user explicitly asks to make a distributable binary — that one re-enables full LTO and takes ~2min.
- `cargo check` for "does it compile" sanity is ~1s and even cheaper.
- `cargo build` (debug) is also fast and runs the game fine for testing UI.
- the linker block in `.cargo/config.toml` is commented out by default (needs `mold` + `clang` on PATH — which the flake's dev shell provides — uncomment only after `nix develop` has been entered).
