//! A month: its days in a grid of six weeks, under the names of the
//! days of the week, and a header with its title between the buttons to
//! the months before and after. No locale here: the title and the names
//! are the app's, the first day of the week too (Monday unless it says).
//! The days around the month fill the grid, `.other`; the month's days
//! are buttons when the app says what a press sends.

use std::borrow::Cow;

use super::flex::{button, column, row};
use super::text::{mark, text};
use crate::Element;
use crate::style::Node;

/// A month of a year, in the Gregorian calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Month {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
}

impl Month {
    /// `month` of `year`, 1 to 12 (out of it, the nearest).
    pub fn new(year: i32, month: u32) -> Self {
        Self {
            year,
            month: month.clamp(1, 12),
        }
    }

    pub fn next(self) -> Self {
        match self.month {
            12 => Self::new(self.year + 1, 1),
            m => Self::new(self.year, m + 1),
        }
    }

    pub fn previous(self) -> Self {
        match self.month {
            1 => Self::new(self.year - 1, 12),
            m => Self::new(self.year, m - 1),
        }
    }

    /// How many days it has.
    pub fn days(self) -> u32 {
        match self.month {
            4 | 6 | 9 | 11 => 30,
            2 if self.year % 4 == 0 && (self.year % 100 != 0 || self.year % 400 == 0) => 29,
            2 => 28,
            _ => 31,
        }
    }

    /// The day of the week of its first: 0 for Monday, 6 for Sunday.
    pub fn first_weekday(self) -> u32 {
        // 1970-01-01, day 0, was a Thursday.
        (days_from_civil(self.year, self.month, 1) + 3).rem_euclid(7) as u32
    }
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (i64::from(month) + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub struct Calendar<'a, Message> {
    node: Node<'a>,
    month: Month,
    title: Cow<'a, str>,
    /// Monday first.
    weekdays: [Cow<'a, str>; 7],
    /// The week's first day, 0 for Monday.
    first: u32,
    today: Option<u32>,
    selected: Option<u32>,
    on_previous: Option<Message>,
    on_next: Option<Message>,
    on_select: Option<Box<dyn Fn(u32) -> Message + 'a>>,
}

/// `month`, its `title` (its name and year, as the app writes them),
/// the names of the days of the week from Monday: `node`.
pub fn calendar<'a, Message>(
    node: Node<'a>,
    month: Month,
    title: impl Into<Cow<'a, str>>,
    weekdays: [impl Into<Cow<'a, str>>; 7],
) -> Calendar<'a, Message> {
    Calendar {
        node,
        month,
        title: title.into(),
        weekdays: weekdays.map(Into::into),
        first: 0,
        today: None,
        selected: None,
        on_previous: None,
        on_next: None,
        on_select: None,
    }
}

impl<'a, Message> Calendar<'a, Message> {
    /// The day the week starts on: 0 for Monday, 6 for Sunday.
    pub fn first_weekday(mut self, first: u32) -> Self {
        self.first = first % 7;
        self
    }

    /// Today, `.today` when it's in the month shown.
    pub fn today(mut self, year: i32, month: u32, day: u32) -> Self {
        self.today = (Month::new(year, month) == self.month).then_some(day);
        self
    }

    /// The day picked, `.selected` when it's in the month shown.
    pub fn selected(mut self, year: i32, month: u32, day: u32) -> Self {
        self.selected = (Month::new(year, month) == self.month).then_some(day);
        self
    }

    /// What the button to the month before sends.
    pub fn on_previous(mut self, message: Message) -> Self {
        self.on_previous = Some(message);
        self
    }

    /// What the button to the month after sends.
    pub fn on_next(mut self, message: Message) -> Self {
        self.on_next = Some(message);
        self
    }

    /// What a day's press sends: the month's days are buttons.
    pub fn on_select(mut self, on_select: impl Fn(u32) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(on_select));
        self
    }
}

impl<'a, Message: Clone + 'a> From<Calendar<'a, Message>> for Element<'a, Message> {
    fn from(calendar: Calendar<'a, Message>) -> Self {
        let node = calendar.node;
        let header = node.child("header");
        let arrow = |class: &str, message: Option<Message>| -> Element<'a, Message> {
            let node = header
                .child("button")
                .class(class)
                .disabled_if(message.is_none());
            let mut pressed = button(node, [mark(node.child("arrow"))]);
            if let Some(message) = message {
                pressed = pressed.on_press(message);
            }
            pressed.into()
        };
        let header = row(
            header,
            [
                arrow("previous", calendar.on_previous),
                text(header.child("title"), calendar.title).into(),
                arrow("next", calendar.on_next),
            ],
        );

        let weekdays = node.child("weekdays");
        let names = (0..7).map(|i| {
            let name = calendar.weekdays[(calendar.first as usize + i) % 7].clone();
            let cell = weekdays.child_at("weekday", i, 7);
            row(cell, [text(cell.child("label"), name).into()]).into()
        });
        let weekdays = row(weekdays, names);

        // Six weeks from the week of the 1st: the month, the days around.
        let month = calendar.month;
        let lead = (month.first_weekday() + 7 - calendar.first) % 7;
        let before = month.previous().days();
        let days = node.child("days");
        let weeks = (0..6).map(|w| {
            let week = days.child_at("week", w, 6);
            let cells = (0..7).map(|d| {
                let at = (w * 7 + d) as u32;
                let (day, other) = if at < lead {
                    (before - lead + at + 1, true)
                } else if at - lead < month.days() {
                    (at - lead + 1, false)
                } else {
                    (at - lead - month.days() + 1, true)
                };
                let cell = week
                    .child_at("day", d, 7)
                    .class_if("other", other)
                    .class_if("today", !other && calendar.today == Some(day))
                    .class_if("selected", !other && calendar.selected == Some(day));
                let label = text(cell.child("label"), day.to_string()).into();
                match &calendar.on_select {
                    Some(on_select) if !other => button(cell, [label]).on_press(on_select(day)),
                    _ => row(cell, [label]),
                }
                .into()
            });
            row(week, cells).into()
        });
        let days = column(days, weeks);
        column(node, [header.into(), weekdays.into(), days.into()]).into()
    }
}

#[cfg(test)]
mod tests {
    use super::Month;

    #[test]
    fn months_know_their_days_and_their_first() {
        // 2026: October starts on a Thursday, September on a Tuesday.
        assert_eq!(Month::new(2026, 10).first_weekday(), 3);
        assert_eq!(Month::new(2026, 9).first_weekday(), 1);
        assert_eq!(Month::new(2027, 2).first_weekday(), 0);
        assert_eq!(Month::new(1969, 12).first_weekday(), 0);
        assert_eq!(Month::new(2032, 2).days(), 29);
        assert_eq!(Month::new(2000, 2).days(), 29);
        assert_eq!(Month::new(2100, 2).days(), 28);
        assert_eq!(Month::new(2026, 4).days(), 30);
        assert_eq!(Month::new(2026, 12).next(), Month::new(2027, 1));
        assert_eq!(Month::new(2026, 1).previous(), Month::new(2025, 12));
    }
}
