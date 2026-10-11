//! The toolkit's demo, after gtk4-demo: the demos in a list, each shown
//! live with its description, its Rust and its CSS.
//!
//!     cargo run -p icssed --example demo [-- theme.css]
//!
//! A demo is a module of this directory and a stylesheet beside it
//! (`buttons.rs`, `buttons.css`): its `//!` comment is its description,
//! the file itself its source. The stylesheets and the theme are reloaded
//! when their files change. What a sheet skips is logged (`RUST_LOG=warn`
//! and up).

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::advanced::{image, svg};
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::{Subscription, Task, window};
use icssed::{Element, IconFile, Node, Scheme, Sheet, Source, Tab, Theme};

/// Every demo, by group, in the sidebar's order: a module of this
/// directory, with `TITLE`, `State`, `Message`, `update` and `view`, and
/// its stylesheet beside it.
macro_rules! demos {
    ($($group:literal => [$($name:ident),* $(,)?]),* $(,)?) => {
        $($(mod $name;)*)*

        /// A demo's message, tagged with the demo.
        #[allow(non_camel_case_types)]
        #[derive(Debug, Clone)]
        enum DemoMessage {
            $($($name($name::Message),)*)*
        }

        /// Every demo's state, kept while another is shown.
        #[derive(Default)]
        struct Demos {
            $($($name: $name::State,)*)*
        }

        const DEMOS: &[Entry] = &[$($(Entry {
            name: stringify!($name),
            title: $name::TITLE,
            group: $group,
            source: include_str!(concat!(stringify!($name), ".rs")),
            style: include_str!(concat!(stringify!($name), ".css")),
        },)*)*];

        impl Demos {
            fn update(&mut self, message: DemoMessage) {
                match message {
                    $($(DemoMessage::$name(message) => $name::update(&mut self.$name, message),)*)*
                }
            }

            fn view<'a>(
                &'a self,
                name: &str,
                node: Node<'a>,
                assets: &'a Assets,
            ) -> Element<'a, DemoMessage> {
                match name {
                    $($(stringify!($name) => {
                        $name::view(&self.$name, node, assets).map(DemoMessage::$name)
                    })*)*
                    _ => unreachable!("{name} is no demo"),
                }
            }
        }
    };
}

demos! {
    "Widgets" => [
        buttons, toggle, slider, meter, input, select, menu, tabs, calendar, graph, tooltip,
        scroll,
    ],
    "Content" => [text, icon, images],
    "Layout" => [boxes, flex, surface],
    "Styling" => [nodes, palette, states, animation],
}

/// A demo, as the sidebar and the tabs show it.
struct Entry {
    /// Its module, and its node's id: `demo#buttons`.
    name: &'static str,
    title: &'static str,
    group: &'static str,
    /// Its module's file, as built.
    source: &'static str,
    /// Its stylesheet, as built: shown and loaded until the file is read.
    style: &'static str,
}

/// What the demos share: icons and pictures an app would load its own way.
pub struct Assets {
    /// A symbolic star: one colour, the element's.
    pub star: IconFile,
    /// An eye, to peek at a password.
    pub eye: IconFile,
    /// A picture: a square gradient.
    pub picture: IconFile,
    /// A landscape, for `object-fit`.
    pub photo: image::Handle,
    /// The star and the eye by name, as an app's icon theme answers.
    pub named: icon::Named,
}

/// The demo's frame: the sidebar, the header, the tabs.
const FRAME: &str = include_str!("demo.css");

/// Where the stylesheets are read from, and watched.
const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/demo");

fn main() -> iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    iced::application(App::new, App::update, App::view)
        .title("icssed demo")
        .theme(App::theme)
        .subscription(App::subscription)
        .run()
}

/// The tabs under a demo.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Pane {
    Info,
    Source,
    Style,
}

impl Pane {
    const ALL: [Pane; 3] = [Pane::Info, Pane::Source, Pane::Style];

    fn title(self) -> &'static str {
        match self {
            Pane::Info => "Info",
            Pane::Source => "Source",
            Pane::Style => "Style",
        }
    }
}

struct App {
    /// The theme given on the command line, absolute.
    theme: Option<PathBuf>,
    scheme: Scheme,
    sheet: Sheet,
    /// The frame's stylesheet as last read.
    frame: String,
    /// Every demo's stylesheet as last read, in `DEMOS`' order.
    styles: Vec<String>,
    /// The window's device pixels per logical one.
    scale: f32,
    assets: Assets,
    demos: Demos,
    /// The demo shown, an index in `DEMOS`.
    shown: usize,
    pane: Pane,
    search: String,
}

