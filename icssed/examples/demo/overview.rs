//! Every widget of the toolkit on one page, as the base and the theme
//! draw it: what to look at while writing a theme, to keep its design
//! coherent. Nothing here is laid out by hand beyond rows and gaps; each
//! widget's own demo, in the sidebar, says how it works.
//!
//! Each line shows a family, then two lines their disabled states:
//!
//! - buttons: the base's classes, `.flat`, `.primary`, `.danger`, and
//!   one with an icon alone and a tip;
//! - toggles: a button that stays pressed, check boxes, radios and
//!   switches, on and off;
//! - a slider and a meter; a text field, a password, a select;
//! - tabs; then a menu, a calendar and a graph side by side.
//!
//! Light and dark: the switch at the top right.

use icssed::{Element, MenuItem, Month, Tab, button, column, icon, row, text, toggle};

use crate::Assets;
use crate::calendar::{MONTHS, today};

pub const TITLE: &str = "Overview";

const SORTS: [&str; 2] = ["By name", "By date"];
const TABS: [&str; 3] = ["Overview", "Details", "Settings"];

pub struct State {
    pressed: bool,
    checks: [bool; 2],
    sort: usize,
    switches: [bool; 2],
    volume: f32,
    search: String,
    password: String,
    peek: bool,
    choice: Option<usize>,
    tab: usize,
    hidden: bool,
    more: bool,
    month: Month,
    today: (i32, u32, u32),
    /// The day picked.
    day: Option<(i32, u32, u32)>,
}

