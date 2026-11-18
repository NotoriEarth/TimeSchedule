//! Mouse choices for the editor's year, month and day fields.
use crate::{app::Message, theme, views::WEEKDAYS};
use chrono::{Datelike, NaiveDate};
use iced::widget::{button, column, container, grid, row, space, text};
use iced::{Alignment, Element, Length};
use std::ops::RangeInclusive;
use timeschedule_core::dates::{calendar_days, shift_month};

// Stored editor dates use four digits, including dates outside the main calendar's
// navigation range. Its anchor limits only apply to widget/diary navigation.
const FIRST_YEAR: i32 = 1;
const LAST_YEAR: i32 = 9999;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Part {
    Year,
    Month,
    Day,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Picker {
    pub part: Part,
    selected: NaiveDate,
    page: NaiveDate,
}

pub(crate) fn with_year_month(date: NaiveDate, year: i32, month: u32) -> Option<NaiveDate> {
    if !(FIRST_YEAR..=LAST_YEAR).contains(&year) {
        return None;
    }
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    first.with_day(date.day().min(u32::from(first.num_days_in_month())))
}

impl Picker {
    pub fn new(part: Part, date: NaiveDate) -> Self {
        let selected = date.clamp(
            NaiveDate::from_ymd_opt(FIRST_YEAR, 1, 1).unwrap(),
            NaiveDate::from_ymd_opt(LAST_YEAR, 12, 31).unwrap(),
        );
        Self {
            part,
            selected,
            page: selected.with_day(1).unwrap(),
        }
    }

    fn years(&self) -> RangeInclusive<i32> {
        let decade = self.page.year() / 10 * 10;
        decade.max(FIRST_YEAR)..=decade + 9
    }

    fn days(&self) -> impl Iterator<Item = Option<NaiveDate>> {
        let first = self.page;
        let next_month = shift_month(first, 1);
        calendar_days(first)
            .into_iter()
            .take_while(move |date| *date < next_month)
            .map(move |date| (date >= first).then_some(date))
    }

    fn adjacent(&self, direction: i32) -> Option<NaiveDate> {
        if !matches!(direction, -1 | 1) {
            return None;
        }
        let next = match self.part {
            Part::Year => {
                let decade = (self.page.year() / 10 + direction) * 10;
                if decade < 0 {
                    return None;
                }
                NaiveDate::from_ymd_opt(decade.max(FIRST_YEAR), 1, 1)?
            }
            Part::Month => return None,
            Part::Day => shift_month(self.page, direction),
        };
        (FIRST_YEAR..=LAST_YEAR)
            .contains(&next.year())
            .then_some(next)
    }

    pub fn browse(&mut self, direction: i32) {
        if let Some(next) = self.adjacent(direction) {
            self.page = next;
        }
    }

    fn choice(&self, label: String, date: NaiveDate) -> Element<'static, Message> {
        button(
            text(label)
                .size(13)
                .width(Length::Fill)
                .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([4, 0])
        .style(if date == self.selected {
            theme::primary
        } else {
            theme::text_button
        })
        .on_press(Message::PickEditorDate(date))
        .into()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let heading = match self.part {
            Part::Year => format!("{:04}–{:04}년", self.years().start(), self.years().end()),
            Part::Month => format!("{:04}년", self.page.year()),
            Part::Day => format!("{:04}년 {}월", self.page.year(), self.page.month()),
        };
        let mut header = row![text(heading).size(14), space().width(Length::Fill)]
            .align_y(Alignment::Center)
            .spacing(4);
        if self.part != Part::Month {
            let labels = if self.part == Part::Year {
                ["이전 10년", "다음 10년"]
            } else {
                ["이전 달", "다음 달"]
            };
            for (label, direction) in labels.into_iter().zip([-1, 1]) {
                header = header.push(
                    button(text(label).size(12))
                        .padding([4, 6])
                        .style(theme::secondary)
                        .on_press_maybe(
                            self.adjacent(direction)
                                .map(|_| Message::BrowseDatePicker(direction)),
                        ),
                );
            }
        }
        let choices = match self.part {
            Part::Year => {
                let dates = self
                    .years()
                    .filter_map(|year| with_year_month(self.selected, year, self.selected.month()));
                grid(dates.map(|date| self.choice(format!("{:04}", date.year()), date))).columns(5)
            }
            Part::Month => {
                let dates = (1..=12)
                    .filter_map(|month| with_year_month(self.selected, self.page.year(), month));
                grid(dates.map(|date| self.choice(format!("{}월", date.month()), date))).columns(4)
            }
            Part::Day => {
                let weekdays = WEEKDAYS.into_iter().map(|label| {
                    text(label)
                        .size(12)
                        .width(Length::Fill)
                        .align_x(Alignment::Center)
                        .style(theme::muted)
                        .into()
                });
                let days = self.days().map(|day| match day {
                    Some(date) => self.choice(date.day().to_string(), date),
                    None => space().into(),
                });
                grid(weekdays.chain(days)).columns(7)
            }
        };
        container(column![header, choices.height(Length::Shrink).spacing(2)].spacing(4))
            .width(Length::Fill)
            .max_width(320)
            .padding(8)
            .style(theme::surface)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn year_and_month_choices_clamp_only_days_missing_from_the_target_month() {
        let leap_day = date(2000, 2, 29);
        assert_eq!(with_year_month(leap_day, 1900, 2), Some(date(1900, 2, 28)));
        assert_eq!(with_year_month(leap_day, 2004, 2), Some(date(2004, 2, 29)));
        assert_eq!(
            with_year_month(date(2026, 3, 31), 2026, 4),
            Some(date(2026, 4, 30))
        );
        assert_eq!(with_year_month(leap_day, 1, 3), Some(date(1, 3, 29)));
        assert_eq!(with_year_month(leap_day, 9999, 1), Some(date(9999, 1, 29)));
        for (year, month) in [(0, 1), (10000, 12), (2026, 0), (2026, 13)] {
            assert_eq!(with_year_month(leap_day, year, month), None);
        }
    }

    #[test]
    fn browsing_to_leap_february_aligns_days_to_weekdays_and_preserves_the_selection() {
        let january = date(2024, 1, 31);
        let mut picker = Picker::new(Part::Day, january);
        picker.browse(1);
        let days: Vec<_> = picker.days().collect();
        assert_eq!(days.iter().flatten().count(), 29);
        assert_eq!(&days[..4], &[None; 4]);
        assert_eq!(days[4], Some(date(2024, 2, 1)));
        assert_eq!(days[32], Some(date(2024, 2, 29)));
        assert_eq!(picker.selected, january);
        picker.browse(-1);
        assert!(picker.days().any(|day| day == Some(january)));
    }

    #[test]
    fn year_and_day_pages_stop_at_storage_limits_and_can_browse_back() {
        let mut years = Picker::new(Part::Year, date(1, 1, 1));
        assert!(years.adjacent(-1).is_none());
        assert_eq!(years.years(), 1..=9);
        years.browse(1);
        assert_eq!(years.years(), 10..=19);
        years.browse(-1);
        assert_eq!(years.years(), 1..=9);
        let last = Picker::new(Part::Year, date(9999, 12, 31));
        assert_eq!(last.years(), 9990..=9999);
        assert!(last.adjacent(1).is_none());

        for (boundary, blocked) in [(date(1, 1, 1), -1), (date(9999, 12, 31), 1)] {
            let mut days = Picker::new(Part::Day, boundary);
            assert!(days.adjacent(blocked).is_none());
            days.browse(blocked);
            assert!(days.days().any(|day| day == Some(boundary)));
            assert!(days.adjacent(-blocked).is_some());
        }
    }
}
