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
