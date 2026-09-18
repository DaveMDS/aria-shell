//! The application launcher: a centred overlay with a search field and
//! the matching desktop entries; Enter or a click runs one.
//!
//! A component the daemon owns while it's open (one at a time, on the
//! focused output), not a gadget: it has its own layer surface and the
//! keyboard. It searches the desktop database already indexed for the
//! window icons (`icons::Index`), taken as a snapshot when it opens and
//! swapped when the index is rebuilt. Icons are resolved by the daemon
//! ([`Launcher::visible_ids`]) and read here in `view`, as gadgets do.

use std::sync::Arc;

use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{Space, column, operation, scrollable};
use iced::{Element, Event, Length, Rectangle, Subscription, Task, widget, window};

use crate::config::{RawSection, Section};
use crate::gadget::Shared;
use crate::icons::Index;
use crate::icons::desktop::{self, DesktopEntry};
use crate::theme::{self, Node};
use crate::widgets;

/// `[launcher]` section.
#[derive(Debug, Clone)]
pub struct LauncherConfig {
    /// Terminal emulator for `Terminal=true` entries, run as
    /// `<terminal> -e <command>`. Empty: `$TERMINAL`, else the first
    /// of [`TERMINALS`] on the PATH, else `xterm`.
    pub terminal: String,
}

/// Terminals tried when neither the config nor `$TERMINAL` says.
pub const TERMINALS: &[&str] = &[
    "kitty",
    "alacritty",
    "foot",
    "wezterm",
    "ghostty",
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "xterm",
];

impl Section for LauncherConfig {
    const NAME: &'static str = "launcher";

    fn from_raw(raw: &RawSection) -> Self {
        let terminal = raw
            .get("terminal")
            .map(str::to_owned)
            .or_else(|| std::env::var("TERMINAL").ok().filter(|t| !t.is_empty()))
            .unwrap_or_else(|| {
                crate::process::first_on_path(TERMINALS)
                    .unwrap_or("xterm")
                    .to_owned()
            });
        Self { terminal }
    }
}

pub struct Launcher {
    config: LauncherConfig,
    apps: Option<Arc<Index>>,
    query: String,
    /// Indices into the desktop db, best match first.
    results: Vec<usize>,
    selected: usize,
    input: widget::Id,
    list: widget::Id,
    /// How far the list is scrolled, from its `on_scroll`.
    offset: f32,
    node: Node,
}

#[derive(Debug, Clone)]
pub enum Message {
    Query(String),
    /// Enter: run the selected entry.
    Submit,
    Up,
    Down,
    /// A click on that result.
    Activate(usize),
    /// Esc, or a click outside.
    Close,
    /// The list was scrolled (by the wheel, or by us).
    Scrolled(scrollable::Viewport),
    /// Where the selected row and the list are, to keep the row in view.
    Located {
        item: Option<Rectangle>,
        list: Option<Rectangle>,
    },
}

pub enum Action {
    Run(Task<Message>),
    Close,
}

impl Launcher {
    pub fn new(config: LauncherConfig, apps: Option<Arc<Index>>) -> Self {
        let mut launcher = Self {
            config,
            apps,
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            input: widget::Id::unique(),
            list: widget::Id::unique(),
            offset: 0.0,
            node: Node::root("launcher"),
        };
        launcher.search();
        launcher
    }

    /// The desktop database was rebuilt.
    pub fn set_apps(&mut self, apps: Arc<Index>) {
        self.apps = Some(apps);
        self.search();
    }

    /// Give the search field the keyboard, once the surface exists.
    pub fn focus(&self) -> Task<Message> {
        operation::focus(self.input.clone())
    }

    /// Desktop ids of the entries on the list, for icon resolution.
    pub fn visible_ids(&self) -> impl Iterator<Item = &str> {
        self.entries().map(|e| e.id.as_str())
    }

    fn entries(&self) -> impl Iterator<Item = &DesktopEntry> {
        let all = self.apps.as_deref().map(|i| i.apps().entries());
        self.results.iter().filter_map(move |&i| all?.get(i))
    }

    fn search(&mut self) {
        let all = self.apps.as_deref().map_or(&[][..], |i| i.apps().entries());
        self.results = search(all, &self.query);
        self.selected = 0;
    }

    /// The node of result `i` as `view` builds it: its path is the
    /// widget id the theme helpers tag it with.
    fn item_node(&self, i: usize) -> Node {
        self.node
            .child("list")
            .child("item")
            .class_if("selected", i == self.selected)
            .nth(i, self.results.len())
    }

