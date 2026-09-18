//! A month view: weekday header, the days in a 6x7 grid, prev/next month
//! navigation. Weeks start on Monday; month names are English (chrono
//! has no locale support without extra features).
//!
//! Styled through the host's theme node (`calendar > header | weekday |
//! day`). The cell grid and outer padding are fixed so [`Calendar::SIZE`]
//! can be known before layout, which a popup surface needs.

use chrono::{Datelike, Days, Months, NaiveDate};
use iced::widget::{Space, column, container, row};
use iced::{Alignment, Element, Length};

use crate::locale::Locale;
use crate::theme::{Node, Theme};

const CELL: u32 = 32;
const PADDING: u32 = 12;

pub struct Calendar {
    /// First day of the month shown.
    month: NaiveDate,
}

#[derive(Clone, Debug)]
pub enum Message {
    PrevMonth,
    NextMonth,
}

impl Calendar {
    /// Pixel size of the view, for hosts that need it up front (popups).
    pub const SIZE: (u32, u32) = (7 * CELL + 2 * PADDING, 8 * CELL + 2 * PADDING);

    /// Showing the month of `date`.
    pub fn new(date: NaiveDate) -> Self {
        Self {
            month: first_of_month(date),
        }
    }

    pub fn show(&mut self, date: NaiveDate) {
        self.month = first_of_month(date);
    }

    pub fn update(&mut self, message: Message) {
        self.month = match message {
            Message::PrevMonth => self.month - Months::new(1),
            Message::NextMonth => self.month + Months::new(1),
        };
    }

    /// `today` is highlighted when it falls in the month shown. `node`
    /// is this calendar's element (`... > calendar`).
    pub fn view<'a>(
        &'a self,
        today: NaiveDate,
        theme: &'a Theme,
        locale: &Locale,
        node: &Node,
    ) -> Element<'a, Message> {
        // Cells go through the theme so they carry their node (for
        // `debug widgets`); the size is fixed regardless of it.
        let cell = |node: &Node, content: Element<'a, Message>| {
            theme
                .container(node, content)
                .width(CELL)
                .height(CELL)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
        };
        let header_node = node.child("header");
        let prev = header_node.child("button").class("prev");
        let next = header_node.child("button").class("next");
        let header = row![
            theme
                .button(&prev, theme.text(&prev.child("text"), "<"))
                .on_press(Message::PrevMonth),
            container(theme.text(
                &header_node.child("text"),
                locale.naive_date(&self.month, "%B %Y")
            ))
            .width(Length::Fill)
            .align_x(Alignment::Center),
            theme
                .button(&next, theme.text(&next.child("text"), ">"))
                .on_press(Message::NextMonth),
        ]
        .height(CELL)
        .align_y(Alignment::Center);
        let weekday = node.child("weekday");
        // Monday to Sunday, abbreviated in the language (any week does).
        let monday = self.month - Days::new(u64::from(self.month.weekday().num_days_from_monday()));
        let weekdays = row((0..7).map(|i| {
            let name = locale.naive_date(&(monday + Days::new(i)), "%a");
            cell(&weekday, theme.text(&weekday, name).into()).into()
        }));
        let day_node = node.child("day");
        let days = month_grid(self.month)
            .chunks(7)
            .fold(column![], |col, week| {
                col.push(row(week.iter().map(|day| match day {
                    Some(day) => {
                        let date = self.month.with_day(*day).expect("day of this month");
                        let node = day_node.class_if("today", date == today);
                        let label = theme.text(&node, day.to_string());
                        cell(&node, label.into()).into()
                    }
                    None => cell(&day_node, Space::new().into()).into(),
                })))
            });
        theme
            .container(node, column![header, weekdays, days])
            .padding(PADDING as f32)
            .into()
    }
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).expect("the 1st exists in every month")
}

/// Six weeks of day numbers starting on Monday, `None` outside `month`.
fn month_grid(month: NaiveDate) -> [Option<u32>; 42] {
    let first = first_of_month(month);
    let offset = first.weekday().num_days_from_monday() as usize;
    let days = (first + Months::new(1) - first).num_days() as u32;
    let mut grid = [None; 42];
    for day in 1..=days {
        grid[offset + day as usize - 1] = Some(day);
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_layout() {
        // September 2026 starts on a Tuesday and has 30 days.
        let grid = month_grid(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap());
        assert_eq!(grid[0], None);
        assert_eq!(grid[1], Some(1));
        assert_eq!(grid[30], Some(30));
        assert_eq!(grid[31], None);

        // February 2027 starts on a Monday and has 28 days: exactly 4 rows.
        let grid = month_grid(NaiveDate::from_ymd_opt(2027, 2, 1).unwrap());
        assert_eq!(grid[0], Some(1));
        assert_eq!(grid[27], Some(28));
        assert_eq!(grid[28], None);

        // Leap year, starting on a Sunday.
        let grid = month_grid(NaiveDate::from_ymd_opt(2032, 2, 1).unwrap());
        assert_eq!(grid[6], Some(1));
        assert_eq!(grid[34], Some(29));
        assert_eq!(grid[35], None);
    }

    #[test]
    fn navigation() {
        let mut cal = Calendar::new(NaiveDate::from_ymd_opt(2026, 1, 31).unwrap());
        assert_eq!(cal.month, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        cal.update(Message::PrevMonth);
        assert_eq!(cal.month, NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
        cal.update(Message::NextMonth);
        cal.update(Message::NextMonth);
        assert_eq!(cal.month, NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
    }
}
