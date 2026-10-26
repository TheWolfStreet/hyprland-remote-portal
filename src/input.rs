use std::{
    collections::HashMap,
    os::{
        fd::{AsFd, OwnedFd},
        unix::fs::FileExt,
    },
    time::Instant,
};

use calloop::{EventLoop, channel::Channel};
use calloop_wayland_source::WaylandSource;
use wayland_client::{
    Connection as WaylandConnection, Dispatch, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_keyboard::{self, KeyState, WlKeyboard},
        wl_pointer::{Axis, ButtonState},
        wl_registry::WlRegistry,
        wl_seat::WlSeat,
    },
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};
use xkbcommon::xkb;

pub enum Command {
    Start {
        session: String,
    },
    Close {
        session: String,
    },
    Motion {
        session: String,
        dx: f64,
        dy: f64,
    },
    Button {
        session: String,
        button: u32,
        pressed: bool,
    },
    Axis {
        session: String,
        dx: f64,
        dy: f64,
    },
    Keycode {
        session: String,
        key: u32,
        pressed: bool,
    },
    Keysym {
        session: String,
        keysym: u32,
        pressed: bool,
    },
}

const SINGLE_KEYCODE: u32 = 9;

struct Keymap {
    fd: OwnedFd,
    size: u32,
}

impl Keymap {
    fn upload(&self, device: &ZwpVirtualKeyboardV1) {
        device.keymap(
            wl_keyboard::KeymapFormat::XkbV1.into(),
            self.fd.as_fd(),
            self.size,
        );
    }
}

struct VirtualKeyboard {
    device: ZwpVirtualKeyboardV1,
    xkb: xkb::State,
    keymap: Keymap,
}

impl VirtualKeyboard {
    fn send_modifiers(&self) {
        self.device.modifiers(
            self.xkb.serialize_mods(xkb::STATE_MODS_DEPRESSED),
            self.xkb.serialize_mods(xkb::STATE_MODS_LATCHED),
            self.xkb.serialize_mods(xkb::STATE_MODS_LOCKED),
            self.xkb.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE),
        );
    }
}

#[derive(Default)]
struct Held {
    keys: Vec<u32>,
    buttons: Vec<u32>,
}

struct State {
    queue: QueueHandle<State>,
    keyboard_manager: ZwpVirtualKeyboardManagerV1,
    seat: WlSeat,
    pointer: ZwlrVirtualPointerV1,
    keyboard: Option<VirtualKeyboard>,
    sessions: HashMap<String, Held>,
    started: Instant,
}