    /// Ask the widget tree where row `i` and the list are;
    /// [`Message::Located`] then scrolls only if the row is out of view.
    /// Called with the row *past* the selection in the direction of
    /// travel, so the next one is already visible before it's selected.
    fn locate(&self, i: usize) -> Task<Message> {
        if self.results.is_empty() {
            return Task::none();
        }
        let i = i.min(self.results.len() - 1);
        let item = theme::widget_id(&self.item_node(i));
        let list = theme::widget_id(&self.node.child("list"));
        widgets::bounds(item).and_then(move |item| {
            widgets::bounds(list.clone()).map(move |list| Message::Located {
                item: Some(item),
                list,
            })
        })
    }

    /// Scroll the least that brings the row into the list's viewport:
    /// the bounds are layout coordinates (the row where it would be
    /// unscrolled), so the row's place in the content is its offset
    /// from the list's top.
    fn keep_in_view(&mut self, item: Rectangle, list: Rectangle) -> Task<Message> {
        let top = item.y - list.y;
        let bottom = top + item.height;
        let y = if top < self.offset {
            top
        } else if bottom > self.offset + list.height {
            bottom - list.height
        } else {
            return Task::none();
        };
        self.offset = y.max(0.0);
        operation::scroll_to(
            self.list.clone(),
            AbsoluteOffset {
                x: None,
                y: Some(self.offset),
            },
        )
    }

    fn launch(&self, index: usize) {
        let Some(entry) = self
            .results
            .get(index)
            .and_then(|&i| self.apps.as_deref().and_then(|a| a.apps().entries().get(i)))
        else {
            return;
        };
        if let Err(e) = desktop::launch(entry, &self.config.terminal) {
            log::error!("cannot launch {:?}: {e}", entry.id);
        }
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Query(q) => {
                self.query = q;
                self.search();
                self.offset = 0.0;
                Action::Run(operation::scroll_to(
                    self.list.clone(),
                    AbsoluteOffset {
                        x: None,
                        y: Some(0.0),
                    },
                ))
            }
            Message::Up => {
                self.selected = self.selected.saturating_sub(1);
                Action::Run(self.locate(self.selected.saturating_sub(1)))
            }
            Message::Down => {
                if self.selected + 1 < self.results.len() {
                    self.selected += 1;
                }
                Action::Run(self.locate(self.selected + 1))
            }
            Message::Scrolled(viewport) => {
                self.offset = viewport.absolute_offset().y;
                Action::Run(Task::none())
            }
            Message::Located {
                item: Some(item),
                list: Some(list),
            } => Action::Run(self.keep_in_view(item, list)),
            Message::Located { .. } => Action::Run(Task::none()),
            Message::Submit => {
                self.launch(self.selected);
                Action::Close
            }
            Message::Activate(i) => {
                self.launch(i);
                Action::Close
            }
            Message::Close => Action::Close,
        }
    }

    pub fn view<'a>(&'a self, shared: Shared<'a>) -> Element<'a, Message> {
        let theme = shared.theme;
        let input = theme
            .text_input(
                &self.node.child("input"),
                shared.locale.tr("launcher.search"),
                &self.query,
            )
            .id(self.input.clone())
            .on_input(Message::Query)
            .on_submit(Message::Submit)
            .width(Length::Fill);
        let list_node = self.node.child("list");
        let items = self.entries().enumerate().map(|(i, entry)| {
            let node = self.item_node(i);
            let icon_node = node.child("icon");
            let style = theme.resolve(&icon_node);
            let size = style.height.or(style.width).and_then(|l| match l {
                theme::Length::Px(px) => Some(px),
                _ => None,
            });
            let icon: Element<'a, Message> = match (shared.icons.get(&entry.id), size) {
                (Some(icon), Some(size)) => icon.view(size, style.color),
                (_, Some(size)) => Space::new().width(size).height(size).into(),
                _ => Space::new().into(),
            };
            let mut text = column![theme.text(&node.child("title"), &entry.name)];
            if let Some(comment) = &entry.comment {
                text = text.push(theme.text(&node.child("subtitle"), comment));
            }
            let content = theme
                .row(&node, [icon, text.into()])
                .align_y(iced::Alignment::Center)
                .width(Length::Fill);
            theme
                .button(&node, content)
                .width(Length::Fill)
                .on_press(Message::Activate(i))
                .into()
        });
        // The list is tagged so `locate_selected` finds its viewport.
        let list = theme
            .tag(
                &list_node,
                scrollable(theme.column(&list_node, items).width(Length::Fill))
                    .id(self.list.clone())
                    .on_scroll(Message::Scrolled)
                    .height(Length::Fill),
            )
            .height(Length::Fill);
        // The daemon's root container already applies the `launcher`
        // padding; only its `gap` is ours.
        column![input, list]
            .spacing(theme.resolve(&self.node).gap)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The keys the search field doesn't handle itself (Esc is one it
    /// does handle, dropping focus, but a captured event still reaches
    /// `listen_with`), and the keyboard leaving the surface: the
    /// compositor gave the focus to something else (a click on a
    /// window, a keybind), which closes the launcher as cosmic-launcher
    /// does. The daemon checks that `window` is the launcher's.
    pub fn subscription(&self) -> Subscription<(window::Id, Message)> {
        iced::event::listen_with(|event, _status, window| {
            let message = match event {
                Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => match key.as_ref() {
                    Key::Named(Named::ArrowUp) => Message::Up,
                    Key::Named(Named::ArrowDown) => Message::Down,
                    Key::Named(Named::Escape) => Message::Close,
                    _ => return None,
                },
                Event::Window(window::Event::Unfocused) => {
                    log::debug!("launcher: keyboard left window {window:?}");
                    Message::Close
                }
                Event::Window(window::Event::Focused) => {
                    log::debug!("launcher: keyboard entered window {window:?}");
                    return None;
                }
                _ => return None,
            };
            Some((window, message))
        })
    }
}

