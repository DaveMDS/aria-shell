//! Workspaces gadget: one button per workspace, with the icon of each
//! window on it (a dot until the icon is known); click to switch
//! workspace (or focus a window). After them, the icon and title of the
//! active window, when it's on this panel's output.
//!
//! Holds no compositor state: it reads the daemon's [`Compositor`] from
//! the view context and filters it for the output the panel is on.

use iced_wayland_subscriber::OutputInfo;
use icssed::Element;

use crate::config::{RawSection, Section};
use crate::gadgets::{Action, Context, Gadget};
use crate::services::compositor::{Command, Window, Workspace};

/// `[WorkSpaces]` section (spelled as in the Python implementation).
#[derive(Debug, Clone)]
pub struct WorkspacesConfig {
    pub show_name: bool,
    pub show_windows: bool,
    /// Show every workspace, not only the ones on this panel's output.
    pub all_monitors: bool,
    /// Clicking a window marker focuses that window instead of just
    /// switching to its workspace.
    pub focus_window_on_click: bool,
    /// The active window's icon and title after the workspaces.
    pub show_title: bool,
}

impl Section for WorkspacesConfig {
    const NAME: &'static str = "WorkSpaces";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            show_name: raw.bool_or("show_name", true),
            show_windows: raw.bool_or("show_windows", true),
            all_monitors: raw.bool_or("all_monitors", false),
            focus_window_on_click: raw.bool_or("focus_window_on_click", false),
            show_title: raw.bool_or("show_title", true),
        }
    }
}

pub struct Workspaces {
    config: WorkspacesConfig,
    /// Connector name of the panel's output; `None` if the compositor
    /// didn't tell us, in which case every workspace is shown.
    output: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Message {
    Activate(String),
    Focus(String),
}

impl Gadget for Workspaces {
    type Config = WorkspacesConfig;
    type Message = Message;

    fn new(config: WorkspacesConfig, output: &OutputInfo) -> Self {
        if output.name.is_none() {
            log::warn!("output has no name, workspaces can't be filtered per monitor");
        }
        Self {
            config,
            output: output.name.clone(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Activate(id) => Action::Compositor(Command::ActivateWorkspace(id)),
            Message::Focus(id) => Action::Compositor(Command::ActivateWindow(id)),
        }
    }

    fn view<'a>(&'a self, ctx: Context<'a>) -> Element<'a, Message> {
        let shown: Vec<&Workspace> = ctx
            .compositor
            .workspaces
            .iter()
            .filter(|ws| {
                self.config.all_monitors
                    || self
                        .output
                        .as_ref()
                        .is_none_or(|output| ws.output == *output)
            })
            .collect();
        let mut children: Vec<Element<'a, Message>> = shown
            .iter()
            .map(|ws| self.workspace_view(ws, ctx))
            .collect();
        if self.config.show_title
            && let Some(win) = ctx
                .compositor
                .windows
                .iter()
                .find(|w| w.active && shown.iter().any(|ws| ws.id == w.workspace_id))
        {
            children.push(title_view(win, ctx));
        }
        icssed::row(children).into()
    }
}

impl Workspaces {
    fn workspace_view<'a>(&'a self, ws: &'a Workspace, ctx: Context<'a>) -> Element<'a, Message> {
        let mut children: Vec<Element<'a, Message>> = Vec::new();
        if self.config.show_name {
            children.push(icssed::text(ws.name.as_str()).into());
        }
        if self.config.show_windows {
            children.extend(
                ctx.compositor
                    .windows_of(ws)
                    .map(|w| self.window_view(w, ctx)),
            );
        }
        icssed::button(children)
            .class("workspace")
            .class_if("active", ws.active)
            .class_if("danger", ws.urgent)
            .attr("name", &ws.name)
            .on_press(Message::Activate(ws.id.clone()))
            .into()
    }

    /// The window's app icon, or a dot until it resolves; a button with
    /// `focus_window_on_click`.
    fn window_view<'a>(&'a self, win: &'a Window, ctx: Context<'a>) -> Element<'a, Message> {
        let content: Element<'a, Message> = match ctx.icons.get(&win.class) {
            Some(icon) => icssed::icon(icon.clone()).into(),
            None => icssed::mark().kind("dot").into(),
        };
        let window = icssed::row![content]
            .kind("window")
            .class_if("active", win.active)
            .class_if("danger", win.urgent)
            .attr("class", &win.class);
        if self.config.focus_window_on_click {
            window.on_press(Message::Focus(win.id.clone())).into()
        } else {
            window.into()
        }
    }
}

/// The active window's icon and title, as `title[class="<app id>"]`:
/// one line, cut where the slot ends.
fn title_view<'a>(win: &'a Window, ctx: Context<'a>) -> Element<'a, Message> {
    let mut parts: Vec<Element<'a, Message>> = Vec::new();
    if let Some(icon) = ctx.icons.get(&win.class) {
        parts.push(icssed::icon(icon.clone()).into());
    }
    parts.push(icssed::text(win.title.as_str()).into());
    icssed::row(parts)
        .kind("title")
        .attr("class", &win.class)
        .into()
}
