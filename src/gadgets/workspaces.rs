//! Workspaces gadget: one button per workspace, with the icon of each
//! window on it (a dot until the icon is known); click to switch
//! workspace (or focus a window). After them, the icon and title of the
//! active window, when it's on this panel's output.
//!
//! Holds no compositor state: it reads the daemon's [`Compositor`] from
//! the view context and filters it for the output the panel is on.

use iced::Element;
use iced_wayland_subscriber::OutputInfo;

use crate::compositor::{Command, Window, Workspace};
use crate::config::{RawSection, Section};
use crate::gadget::{Action, Context, Gadget};
use crate::theme::{Length, Node};

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
        let shown = ctx.compositor.workspaces.iter().filter(|ws| {
            self.config.all_monitors
                || self
                    .output
                    .as_ref()
                    .is_none_or(|output| ws.output == *output)
        });
        let shown: Vec<&Workspace> = shown.collect();
        let count = shown.len();
        let mut children: Vec<Element<'a, Message>> = shown
            .iter()
            .enumerate()
            .map(|(i, ws)| self.workspace_view(ws, i, count, &ctx))
            .collect();
        if self.config.show_title
            && let Some(win) = ctx
                .compositor
                .windows
                .iter()
                .find(|w| w.active && shown.iter().any(|ws| ws.id == w.workspace_id))
        {
            children.push(self.title_view(win, &ctx));
        }
        ctx.theme
            .row(&ctx.node, children)
            .align_y(iced::Alignment::Center)
            .into()
    }
}

impl Workspaces {
    fn workspace_view<'a>(
        &'a self,
        ws: &'a Workspace,
        index: usize,
        count: usize,
        ctx: &Context<'a>,
    ) -> Element<'a, Message> {
        let node = ctx
            .node
            .child("workspace")
            .class_if("active", ws.active)
            .class_if("urgent", ws.urgent)
            .attr("name", ws.name.clone())
            .nth(index, count);
        let mut children: Vec<Element<'a, Message>> = Vec::new();
        if self.config.show_name {
            children.push(ctx.theme.text(&node.child("text"), &ws.name).into());
        }
        if self.config.show_windows {
            let windows: Vec<&Window> = ctx.compositor.windows_of(ws).collect();
            let count = windows.len();
            children.extend(
                windows
                    .into_iter()
                    .enumerate()
                    .map(|(i, w)| self.window_view(w, &node.child("window").nth(i, count), ctx)),
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

    /// The active window's icon (at the `icon` node's `height`) and
    /// title, as `title[class="<app id>"]`.
    fn title_view<'a>(&'a self, win: &'a Window, ctx: &Context<'a>) -> Element<'a, Message> {
        let theme = ctx.theme;
        let node = ctx.node.child("title").attr("class", win.class.clone());
        let icon_node = node.child("icon");
        let style = theme.resolve(&icon_node);
        let size = style.height.or(style.width).and_then(|l| match l {
            Length::Px(px) => Some(px),
            _ => None,
        });
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if let (Some(icon), Some(size)) = (ctx.icons.get(&win.class), size) {
            parts.push(icon.view(size, style.color));
        }
        parts.push(theme.text(&node.child("text"), &win.title).into());
        theme
            .container(
                &node,
                theme.row(&node, parts).align_y(iced::Alignment::Center),
            )
            .into()
    }

    /// The window's app icon at the CSS `height` of the `window` node
    /// (its `width` if there's no height), or a dot until it resolves.
    fn window_view<'a>(
        &'a self,
        win: &'a Window,
        node: &Node,
        ctx: &Context<'a>,
    ) -> Element<'a, Message> {
        let theme = ctx.theme;
        let node = node
            .class_if("active", win.active)
            .class_if("urgent", win.urgent)
            .attr("class", win.class.clone());
        let style = theme.resolve(&node);
        let size = style.height.or(style.width).and_then(|l| match l {
            Length::Px(px) => Some(px),
            _ => None,
        });
        let content: Element<'a, Message> = match (ctx.icons.get(&win.class), size) {
            (Some(icon), Some(size)) => icon.view(size, style.color),
            _ => theme
                .text(&node.child("text"), if win.active { "◉" } else { "●" })
                .into(),
        };
        if self.config.focus_window_on_click {
            theme
                .button(&node, content)
                .on_press(Message::Focus(win.id.clone()))
                .into()
        } else {
            theme.container(&node, content).into()
        }
    }
}
