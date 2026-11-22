# The pinned release channels: one package per entry in ./sources.json, each built from the
# upstream commit its release tag named, with the source and Cargo dependency hashes recorded by
# ./update.sh. The build uses the flake's own nixpkgs, so its compiler is the one the flake pins.
pkgs:
let
  sources = pkgs.lib.importJSON ./sources.json;
  parts = pkgs.lib.splitString "/" sources.upstream;
  build =
    pin:
    import ./package.nix pkgs {
      inherit (pin) version rustVersion cargoHash;
      src = pkgs.fetchFromGitHub {
        owner = builtins.elemAt parts 0;
        repo = builtins.elemAt parts 1;
        inherit (pin) rev hash;
      };
    };
in
{
  stable = build sources.stable;
  prerelease = build sources.prerelease;
}
