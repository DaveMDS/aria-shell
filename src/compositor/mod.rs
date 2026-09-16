//! The compositor's view of the desktop: workspaces and windows, which
//! one is active, and the commands to change that.
//!
//! One [`Compositor`] lives in the daemon. Its [`Compositor::subscription`]
//! is the single IPC connection; the [`Event`]s it yields go through
//! [`Compositor::apply`], and every gadget reads the resulting state by
//! reference from its view context. Gadgets never talk to the IPC socket:
//! they emit a [`Command`], the daemon runs it with [`Compositor::run`].
//!
//! Backends are detected from the environment, Hyprland only for now.

mod hyprland;

use iced::{Subscription, Task};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// Connector name of the output showing it, e.g. `HDMI-A-1`.
    pub output: String,
    /// The one currently shown on its output (one per output).
    pub active: bool,
    pub urgent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub id: String,
    /// The app id (X11 "class"): `firefox`, `kitty`, ...
    pub class: String,
    pub title: String,
    pub workspace_id: String,
    pub active: bool,
    pub urgent: bool,
}

/// What a backend reports. Full-list events replace the current list
/// (keeping the active/urgent flags), the others patch it.
#[derive(Debug, Clone)]
pub enum Event {
    Workspaces(Vec<Workspace>),
    Windows(Vec<Window>),
    /// This workspace is now the one shown on its output.
    ActiveWorkspace(String),
    ActiveWindow(Option<String>),
    UrgentWindow(String),
    /// Keyboard focus moved to this output (connector name).
    FocusedOutput(String),
}

#[derive(Debug, Clone)]
pub enum Command {
    ActivateWorkspace(String),
    ActivateWindow(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Hyprland,
}

#[derive(Debug, Default)]
pub struct Compositor {
    backend: Option<Backend>,
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
    /// Connector name of the output with keyboard focus, where things
    /// like the launcher should appear.
    pub focused_output: Option<String>,
}

impl Compositor {
    pub fn detect() -> Self {
        let backend = if hyprland::available() {
            Some(Backend::Hyprland)
        } else {
            None
        };
        match backend {
            Some(b) => log::info!("compositor backend: {b:?}"),
            None => log::warn!("no supported compositor found, workspaces won't work"),
        }
        Self {
            backend,
            ..Self::default()
        }
    }

    pub fn subscription(&self) -> Subscription<Event> {
        match self.backend {
            Some(Backend::Hyprland) => Subscription::run(hyprland::events),
            None => Subscription::none(),
        }
    }

    pub fn run(&self, command: Command) -> Task<Event> {
        match self.backend {
            Some(Backend::Hyprland) => Task::future(hyprland::run(command)).discard(),
            None => Task::none(),
        }
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Workspaces(mut list) => {
                for ws in &mut list {
                    if let Some(old) = self.workspaces.iter().find(|w| w.id == ws.id) {
                        ws.active = old.active;
                        ws.urgent = old.urgent;
                    }
                }
                self.workspaces = list;
            }
            Event::Windows(mut list) => {
                for win in &mut list {
                    if let Some(old) = self.windows.iter().find(|w| w.id == win.id) {
                        win.active = old.active;
                        win.urgent = old.urgent;
                    }
                }
                self.windows = list;
                self.sync_urgent_workspaces();
            }
            Event::ActiveWorkspace(id) => {
                let Some(output) = self
                    .workspaces
                    .iter()
                    .find(|ws| ws.id == id)
                    .map(|ws| ws.output.clone())
                else {
                    return;
                };
                for ws in &mut self.workspaces {
                    if ws.output == output {
                        ws.active = ws.id == id;
                    }
                }
            }
            Event::ActiveWindow(id) => {
                for win in &mut self.windows {
                    win.active = id.as_deref() == Some(&win.id);
                    if win.active {
                        win.urgent = false;
                    }
                }
                self.sync_urgent_workspaces();
            }
            Event::UrgentWindow(id) => {
                if let Some(win) = self.windows.iter_mut().find(|w| w.id == id) {
                    win.urgent = true;
                }
                self.sync_urgent_workspaces();
            }
            Event::FocusedOutput(name) => self.focused_output = Some(name),
        }
    }

    /// A workspace is urgent while any of its windows is.
    fn sync_urgent_workspaces(&mut self) {
        for ws in &mut self.workspaces {
            ws.urgent = self
                .windows
                .iter()
                .any(|w| w.urgent && w.workspace_id == ws.id);
        }
    }

    /// Windows on the given workspace, in the backend's order.
    pub fn windows_of<'a>(&'a self, workspace: &'a Workspace) -> impl Iterator<Item = &'a Window> {
        self.windows
            .iter()
            .filter(move |w| w.workspace_id == workspace.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: &str) -> Workspace {
        ws_on(id, "DP-1")
    }

    fn ws_on(id: &str, output: &str) -> Workspace {
        Workspace {
            id: id.into(),
            name: id.into(),
            output: output.into(),
            active: false,
            urgent: false,
        }
    }

    fn win(id: &str, workspace: &str) -> Window {
        Window {
            id: id.into(),
            class: "app".into(),
            title: "t".into(),
            workspace_id: workspace.into(),
            active: false,
            urgent: false,
        }
    }

    #[test]
    fn active_flags_survive_list_refresh() {
        let mut c = Compositor::default();
        c.apply(Event::Workspaces(vec![ws("1"), ws("2")]));
        c.apply(Event::ActiveWorkspace("2".into()));
        c.apply(Event::Workspaces(vec![ws("1"), ws("2"), ws("3")]));
        assert!(!c.workspaces[0].active);
        assert!(c.workspaces[1].active);
        assert!(!c.workspaces[2].active);
    }

    #[test]
    fn one_active_workspace_per_output() {
        let mut c = Compositor::default();
        c.apply(Event::Workspaces(vec![
            ws_on("1", "DP-1"),
            ws_on("2", "DP-1"),
            ws_on("3", "DP-2"),
        ]));
        c.apply(Event::ActiveWorkspace("1".into()));
        c.apply(Event::ActiveWorkspace("3".into()));
        c.apply(Event::ActiveWorkspace("2".into()));
        let active: Vec<_> = c
            .workspaces
            .iter()
            .filter(|w| w.active)
            .map(|w| w.id.as_str())
            .collect();
        assert_eq!(active, ["2", "3"]);
        c.apply(Event::ActiveWorkspace("nope".into()));
        assert_eq!(c.workspaces.iter().filter(|w| w.active).count(), 2);
    }

    #[test]
    fn urgency_propagates_and_clears_on_focus() {
        let mut c = Compositor::default();
        c.apply(Event::Workspaces(vec![ws("1"), ws("2")]));
        c.apply(Event::Windows(vec![win("a", "1"), win("b", "2")]));
        c.apply(Event::UrgentWindow("b".into()));
        assert!(c.windows[1].urgent);
        assert!(c.workspaces[1].urgent);
        assert!(!c.workspaces[0].urgent);

        c.apply(Event::ActiveWindow(Some("b".into())));
        assert!(!c.windows[1].urgent);
        assert!(!c.workspaces[1].urgent);
    }

    #[test]
    fn windows_of_workspace() {
        let mut c = Compositor::default();
        c.apply(Event::Workspaces(vec![ws("1")]));
        c.apply(Event::Windows(vec![
            win("a", "1"),
            win("b", "2"),
            win("c", "1"),
        ]));
        let ids: Vec<_> = c
            .windows_of(&c.workspaces[0])
            .map(|w| w.id.as_str())
            .collect();
        assert_eq!(ids, ["a", "c"]);
    }
}