#[derive(Debug, Clone)]
enum Message {
    /// A stylesheet or the theme changed on disk.
    Changed,
    Window(window::Id, window::Event),
    Scale(f32),
    Show(usize),
    Pane(Pane),
    Search(String),
    Dark(bool),
    Demo(DemoMessage),
}

impl App {
    fn new() -> Self {
        let theme = std::env::args_os()
            .nth(1)
            .map(|p| std::path::absolute(PathBuf::from(p)).expect("a path"));
        let scheme = Scheme::Light;
        let frame = read("demo", FRAME);
        let styles = read_styles();
        let sheet = load(&frame, &styles, theme.as_deref(), scheme).unwrap_or_else(|e| {
            log::error!("{e}, using the stylesheets as built");
            let built = DEMOS
                .iter()
                .map(|d| d.style.to_string())
                .collect::<Vec<_>>();
            load(FRAME, &built, None, scheme).expect("the built stylesheets are valid")
        });
        Self {
            theme,
            scheme,
            sheet,
            frame,
            styles,
            scale: 1.0,
            assets: Assets::new(),
            demos: Demos::default(),
            shown: 0,
            pane: Pane::Info,
            search: String::new(),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Changed => self.reload(),
            Message::Window(id, window::Event::Opened { .. }) => {
                return window::scale_factor(id).map(Message::Scale);
            }
            Message::Window(_, window::Event::Rescaled(scale)) | Message::Scale(scale) => {
                self.scale = scale;
            }
            Message::Window(..) => {}
            Message::Show(index) => self.shown = index,
            Message::Pane(pane) => self.pane = pane,
            Message::Search(text) => self.search = text,
            Message::Dark(dark) => {
                self.scheme = if dark { Scheme::Dark } else { Scheme::Light };
                self.reload();
            }
            Message::Demo(message) => self.demos.update(message),
        }
        Task::none()
    }