/// The surface behind the launcher on every output: transparent, and a
/// click on it closes the launcher (the click is swallowed, as a
/// compositor does for a popup's). The click is caught by
/// [`grab_clicks`], not a `mouse_area`: the surface appears under a
/// pointer that may not move before clicking (the bar button that
/// opened the launcher, clicked again), and iced places the cursor
/// only on motion.
pub fn grab_view<'a>() -> Element<'a, Message> {
    Space::new().width(Length::Fill).height(Length::Fill).into()
}

/// Mouse buttons released on any of our windows; the daemon closes the
/// launcher when the click wasn't on it. On the release, not the press:
/// closing on the press destroys the surface before the release
/// reaches it, and Hyprland then swallows the next click.
pub fn grab_clicks() -> Subscription<window::Id> {
    iced::event::listen_with(|event, _status, window| match event {
        Event::Mouse(iced::mouse::Event::ButtonReleased(_)) => Some(window),
        _ => None,
    })
}

/// Indices of the entries matching `query`, best first: an empty query
/// lists every entry; otherwise the id, name and comment are tried for
/// an exact match (10), a prefix (8), a substring (6), case-insensitive,
/// as the Python implementation scored. Ties keep name order.
fn search(entries: &[DesktopEntry], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut scored: Vec<(u8, usize)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.no_display && e.exec_line.is_some())
        .filter_map(|(i, e)| {
            let score = if query.is_empty() {
                1
            } else {
                [Some(&e.id), Some(&e.name), e.comment.as_ref()]
                    .into_iter()
                    .flatten()
                    .map(|field| {
                        let field = field.to_lowercase();
                        if field == query {
                            10
                        } else if field.starts_with(&query) {
                            8
                        } else if field.contains(&query) {
                            6
                        } else {
                            0
                        }
                    })
                    .max()
                    .unwrap_or(0)
            };
            (score > 0).then_some((score, i))
        })
        .collect();
    scored.sort_by(|(sa, a), (sb, b)| {
        sb.cmp(sa).then_with(|| {
            entries[*a]
                .name
                .to_lowercase()
                .cmp(&entries[*b].name.to_lowercase())
        })
    });
    scored.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(id: &str, name: &str, comment: Option<&str>, no_display: bool) -> DesktopEntry {
        DesktopEntry {
            id: id.into(),
            name: name.into(),
            comment: comment.map(str::to_owned),
            icon: None,
            exec: Some(id.into()),
            exec_line: Some(id.into()),
            terminal: false,
            working_dir: None,
            wm_class: None,
            no_display,
            path: PathBuf::from(format!("/x/{id}.desktop")),
        }
    }

    fn names<'a>(entries: &'a [DesktopEntry], query: &str) -> Vec<&'a str> {
        search(entries, query)
            .into_iter()
            .map(|i| entries[i].name.as_str())
            .collect()
    }

    #[test]
    fn ranks_exact_prefix_substring() {
        let all = [
            entry(
                "org.gnome.terminal",
                "Terminal",
                Some("Use the command line"),
                false,
            ),
            entry("kitty", "kitty", Some("A fast terminal emulator"), false),
            entry("term", "Zeta", None, false),
            entry("firefox", "Firefox", Some("Browse the web"), false),
            entry("hidden", "Term hidden", None, true),
        ];
        assert_eq!(names(&all, "term"), ["Zeta", "Terminal", "kitty"]);
        assert_eq!(names(&all, "  FIRE "), ["Firefox"]);
        assert_eq!(names(&all, ""), ["Firefox", "kitty", "Terminal", "Zeta"]);
        assert!(names(&all, "nothing").is_empty());
    }
}
