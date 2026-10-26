# hyprland-remote-portal

[![NixOS](https://img.shields.io/badge/NixOS-unstable-blue?logo=nixos)](https://nixos.org)
[![Hyprland](https://img.shields.io/badge/Hyprland-Wayland-58e1ff?logo=wayland)](https://hyprland.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A minimal `xdg-desktop-portal` RemoteDesktop backend for Hyprland. It lets trusted apps such as KDE Connect move the pointer, click, scroll, and type through the Wayland virtual pointer and virtual keyboard protocols. Screen sharing stays with `xdg-desktop-portal-hyprland`.

## Included

- `org.freedesktop.impl.portal.RemoteDesktop` with pointer motion, buttons, scrolling, keycodes, and keysyms
- No permission dialog: apps whose id starts with a trusted prefix are allowed, every other app is denied
- Typing for any character: keys in the active layout are pressed with their modifiers, and anything else, such as other scripts or emoji, is typed through a temporary one-key keymap
- Held keys and buttons are released when a session closes
- A Nix package and a NixOS module

## Quick Start

### 1. Add The Input

```nix
{
  inputs.hyprland-remote-portal = {
    url = "github:TheWolfStreet/hyprland-remote-portal";
    inputs.nixpkgs.follows = "nixpkgs";
  };
}
```

### 2. Enable The Module

```nix
{inputs, ...}: {
  imports = [inputs.hyprland-remote-portal.nixosModules.default];

  programs.hyprland.enable = true;

  services.hyprland-remote-portal = {
    enable = true;
    trustedApps = ["org.kde.kdeconnect" "kdeconnect"];
  };
}
```

The module adds the portal to the NixOS `xdg.portal.extraPortals` and writes `/etc/xdg/xdg-desktop-portal/hyprland-portals.conf`, which routes `org.freedesktop.impl.portal.RemoteDesktop` to it and defaults everything else to `hyprland` and `gtk`. Install `xdg-desktop-portal-gtk` as well if you rely on that default.

When Home Manager also configures Hyprland, set `wayland.windowManager.hyprland.portalPackage = null`. Otherwise Home Manager enables its own portal setup, which points `xdg-desktop-portal` at the user profile and hides portals installed by NixOS.

### 3. Rebuild

`nixos-rebuild switch` reloads the session bus, so upgrades take effect immediately. The first install also needs `xdg-desktop-portal` to restart, because it lists its backends only at startup; log in again or run:

```bash
systemctl --user restart xdg-desktop-portal.service
```

The portal then starts on demand when an app asks for remote input.

## Options

| Option | Purpose |
| --- | --- |
| `services.hyprland-remote-portal.enable` | Install the portal and route RemoteDesktop to it |
| `services.hyprland-remote-portal.package` | The portal package; defaults to this flake's build |
| `services.hyprland-remote-portal.trustedApps` | App id prefixes allowed to control input; every other app is denied |

Trusted prefixes are written to `/etc/xdg/hyprland-remote-portal/trusted-apps`, one per line. A user's own `~/.config/hyprland-remote-portal/trusted-apps` takes precedence, following the XDG base directory lookup. The list is read on every new session, so edits apply without a restart. Unsandboxed apps such as KDE Connect report an empty app id; for them the prefix is matched against the session token the app chose, which for KDE Connect starts with `kdeconnect`.

## Checks

```bash
cargo test
cargo clippy --all-targets -- -D warnings
nix build .#default
busctl --user status org.freedesktop.impl.portal.desktop.hyprland-remote
```

Denied requests and typing failures are logged to stderr, which ends up in the user journal.

## Limitations

- Keyboard and pointer only; screen casting and libei (`ConnectToEIS`) are not offered, so clients use the portal's Notify methods
- Pointer motion is relative; absolute positioning is not supported
- KDE Connect 26.08 drops emoji before they reach any portal; its Wayland input needs a small patch to send whole characters

## License

MIT. Provided as-is.
