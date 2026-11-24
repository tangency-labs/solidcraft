# Home Manager module: `programs.solidcraft.enable = true;` installs the latest stable release.
# ../flake.nix exports it as homeManagerModules.default. The packages come from this flake, built
# with its pinned nixpkgs, not from the nixpkgs of the configuration that imports the module.
self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.solidcraft;
  system = pkgs.stdenv.hostPlatform.system;
  packages = self.packages.${system} or { };
  attr = {
    stable = "solidcraft-stable";
    prerelease = "solidcraft-prerelease";
    main = "solidcraft";
  };
in
{
  options.programs.solidcraft = {
    enable = lib.mkEnableOption "SolidCraft, a parametric 3D CAD app";
    channel = lib.mkOption {
      type = lib.types.enum [
        "stable"
        "prerelease"
        "main"
      ];
      default = "stable";
      description = ''
        Which SolidCraft to install. `stable` is the newest release marked Latest; `prerelease`
        is the newest published release of any kind (a nightly or release candidate when one is
        newer); `main` is the commit this flake input is locked to.
      '';
    };
    package = lib.mkOption {
      type = lib.types.package;
      default = packages.${attr.${cfg.channel}};
      defaultText = lib.literalExpression ''inputs.solidcraft.packages.''${system}.solidcraft-stable'';
      description = "The SolidCraft package to install; overrides `channel`.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = packages ? ${attr.${cfg.channel}};
        message = "programs.solidcraft: the SolidCraft flake has no package for ${system}.";
      }
    ];
    home.packages = [ cfg.package ];
  };
}
