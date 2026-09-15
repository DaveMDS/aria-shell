//! Workspaces gadget: one button per workspace, with a marker per window
//! on it; click to switch workspace (or focus a window).
//!
//! Holds no compositor state: it reads the daemon's [`Compositor`] from
//! the view context and filters it for the output the panel is on.

use iced::widget::{button, row, text};
use iced::{Element, Theme};
use iced_wayland_subscriber::OutputInfo;

use crate::compositor::{Command, Compositor, Window, Workspace};
use crate::config::{RawSection, Section};
use crate::gadget::{Action, Context, Gadget};

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
        row(shown.map(|ws| self.workspace_view(ws, ctx.compositor)))
            .spacing(2)
            .into()
    }
}

impl Workspaces {
    fn workspace_view<'a>(
        &'a self,
        ws: &'a Workspace,
        compositor: &'a Compositor,
    ) -> Element<'a, Message> {
        let mut content = row![].spacing(4).align_y(iced::Alignment::Center);
        if self.config.show_name {
            content = content.push(text(&ws.name));
        }
        if self.config.show_windows {
            content = content.extend(compositor.windows_of(ws).map(|w| self.window_view(w)));
        }
        button(content)
            .padding([0, 6])
            .style(move |theme, status| workspace_style(theme, status, ws))
            .on_press(Message::Activate(ws.id.clone()))
            .into()
    }

    fn window_view<'a>(&'a self, win: &'a Window) -> Element<'a, Message> {
        let marker = text(if win.active { "◉" } else { "●" }).size(9);
        if self.config.focus_window_on_click {
            button(marker)
                .padding(0)
                .style(move |theme, status| window_style(theme, status, win))
                .on_press(Message::Focus(win.id.clone()))
                .into()
        } else {
            marker.into()
        }
    }
}

fn workspace_style(theme: &Theme, status: button::Status, ws: &Workspace) -> button::Style {
    if ws.urgent {
        button::danger(theme, status)
    } else if ws.active {
        button::primary(theme, status)
    } else {
        button::secondary(theme, status)
    }
}

fn window_style(theme: &Theme, status: button::Status, win: &Window) -> button::Style {
    if win.urgent {
        button::danger(theme, status)
    } else {
        button::text(theme, status)
    }
}
