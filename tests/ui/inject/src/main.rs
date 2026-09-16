//! Input for the UI scenarios (`tests/ui`): a virtual keyboard and a
//! virtual pointer, through the wlroots protocols Sway and Hyprland
//! implement, that live as long as this process. A compositor treats a
//! new input device as a reason to re-evaluate focus (Sway resets
//! keyboard focus when the seat gets its first keyboard), so one
//! persistent pair beats one `wtype` per keystroke.
//!
//! Driven by lines on stdin, each answered on stdout with `ok` (after a
//! roundtrip, so the compositor has seen it) or `err <why>`:
//!
//!   layout W H      the global space `move` refers to (default 3840x1080)
//!   move X Y        pointer to global X,Y
//!   click [button]  press and release (left, right, middle; left by default)
//!   key NAME        press and release an xkb keysym name: Down, Return,
//!                   Escape, BackSpace, Tab, space, a, ...
//!   type TEXT       each character, as a Unicode keysym
//!
//! The keymap is ours, regenerated whenever a new symbol shows up, as
//! `wtype` does: every keysym gets its own keycode, so nothing depends
//! on the layout the compositor would otherwise use.

use std::collections::HashMap;
use std::io::{self, BufRead, Seek, Write};
use std::os::fd::AsFd;
use std::time::Instant;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_keyboard, wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ZwpVirtualKeyboardV1);
delegate_noop!(State: ZwlrVirtualPointerManagerV1);
delegate_noop!(State: ZwlrVirtualPointerV1);

struct Injector {
    queue: EventQueue<State>,
    keyboard: ZwpVirtualKeyboardV1,
    pointer: ZwlrVirtualPointerV1,
    started: Instant,
    /// Keysym name -> its index; the keycode is `9 + index` in the
    /// keymap, `1 + index` on the wire (evdev, without the 8 offset).
    symbols: HashMap<String, u32>,
    layout: (u32, u32),
}