    /// The stylesheets read again, the sheet compiled again; a syntax
    /// error keeps the last good one.
    fn reload(&mut self) {
        self.frame = read("demo", FRAME);
        self.styles = read_styles();
        match load(
            &self.frame,
            &self.styles,
            self.theme.as_deref(),
            self.scheme,
        ) {
            Ok(sheet) => self.sheet = sheet,
            Err(e) => log::error!("{e}, keeping the last good sheet"),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let root = self.sheet.root("app", self.scale);
        icssed::focus_scope(icssed::row(root, [self.sidebar(root), self.main(root)])).into()
    }

    /// The search, then the demos that match it, by group.
    fn sidebar<'a>(&'a self, root: Node<'a>) -> Element<'a, Message> {
        let sidebar = root.child("sidebar");
        let scroll = sidebar.child("scroll");
        let entries = scroll.child("entries");
        let wanted = self.search.to_lowercase();
        let mut items: Vec<Element<'a, Message>> = Vec::new();
        let mut group = "";
        for (index, demo) in DEMOS.iter().enumerate() {
            if !demo.title.to_lowercase().contains(&wanted) {
                continue;
            }
            if demo.group != group {
                group = demo.group;
                let node = entries.child("group");
                items.push(
                    icssed::row(node, [icssed::text(node.child("label"), group).into()]).into(),
                );
            }
            let entry = entries
                .child("entry")
                .class_if("active", index == self.shown);
            items.push(
                icssed::button(
                    entry,
                    [icssed::text(entry.child("label"), demo.title).into()],
                )
                .on_press(Message::Show(index))
                .into(),
            );
        }
        let search = sidebar.child("input");
        icssed::column(
            sidebar,
            [
                icssed::input(search, "Search", &self.search)
                    .on_input(Message::Search)
                    .leading(icssed::icon(search.child("icon"), self.assets.star.clone()))
                    .into(),
                icssed::scroll(scroll, icssed::column(entries, items)).into(),
            ],
        )
        .into()
    }

    /// The demo shown: its header, the demo itself, its tabs.
    fn main<'a>(&'a self, root: Node<'a>) -> Element<'a, Message> {
        let demo = &DEMOS[self.shown];
        let main = root.child("main");
        let header = main.child("header");
        let dark = header.child("toggle");
        let stage = main.child("stage");
        let tabs = main.child("tabs");
        icssed::column(
            main,
            [
                icssed::row(
                    header,
                    [
                        icssed::text(header.child("title"), demo.title).into(),
                        icssed::text(header.child("label"), "Dark").into(),
                        icssed::toggle(dark, self.scheme == Scheme::Dark, Message::Dark)
                            .switch()
                            .into(),
                    ],
                )
                .into(),
                icssed::row(
                    stage,
                    [self
                        .demos
                        .view(demo.name, stage.child("demo").id(demo.name), &self.assets)
                        .map(Message::Demo)],
                )
                .into(),
                icssed::tabs(
                    tabs,
                    Pane::ALL.map(|pane| {
                        Tab::new(pane.title(), Message::Pane(pane)).active(pane == self.pane)
                    }),
                ),
                self.pane(main),
            ],
        )
        .into()
    }

    /// The tab shown: the description, the source or the stylesheet.
    fn pane<'a>(&'a self, main: Node<'a>) -> Element<'a, Message> {
        let demo = &DEMOS[self.shown];
        match self.pane {
            Pane::Info => {
                let scroll = main.child("scroll").class("info");
                icssed::scroll(scroll, doc(scroll.child("doc"), demo.source)).into()
            }
            Pane::Source => listing(main, demo.source),
            Pane::Style => listing(main, &self.styles[self.shown]),
        }
    }

    fn theme(&self) -> Theme {
        let root = self.sheet.root("app", self.scale).base();
        Theme::window(root.background, root.color)
    }

    fn subscription(&self) -> Subscription<Message> {
        let windows = window::events().map(|(id, event)| Message::Window(id, event));
        let mut watched = vec![windows, watch(PathBuf::from(DIR), None)];
        if let Some(path) = &self.theme {
            let dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
            watched.push(watch(dir, Some(path.clone())));
        }
        Subscription::batch(watched)
    }
}

/// A file's text, lines kept, scrolled both ways.
fn listing<'a>(main: Node<'a>, text: &'a str) -> Element<'a, Message> {
    let scroll = main.child("scroll").class("listing");
    let code = scroll.child("code");
    icssed::scroll(
        scroll,
        icssed::column(code, [icssed::text(code.child("text"), text).into()]),
    )
    .both()
    .into()
}

/// A module's `//!` comment as paragraphs, bullets and code: a blank
/// line ends a paragraph, `- ` starts a bullet, four spaces a code line.
/// Backticks are dropped: a text has one font.
fn doc<'a>(node: Node<'a>, source: &'a str) -> Element<'a, Message> {
    let lines = source
        .lines()
        .map_while(|line| line.strip_prefix("//!"))
        .map(|line| line.strip_prefix(' ').unwrap_or(line));
    let mut blocks: Vec<(&str, String)> = Vec::new();
    for line in lines {
        let (kind, text) = if line.trim().is_empty() {
            ("", "")
        } else if let Some(code) = line.strip_prefix("    ") {
            ("code", code)
        } else if let Some(bullet) = line.strip_prefix("- ") {
            ("bullet", bullet)
        } else {
            ("para", line)
        };
        match blocks.last_mut() {
            // A bullet's next lines are indented two spaces: its own.
            Some(("bullet", last)) if kind == "para" && line.starts_with("  ") => {
                last.push(' ');
                last.push_str(text.trim());
            }
            Some(("code", last)) if kind == "code" => {
                last.push('\n');
                last.push_str(text);
            }
            Some(("para", last)) if kind == "para" => {
                last.push(' ');
                last.push_str(text);
            }
            _ if kind.is_empty() => {}
            _ => blocks.push((kind, text.to_string())),
        }
    }
    // Bullets in a row are one list.
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    let mut list: Vec<Element<'a, Message>> = Vec::new();
    let list_node = node.child("list");
    for (kind, text) in blocks {
        if kind != "bullet" && !list.is_empty() {
            items.push(icssed::column(list_node, std::mem::take(&mut list)).into());
        }
        match kind {
            "bullet" => {
                let bullet = list_node.child("bullet");
                list.push(
                    icssed::row(
                        bullet,
                        [
                            icssed::mark(bullet.child("mark")),
                            icssed::text(bullet.child("text"), text.replace('`', "")).into(),
                        ],
                    )
                    .into(),
                );
            }
            "code" => {
                let code = node.child("code");
                items.push(
                    icssed::column(code, [icssed::text(code.child("text"), text).into()]).into(),
                );
            }
            _ => items.push(icssed::text(node.child("para"), text.replace('`', "")).into()),
        }
    }
    if !list.is_empty() {
        items.push(icssed::column(list_node, list).into());
    }
    icssed::column(node, items).into()
}

impl Assets {
    fn new() -> Self {
        const STAR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 1l2.1 4.5 4.9.6-3.6 3.4.9 4.9L8 12l-4.3 2.4.9-4.9L1 6.1l4.9-.6z" fill="#000"/></svg>"##;
        const EYE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 3C4.4 3 1.7 5.6 1 8c.7 2.4 3.4 5 7 5s6.3-2.6 7-5c-.7-2.4-3.4-5-7-5zm0 8.2A3.2 3.2 0 1 1 8 4.8a3.2 3.2 0 0 1 0 6.4zM8 6.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z" fill="#000"/></svg>"##;
        let symbolic = |svg: &'static str| IconFile::Svg {
            handle: svg::Handle::from_memory(svg.as_bytes()),
            symbolic: true,
        };
        Self {
            star: symbolic(STAR),
            eye: symbolic(EYE),
            picture: IconFile::Image(gradient()),
            photo: photo(),
            named: icon::Named {
                star: symbolic(STAR),
                eye: symbolic(EYE),
            },
        }
    }
}

