//! The toolkit's gallery: every widget in every state, in a plain iced
//! window, styled by the theme given on the command line:
//!
//!     cargo run -p tk --example gallery [-- theme.css]
//!
//! The theme is reloaded when its file changes; `d` toggles light and
//! dark. What a sheet skips is logged (`RUST_LOG=warn` and up).

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::advanced::{image, svg};
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::{Subscription, Task, keyboard, window};
use tk::{Element, IconFile, Node, Scheme, Sheet, Source, Theme};

/// The gallery's own elements.
const GALLERY: &str = include_str!("gallery.css");

fn main() -> iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    iced::application(Gallery::new, Gallery::update, Gallery::view)
        .title("tk gallery")
        .theme(Gallery::theme)
        .subscription(Gallery::subscription)
        .run()
}

struct Gallery {
    /// The theme given on the command line, absolute.
    theme: Option<PathBuf>,
    scheme: Scheme,
    sheet: Sheet,
    /// The window's device pixels per logical one.
    scale: f32,
    star: IconFile,
    picture: IconFile,
    /// A landscape, for `object-fit`.
    photo: image::Handle,
    /// The last button pressed, and how many times in a row.
    pressed: Option<(&'static str, u32)>,
    /// The box shakes until its animation ends.
    shaking: bool,
    /// The toast, shown: `true` while it leaves.
    toast: Option<bool>,
}

#[derive(Debug, Clone)]
enum Message {
    Key(keyboard::Event),
    /// The theme's file changed.
    Changed,
    Window(window::Id, window::Event),
    Scale(f32),
    Pressed(&'static str),
    Shake,
    Shaken,
    Toast,
    Dismiss,
    /// The toast's animation ended: in, or out.
    Toasted,
}

/// A symbolic icon: one colour, the element's.
const STAR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 1l2.1 4.5 4.9.6-3.6 3.4.9 4.9L8 12l-4.3 2.4.9-4.9L1 6.1l4.9-.6z" fill="#000"/></svg>"##;

/// A picture: a gradient, square.
fn picture() -> image::Handle {
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

/// Long enough to wrap, or to be cut.
const LONG: &str = "A text long enough to wrap at its box's width, \
    words kept whole, lines a whole number of device pixels apart.";

impl Gallery {
    fn new() -> Self {
        let theme = std::env::args_os()
            .nth(1)
            .map(|p| std::path::absolute(PathBuf::from(p)).expect("a path"));
        let scheme = Scheme::Light;
        let sheet = load(theme.as_deref(), scheme).unwrap_or_else(|e| {
            log::error!("{e}, using the base alone");
            load(None, scheme).expect("the base stylesheets are valid")
        });
        Self {
            theme,
            scheme,
            sheet,
            scale: 1.0,
            star: IconFile::Svg {
                handle: svg::Handle::from_memory(STAR.as_bytes()),
                symbolic: true,
            },
            picture: IconFile::Image(picture()),
            photo: photo(),
            pressed: None,
            shaking: false,
            toast: None,
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Key(keyboard::Event::KeyPressed {
                key: keyboard::Key::Character(c),
                ..
            }) if c == "d" => {
                self.scheme = self.scheme.toggled();
                self.reload();
            }
            Message::Key(_) => {}
            Message::Changed => self.reload(),
            Message::Window(id, window::Event::Opened { .. }) => {
                return window::scale_factor(id).map(Message::Scale);
            }
            Message::Window(_, window::Event::Rescaled(scale)) | Message::Scale(scale) => {
                self.scale = scale;
            }
            Message::Window(..) => {}
            Message::Shake => self.shaking = true,
            Message::Shaken => self.shaking = false,
            Message::Toast => self.toast = Some(false),
            Message::Dismiss => self.toast = self.toast.map(|_| true),
            Message::Toasted => {
                if self.toast == Some(true) {
                    self.toast = None;
                }
            }
            Message::Pressed(name) => {
                self.pressed = match self.pressed {
                    Some((last, n)) if last == name => Some((name, n + 1)),
                    _ => Some((name, 1)),
                }
            }
        }
        Task::none()
    }

    fn reload(&mut self) {
        match load(self.theme.as_deref(), self.scheme) {
            Ok(sheet) => self.sheet = sheet,
            Err(e) => log::error!("{e}, keeping the last good sheet"),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let root = self.sheet.root("gallery", self.scale);
        tk::focus_scope(tk::column(
            root,
            [
                self.buttons(root),
                self.animation(root),
                self.boxes(root),
                self.painting(root),
                self.images(root),
                self.texts(root),
                self.flex(root),
                self.icons(root),
            ],
        ))
        .into()
    }

    fn buttons<'a>(&'a self, root: Node<'a>) -> Element<'a, Message> {
        let section = root.child("section");
        let row = section.child("row");
        let button = |class: &str, label: &'static str, disabled: bool| -> Element<'a, Message> {
            let node = row
                .child("button")
                .class_if(class, !class.is_empty())
                .disabled_if(disabled);
            tk::button(
                node,
                [
                    tk::icon(node.child("icon"), self.star.clone()).into(),
                    tk::text(node.child("label"), label).into(),
                ],
            )
            .on_press(Message::Pressed(label))
            .into()
        };
        let pressed = match self.pressed {
            None => "Nothing pressed yet: click, or Tab then Enter".to_string(),
            Some((name, 1)) => format!("Pressed: {name}"),
            Some((name, n)) => format!("Pressed: {name}, {n} times"),
        };
        let card = row.child("card");
        let grow = row.child("grow");
        let content = tk::column(
            section.child("buttons"),
            [
                tk::row(
                    row,
                    [
                        button("", "Default", false),
                        button("flat", "Flat", false),
                        button("primary", "Primary", false),
                        button("danger", "Danger", false),
                        button("grow", "Grows", false),
                        button("", "Disabled", true),
                    ],
                )
                .into(),
                tk::text(section.child("buttons").child("status"), pressed).into(),
                tk::row(
                    row,
                    [
                        tk::row(
                            card,
                            [
                                tk::icon(card.child("icon"), self.star.clone()).into(),
                                tk::text(
                                    card.child("label"),
                                    "Hover the card: its icon and label follow",
                                )
                                .into(),
                            ],
                        )
                        .into(),
                        tk::row(
                            grow,
                            [tk::text(grow.child("label"), "Grows on hover").into()],
                        )
                        .into(),
                    ],
                )
                .into(),
            ],
        );
        self.section(section, "Buttons and states", content.into())
    }

    fn section<'a>(
        &self,
        section: Node<'a>,
        title: &'static str,
        content: Element<'a, Message>,
    ) -> Element<'a, Message> {
        tk::column(
            section,
            [tk::text(section.child("heading"), title).into(), content],
        )
        .into()
    }

    fn animation<'a>(&'a self, root: Node<'a>) -> Element<'a, Message> {
        let section = root.child("section");
        let row = section.child("row");
        let labelled = |node: Node<'a>, text: &'static str| -> Element<'a, Message> {
            tk::text(node.child("label"), text).into()
        };
        let pulse = row.child("pulse");
        let shaken = row
            .child("box")
            .class("shake")
            .class_if("shaking", self.shaking);
        let shake = row.child("button");
        let toggle = row.child("button");
        let mut items: Vec<Element<'a, Message>> = vec![
            tk::icon(row.child("spinner"), self.star.clone()).into(),
            tk::row(pulse, [labelled(pulse, "Pulse")]).into(),
            tk::row(shaken, [labelled(shaken, "Shaken on demand")])
                .on_animation_end(Message::Shaken)
                .into(),
            tk::button(shake, [labelled(shake, "Shake")])
                .on_press(Message::Shake)
                .into(),
            tk::button(
                toggle,
                [labelled(
                    toggle,
                    if self.toast.is_some() {
                        "Dismiss"
                    } else {
                        "Toast"
                    },
                )],
            )
            .on_press(if self.toast.is_some() {
                Message::Dismiss
            } else {
                Message::Toast
            })
            .into(),
        ];
        if let Some(leaving) = self.toast {
            let toast = row.child("toast").class_if("leaving", leaving);
            items.push(
                tk::row(
                    toast,
                    [
                        tk::icon(toast.child("icon"), self.star.clone()).into(),
                        labelled(toast, "A toast: in, and out when dismissed"),
                    ],
                )
                .on_animation_end(Message::Toasted)
                .into(),
            );
        }
        self.section(section, "Animation", tk::row(row, items).into())
    }

