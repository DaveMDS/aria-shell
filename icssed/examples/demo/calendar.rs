//! A calendar shows a month: `icssed::calendar(month, title, weekdays)`,
//! its days in six weeks under the names of the days, its title between
//! the buttons to the months before and after.
//!
//!     icssed::calendar(self.month, title, ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"])
//!         .today(year, month, day)
//!         .selected(year, month, day)
//!         .on_previous(Message::PreviousMonth)
//!         .on_next(Message::NextMonth)
//!         .on_select(Message::Day)
//!
//! - No locale in the toolkit: the title and the names of the days are
//!   the app's words, and `.first_weekday(n)` its first day of the week:
//!   0 for Monday (if not said) to 6 for Sunday.
//! - `Month::new(year, month)` is the month shown; `.previous()` and
//!   `.next()` are the months around it. The app holds it, and moves it
//!   on `on_previous` and `on_next`.
//! - The days around the month fill the grid, `.other`; today and the
//!   day picked are marked. The month's days are buttons once
//!   `on_select` says what a press sends: the day of the month shown.

use icssed::{Element, Month, calendar, column, text};

use crate::Assets;

pub const TITLE: &str = "Calendar";

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

pub struct State {
    month: Month,
    today: (i32, u32, u32),
    /// The day picked.
    day: Option<(i32, u32, u32)>,
}

impl Default for State {
    fn default() -> Self {
        let today = today();
        Self {
            month: Month::new(today.0, today.1),
            today,
            day: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    PreviousMonth,
    NextMonth,
    Day(u32),
}

pub fn update(state: &mut State, message: Message) {
    match message {
        Message::PreviousMonth => state.month = state.month.previous(),
        Message::NextMonth => state.month = state.month.next(),
        Message::Day(day) => state.day = Some((state.month.year, state.month.month, day)),
    }
}

pub fn view<'a>(state: &'a State, _assets: &'a Assets) -> Element<'a, Message> {
    let title = format!(
        "{} {}",
        MONTHS[state.month.month as usize - 1],
        state.month.year
    );
    let (year, month, day) = state.today;
    let mut calendar = calendar(state.month, title, ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"])
        .today(year, month, day)
        .on_previous(Message::PreviousMonth)
        .on_next(Message::NextMonth)
        .on_select(Message::Day);
    if let Some((year, month, day)) = state.day {
        calendar = calendar.selected(year, month, day);
    }
    let picked = match state.day {
        Some((year, month, day)) => format!("Picked: {day} {} {year}", MONTHS[month as usize - 1]),
        None => "No day picked yet".into(),
    };
    column![calendar, text(picked).kind("status")].into()
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