impl Default for State {
    fn default() -> Self {
        let today = today();
        Self {
            pressed: true,
            checks: [true, false],
            sort: 0,
            switches: [true, false],
            volume: 40.0,
            search: String::new(),
            password: "hunter2".into(),
            peek: false,
            choice: None,
            tab: 0,
            hidden: false,
            more: false,
            month: Month::new(today.0, today.1),
            today,
            day: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Nothing,
    Pressed(bool),
    Check(usize, bool),
    Sort(usize),
    Switch(usize, bool),
    Volume(f32),
    Search(String),
    Password(String),
    Peek,
    Choose(usize),
    Tab(usize),
    Hidden,
    More,
    PreviousMonth,
    NextMonth,
    Day(u32),
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::Nothing => {}
        Message::Pressed(on) => state.pressed = on,
        Message::Check(i, on) => state.checks[i] = on,
        Message::Sort(i) => state.sort = i,
        Message::Switch(i, on) => state.switches[i] = on,
        Message::Volume(volume) => state.volume = volume,
        Message::Search(text) => state.search = text,
        Message::Password(text) => state.password = text,
        Message::Peek => state.peek = !state.peek,
        Message::Choose(choice) => state.choice = Some(choice),
        Message::Tab(tab) => state.tab = tab,
        Message::Hidden => state.hidden = !state.hidden,
        Message::More => state.more = !state.more,
        Message::PreviousMonth => state.month = state.month.previous(),
        Message::NextMonth => state.month = state.month.next(),
        Message::Day(day) => state.day = Some((state.month.year, state.month.month, day)),
    }
}

pub fn view<'a>(state: &'a State, assets: &'a Assets) -> Element<'a, Message> {
    let star = || icon(assets.star.clone());
    let buttons = row![
        button![star(), text("Default")].on_press(Message::Nothing),
        button![text("Flat")].class("flat").on_press(Message::Nothing),
        button![text("Primary")]
            .class("primary")
            .on_press(Message::Nothing),
        button![text("Danger")]
            .class("danger")
            .on_press(Message::Nothing),
        icssed::tooltip(button![star()].on_press(Message::Nothing), "A tip"),
    ];
    let checks = (0..2).map(|i| {
        toggle(state.checks[i], move |on| Message::Check(i, on))
            .check()
            .label(["Check", "Box"][i])
            .into()
    });
    let radios = SORTS.iter().enumerate().map(|(i, &by)| {
        toggle(i == state.sort, move |_| Message::Sort(i))
            .radio()
            .label(by)
            .into()
    });
    let switches = (0..2).map(|i| {
        toggle(state.switches[i], move |on| Message::Switch(i, on))
            .switch()
            .into()
    });
    let toggles = row(std::iter::once(
        toggle(state.pressed, Message::Pressed)
            .icon(assets.star.clone())
            .label("Toggle")
            .into(),
    )
    .chain(checks)
    .chain(radios)
    .chain(switches));
    let values = row![
        icssed::slider(0.0..=100.0, state.volume, Message::Volume).step(1.0),
        icssed::meter(state.volume / 100.0),
    ];
    let fields = row![
        icssed::input("Search", &state.search)
            .on_input(Message::Search)
            .leading(star()),
        icssed::input("Password", &state.password)
            .on_input(Message::Password)
            .secure(!state.peek)
            .trailing(
                button![icon(assets.eye.clone())]
                    .class("peek")
                    .class_if("active", state.peek)
                    .on_press(Message::Peek),
            ),
        icssed::select(["Light", "Dark", "Follow the system"], state.choice, Message::Choose)
            .placeholder("Select"),
    ];
    let disabled = row![
        button![text("Button")].disabled_if(true),
        toggle(true, Message::Pressed)
            .label("Toggle")
            .disabled_if(true),
        toggle(true, Message::Pressed)
            .check()
            .label("Check")
            .disabled_if(true),
        toggle(true, Message::Pressed)
            .radio()
            .label("Radio")
            .disabled_if(true),
        toggle(true, Message::Pressed).switch().disabled_if(true),
    ];
    let disabled_fields = row![
        icssed::slider(0.0..=1.0, 0.3, Message::Volume).disabled_if(true),
        icssed::input("Input", "")
            .on_input(Message::Search)
            .disabled_if(true),
        icssed::select(["Select"], Some(0), Message::Choose).disabled_if(true),
    ];
    let tabs = icssed::tabs(
        TABS.iter()
            .enumerate()
            .map(|(i, &title)| Tab::new(title, Message::Tab(i)).active(i == state.tab)),
    );
    let menu = icssed::menu([
        MenuItem::new("Open")
            .icon(assets.star.clone())
            .shortcut("Ctrl+O")
            .on_press(Message::Nothing),
        MenuItem::new("Paste").shortcut("Ctrl+V"),
        MenuItem::separator(),
        MenuItem::new("Show hidden files")
            .check(state.hidden)
            .on_press(Message::Hidden),
        MenuItem::new("Sort by name")
            .radio(state.sort == 0)
            .on_press(Message::Sort(0)),
        MenuItem::new("Sort by date")
            .radio(state.sort == 1)
            .on_press(Message::Sort(1)),
        MenuItem::separator(),
        MenuItem::new("More").on_press(Message::More).submenu(
            state.more,
            [
                MenuItem::new("Rename").on_press(Message::Nothing),
                MenuItem::new("Delete").on_press(Message::Nothing),
            ],
        ),
    ]);
    let title = format!(
        "{} {}",
        MONTHS[state.month.month as usize - 1],
        state.month.year
    );
    let mut calendar = icssed::calendar(
        state.month,
        title,
        ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"],
    )
    .today(state.today.0, state.today.1, state.today.2)
    .on_previous(Message::PreviousMonth)
    .on_next(Message::NextMonth)
    .on_select(Message::Day);
    if let Some((year, month, day)) = state.day {
        calendar = calendar.selected(year, month, day);
    }
    let load = (0..60).map(|i| {
        let t = i as f32 / 6.0;
        50.0 + 30.0 * t.sin() + 10.0 * (t * 3.1).cos()
    });
    let graph = icssed::graph(60).series(load).max(100.0).label("100%");
    column![
        buttons.kind("line"),
        toggles.kind("line"),
        values.kind("line"),
        fields.kind("line"),
        disabled.kind("line"),
        disabled_fields.kind("line"),
        tabs,
        row![menu, calendar, graph].kind("panels"),
    ]
    .into()
}