    fn boxes<'a>(&self, root: Node<'a>) -> Element<'a, Message> {
        let section = root.child("section");
        let row = section.child("row");
        let boxed = |class: &str, label: &'static str| -> Element<'a, Message> {
            let node = row.child("box").class(class);
            tk::row(node, [tk::text(node.child("label"), label).into()]).into()
        };
        let content = tk::row(
            row,
            [
                boxed("plain", "Plain"),
                boxed("bordered", "Border"),
                boxed("rounded", "Radius"),
                boxed("shadow", "Shadow"),
                boxed("inset", "Inset"),
                boxed("sides", "Sides"),
            ],
        );
        self.section(section, "Boxes", content.into())
    }

    fn painting<'a>(&'a self, root: Node<'a>) -> Element<'a, Message> {
        let section = root.child("section");
        let row = section.child("row");
        let boxed = |class: &str, label: &'static str| -> Element<'a, Message> {
            let node = row.child("box").class(class);
            tk::row(node, [tk::text(node.child("label"), label).into()]).into()
        };
        let faded = row.child("box").class("faded");
        let aligned = section.child("aligned");
        let line = |class: &str, text: &'static str| -> Element<'a, Message> {
            tk::text(aligned.child("label").class(class), text).into()
        };
        let content = tk::column(
            section.child("painting"),
            [
                tk::row(
                    row,
                    [
                        boxed("tab", "Tab"),
                        boxed("four", "Four sides"),
                        boxed("ring", "Ring"),
                        boxed("outlined", "Outline"),
                        boxed("scaled", "Scaled 80%"),
                        boxed("moved", "Moved"),
                        tk::row(
                            faded,
                            [
                                tk::icon(faded.child("icon"), self.star.clone()).into(),
                                tk::text(faded.child("label"), "Opacity 50%").into(),
                            ],
                        )
                        .into(),
                    ],
                )
                .into(),
                tk::column(
                    aligned,
                    [
                        line("left", "Aligned left"),
                        line("center", "Centred"),
                        line("right", "Aligned right"),
                        line("italic", "Italic, as the theme says"),
                    ],
                )
                .into(),
            ],
        );
        self.section(section, "Painting and text", content.into())
    }

    fn images<'a>(&'a self, root: Node<'a>) -> Element<'a, Message> {
        let section = root.child("section");
        let row = section.child("row");
        let boxed = |class: &str, label: &'static str| -> Element<'a, Message> {
            let node = row.child("box").class(class);
            tk::row(node, [tk::text(node.child("label"), label).into()]).into()
        };
        let fitted = |class: &str| -> Element<'a, Message> {
            tk::image(row.child("picture").class(class), self.photo.clone()).into()
        };
        let content = tk::row(
            row,
            [
                boxed("textured", "Tiled"),
                boxed("framed", "9-patch"),
                boxed("scenery", "Cover"),
                fitted("fill"),
                fitted("contain"),
                fitted("cover"),
            ],
        );
        self.section(section, "Images", content.into())
    }

    fn texts<'a>(&'a self, root: Node<'a>) -> Element<'a, Message> {
        let section = root.child("section");
        let texts = section.child("texts");
        let label = |class: &str, text: &'static str| -> Element<'a, Message> {
            tk::text(texts.child("label").class(class), text).into()
        };
        let boxed = |class: &str| -> Element<'a, Message> {
            let node = texts.child("box").class(class);
            tk::row(node, [tk::text(node.child("label"), LONG).into()]).into()
        };
        let pair = |untrimmed: bool, text: &'static str| -> Element<'a, Message> {
            let node = texts.child("pair").class_if("untrimmed", untrimmed);
            tk::row(
                node,
                [
                    tk::icon(node.child("icon"), self.star.clone()).into(),
                    tk::text(node.child("label"), text).into(),
                ],
            )
            .into()
        };
        let content = tk::column(
            texts,
            [
                label("small", "Small, 12px"),
                label("regular", "Regular: the root's 14px"),
                label("large", "Large, 20px"),
                label("bold", "Bold"),
                label("muted", "Muted"),
                label("caps", "Upper case, by the theme"),
                boxed("wrap"),
                boxed("cut"),
                pair(false, "Trimmed: Centred On Its Letters"),
                pair(true, "Untrimmed: Centred On Its Line Box"),
            ],
        );
        self.section(section, "Text", content.into())
    }

    fn flex<'a>(&'a self, root: Node<'a>) -> Element<'a, Message> {
        let section = root.child("section");
        let flex = section.child("flex");
        let track = |class: &str, cells: &[&'static str]| -> Element<'a, Message> {
            let node = flex.child("track").class(class);
            tk::row(
                node,
                cells.iter().map(|&cell| {
                    let cell_node = node.child("cell").class(cell);
                    tk::row(cell_node, [tk::text(cell_node.child("label"), cell).into()]).into()
                }),
            )
            .into()
        };
        let menu = flex.child("menu");
        let entries = [
            ("Open", "Ctrl+O"),
            ("A label far too long for its menu, cut short", "Ctrl+L"),
            ("Save", "Ctrl+S"),
        ];
        let menu = tk::column(
            menu,
            entries.iter().map(|&(label, shortcut)| {
                let item = menu.child("item");
                tk::row(
                    item,
                    [
                        tk::icon(item.child("icon"), self.star.clone()).into(),
                        tk::text(item.child("label"), label).into(),
                        tk::text(item.child("shortcut"), shortcut).into(),
                    ],
                )
                .into()
            }),
        );
        let content = tk::column(
            flex,
            [
                track("plain", &["fill", "fixed", "fill"]),
                track("center", &["fixed", "fixed"]),
                track("end", &["fixed", "fixed"]),
                track("align-center", &["tall", "mid", "short"]),
                menu.into(),
            ],
        );
        self.section(section, "Flex", content.into())
    }

    fn icons<'a>(&'a self, root: Node<'a>) -> Element<'a, Message> {
        let section = root.child("section");
        let icons = section.child("icons");
        let content = tk::row(
            icons,
            [
                tk::icon(icons.child("icon").class("small"), self.star.clone()).into(),
                tk::icon(icons.child("icon"), self.star.clone()).into(),
                tk::icon(
                    icons.child("icon").class("large").class("accent"),
                    self.star.clone(),
                )
                .into(),
                tk::icon(icons.child("avatar"), self.picture.clone()).into(),
            ],
        );
        self.section(section, "Icons", content.into())
    }

    fn theme(&self) -> Theme {
        let root = self.sheet.root("gallery", self.scale).base();
        Theme::window(root.background, root.color)
    }

    fn subscription(&self) -> Subscription<Message> {
        let keys = keyboard::listen().map(Message::Key);
        let windows = window::events().map(|(id, event)| Message::Window(id, event));
        match &self.theme {
            Some(path) => Subscription::batch([keys, windows, watch(path.clone())]),
            None => Subscription::batch([keys, windows]),
        }
    }
}

