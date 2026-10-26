use std::{collections::HashMap, ffi::OsString, path::PathBuf};

use calloop::channel::Sender;
use zbus::{
    ObjectServer, interface,
    zvariant::{ObjectPath, OwnedValue, Value},
};

use crate::input::Command;

const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.hyprland-remote";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const KEYBOARD: u32 = 1;
const POINTER: u32 = 2;
const SUCCESS: u32 = 0;
const DENIED: u32 = 2;

type Results = HashMap<String, OwnedValue>;
type Options<'a> = HashMap<&'a str, Value<'a>>;

pub fn serve(commands: Sender<Command>) -> zbus::Result<zbus::blocking::Connection> {
    zbus::blocking::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(PORTAL_PATH, RemoteDesktop { commands })?
        .build()
}

fn config_dirs(
    config_home: Option<OsString>,
    home: Option<OsString>,
    config_dirs: Option<OsString>,
) -> Vec<PathBuf> {
    let user = config_home
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|home| PathBuf::from(home).join(".config")));
    let system = config_dirs
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| OsString::from("/etc/xdg"));
    user.into_iter()
        .chain(std::env::split_paths(&system).filter(|dir| dir.is_absolute()))
        .collect()
}

fn read_trusted_apps() -> Vec<String> {
    let dirs = config_dirs(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
        std::env::var_os("XDG_CONFIG_DIRS"),
    );
    dirs.iter()
        .find_map(|dir| {
            std::fs::read_to_string(dir.join("hyprland-remote-portal").join("trusted-apps")).ok()
        })
        .map(|text| parse_trusted_apps(&text))
        .unwrap_or_default()
}

fn parse_trusted_apps(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect()
}

fn is_trusted(trusted_apps: &[String], app_id: &str, session_handle: &str) -> bool {
    let token = session_handle.rsplit('/').next().unwrap_or("");
    trusted_apps.iter().any(|name| {
        app_id.starts_with(name.as_str()) || (app_id.is_empty() && token.starts_with(name.as_str()))
    })
}

fn results(devices: Option<u32>) -> Results {
    let mut results = Results::new();
    if let Some(devices) = devices {
        results.insert("devices".to_string(), OwnedValue::from(devices));
    }
    results
}

struct RemoteDesktop {
    commands: Sender<Command>,
}

#[interface(name = "org.freedesktop.impl.portal.RemoteDesktop")]
impl RemoteDesktop {
    async fn create_session(
        &self,
        _handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        _options: Options<'_>,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> (u32, Results) {
        if !is_trusted(&read_trusted_apps(), app_id, session_handle.as_str()) {
            eprintln!("hyprland-remote-portal: denied {app_id:?} for {session_handle}");
            return (DENIED, results(None));
        }
        let session = Session {
            path: session_handle.to_string(),
            commands: self.commands.clone(),
        };
        if let Err(error) = server.at(&session_handle, session).await {
            eprintln!("hyprland-remote-portal: cannot export {session_handle}: {error}");
            return (DENIED, results(None));
        }
        (SUCCESS, results(None))
    }

    async fn select_devices(
        &self,
        _handle: ObjectPath<'_>,
        _session_handle: ObjectPath<'_>,
        _app_id: &str,
        _options: Options<'_>,
    ) -> (u32, Results) {
        (SUCCESS, results(None))
    }

    fn start(
        &self,
        _handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        _app_id: &str,
        _parent_window: &str,
        _options: Options<'_>,
    ) -> (u32, Results) {
        let _ = self.commands.send(Command::Start {
            session: session_handle.to_string(),
        });
        (SUCCESS, results(Some(KEYBOARD | POINTER)))
    }

    fn notify_pointer_motion(
        &self,
        session_handle: ObjectPath<'_>,
        _options: Options<'_>,
        dx: f64,
        dy: f64,
    ) {
        let _ = self.commands.send(Command::Motion {
            session: session_handle.to_string(),
            dx,
            dy,
        });
    }

