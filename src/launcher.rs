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
use iced::widget::scrollable::RelativeOffset;
use iced::widget::{Space, column, mouse_area, operation, scrollable};
use iced::{Element, Event, Length, Subscription, Task, widget, window};

use crate::config::{RawSection, Section};
use crate::gadget::Shared;
use crate::icons::Index;
use crate::icons::desktop::{self, DesktopEntry};
use crate::theme::{self, Node};

/// `[launcher]` section.
#[derive(Debug, Clone)]
pub struct LauncherConfig {
    /// Terminal emulator for `Terminal=true` entries, run as
    /// `<terminal> -e <command>`. Empty: `$TERMINAL`, else `xterm`.
    pub terminal: String,
}

impl Section for LauncherConfig {
    const NAME: &'static str = "launcher";

    fn from_raw(raw: &RawSection) -> Self {
        let terminal = raw
            .get("terminal")
            .map(str::to_owned)
            .unwrap_or_else(|| std::env::var("TERMINAL").unwrap_or_else(|_| "xterm".to_owned()));
        Self { terminal }
    }
}

/// Placeholder shown in the empty search field.
const PLACEHOLDER: &str = "Search applications…";

pub struct Launcher {
    config: LauncherConfig,
    apps: Option<Arc<Index>>,
    query: String,
    /// Indices into the desktop db, best match first.
    results: Vec<usize>,
    selected: usize,
    input: widget::Id,
    list: widget::Id,
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

    /// Keep `selected` in view. The scroll position is proportional to
    /// the selection's place in the list, which always shows it and
    /// needs no row height.
    fn scroll_to_selected(&self) -> Task<Message> {
        let last = self.results.len().saturating_sub(1);
        let y = if last == 0 {
            0.0
        } else {
            self.selected as f32 / last as f32
        };
        operation::snap_to(self.list.clone(), RelativeOffset { x: 0.0, y })
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
                Action::Run(self.scroll_to_selected())
            }
            Message::Up => {
                self.selected = self.selected.saturating_sub(1);
                Action::Run(self.scroll_to_selected())
            }
            Message::Down => {
                if self.selected + 1 < self.results.len() {
                    self.selected += 1;
                }
                Action::Run(self.scroll_to_selected())
            }
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
            .text_input(&self.node.child("input"), PLACEHOLDER, &self.query)
            .id(self.input.clone())
            .on_input(Message::Query)
            .on_submit(Message::Submit)
            .width(Length::Fill);
        let list_node = self.node.child("list");
        let count = self.results.len();
        let items = self.entries().enumerate().map(|(i, entry)| {
            let node = list_node
                .child("item")
                .class_if("selected", i == self.selected)
                .nth(i, count);
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
        let list = scrollable(theme.column(&list_node, items).width(Length::Fill))
            .id(self.list.clone())
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
/// compositor does for a popup's).
pub fn grab_view<'a>() -> Element<'a, Message> {
    mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
        .on_press(Message::Close)
        .into()
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
