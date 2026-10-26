{
  description = "Minimal xdg-desktop-portal RemoteDesktop backend for Hyprland";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

  outputs = {
    self,
    nixpkgs,
  }: let
    each = nixpkgs.lib.genAttrs ["x86_64-linux" "aarch64-linux"];
  in {
    packages = each (system: {
      default = nixpkgs.legacyPackages.${system}.callPackage ./package.nix {};
    });
    nixosModules.default = import ./module.nix self;
  };
}