/// The toolkit's base, the gallery's, then `theme` if any, for `scheme`.
fn load(theme: Option<&Path>, scheme: Scheme) -> Result<Sheet, String> {
    let name = theme.map(|p| p.display().to_string());
    let text = match theme {
        Some(path) => {
            Some(std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?)
        }
        None => None,
    };
    let mut sources = vec![Source {
        name: "gallery.css",
        text: GALLERY,
        dir: Some(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/examples"))),
    }];
    if let (Some(name), Some(text)) = (&name, &text) {
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
        name.as_deref().unwrap_or("no theme"),
        scheme.name(),
        sheet.stats().rules
    );
    Ok(sheet)
}

/// A message when the file at `path` changes: its directory is watched,
/// as editors save by renaming a new file over the old one.
fn watch(path: PathBuf) -> Subscription<Message> {
    Subscription::run_with(path, |path| changes(path.clone()))
}

fn changes(path: PathBuf) -> impl Stream<Item = Message> {
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
            Err(e) => return log::error!("cannot watch {}: {e}", path.display()),
        };
        let dir = path.parent().unwrap_or(Path::new("/"));
        if let Err(e) =
            notify::Watcher::watch(&mut watcher, dir, notify::RecursiveMode::NonRecursive)
        {
            return log::error!("cannot watch {}: {e}", dir.display());
        }
        // Reading the file is an event too: only writes count.
        let touched = |event: &notify::Event| {
            matches!(
                event.kind,
                notify::EventKind::Create(_)
                    | notify::EventKind::Modify(_)
                    | notify::EventKind::Remove(_)
            ) && event.paths.contains(&path)
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
