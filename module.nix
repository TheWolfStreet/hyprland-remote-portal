self: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.services.hypr-remote-portal;
in {
  options.services.hypr-remote-portal = {
    enable = lib.mkEnableOption "the Hyprland RemoteDesktop portal";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      description = "The hypr-remote-portal package.";
    };

    trustedApps = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [];
      example = ["org.kde.kdeconnect"];
      description = "App id prefixes allowed to control input. Every other app is denied.";
    };
  };

  config = lib.mkIf cfg.enable {
    xdg.portal = {
      extraPortals = [cfg.package];
      config.common."org.freedesktop.impl.portal.RemoteDesktop" = ["hypr-remote"];
    };
    xdg.configFile."hypr-remote-portal/trusted-apps".text = lib.concatLines cfg.trustedApps;
  };
}