impl Injector {
    fn connect() -> Result<Self, String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("no compositor: {e}"))?;
        let (globals, queue) =
            registry_queue_init::<State>(&conn).map_err(|e| format!("registry: {e}"))?;
        let qh = queue.handle();
        let seat: wl_seat::WlSeat = globals
            .bind(&qh, 1..=7, ())
            .map_err(|e| format!("wl_seat: {e}"))?;
        let keyboards: ZwpVirtualKeyboardManagerV1 = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("virtual keyboard: {e}"))?;
        let pointers: ZwlrVirtualPointerManagerV1 = globals
            .bind(&qh, 1..=2, ())
            .map_err(|e| format!("virtual pointer: {e}"))?;
        let keyboard = keyboards.create_virtual_keyboard(&seat, &qh, ());
        let pointer = pointers.create_virtual_pointer(Some(&seat), &qh, ());
        let mut injector = Self {
            queue,
            keyboard,
            pointer,
            started: Instant::now(),
            symbols: HashMap::new(),
            layout: (3840, 1080),
        };
        // A keyboard without a keymap is one the compositor ignores.
        injector.send_keymap()?;
        injector.roundtrip()?;
        Ok(injector)
    }

    fn roundtrip(&mut self) -> Result<(), String> {
        self.queue
            .roundtrip(&mut State)
            .map(|_| ())
            .map_err(|e| format!("roundtrip: {e}"))
    }

    fn now(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }

    /// One keycode per symbol used so far.
    fn keymap_text(&self) -> String {
        let mut by_index: Vec<(&String, &u32)> = self.symbols.iter().collect();
        by_index.sort_by_key(|(_, i)| **i);
        let mut codes = String::new();
        let mut syms = String::new();
        for (name, i) in by_index {
            codes.push_str(&format!("<K{i}> = {};\n", 9 + i));
            syms.push_str(&format!("key <K{i}> {{ [ {name} ] }};\n"));
        }
        format!(
            "xkb_keymap {{\n\
             xkb_keycodes \"(unnamed)\" {{\nminimum = 8;\nmaximum = {};\n{codes}}};\n\
             xkb_types \"(unnamed)\" {{ include \"complete\" }};\n\
             xkb_compatibility \"(unnamed)\" {{ include \"complete\" }};\n\
             xkb_symbols \"(unnamed)\" {{\n{syms}}};\n\
             }};\n",
            9 + self.symbols.len()
        )
    }

    fn send_keymap(&mut self) -> Result<(), String> {
        let text = self.keymap_text();
        let mut file = tempfile().map_err(|e| format!("keymap file: {e}"))?;
        file.write_all(text.as_bytes())
            .and_then(|_| file.write_all(&[0]))
            .and_then(|_| file.rewind())
            .map_err(|e| format!("keymap file: {e}"))?;
        self.keyboard.keymap(
            wl_keyboard::KeymapFormat::XkbV1 as u32,
            file.as_fd(),
            text.len() as u32 + 1,
        );
        self.keyboard.modifiers(0, 0, 0, 0);
        Ok(())
    }

    /// The wire keycode for a keysym name, teaching the compositor the
    /// symbol first if it's new.
    fn keycode(&mut self, name: &str) -> Result<u32, String> {
        if let Some(i) = self.symbols.get(name) {
            return Ok(1 + i);
        }
        let i = self.symbols.len() as u32;
        self.symbols.insert(name.to_owned(), i);
        self.send_keymap()?;
        Ok(1 + i)
    }

    fn tap(&mut self, name: &str) -> Result<(), String> {
        let code = self.keycode(name)?;
        self.keyboard
            .key(self.now(), code, wl_keyboard::KeyState::Pressed as u32);
        self.keyboard
            .key(self.now(), code, wl_keyboard::KeyState::Released as u32);
        Ok(())
    }

    fn run(&mut self, line: &str) -> Result<(), String> {
        let (verb, rest) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
        match verb {
            "layout" => {
                let (w, h) = parse_pair(rest)?;
                self.layout = (w, h);
            }
            "move" => {
                let (x, y) = parse_pair(rest)?;
                let (w, h) = self.layout;
                self.pointer
                    .motion_absolute(self.now(), x.min(w), y.min(h), w, h);
                self.pointer.frame();
            }
            "click" => {
                let button = match rest {
                    "" | "left" => BTN_LEFT,
                    "right" => BTN_RIGHT,
                    "middle" => BTN_MIDDLE,
                    other => return Err(format!("unknown button {other:?}")),
                };
                self.pointer
                    .button(self.now(), button, wl_pointer::ButtonState::Pressed);
                self.pointer.frame();
                self.pointer
                    .button(self.now(), button, wl_pointer::ButtonState::Released);
                self.pointer.frame();
            }
            "key" => {
                if rest.is_empty() {
                    return Err("key needs a keysym name".to_owned());
                }
                self.tap(rest)?;
            }
            "type" => {
                for c in rest.chars() {
                    self.tap(&format!("U{:04X}", c as u32))?;
                }
            }
            "" => return Err("empty command".to_owned()),
            other => return Err(format!("unknown command {other:?}")),
        }
        self.roundtrip()
    }
}

fn parse_pair(text: &str) -> Result<(u32, u32), String> {
    let mut words = text.split_whitespace();
    let mut next = || {
        words
            .next()
            .and_then(|w| w.parse::<u32>().ok())
            .ok_or_else(|| format!("expected two numbers, got {text:?}"))
    };
    Ok((next()?, next()?))
}

/// An unlinked file in the runtime dir, to pass as the keymap fd.
fn tempfile() -> io::Result<std::fs::File> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_else(|| "/tmp".into());
    let path = std::path::PathBuf::from(dir).join(format!("aria-inject-{}", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)?;
    std::fs::remove_file(&path)?;
    Ok(file)
}

fn main() {
    let mut injector = match Injector::connect() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("aria-inject: {e}");
            std::process::exit(1);
        }
    };
    let stdout = io::stdout();
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let reply = match injector.run(&line) {
            Ok(()) => "ok".to_owned(),
            Err(e) => format!("err {e}"),
        };
        let mut out = stdout.lock();
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
}
