//! One date range for both the desktop grid and its storage query.
use crate::app::{App, Message};
use chrono::{Datelike, Duration, Local, NaiveDate};
use iced::Task;
use timeschedule_core::dates::{calendar_range, WeekRange};

impl App {
    pub fn widget_range(&self) -> WeekRange {
        let p = &self.preferences.widget;
        calendar_range(
            if p.rolling {
                self.widget_ui.anchor
            } else {
                self.widget_month
            },
            p.rolling.then_some((p.weeks, p.past_weeks)),
        )
        .expect("validated widget preferences and bounded anchor")
    }
    pub fn widget_days(&self) -> Vec<NaiveDate> {
        let range = self.widget_range();
        (0..=(range.to - range.from).num_days())
            .map(|i| range.from + Duration::days(i))
            .collect()
    }
    pub fn jump_widget(&mut self, date: NaiveDate, follow: bool) -> Task<Message> {
        if !(1900..=9998).contains(&date.year()) {
            return Task::none();
        }
        self.widget_month = date.with_day(1).unwrap();
        self.widget_ui.anchor = date;
        self.widget_ui.selected = Some(date);
        self.widget_ui.follow_today = follow;
        self.refresh()
    }
    pub fn widget_today_changed(&mut self) -> Task<Message> {
        let today = Local::now().date_naive();
        if today == self.widget_ui.today {
            return Task::none();
        }
        self.widget_ui.today = today;
        if self.widget_ui.follow_today {
            self.jump_widget(today, true)
        } else {
            self.refresh()
        }
    }
}