impl State {
    fn time(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Start { session } => {
                self.sessions.entry(session).or_default();
            }
            Command::Close { session } => {
                if let Some(held) = self.sessions.remove(&session) {
                    self.release(held);
                }
            }
            Command::Motion { session, dx, dy } => {
                if self.sessions.contains_key(&session) {
                    self.pointer.motion(self.time(), dx, dy);
                    self.pointer.frame();
                }
            }
            Command::Axis { session, dx, dy } => {
                if self.sessions.contains_key(&session) {
                    self.scroll(dx, dy);
                }
            }
            Command::Button {
                session,
                button,
                pressed,
            } => self.button(&session, button, pressed),
            Command::Keycode {
                session,
                key,
                pressed,
            } => self.key(&session, key, pressed),
            Command::Keysym {
                session,
                keysym,
                pressed,
            } => {
                if pressed && self.sessions.contains_key(&session) {
                    self.type_keysym(xkb::Keysym::new(keysym));
                }
            }
        }
    }

    fn send_key(&mut self, key: u32, pressed: bool) {
        let time = self.time();
        let Some(keyboard) = &mut self.keyboard else {
            return;
        };
        let (state, direction) = if pressed {
            (KeyState::Pressed, xkb::KeyDirection::Down)
        } else {
            (KeyState::Released, xkb::KeyDirection::Up)
        };
        keyboard.device.key(time, key, state.into());
        keyboard.xkb.update_key((key + 8).into(), direction);
        keyboard.send_modifiers();
    }

    fn type_keysym(&mut self, keysym: xkb::Keysym) {
        let time = self.time();
        let Some(keyboard) = &self.keyboard else {
            return;
        };
        let layout = keyboard.xkb.serialize_layout(xkb::STATE_LAYOUT_EFFECTIVE);
        let (key, uploaded) = match find_key(&keyboard.xkb.get_keymap(), layout, keysym) {
            Some((key, mask)) => {
                let held = keyboard.xkb.serialize_mods(xkb::STATE_MODS_DEPRESSED);
                keyboard.device.modifiers(held | mask, 0, 0, layout);
                (key, false)
            }
            None => {
                let single = match keymap_file(keymap_text(&[keysym], SINGLE_KEYCODE)) {
                    Ok(single) => single,
                    Err(error) => {
                        eprintln!("hypr-remote-portal: cannot type {keysym:?}: {error}");
                        return;
                    }
                };
                single.upload(&keyboard.device);
                keyboard.device.modifiers(0, 0, 0, 0);
                (1, true)
            }
        };
        keyboard.device.key(time, key, KeyState::Pressed.into());
        keyboard.device.key(time, key, KeyState::Released.into());
        if uploaded {
            keyboard.keymap.upload(&keyboard.device);
        }
        keyboard.send_modifiers();
    }

    fn key(&mut self, session: &str, key: u32, pressed: bool) {
        if self.keyboard.is_none() {
            return;
        }
        let Some(held) = self.sessions.get_mut(session) else {
            return;
        };
        held.keys.retain(|held_key| *held_key != key);
        if pressed {
            held.keys.push(key);
        }
        self.send_key(key, pressed);
    }

    fn button(&mut self, session: &str, button: u32, pressed: bool) {
        let time = self.time();
        let Some(held) = self.sessions.get_mut(session) else {
            return;
        };
        held.buttons.retain(|held_button| *held_button != button);
        if pressed {
            held.buttons.push(button);
        }
        let state = if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        };
        self.pointer.button(time, button, state);
        self.pointer.frame();
    }

    fn scroll(&self, dx: f64, dy: f64) {
        let time = self.time();
        if dx != 0.0 {
            self.pointer.axis(time, Axis::HorizontalScroll, dx);
        }
        if dy != 0.0 {
            self.pointer.axis(time, Axis::VerticalScroll, dy);
        }
        self.pointer.frame();
    }

    fn release(&mut self, held: Held) {
        for key in held.keys {
            self.send_key(key, false);
        }
        let time = self.time();
        for button in held.buttons {
            self.pointer.button(time, button, ButtonState::Released);
        }
        self.pointer.frame();
    }

    fn set_keymap(&mut self, fd: OwnedFd, size: u32) {
        let xkb = match xkb_state(&fd, size) {
            Ok(xkb) => xkb,
            Err(error) => {
                eprintln!("hypr-remote-portal: cannot load the seat keymap: {error}");
                return;
            }
        };
        let keymap = xkb.get_keymap();
        if keymap.max_keycode().raw() <= SINGLE_KEYCODE {
            return;
        }
        if let Some(keyboard) = &self.keyboard
            && keyboard
                .xkb
                .get_keymap()
                .get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1)
                == keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1)
        {
            return;
        }
        let device = match self.keyboard.take() {
            Some(keyboard) => keyboard.device,
            None => self
                .keyboard_manager
                .create_virtual_keyboard(&self.seat, &self.queue, ()),
        };
        let keymap = Keymap { fd, size };
        keymap.upload(&device);
        self.keyboard = Some(VirtualKeyboard {
            device,
            xkb,
            keymap,
        });
    }
}

fn keymap_text(keysyms: &[xkb::Keysym], first_code: u32) -> String {
    let mut codes = String::new();
    let mut symbols = String::new();
    for (index, keysym) in keysyms.iter().enumerate() {
        let code = first_code + index as u32;
        codes.push_str(&format!("    <K{index}> = {code};\n"));
        let name = xkb::keysym_get_name(*keysym);
        symbols.push_str(&format!("    key <K{index}> {{ [ {name} ] }};\n"));
    }
    let maximum = first_code + keysyms.len() as u32;
    format!(
        "xkb_keymap {{\n\
         xkb_keycodes \"hypr-remote\" {{\n    minimum = 8;\n    maximum = {maximum};\n{codes}}};\n\
         xkb_types \"hypr-remote\" {{ include \"complete\" }};\n\
         xkb_compatibility \"hypr-remote\" {{ include \"complete\" }};\n\
         xkb_symbols \"hypr-remote\" {{\n{symbols}}};\n\
         }};\n"
    )
}

fn keymap_file(text: String) -> Result<Keymap, String> {
    let mut bytes = text.into_bytes();
    bytes.push(0);
    let fd = rustix::fs::memfd_create("hypr-remote-keymap", rustix::fs::MemfdFlags::CLOEXEC)
        .map_err(|error| format!("cannot create a keymap file: {error}"))?;
    let file = std::fs::File::from(fd);
    file.write_all_at(&bytes, 0)
        .map_err(|error| format!("cannot write a keymap file: {error}"))?;
    Ok(Keymap {
        fd: file.into(),
        size: bytes.len() as u32,
    })
}

fn find_key(keymap: &xkb::Keymap, layout: u32, keysym: xkb::Keysym) -> Option<(u32, u32)> {
    if keysym.raw() == 0 {
        return None;
    }
    let first = keymap.min_keycode().raw();
    let last = keymap.max_keycode().raw();
    for level in 0..4 {
        for raw in first..=last {
            let keycode = xkb::Keycode::new(raw);
            if level >= keymap.num_levels_for_key(keycode, layout) {
                continue;
            }
            if !keymap
                .key_get_syms_by_level(keycode, layout, level)
                .contains(&keysym)
            {
                continue;
            }
            let mut masks = [0; 4];
            let found = keymap.key_get_mods_for_level(keycode, layout, level, &mut masks);
            let mask = if found > 0 { masks[0] } else { 0 };
            return Some((raw - 8, mask));
        }
    }
    None
}

