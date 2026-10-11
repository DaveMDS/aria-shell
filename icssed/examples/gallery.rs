//! The toolkit's gallery: every widget in every state, in a plain iced
//! window, styled by the theme given on the command line:
//!
//!     cargo run -p icssed --example gallery [-- theme.css]
//!
//! The theme is reloaded when its file changes; `d` toggles light and
//! dark. What a sheet skips is logged (`RUST_LOG=warn` and up).

use std::path::{Path, PathBuf};
use std::time::Duration;

use iced::advanced::{image, svg};
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::{Subscription, Task, keyboard, window};
use icssed::{Element, IconFile, MenuItem, Month, Scheme, Sheet, Source, Tab, Theme};

/// The gallery's own elements.
const GALLERY: &str = include_str!("gallery.css");

fn main() -> iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    iced::application(Gallery::new, Gallery::update, Gallery::view)
        .title("icssed gallery")
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
    wifi: bool,
    airplane: bool,
    /// 0 to 100.
    volume: f32,
    search: String,
    password: String,
    /// The password shown.
    peek: bool,
    /// The menu's last item picked.
    picked: Option<&'static str>,
    hidden: bool,
    by_date: bool,
    /// The menu's submenu, unfolded.
    more: bool,
    /// The tab shown.
    tab: usize,
    /// The select's choice.
    choice: Option<usize>,
    /// The calendar's month, today, the day picked.
    month: Month,
    today: (i32, u32, u32),
    day: Option<(i32, u32, u32)>,
    eye: IconFile,
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
    Wifi(bool),
    Airplane(bool),
    Volume(f32),
    Search(String),
    Password(String),
    Peek,
    Picked(&'static str),
    Hidden,
    ByDate(bool),
    More,
    Tab(usize),
    Choose(usize),
    PreviousMonth,
    NextMonth,
    Day(u32),
}

/// A symbolic icon: one colour, the element's.
const STAR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 1l2.1 4.5 4.9.6-3.6 3.4.9 4.9L8 12l-4.3 2.4.9-4.9L1 6.1l4.9-.6z" fill="#000"/></svg>"##;

