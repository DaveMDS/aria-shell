//! The application launcher: a centred overlay with a search field and
//! the matching desktop entries; Enter or a click runs one.
//!
//! A component the daemon owns while it's open (one at a time, on the
//! focused output), not a gadget: it has its own layer surface and the
//! keyboard. It searches the desktop database already indexed for the
//! window icons (`icons::Index`), taken as a snapshot when it opens and
//! swapped when the index is rebuilt. Icons are resolved by the daemon
//! ([`Launcher::visible_ids`]) and read here in `view`, as gadgets do.
//! How often each entry was launched ([`Usage`]) ranks ties, so the
//! apps you use come first. An entry with desktop actions ends in a
//! chevron: Right (or a click on it) opens them as child rows below,
//! Left closes them.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use iced::keyboard::key::Named;
use iced::keyboard::{self, Key};
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::{operation, scrollable};
use iced::{Event, Rectangle, Subscription, Task, widget, window};
use icssed::Element;

use crate::components::exiter::{self, ExiterConfig};
use crate::config::{self, RawSection, Section};
use crate::services::icons::Index;
use crate::services::icons::desktop::{self, DesktopAction, DesktopEntry};
use crate::shared::Shared;
use crate::ui;

/// `[launcher]` section.
#[derive(Debug, Clone)]
pub struct LauncherConfig {
    /// The exit menu's buttons shown as a row of icons above the
    /// search field: `all`, `none`, or their names.
    pub actions: Actions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actions {
    All,
    None,
    Some(Vec<String>),
}

impl Actions {
    fn shows(&self, name: &str) -> bool {
        match self {
            Self::All => true,
            Self::None => false,
            Self::Some(names) => names.iter().any(|n| n == name),
        }
    }
}

impl Section for LauncherConfig {
    const NAME: &'static str = "launcher";

    fn from_raw(raw: &RawSection) -> Self {
        let actions = match raw.list_or("actions", &["all"]).as_slice() {
            [one] if one == "all" => Actions::All,
            [one] if one == "none" => Actions::None,
            names => Actions::Some(names.to_vec()),
        };
        Self { actions }
    }
}

/// How many times each desktop entry was launched, by id, kept in
/// `$XDG_STATE_HOME/aria-shell/launcher-usage` as `<id> <count>` lines
/// (edit or delete it to reset). Never in the way: a missing or broken
/// file is an empty one, a failed write is logged.
#[derive(Debug, Default)]
pub struct Usage {
    counts: HashMap<String, u32>,
    path: Option<PathBuf>,
}

impl Usage {
    pub fn load() -> Self {
        Self::load_from(config::state_dir().map(|d| d.join("launcher-usage")))
    }

    fn load_from(path: Option<PathBuf>) -> Self {
        let mut usage = Self {
            counts: HashMap::new(),
            path,
        };
        let Some(path) = &usage.path else {
            return usage;
        };
        match fs::read_to_string(path) {
            Ok(text) => usage.counts = parse_usage(&text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("cannot read {}: {e}", path.display()),
        }
        usage
    }

    fn count(&self, id: &str) -> u32 {
        self.counts.get(id).copied().unwrap_or(0)
    }

    /// One more launch of `id`, saved right away.
    fn bump(&mut self, id: &str) {
        *self.counts.entry(id.to_owned()).or_insert(0) += 1;
        if let Some(path) = &self.path
            && let Err(e) = self.save(path)
        {
            log::warn!("cannot write {}: {e}", path.display());
        }
    }

    /// The whole file, most launched first, through a temporary so a
    /// crash midway leaves the old one.
    fn save(&self, path: &PathBuf) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut lines: Vec<(&str, u32)> =
            self.counts.iter().map(|(k, &v)| (k.as_str(), v)).collect();
        lines.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let tmp = path.with_extension("tmp");
        let mut file = fs::File::create(&tmp)?;
        for (id, count) in lines {
            writeln!(file, "{id} {count}")?;
        }
        file.flush()?;
        fs::rename(&tmp, path)
    }
}

/// `<id> <count>` per line; anything else is skipped.
fn parse_usage(text: &str) -> HashMap<String, u32> {
    text.lines()
        .filter_map(|line| {
            let (id, count) = line.trim().split_once(' ')?;
            Some((id.to_owned(), count.trim().parse().ok()?))
        })
        .collect()
}

pub struct Launcher {
    config: LauncherConfig,
    /// The exit menu's buttons, for the row of actions.
    exiter: ExiterConfig,
    /// `[general] terminal`, for `Terminal=true` entries.
    terminal: Option<String>,
    apps: Option<Arc<Index>>,
    usage: Usage,
    query: String,
    /// Indices into the desktop db, best match first.
    results: Vec<usize>,
    /// The one result whose actions are open as child rows.
    expanded: Option<usize>,
    /// Index into [`Launcher::rows`].
    selected: usize,
    input: widget::Id,
    /// The list's scroll.
    list: widget::Id,
    /// Makes the rows' ids unique (the runtime searches every window).
    serial: u64,
    /// How far the list is scrolled, from its `on_scroll`.
    offset: f32,
}

/// A row of the list: a result, or one of the expanded result's
/// actions (result index, action index).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    App(usize),
    Action(usize, usize),
}

