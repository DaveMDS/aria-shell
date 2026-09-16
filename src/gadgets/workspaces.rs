//! Workspaces gadget: one button per workspace, with a marker per window
//! on it; click to switch workspace (or focus a window).
//!
//! Holds no compositor state: it reads the daemon's [`Compositor`] from
//! the view context and filters it for the output the panel is on.

use iced::Element;
use iced_wayland_subscriber::OutputInfo;

use crate::compositor::{Command, Window, Workspace};
use crate::config::{RawSection, Section};
use crate::gadget::{Action, Context, Gadget};
use crate::theme::{Node, Theme};

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
}

impl Section for WorkspacesConfig {
    const NAME: &'static str = "WorkSpaces";

    fn from_raw(raw: &RawSection) -> Self {
        Self {
            show_name: raw.bool_or("show_name", true),
            show_windows: raw.bool_or("show_windows", true),
            all_monitors: raw.bool_or("all_monitors", false),
            focus_window_on_click: raw.bool_or("focus_window_on_click", false),
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
        let shown = ctx.compositor.workspaces.iter().filter(|ws| {
            self.config.all_monitors
                || self
                    .output
                    .as_ref()
                    .is_none_or(|output| ws.output == *output)
        });
        let children = shown.map(|ws| self.workspace_view(ws, &ctx));
        ctx.theme
            .row(&ctx.node, children)
            .align_y(iced::Alignment::Center)
            .into()
    }
}

impl Workspaces {
    fn workspace_view<'a>(&'a self, ws: &'a Workspace, ctx: &Context<'a>) -> Element<'a, Message> {
        let node = ctx
            .node
            .child("workspace")
            .class_if("active", ws.active)
            .class_if("urgent", ws.urgent);
        let mut children: Vec<Element<'a, Message>> = Vec::new();
        if self.config.show_name {
            children.push(ctx.theme.text(&node.child("text"), &ws.name).into());
        }
        if self.config.show_windows {
            children.extend(
                ctx.compositor
                    .windows_of(ws)
                    .map(|w| self.window_view(w, &node, ctx.theme)),
            );
        }
        let content = ctx
            .theme
            .row(&node, children)
            .align_y(iced::Alignment::Center);
        ctx.theme
            .button(&node, content)
            .on_press(Message::Activate(ws.id.clone()))
            .into()
    }

    fn window_view<'a>(
        &'a self,
        win: &'a Window,
        workspace: &Node,
        theme: &'a Theme,
    ) -> Element<'a, Message> {
        let node = workspace
            .child("window")
            .class_if("active", win.active)
            .class_if("urgent", win.urgent);
        let marker = theme.text(&node.child("text"), if win.active { "◉" } else { "●" });
        if self.config.focus_window_on_click {
            theme
                .button(&node, marker)
                .on_press(Message::Focus(win.id.clone()))
                .into()
        } else {
            theme.container(&node, marker).into()
        }
    }
}
