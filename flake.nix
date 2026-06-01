{
  description = "fishcli — terminal fishing game";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (sys: f nixpkgs.legacyPackages.${sys});
    in {
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            cargo rustc rustfmt clippy rust-analyzer
            mold       # linker — replaces ld for ~5x faster link step
            sccache    # caches rustc artifacts across builds + branches
            clang      # cleaner driver for invoking mold via -fuse-ld=mold
          ];

          shellHook = ''
            # Cache compiled artifacts globally (~/.cache/sccache) so swapping
            # branches or `cargo clean` barely costs anything.
            export RUSTC_WRAPPER="${pkgs.sccache}/bin/sccache"
            export SCCACHE_DIR="$HOME/.cache/sccache"
          '';
        };
      });
    };
}
