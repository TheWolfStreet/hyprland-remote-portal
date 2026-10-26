# hypr-remote-portal

A minimal `xdg-desktop-portal` RemoteDesktop backend for Hyprland. It lets
trusted apps such as KDE Connect move the pointer, click, scroll and type
through the Wayland virtual pointer and virtual keyboard protocols.

- No permission dialog: apps whose id starts with a trusted prefix are allowed,
  every other app is denied.
- Typing works for any character. Characters missing from the active layout
  are sent through a one-key keymap, so every script and emoji arrives intact.
- No screen sharing; pair it with `xdg-desktop-portal-hyprland` for that.

## Home Manager

```nix
{
  inputs.hypr-remote-portal.url = "github:TheWolfStreet/hypr-remote-portal";
}
```

```nix
{inputs, ...}: {
  imports = [inputs.hypr-remote-portal.homeManagerModules.default];

  services.hypr-remote-portal = {
    enable = true;
    trustedApps = ["org.kde.kdeconnect" "kdeconnect"];
  };
}
```

The module routes `org.freedesktop.impl.portal.RemoteDesktop` to this portal.
Trusted prefixes live in `~/.config/hypr-remote-portal/trusted-apps`, one per
line.

## License

MIT