/// A square gradient, made in memory.
fn gradient() -> image::Handle {
    const SIDE: u32 = 64;
    let mut rgba = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            rgba.extend([(x * 4) as u8, (y * 4) as u8, 200, 255]);
        }
    }
    image::Handle::from_rgba(SIDE, SIDE, rgba)
}

/// The landscape of the examples' assets, decoded as an app would.
fn photo() -> image::Handle {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/assets/photo.png");
    let photo = ::image::open(path)
        .expect("the photo is there")
        .into_rgba8();
    let (width, height) = photo.dimensions();
    image::Handle::from_rgba(width, height, photo.into_raw())
}

/// Every demo's stylesheet, in `DEMOS`' order.
fn read_styles() -> Vec<String> {
    DEMOS
        .iter()
        .map(|demo| read(demo.name, demo.style))
        .collect()
}

/// The stylesheet `<name>.css` from its file, or as built if it can't be
/// read.
fn read(name: &str, built: &str) -> String {
    let path = Path::new(DIR).join(format!("{name}.css"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        log::warn!("{}: {e}, using it as built", path.display());
        built.to_string()
    })
}

/// The toolkit's base, the frame's, every demo's, then `theme` if any,
/// for `scheme`.
fn load(
    frame: &str,
    styles: &[String],
    theme: Option<&Path>,
    scheme: Scheme,
) -> Result<Sheet, String> {
    let theme_name = theme.map(|p| p.display().to_string());
    let theme_text = match theme {
        Some(path) => {
            Some(std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?)
        }
        None => None,
    };
    let names: Vec<String> = DEMOS.iter().map(|d| format!("{}.css", d.name)).collect();
    let dir = Some(Path::new(DIR));
    let mut sources = vec![Source {
        name: "demo.css",
        text: frame,
        dir,
    }];
    sources.extend(
        names
            .iter()
            .zip(styles)
            .map(|(name, text)| Source { name, text, dir }),
    );
    if let (Some(name), Some(text)) = (&theme_name, &theme_text) {
        sources.push(Source {
            name,
            text,
            dir: theme.and_then(Path::parent),
        });
    }
    let (sheet, warnings) = Sheet::load(&sources, scheme).map_err(|e| e.to_string())?;
    for warning in &warnings {
        log::warn!("{warning}");
    }
    log::info!(
        "{} ({}): {} rules",
        theme_name.as_deref().unwrap_or("no theme"),
        scheme.name(),
        sheet.stats().rules
    );
    Ok(sheet)
}

/// A message when a stylesheet in `dir` changes, or `file` alone if
/// given: the directory is watched, as editors save by renaming a new
/// file over the old one.
fn watch(dir: PathBuf, file: Option<PathBuf>) -> Subscription<Message> {
    Subscription::run_with((dir, file), |(dir, file)| {
        changes(dir.clone(), file.clone())
    })
}

fn changes(dir: PathBuf, file: Option<PathBuf>) -> impl Stream<Item = Message> {
    // A save is a burst of events: reported once it goes quiet.
    const SETTLE: Duration = Duration::from_millis(100);
    iced::stream::channel(4, async move |mut output| {
        let (tx, mut rx) = mpsc::unbounded::<notify::Event>();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                let _ = tx.unbounded_send(event);
            }
        });
        let mut watcher = match watcher {
            Ok(w) => w,
            Err(e) => return log::error!("cannot watch {}: {e}", dir.display()),
        };
        if let Err(e) =
            notify::Watcher::watch(&mut watcher, &dir, notify::RecursiveMode::NonRecursive)
        {
            return log::error!("cannot watch {}: {e}", dir.display());
        }
        // Reading a file is an event too: only writes count.
        let touched = |event: &notify::Event| {
            matches!(
                event.kind,
                notify::EventKind::Create(_)
                    | notify::EventKind::Modify(_)
                    | notify::EventKind::Remove(_)
            ) && event.paths.iter().any(|path| match &file {
                Some(file) => path == file,
                None => path.extension().is_some_and(|e| e == "css"),
            })
        };
        while let Some(event) = rx.next().await {
            if !touched(&event) {
                continue;
            }
            loop {
                tokio::time::sleep(SETTLE).await;
                let mut more = false;
                while rx.try_recv().is_ok() {
                    more = true;
                }
                if !more {
                    break;
                }
            }
            if output.send(Message::Changed).await.is_err() {
                break;
            }
        }
    })
}