    fn notify_pointer_button(
        &self,
        session_handle: ObjectPath<'_>,
        _options: Options<'_>,
        button: i32,
        state: u32,
    ) {
        let Ok(button) = u32::try_from(button) else {
            return;
        };
        let _ = self.commands.send(Command::Button {
            session: session_handle.to_string(),
            button,
            pressed: state == 1,
        });
    }

    fn notify_pointer_axis(
        &self,
        session_handle: ObjectPath<'_>,
        _options: Options<'_>,
        dx: f64,
        dy: f64,
    ) {
        let _ = self.commands.send(Command::Axis {
            session: session_handle.to_string(),
            dx,
            dy,
        });
    }

    fn notify_keyboard_keycode(
        &self,
        session_handle: ObjectPath<'_>,
        _options: Options<'_>,
        keycode: i32,
        state: u32,
    ) {
        let Ok(key) = u32::try_from(keycode) else {
            return;
        };
        let _ = self.commands.send(Command::Keycode {
            session: session_handle.to_string(),
            key,
            pressed: state == 1,
        });
    }

    fn notify_keyboard_keysym(
        &self,
        session_handle: ObjectPath<'_>,
        _options: Options<'_>,
        keysym: i32,
        state: u32,
    ) {
        let Ok(keysym) = u32::try_from(keysym) else {
            return;
        };
        let _ = self.commands.send(Command::Keysym {
            session: session_handle.to_string(),
            keysym,
            pressed: state == 1,
        });
    }

    #[zbus(property)]
    fn available_device_types(&self) -> u32 {
        KEYBOARD | POINTER
    }

    // BUG: KDE Connect's libei keyboard sends shifted characters without Shift and
    // drops characters outside the offered keymap. Version 1 has no ConnectToEIS, so
    // it falls back to these Notify methods, whose keysyms carry every character.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

struct Session {
    path: String,
    commands: Sender<Command>,
}

#[interface(name = "org.freedesktop.impl.portal.Session")]
impl Session {
    async fn close(&self, #[zbus(object_server)] server: &ObjectServer) {
        let _ = self.commands.send(Command::Close {
            session: self.path.clone(),
        });
        if let Err(error) = server.remove::<Session, _>(self.path.as_str()).await {
            eprintln!(
                "hyprland-remote-portal: cannot remove {}: {error}",
                self.path
            );
        }
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_apps_skip_blanks_and_comments() {
        let text = "\n# phones\norg.kde.kdeconnect\n  kdeconnect  \n";
        assert_eq!(
            parse_trusted_apps(text),
            ["org.kde.kdeconnect", "kdeconnect"]
        );
    }

    #[test]
    fn user_config_comes_before_system_dirs() {
        let dirs = config_dirs(
            None,
            Some("/home/user".into()),
            Some("/etc/xdg:relative:/run/xdg".into()),
        );
        assert_eq!(
            dirs,
            [
                PathBuf::from("/home/user/.config"),
                PathBuf::from("/etc/xdg"),
                PathBuf::from("/run/xdg"),
            ]
        );
        assert_eq!(
            config_dirs(Some("/cfg".into()), None, None),
            [PathBuf::from("/cfg"), PathBuf::from("/etc/xdg")]
        );
    }

    #[test]
    fn sandboxed_apps_are_trusted_only_by_app_id() {
        let trusted = parse_trusted_apps("org.kde.kdeconnect\nkdeconnect");
        let session = "/org/freedesktop/portal/desktop/session/1_42/kdeconnect123";
        assert!(is_trusted(&trusted, "org.kde.kdeconnect.daemon", session));
        assert!(is_trusted(&trusted, "", session));
        assert!(!is_trusted(&trusted, "com.example.Spy", session));
        assert!(!is_trusted(
            &trusted,
            "",
            "/org/freedesktop/portal/desktop/session/1_42/other"
        ));
        assert!(!is_trusted(&[], "", session));
    }
}