impl Row {
    fn result(self) -> usize {
        match self {
            Self::App(i) | Self::Action(i, _) => i,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Query(String),
    /// Enter: run the selected row.
    Submit,
    Up,
    Down,
    /// Open the selected result's actions.
    Right,
    /// Close them, back to the result.
    Left,
    /// The chevron of that result: open or close its actions.
    Toggle(usize),
    /// A click on that row.
    Activate(Row),
    /// Esc, or a click outside.
    Close,
    /// One of the exit menu's buttons, by name.
    Exit(String),
    /// The list was scrolled (by the wheel, or by us).
    Scrolled(scrollable::Viewport),
    /// Where a row and the list's viewport are, to keep the row in view.
    Located {
        item: Option<Rectangle>,
        list: Option<Rectangle>,
    },
    /// A press the dialog's content took (see `dialog::content`).
    Nothing,
}

pub enum Action {
    Run(Task<Message>),
    Close,
    /// Close, and do what the exit menu's button `name` does.
    Exit(String),
}

impl Launcher {
    pub fn new(
        config: LauncherConfig,
        exiter: ExiterConfig,
        terminal: Option<String>,
        apps: Option<Arc<Index>>,
    ) -> Self {
        let mut launcher = Self {
            config,
            exiter,
            terminal,
            apps,
            usage: Usage::load(),
            query: String::new(),
            results: Vec::new(),
            expanded: None,
            selected: 0,
            input: widget::Id::unique(),
            list: widget::Id::unique(),
            serial: {
                static NEXT: AtomicU64 = AtomicU64::new(1);
                NEXT.fetch_add(1, Ordering::Relaxed)
            },
            offset: 0.0,
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

    /// The entry of result `i`.
    fn entry(&self, i: usize) -> Option<&DesktopEntry> {
        let index = *self.results.get(i)?;
        self.apps.as_deref()?.apps().entries().get(index)
    }

    /// The list as laid out: every result, and the expanded one's
    /// actions right below it.
    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::with_capacity(self.results.len());
        for i in 0..self.results.len() {
            rows.push(Row::App(i));
            if self.expanded == Some(i) {
                let n = self.entry(i).map_or(0, |e| e.actions.len());
                rows.extend((0..n).map(|a| Row::Action(i, a)));
            }
        }
        rows
    }

    fn selected_row(&self) -> Option<Row> {
        self.rows().get(self.selected).copied()
    }

    /// Select `row` wherever it is once the rows are rebuilt.
    fn select(&mut self, row: Row) {
        self.selected = self.rows().iter().position(|r| *r == row).unwrap_or(0);
    }

    fn search(&mut self) {
        let all = self.apps.as_deref().map_or(&[][..], |i| i.apps().entries());
        self.results = search(all, &self.query, &self.usage);
        self.expanded = None;
        self.selected = 0;
    }

    /// The id of row `i`'s box.
    fn row_id(&self, i: usize) -> widget::Id {
        widget::Id::from(format!("launcher-{}-row-{i}", self.serial))
    }

    /// Ask the widget tree where row `i` and the list's viewport are;
    /// [`Message::Located`] then scrolls only if the row is out of view.
    /// Called with the row *past* the selection in the direction of
    /// travel, so the next one is already visible before it's selected.
    fn locate(&self, i: usize) -> Task<Message> {
        let rows = self.rows();
        if rows.is_empty() {
            return Task::none();
        }
        let item = self.row_id(i.min(rows.len() - 1));
        let list = self.list.clone();
        ui::bounds(item).and_then(move |item| {
            ui::scroll_bounds(list.clone()).map(move |list| Message::Located {
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

    /// Run the row's entry or action; either counts as a launch of
    /// the entry.
    fn launch(&mut self, row: Row) {
        let Some(entry) = self.entry(row.result()) else {
            return;
        };
        let action: Option<&DesktopAction> = match row {
            Row::App(_) => None,
            Row::Action(_, a) => match entry.actions.get(a) {
                Some(action) => Some(action),
                None => return,
            },
        };
        match desktop::launch(entry, action, self.terminal.as_deref()) {
            Ok(()) => {
                let id = entry.id.clone();
                self.usage.bump(&id);
            }
            Err(e) => log::error!("cannot launch {:?}: {e}", entry.id),
        }
    }

    /// Open the actions of result `i` (closing any other's) and
    /// select its first one, or close them and select the result.
    fn toggle(&mut self, i: usize) {
        let has_actions = self.entry(i).is_some_and(|e| !e.actions.is_empty());
        if self.expanded == Some(i) || !has_actions {
            self.expanded = None;
            self.select(Row::App(i));
        } else {
            self.expanded = Some(i);
            self.select(Row::Action(i, 0));
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
                if self.selected + 1 < self.rows().len() {
                    self.selected += 1;
                }
                Action::Run(self.locate(self.selected + 1))
            }
            Message::Right => match self.selected_row() {
                Some(Row::App(i)) if self.expanded != Some(i) => {
                    self.toggle(i);
                    Action::Run(self.locate(self.selected + 1))
                }
                _ => Action::Run(Task::none()),
            },
            Message::Left => match self.selected_row() {
                Some(Row::Action(i, _)) => {
                    self.toggle(i);
                    Action::Run(self.locate(self.selected))
                }
                Some(Row::App(i)) if self.expanded == Some(i) => {
                    self.toggle(i);
                    Action::Run(Task::none())
                }
                _ => Action::Run(Task::none()),
            },
            Message::Toggle(i) => {
                self.toggle(i);
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
                if let Some(row) = self.selected_row() {
                    self.launch(row);
                }
                Action::Close
            }
            Message::Activate(row) => {
                self.launch(row);
                Action::Close
            }
            Message::Close => Action::Close,
            Message::Exit(name) => Action::Exit(name),
            Message::Nothing => Action::Run(Task::none()),
        }
    }

    /// The exit menu's buttons the row shows.
    fn actions(&self) -> impl Iterator<Item = &exiter::Button> {
        self.exiter
            .buttons
            .iter()
            .filter(|b| self.config.actions.shows(&b.name))
    }

    /// Icon names the view may draw, for the daemon to resolve.
    pub fn icon_names(&self) -> impl Iterator<Item = &str> {
        self.actions()
            .flat_map(|b| b.icons.iter().map(String::as_str))
    }

    /// Its dialog's box: the exit menu's buttons, the search field, the
    /// results.
    pub fn view<'a>(&'a self, shared: Shared<'a>) -> Element<'a, Message> {
        let icons: &'a dyn icssed::Icons = shared.icons;
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        let buttons: Vec<&exiter::Button> = self.actions().collect();
        if !buttons.is_empty() {
            let buttons = buttons.into_iter().map(|button| {
                let icon = match shared.icons.first_of(&button.icons) {
                    Some(icon) => icssed::icon(icon.clone()),
                    None => icssed::icon((icons, "")),
                };
                icssed::button![icon]
                    .class(&button.name)
                    .class("flat")
                    .on_press(Message::Exit(button.name.clone()))
                    .into()
            });
            parts.push(icssed::row(buttons).kind("actions").into());
        }
        parts.push(
            icssed::input(shared.locale.tr("launcher.search"), &self.query)
                .id(self.input.clone())
                .on_input(Message::Query)
                .on_submit(Message::Submit)
                .into(),
        );
        let rows = self.rows();
        let items = rows.iter().enumerate().filter_map(|(i, &row)| {
            let entry = self.entry(row.result())?;
            let icon = match shared.icons.get(&entry.id) {
                Some(icon) => icssed::icon(icon.clone()),
                None => icssed::icon((icons, "")),
            };
            // An action row: its name under the entry's icon. An entry
            // with actions: a chevron at the end, a button of its own
            // (it takes the press before the row does).
            let mut lines: Vec<Element<'a, Message>> = Vec::new();
            let mut chevron: Option<Element<'a, Message>> = None;
            match row {
                Row::Action(_, a) => {
                    let action = entry.actions.get(a)?;
                    lines.push(icssed::text(action.name.as_str()).kind("title").into());
                }
                Row::App(r) => {
                    lines.push(icssed::text(entry.name.as_str()).kind("title").into());
                    if let Some(comment) = &entry.comment {
                        lines.push(icssed::text(comment.as_str()).kind("subtitle").into());
                    }
                    if !entry.actions.is_empty() {
                        chevron = Some(
                            icssed::button![icssed::mark().kind("arrow")]
                                .class("chevron")
                                .class("plain")
                                .class_if("active", self.expanded == Some(r))
                                .on_press(Message::Toggle(r))
                                .into(),
                        );
                    }
                }
            }
            let mut parts: Vec<Element<'a, Message>> =
                vec![icon.into(), icssed::column(lines).kind("texts").into()];
            parts.extend(chevron);
            Some(
                icssed::button(parts)
                    .kind("item")
                    .class_if("action", matches!(row, Row::Action(..)))
                    .class_if("selected", i == self.selected)
                    .on_press(Message::Activate(row))
                    .id(self.row_id(i))
                    .into(),
            )
        });
        parts.push(
            icssed::scroll(icssed::column(items).kind("list"))
                .id(self.list.clone())
                .on_scroll(Message::Scrolled)
                .into(),
        );
        icssed::column(parts).into()
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
                    Key::Named(Named::ArrowRight) => Message::Right,
                    Key::Named(Named::ArrowLeft) => Message::Left,
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

/// Indices of the entries matching `query`, best first: an empty query
/// lists every entry; otherwise the id, name and comment are tried for
/// an exact match (10), a prefix (8), a substring (6), case-insensitive,
/// as the Python implementation scored. Ties go to the entry launched
/// more often, then to name order.
fn search(entries: &[DesktopEntry], query: &str, usage: &Usage) -> Vec<usize> {
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
        sb.cmp(sa)
            .then_with(|| {
                usage
                    .count(&entries[*b].id)
                    .cmp(&usage.count(&entries[*a].id))
            })
            .then_with(|| {
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
            actions: Vec::new(),
        }
    }

    fn names<'a>(entries: &'a [DesktopEntry], query: &str) -> Vec<&'a str> {
        names_used(entries, query, &[])
    }

    fn names_used<'a>(
        entries: &'a [DesktopEntry],
        query: &str,
        used: &[(&str, u32)],
    ) -> Vec<&'a str> {
        let usage = Usage {
            counts: used.iter().map(|(id, n)| (id.to_string(), *n)).collect(),
            path: None,
        };
        search(entries, query, &usage)
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

    #[test]
    fn usage_breaks_ties_only() {
        let all = [
            entry("org.gnome.terminal", "Terminal", None, false),
            entry("termite", "Termite", None, false),
            entry("kitty", "kitty", Some("A fast terminal emulator"), false),
            entry("term", "Zeta", None, false),
        ];
        // Exact, prefix, prefix, substring: usage doesn't reorder classes.
        assert_eq!(
            names_used(&all, "term", &[("kitty", 99)]),
            ["Zeta", "Terminal", "Termite", "kitty"]
        );
        // Within one, the launched entry comes first.
        assert_eq!(
            names_used(&all, "term", &[("termite", 1)]),
            ["Zeta", "Termite", "Terminal", "kitty"]
        );
        assert_eq!(
            names_used(&all, "", &[("kitty", 2), ("term", 5)]),
            ["Zeta", "kitty", "Terminal", "Termite"]
        );
    }

    /// A launcher over two entries in a temp dir, Zed with two
    /// actions and Ant without, no usage and an empty query.
    fn tree_launcher() -> Launcher {
        use crate::services::icons::desktop::DesktopDb;
        let dir = std::env::temp_dir().join(format!("aria-launcher-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("zed.desktop"),
            "[Desktop Entry]\nType=Application\nName=Zed\nExec=zed\nActions=a;b\n\
             [Desktop Action a]\nName=A\nExec=zed a\n[Desktop Action b]\nName=B\nExec=zed b\n",
        )
        .unwrap();
        fs::write(
            dir.join("ant.desktop"),
            "[Desktop Entry]\nType=Application\nName=Ant\nExec=ant\n",
        )
        .unwrap();
        let db = DesktopDb::load(std::slice::from_ref(&dir), &[]);
        fs::remove_dir_all(&dir).unwrap();
        let mut launcher = Launcher::new(
            LauncherConfig::from_raw(&RawSection::default()),
            ExiterConfig::from_raw(&RawSection::default()),
            None,
            Some(Arc::new(Index::from_apps(db))),
        );
        launcher.usage = Usage::default();
        launcher.search();
        launcher
    }

    #[test]
    fn actions_open_and_close_as_child_rows() {
        use Row::{Action, App};
        let mut l = tree_launcher();
        assert_eq!(l.rows(), [App(0), App(1)], "Ant, Zed");
        assert_eq!(l.selected_row(), Some(App(0)));
        // Right on an entry without actions: nothing.
        l.update(Message::Right);
        assert_eq!((l.rows(), l.selected), ([App(0), App(1)].to_vec(), 0));
        // Right on Zed opens its actions and selects the first.
        l.update(Message::Down);
        l.update(Message::Right);
        assert_eq!(l.rows(), [App(0), App(1), Action(1, 0), Action(1, 1)]);
        assert_eq!(l.selected_row(), Some(Action(1, 0)));
        assert_eq!(l.expanded, Some(1));
        l.update(Message::Down);
        assert_eq!(l.selected_row(), Some(Action(1, 1)));
        l.update(Message::Down);
        assert_eq!(l.selected_row(), Some(Action(1, 1)), "last row stays");
        // Left from an action: back on Zed, closed.
        l.update(Message::Left);
        assert_eq!(
            (l.rows(), l.selected_row()),
            ([App(0), App(1)].to_vec(), Some(App(1)))
        );
        // The chevron toggles; Left on the open entry closes it too.
        l.update(Message::Toggle(1));
        assert_eq!(l.selected_row(), Some(Action(1, 0)));
        l.update(Message::Up);
        assert_eq!(l.selected_row(), Some(App(1)));
        l.update(Message::Left);
        assert_eq!(l.rows().len(), 2);
        // Typing closes and selects the first row.
        l.update(Message::Toggle(1));
        l.update(Message::Query("z".into()));
        assert_eq!((l.rows(), l.selected), ([App(0)].to_vec(), 0));
        assert_eq!(l.expanded, None);
    }

    #[test]
    fn usage_round_trips() {
        let dir = std::env::temp_dir().join(format!("aria-usage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("state").join("launcher-usage");
        // No file (nor directory) yet: empty, and `bump` creates them.
        let mut usage = Usage::load_from(Some(path.clone()));
        assert_eq!(usage.count("firefox"), 0);
        usage.bump("firefox");
        usage.bump("firefox");
        usage.bump("kitty");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "firefox 2\nkitty 1\n",
            "most launched first"
        );
        let again = Usage::load_from(Some(path.clone()));
        assert_eq!((again.count("firefox"), again.count("kitty")), (2, 1));
        // Junk lines are skipped, the rest read.
        fs::write(&path, "garbage\nfirefox x\n\n  kitty 4 \n").unwrap();
        let junk = Usage::load_from(Some(path.clone()));
        assert_eq!((junk.count("firefox"), junk.count("kitty")), (0, 4));
        assert_eq!(Usage::load_from(None).count("kitty"), 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