fn xkb_state(fd: &OwnedFd, size: u32) -> Result<xkb::State, String> {
    let file = std::fs::File::from(fd.try_clone().map_err(|error| error.to_string())?);
    let mut bytes = vec![0; size as usize];
    file.read_exact_at(&mut bytes, 0)
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8(bytes)
        .map_err(|error| error.to_string())?
        .trim_end_matches('\0')
        .to_string();
    let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap = xkb::Keymap::new_from_string(
        &context,
        text,
        xkb::KEYMAP_FORMAT_TEXT_V1,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .ok_or("the keymap does not compile")?;
    Ok(xkb::State::new(&keymap))
}

pub fn run(commands: Channel<Command>) -> Result<(), String> {
    let wayland = WaylandConnection::connect_to_env()
        .map_err(|error| format!("cannot reach Wayland: {error}"))?;
    let (globals, queue) = registry_queue_init::<State>(&wayland)
        .map_err(|error| format!("cannot list Wayland globals: {error}"))?;
    let queue_handle = queue.handle();
    let seat: WlSeat = globals
        .bind(&queue_handle, 1..=7, ())
        .map_err(|error| format!("no wl_seat: {error}"))?;
    let pointer_manager: ZwlrVirtualPointerManagerV1 = globals
        .bind(&queue_handle, 1..=2, ())
        .map_err(|error| format!("no virtual pointer support: {error}"))?;
    let keyboard_manager: ZwpVirtualKeyboardManagerV1 = globals
        .bind(&queue_handle, 1..=1, ())
        .map_err(|error| format!("no virtual keyboard support: {error}"))?;
    seat.get_keyboard(&queue_handle, ());
    let pointer = pointer_manager.create_virtual_pointer(Some(&seat), &queue_handle, ());

    let mut event_loop: EventLoop<State> =
        EventLoop::try_new().map_err(|error| format!("cannot create event loop: {error}"))?;
    let handle = event_loop.handle();
    WaylandSource::new(wayland, queue)
        .insert(handle.clone())
        .map_err(|error| format!("cannot watch Wayland: {error}"))?;
    handle
        .insert_source(commands, |event, (), state: &mut State| {
            if let calloop::channel::Event::Msg(command) = event {
                state.apply(command);
            }
        })
        .map_err(|error| format!("cannot watch portal commands: {error}"))?;

    let mut state = State {
        queue: queue_handle,
        keyboard_manager,
        seat,
        pointer,
        keyboard: None,
        sessions: HashMap::new(),
        started: Instant::now(),
    };
    event_loop
        .run(None, &mut state, |_| {})
        .map_err(|error| format!("event loop stopped: {error}"))
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &WaylandConnection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &WaylandConnection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Keymap { format, fd, size } = event
            && format == wayland_client::WEnum::Value(wl_keyboard::KeymapFormat::XkbV1)
        {
            state.set_keymap(fd, size);
        }
    }
}

delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ignore ZwlrVirtualPointerManagerV1);
delegate_noop!(State: ignore ZwlrVirtualPointerV1);
delegate_noop!(State: ignore ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ignore ZwpVirtualKeyboardV1);

#[cfg(test)]
mod tests {
    use super::*;

    fn us_keymap() -> xkb::Keymap {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        xkb::Keymap::new_from_names(
            &context,
            "evdev",
            "pc105",
            "us",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .expect("the us keymap compiles")
    }

    #[test]
    fn a_single_character_keymap_puts_it_on_the_first_key() {
        let keysym = xkb::utf32_to_keysym('あ'.into());
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_string(
            &context,
            keymap_text(&[keysym], SINGLE_KEYCODE),
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .expect("the single keymap compiles");
        assert_eq!(find_key(&keymap, 0, keysym), Some((1, 0)));
        assert!(keymap.max_keycode().raw() <= SINGLE_KEYCODE);
        assert!(us_keymap().max_keycode().raw() > SINGLE_KEYCODE);
    }

    #[test]
    fn characters_resolve_to_their_key_and_shift_level() {
        let keymap = us_keymap();
        let shift = 1 << keymap.mod_get_index(xkb::MOD_NAME_SHIFT);
        let find = |character: char| find_key(&keymap, 0, xkb::utf32_to_keysym(character.into()));
        assert_eq!(find('a'), Some((30, 0)));
        assert_eq!(find('A'), Some((30, shift)));
        assert_eq!(find('!'), Some((2, shift)));
        assert_eq!(find('ж'), None);
    }
}
