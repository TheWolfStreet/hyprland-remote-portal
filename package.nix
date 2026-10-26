{
  lib,
  rustPlatform,
  libxkbcommon,
  xkeyboard_config,
}:
rustPlatform.buildRustPackage {
  pname = "hyprland-remote-portal";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [./Cargo.toml ./Cargo.lock ./src ./data];
  };

  cargoLock.lockFile = ./Cargo.lock;

  buildInputs = [libxkbcommon];

  env.XKB_CONFIG_ROOT = "${xkeyboard_config}/share/X11/xkb";

  postInstall = ''
    install -Dm644 data/hyprland-remote.portal -t $out/share/xdg-desktop-portal/portals
    install -Dm644 data/org.freedesktop.impl.portal.desktop.hyprland-remote.service \
      -t $out/share/dbus-1/services
    substituteInPlace $out/share/dbus-1/services/org.freedesktop.impl.portal.desktop.hyprland-remote.service \
      --replace-fail @out@ $out
  '';

  meta = {
    description = "Minimal xdg-desktop-portal RemoteDesktop backend for Hyprland";
    license = lib.licenses.mit;
    mainProgram = "hyprland-remote-portal";
    platforms = lib.platforms.linux;
  };
}