/// An eye, to peek at a password.
const EYE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 3C4.4 3 1.7 5.6 1 8c.7 2.4 3.4 5 7 5s6.3-2.6 7-5c-.7-2.4-3.4-5-7-5zm0 8.2A3.2 3.2 0 1 1 8 4.8a3.2 3.2 0 0 1 0 6.4zM8 6.5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z" fill="#000"/></svg>"##;

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
        let today = today();
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
            wifi: true,
            airplane: false,
            volume: 40.0,
            search: String::new(),
            password: "hunter2".into(),
            peek: false,
            picked: None,
            hidden: false,
            by_date: false,
            more: false,
            tab: 0,
            choice: None,
            month: Month::new(today.0, today.1),
            today,
            day: None,
            eye: IconFile::Svg {
                handle: svg::Handle::from_memory(EYE.as_bytes()),
                symbolic: true,
            },
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
            Message::Wifi(on) => self.wifi = on,
            Message::Airplane(on) => self.airplane = on,
            Message::Volume(volume) => self.volume = volume,
            Message::Search(text) => self.search = text,
            Message::Password(text) => self.password = text,
            Message::Peek => self.peek = !self.peek,
            Message::Picked(label) => self.picked = Some(label),
            Message::Hidden => self.hidden = !self.hidden,
            Message::ByDate(by_date) => self.by_date = by_date,
            Message::More => self.more = !self.more,
            Message::Tab(tab) => self.tab = tab,
            Message::Choose(choice) => self.choice = Some(choice),
            Message::PreviousMonth => self.month = self.month.previous(),
            Message::NextMonth => self.month = self.month.next(),
            Message::Day(day) => self.day = Some((self.month.year, self.month.month, day)),
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

    fn view<'a>(&'a self) -> Element<'a, Message> {
        let page = icssed::column([
            self.buttons(),
            self.controls(),
            self.animation(),
            self.boxes(),
            self.painting(),
            self.images(),
            self.texts(),
            self.flex(),
            self.menus(),
            self.tabs(),
            self.calendar(),
            self.graphs(),
            self.scrolls(),
            self.icons(),
        ])
        .kind("page");
        icssed::focus_scope(
            icssed::column![icssed::scroll(page)].at(self.sheet.root("gallery", self.scale)),
        )
        .into()
    }

    /// A section: its title over `content`.
    fn section<'a>(
        &self,
        title: &'static str,
        content: Element<'a, Message>,
    ) -> Element<'a, Message> {
        icssed::column([icssed::text(title).kind("heading").into(), content])
            .kind("section")
            .into()
    }

    /// A box with a label in it.
    fn boxed<'a>(class: &str, label: &'a str) -> Element<'a, Message> {
        icssed::row![icssed::text(label)]
            .kind("box")
            .class(class)
            .into()
    }

    fn buttons<'a>(&'a self) -> Element<'a, Message> {
        let button = |class: &str, label: &'static str, disabled: bool| -> Element<'a, Message> {
            icssed::button![icssed::icon(self.star.clone()), icssed::text(label)]
                .class_if(class, !class.is_empty())
                .disabled_if(disabled)
                .on_press(Message::Pressed(label))
                .into()
        };
        let pressed = match self.pressed {
            None => "Nothing pressed yet: click, or Tab then Enter".to_string(),
            Some((name, 1)) => format!("Pressed: {name}"),
            Some((name, n)) => format!("Pressed: {name}, {n} times"),
        };
        let content = icssed::column![
            icssed::row([
                button("", "Default", false),
                button("flat", "Flat", false),
                button("primary", "Primary", false),
                button("danger", "Danger", false),
                button("grow", "Grows", false),
                button("", "Disabled", true),
            ]),
            icssed::text(pressed).kind("status"),
            icssed::row![
                icssed::row![
                    icssed::icon(self.star.clone()),
                    icssed::text("Hover the card: its icon and label follow"),
                ]
                .kind("card"),
                icssed::row![icssed::text("Grows on hover")].kind("grow"),
            ],
        ]
        .kind("buttons");
        self.section("Buttons and states", content.into())
    }

    fn controls<'a>(&'a self) -> Element<'a, Message> {
        let content = icssed::column![
            icssed::row![
                icssed::toggle(self.wifi, Message::Wifi).switch(),
                icssed::text("Wi-Fi"),
                icssed::toggle(self.airplane, Message::Airplane).switch(),
                icssed::text("Airplane mode"),
                icssed::toggle(true, Message::Wifi)
                    .disabled_if(true)
                    .switch(),
                icssed::text("Disabled"),
            ]
            .kind("line"),
            icssed::row![
                icssed::slider(0.0..=100.0, self.volume, Message::Volume).step(1.0),
                icssed::text(format!("Volume {:.0}%", self.volume)),
                icssed::slider(0.0..=1.0, 0.3, Message::Volume).disabled_if(true),
                icssed::text("Disabled"),
            ]
            .kind("line"),
            icssed::row![
                icssed::input("Search", &self.search)
                    .on_input(Message::Search)
                    .leading(icssed::icon(self.star.clone())),
                icssed::input("Password", &self.password)
                    .on_input(Message::Password)
                    .secure(!self.peek)
                    .trailing(
                        icssed::button![icssed::icon(self.eye.clone())]
                            .class("peek")
                            .class_if("active", self.peek)
                            .on_press(Message::Peek),
                    ),
                icssed::input("Disabled", "")
                    .disabled_if(true)
                    .on_input(Message::Search),
            ]
            .kind("line"),
            icssed::row![
                icssed::select(
                    [
                        "Follow the system",
                        "Light",
                        "Dark",
                        "A choice far too long for its box",
                    ],
                    self.choice,
                    Message::Choose,
                )
                .placeholder("Pick a scheme"),
                icssed::select(["Disabled"], Some(0), Message::Choose).disabled_if(true),
            ]
            .kind("line"),
            icssed::row![
                icssed::meter(self.volume / 100.0),
                icssed::meter(0.85).class("warning"),
                icssed::meter(0.42).class("gauge").label("CPU 42%"),
            ]
            .kind("line"),
        ]
        .kind("controls");
        self.section("Controls", content.into())
    }

    fn animation<'a>(&'a self) -> Element<'a, Message> {
        let mut items: Vec<Element<'a, Message>> = vec![
            icssed::icon(self.star.clone()).kind("spinner").into(),
            icssed::row![icssed::text("Pulse")].kind("pulse").into(),
            icssed::row![icssed::text("Shaken on demand")]
                .kind("box")
                .class("shake")
                .class_if("shaking", self.shaking)
                .on_animation_end(Message::Shaken)
                .into(),
            icssed::tooltip(
                icssed::button![icssed::text("Shake")].on_press(Message::Shake),
                "Shakes the box beside it, once",
            )
            .into(),
            icssed::button![icssed::text(if self.toast.is_some() {
                "Dismiss"
            } else {
                "Toast"
            })]
            .on_press(if self.toast.is_some() {
                Message::Dismiss
            } else {
                Message::Toast
            })
            .into(),
        ];
        if let Some(leaving) = self.toast {
            items.push(
                icssed::row![
                    icssed::icon(self.star.clone()),
                    icssed::text("A toast: in, and out when dismissed"),
                ]
                .kind("toast")
                .class_if("leaving", leaving)
                .on_animation_end(Message::Toasted)
                .into(),
            );
        }
        self.section("Animation", icssed::row(items).into())
    }

    fn boxes<'a>(&'a self) -> Element<'a, Message> {
        let content = icssed::row([
            Self::boxed("plain", "Plain"),
            Self::boxed("bordered", "Border"),
            Self::boxed("rounded", "Radius"),
            Self::boxed("shadow", "Shadow"),
            Self::boxed("inset", "Inset"),
            Self::boxed("sides", "Sides"),
        ]);
        self.section("Boxes", content.into())
    }

    fn painting<'a>(&'a self) -> Element<'a, Message> {
        let line = |class: &str, text: &'static str| -> Element<'a, Message> {
            icssed::text(text).class(class).into()
        };
        let content = icssed::column![
            icssed::row([
                Self::boxed("tab", "Tab"),
                Self::boxed("four", "Four sides"),
                Self::boxed("ring", "Ring"),
                Self::boxed("outlined", "Outline"),
                Self::boxed("scaled", "Scaled 80%"),
                Self::boxed("moved", "Moved"),
                icssed::row![icssed::icon(self.star.clone()), icssed::text("Opacity 50%"),]
                    .kind("box")
                    .class("faded")
                    .into(),
            ]),
            icssed::column([
                line("left", "Aligned left"),
                line("center", "Centred"),
                line("right", "Aligned right"),
                line("italic", "Italic, as the theme says"),
            ])
            .kind("aligned"),
        ]
        .kind("painting");
        self.section("Painting and text", content.into())
    }

    fn images<'a>(&'a self) -> Element<'a, Message> {
        let fitted = |class: &str| -> Element<'a, Message> {
            icssed::image(self.photo.clone())
                .kind("picture")
                .class(class)
                .into()
        };
        let content = icssed::row([
            Self::boxed("textured", "Tiled"),
            Self::boxed("framed", "9-patch"),
            Self::boxed("scenery", "Cover"),
            fitted("fill"),
            fitted("contain"),
            fitted("cover"),
        ]);
        self.section("Images", content.into())
    }

    fn texts<'a>(&'a self) -> Element<'a, Message> {
        let label = |class: &str, text: &'static str| -> Element<'a, Message> {
            icssed::text(text).class(class).into()
        };
        let pair = |untrimmed: bool, text: &'static str| -> Element<'a, Message> {
            icssed::row![icssed::icon(self.star.clone()), icssed::text(text)]
                .kind("pair")
                .class_if("untrimmed", untrimmed)
                .into()
        };
        let content = icssed::column([
            label("small", "Small, 12px"),
            label("regular", "Regular: the root's 14px"),
            label("large", "Large, 20px"),
            label("bold", "Bold"),
            label("muted", "Muted"),
            label("caps", "Upper case, by the theme"),
            Self::boxed("wrap", LONG),
            Self::boxed("cut", LONG),
            pair(false, "Trimmed: Centred On Its Letters"),
            pair(true, "Untrimmed: Centred On Its Line Box"),
        ])
        .kind("texts");
        self.section("Text", content.into())
    }

    fn flex<'a>(&'a self) -> Element<'a, Message> {
        let lane = |class: &str, cells: &[&'static str]| -> Element<'a, Message> {
            icssed::row(cells.iter().map(|&cell| {
                icssed::row![icssed::text(cell)]
                    .kind("cell")
                    .class(cell)
                    .into()
            }))
            .kind("lane")
            .class(class)
            .into()
        };
        let content = icssed::column([
            lane("plain", &["fill", "fixed", "fill"]),
            lane("center", &["fixed", "fixed"]),
            lane("end", &["fixed", "fixed"]),
            lane("align-center", &["tall", "mid", "short"]),
        ])
        .kind("flex");
        self.section("Flex", content.into())
    }

    fn menus<'a>(&'a self) -> Element<'a, Message> {
        let picked = match self.picked {
            Some(label) => format!("Picked: {label}"),
            None => "Nothing picked yet: click, or Tab to an item then Up, Down, Enter".into(),
        };
        let item = |label: &'static str| MenuItem::new(label).on_press(Message::Picked(label));
        let menu = icssed::menu([
            item("Open").icon(self.star.clone()).shortcut("Ctrl+O"),
            item("A label far too long for its menu, cut short").shortcut("Ctrl+L"),
            MenuItem::new("Paste, disabled").shortcut("Ctrl+V"),
            MenuItem::separator(),
            MenuItem::new("Show hidden files")
                .check(self.hidden)
                .on_press(Message::Hidden),
            MenuItem::new("Sort by name")
                .radio(!self.by_date)
                .on_press(Message::ByDate(false)),
            MenuItem::new("Sort by date")
                .radio(self.by_date)
                .on_press(Message::ByDate(true)),
            MenuItem::separator(),
            MenuItem::new("More").on_press(Message::More).submenu(
                self.more,
                [
                    item("Rename").icon(self.star.clone()),
                    item("Delete").class("danger"),
                ],
            ),
        ]);
        let content = icssed::column![menu, icssed::text(picked).kind("status")].kind("menus");
        self.section("Menu", content.into())
    }

    fn tabs<'a>(&'a self) -> Element<'a, Message> {
        const PAGES: [(&str, &str); 3] = [
            (
                "Overview",
                "The first page: Left and Right move among the tabs once one has the focus.",
            ),
            ("Details", "The second page, its tab marked by the theme."),
            ("Settings", "The third page."),
        ];
        let content = icssed::column![
            icssed::tabs(PAGES.iter().enumerate().map(|(i, (title, _))| {
                Tab::new(*title, Message::Tab(i)).active(i == self.tab)
            })),
            icssed::text(PAGES[self.tab].1).kind("page"),
        ]
        .kind("pages");
        self.section("Tabs", content.into())
    }

    fn calendar<'a>(&'a self) -> Element<'a, Message> {
        const MONTHS: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        let title = format!(
            "{} {}",
            MONTHS[self.month.month as usize - 1],
            self.month.year
        );
        let (year, month, day) = self.today;
        let mut calendar = icssed::calendar(
            self.month,
            title,
            ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"],
        )
        .today(year, month, day)
        .on_previous(Message::PreviousMonth)
        .on_next(Message::NextMonth)
        .on_select(Message::Day);
        if let Some((year, month, day)) = self.day {
            calendar = calendar.selected(year, month, day);
        }
        self.section("Calendar", calendar.into())
    }

    fn graphs<'a>(&'a self) -> Element<'a, Message> {
        // Two made-up series: a load, and traffic in and out.
        let load = (0..60).map(|i| {
            let t = i as f32 / 6.0;
            50.0 + 30.0 * t.sin() + 10.0 * (t * 3.1).cos()
        });
        let down = (0..40).map(|i| {
            let t = i as f32 / 4.0;
            (400.0 + 300.0 * (t * 0.7).sin()).max(0.0)
        });
        let up = (0..40).map(|i| {
            let t = i as f32 / 4.0;
            (120.0 + 90.0 * (t * 1.3).cos()).max(0.0)
        });
        let content = icssed::row![
            icssed::graph(60).series(load).max(100.0).label("100%"),
            icssed::graph(60)
                .class("traffic")
                .series_class("down", down)
                .series_class("up", up)
                .label("1 MB/s"),
            icssed::graph(30)
                .kind("sparkline")
                .series((0..30).map(|i| 5.0 + 4.0 * (i as f32 / 3.0).sin()))
                .label("CPU 42%"),
        ]
        .kind("graphs");
        self.section("Graphs", content.into())
    }

    fn scrolls<'a>(&'a self) -> Element<'a, Message> {
        let rows = icssed::column(
            (1..=30).map(|i| icssed::text(format!("Row {i} of a list that scrolls")).into()),
        )
        .kind("rows");
        let cells = icssed::row((1..=20).map(|i| {
            icssed::row![icssed::text(format!("Cell {i}"))]
                .kind("cell")
                .into()
        }))
        .kind("cells");
        let content = icssed::row![
            icssed::scroll(rows).class("list"),
            icssed::scroll(cells).class("strip").horizontal(),
        ]
        .kind("scrolls");
        self.section("Scroll", content.into())
    }

    fn icons<'a>(&'a self) -> Element<'a, Message> {
        let content = icssed::row![
            icssed::icon(self.star.clone()).class("small"),
            icssed::icon(self.star.clone()),
            icssed::icon(self.star.clone())
                .class("large")
                .class("accent"),
            icssed::tooltip(
                icssed::icon(self.picture.clone()).kind("avatar"),
                "A picture, rounded by the theme: rest the pointer on anything with a tip",
            ),
        ]
        .kind("icons");
        self.section("Icons", content.into())
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

/// Today's date, in UTC: (year, month, day).
fn today() -> (i32, u32, u32) {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // Howard Hinnant's `civil_from_days`.
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_from_march + 2) / 5 + 1) as u32;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    } as u32;
    let year = (year_of_era + era * 400 + i64::from(month <= 2)) as i32;
    (year, month, day)
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
