//! Input for the UI scenarios (`tests/ui`): a virtual keyboard and a
//! virtual pointer, through the wlroots protocols Sway and Hyprland
//! implement, that live as long as this process. A compositor treats a
//! new input device as a reason to re-evaluate focus (Sway resets
//! keyboard focus when the seat gets its first keyboard), so one
//! persistent pair beats one `wtype` per keystroke. Also plain windows
//! (xdg-shell toplevels, a solid colour), so scenarios about the
//! compositor's state need no real application.
//!
//! Driven by lines on stdin, each answered on stdout with `ok` (after a
//! roundtrip, so the compositor has seen it) or `err <why>`:
//!
//!   layout W H      the global space `move` refers to (default 3840x1080)
//!   move X Y        pointer to global X,Y
//!   click [button]  press and release (left, right, middle; left by default)
//!   scroll N        N wheel clicks down (negative: up)
//!   key NAME        press and release an xkb keysym name: Down, Return,
//!                   Escape, BackSpace, Tab, space, a, ...
//!   type TEXT       each character, as a Unicode keysym
//!   window APP_ID [TITLE]   open a window (the compositor decides where)
//!   title APP_ID TITLE      retitle the last window opened with that app id
//!   close APP_ID    close the last window opened with that app id
//!
//! The keymap is ours, regenerated whenever a new symbol shows up, as
//! `wtype` does: every keysym gets its own keycode, so nothing depends
//! on the layout the compositor would otherwise use.

use std::collections::HashMap;
use std::io::{self, BufRead, Seek, Write};
use std::os::fd::AsFd;
use std::time::Instant;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_shm, wl_shm_pool,
    wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
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
delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);
delegate_noop!(State: ignore xdg_toplevel::XdgToplevel);

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for State {
    fn event(
        _: &mut Self,
        base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            base.pong(serial);
        }
    }
}

/// A window is a surface with a buffer attached after every configure
/// (the compositor won't map it before the first one is acknowledged).
impl Dispatch<xdg_surface::XdgSurface, Window> for State {
    fn event(
        _: &mut Self,
        xdg: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        window: &Window,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg.ack_configure(serial);
            window.surface.attach(Some(&window.buffer), 0, 0);
            window.surface.commit();
        }
    }
}

const WINDOW_SIZE: i32 = 64;

/// One of our windows; the compositor sizes it as it likes, we keep
/// drawing the same small buffer.
#[derive(Clone)]
struct Window {
    surface: wl_surface::WlSurface,
    buffer: wl_buffer::WlBuffer,
}

/// The globals and windows of `window`/`title`/`close`, bound on first use.
struct Windows {
    compositor: wl_compositor::WlCompositor,
    shm: wl_shm::WlShm,
    wm_base: xdg_wm_base::XdgWmBase,
    /// Open windows by app id, in opening order.
    open: Vec<(String, xdg_toplevel::XdgToplevel, xdg_surface::XdgSurface, Window)>,
}

struct Injector {
    queue: EventQueue<State>,
    keyboard: ZwpVirtualKeyboardV1,
    pointer: ZwlrVirtualPointerV1,
    started: Instant,
    /// Keysym name -> its index; the keycode is `9 + index` in the
    /// keymap, `1 + index` on the wire (evdev, without the 8 offset).
    symbols: HashMap<String, u32>,
    layout: (u32, u32),
    globals: wayland_client::globals::GlobalList,
    windows: Option<Windows>,
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
            globals,
            windows: None,
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

    fn windows(&mut self) -> Result<&mut Windows, String> {
        if self.windows.is_none() {
            let qh = self.queue.handle();
            let compositor = self
                .globals
                .bind(&qh, 1..=4, ())
                .map_err(|e| format!("wl_compositor: {e}"))?;
            let shm = self
                .globals
                .bind(&qh, 1..=1, ())
                .map_err(|e| format!("wl_shm: {e}"))?;
            let wm_base = self
                .globals
                .bind(&qh, 1..=2, ())
                .map_err(|e| format!("xdg_wm_base: {e}"))?;
            self.windows = Some(Windows {
                compositor,
                shm,
                wm_base,
                open: Vec::new(),
            });
        }
        Ok(self.windows.as_mut().unwrap())
    }

    fn open_window(&mut self, app_id: &str, title: &str) -> Result<(), String> {
        let qh = self.queue.handle();
        let windows = self.windows()?;
        let stride = WINDOW_SIZE * 4;
        let size = stride * WINDOW_SIZE;
        let mut file = tempfile().map_err(|e| format!("buffer file: {e}"))?;
        // A solid, unmistakable colour (xrgb, little endian: b g r x).
        let pixels: Vec<u8> = [0x40u8, 0x80, 0xc0, 0]
            .iter()
            .cycle()
            .take(size as usize)
            .copied()
            .collect();
        file.write_all(&pixels)
            .and_then(|_| file.rewind())
            .map_err(|e| format!("buffer file: {e}"))?;
        let pool = windows.shm.create_pool(file.as_fd(), size, &qh, ());
        let buffer = pool.create_buffer(
            0,
            WINDOW_SIZE,
            WINDOW_SIZE,
            stride,
            wl_shm::Format::Xrgb8888,
            &qh,
            (),
        );
        pool.destroy();
        let surface = windows.compositor.create_surface(&qh, ());
        let window = Window { surface, buffer };
        let xdg = windows
            .wm_base
            .get_xdg_surface(&window.surface, &qh, window.clone());
        let toplevel = xdg.get_toplevel(&qh, ());
        toplevel.set_app_id(app_id.to_owned());
        toplevel.set_title(title.to_owned());
        window.surface.commit();
        windows
            .open
            .push((app_id.to_owned(), toplevel, xdg, window));
        // The configure comes with the roundtrip; the buffer goes on
        // in its handler, then a second roundtrip lets the map be seen.
        self.roundtrip()?;
        self.roundtrip()
    }

    fn last_window(&mut self, app_id: &str) -> Result<usize, String> {
        self.windows()?
            .open
            .iter()
            .rposition(|(id, ..)| id == app_id)
            .ok_or_else(|| format!("no window with app id {app_id:?}"))
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
            "scroll" => {
                let clicks: i32 = rest
                    .parse()
                    .map_err(|_| format!("scroll needs a number, got {rest:?}"))?;
                self.pointer.axis_source(wl_pointer::AxisSource::Wheel);
                // A wheel click is 15 units on the continuous axis.
                self.pointer.axis_discrete(
                    self.now(),
                    wl_pointer::Axis::VerticalScroll,
                    f64::from(clicks) * 15.0,
                    clicks,
                );
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
            "window" => {
                let (app_id, title) = rest.split_once(' ').unwrap_or((rest, rest));
                if app_id.is_empty() {
                    return Err("window needs an app id".to_owned());
                }
                return self.open_window(app_id, title);
            }
            "title" => {
                let Some((app_id, title)) = rest.split_once(' ') else {
                    return Err("title needs an app id and a title".to_owned());
                };
                let i = self.last_window(app_id)?;
                let windows = self.windows()?;
                let (_, toplevel, _, window) = &windows.open[i];
                toplevel.set_title(title.to_owned());
                window.surface.commit();
            }
            "close" => {
                let i = self.last_window(rest)?;
                let windows = self.windows()?;
                let (_, toplevel, xdg, window) = windows.open.remove(i);
                toplevel.destroy();
                xdg.destroy();
                window.surface.destroy();
                window.buffer.destroy();
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
