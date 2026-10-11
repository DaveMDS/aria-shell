//! A month view: the toolkit's calendar, and the month it shows, which
//! the toolkit leaves to its host (its widgets are controlled), moved by
//! its buttons. Weeks start on Monday; the words are the locale's.

use chrono::{Datelike, Days, Months, NaiveDate};
use icssed::{Element, Month};

use crate::locale::Locale;

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

    /// The month as `node` (a `calendar`), `today` marked when it's in it.
    pub fn view<'a>(&self, today: NaiveDate, locale: &Locale) -> Element<'a, Message> {
        // Monday to Sunday, abbreviated in the language (any week does).
        let monday = self.month - Days::new(u64::from(self.month.weekday().num_days_from_monday()));
        let weekdays: [String; 7] =
            std::array::from_fn(|i| locale.naive_date(&(monday + Days::new(i as u64)), "%a"));
        icssed::calendar(
            Month::new(self.month.year(), self.month.month()),
            locale.naive_date(&self.month, "%B %Y"),
            weekdays,
        )
        .today(today.year(), today.month(), today.day())
        .on_previous(Message::PrevMonth)
        .on_next(Message::NextMonth)
        .into()
    }
}

fn first_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).expect("the 1st exists in every month")
}

#[cfg(test)]
mod tests {
    use super::*;

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
