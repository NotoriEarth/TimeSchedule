//! A separate year/month draft; navigation never mutates an event editor.
use crate::{
    app::{App, Message},
    theme,
};
use chrono::{Datelike, NaiveDate};
use iced::widget::{button, column, container, operation, row, space, text, text_input};
use iced::{window, Element, Length, Size, Task};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpField {
    Year,
    Month,
}
impl JumpField {
    fn id(self) -> &'static str {
        match self {
            Self::Year => "jump-year",
            Self::Month => "jump-month",
        }
    }
}
#[derive(Clone, Debug)]
pub enum JumpMessage {
    Open(JumpField),
    Ready(window::Id),
    Year(String),
    Month(String),
    Date(String),
    Apply,
    ApplyDate,
    Cancel,
}
pub struct MonthJump {
    pub window: window::Id,
    pub focus: JumpField,
    pub year: String,
    pub month: String,
    pub date: String,
    pub error: String,
}
impl App {
    pub fn jump_message(&mut self, message: JumpMessage) -> Task<Message> {
        match message {
            JumpMessage::Open(focus) => {
                if let Some(jump) = &mut self.widget_ui.jump {
                    jump.focus = focus;
                    return window::minimize(jump.window, false)
                        .chain(window::gain_focus(jump.window))
                        .chain(operation::focus(focus.id()))
                        .chain(operation::select_all(focus.id()));
                }
                let (id, open) = window::open(window::Settings {
                    size: Size::new(400.0, 300.0),
                    min_size: Some(Size::new(360.0, 300.0)),
                    position: window::Position::Centered,
                    exit_on_close_request: false,
                    ..Default::default()
                });
                self.widget_ui.jump = Some(MonthJump {
                    window: id,
                    focus,
                    year: self.widget_month.year().to_string(),
                    month: self.widget_month.month().to_string(),
                    date: String::new(),
                    error: String::new(),
                });
                open.map(|id| Message::Jump(JumpMessage::Ready(id)))
            }
            JumpMessage::Cancel => self
                .widget_ui
                .jump
                .take()
                .map_or_else(Task::none, |j| window::close(j.window)),
            JumpMessage::Ready(id) => {
                let Some(jump) = self.widget_ui.jump.as_ref().filter(|j| j.window == id) else {
                    return Task::none();
                };
                window::gain_focus(id)
                    .chain(operation::focus(jump.focus.id()))
                    .chain(operation::select_all(jump.focus.id()))
            }
            other => {
                let Some(jump) = &mut self.widget_ui.jump else {
                    return Task::none();
                };
                match other {
                    JumpMessage::Year(value) => jump.year = value.chars().take(4).collect(),
                    JumpMessage::Month(value) => jump.month = value.chars().take(2).collect(),
                    JumpMessage::Date(value) => jump.date = value.chars().take(10).collect(),
                    JumpMessage::Apply | JumpMessage::ApplyDate => {
                        let date = if matches!(other, JumpMessage::ApplyDate) {
                            NaiveDate::parse_from_str(jump.date.trim(), "%Y-%m-%d").ok()
                        } else {
                            jump.year
                                .trim()
                                .parse()
                                .ok()
                                .zip(jump.month.trim().parse().ok())
                                .and_then(|(y, m)| NaiveDate::from_ymd_opt(y, m, 1))
                        }
                        .filter(|d| (1900..=9998).contains(&d.year()));
                        if let Some(date) = date {
                            let close = self.jump_message(JumpMessage::Cancel);
                            return Task::batch([close, self.jump_widget(date, false)]);
                        }
                        jump.error = "1900~9998년의 올바른 연·월 또는 날짜를 입력하세요.".into();
                    }
                    _ => {}
                }
                Task::none()
            }
        }
    }
}
pub fn view(app: &App) -> Element<'_, Message> {
    let jump = app.widget_ui.jump.as_ref().unwrap();
    fn msg(value: JumpMessage) -> Message {
        Message::Jump(value)
    }
    let content = column![
        text("보고 싶은 날짜로 이동").size(20),
        row![
            column![
                text("년").size(12),
                text_input("2025", &jump.year)
                    .id("jump-year")
                    .padding(9)
                    .on_input(|v| msg(JumpMessage::Year(v)))
                    .on_submit(msg(JumpMessage::Apply))
            ]
            .spacing(5),
            column![
                text("월").size(12),
                text_input("9", &jump.month)
                    .id("jump-month")
                    .padding(9)
                    .on_input(|v| msg(JumpMessage::Month(v)))
                    .on_submit(msg(JumpMessage::Apply))
            ]
            .spacing(5),
        ]
        .spacing(12),
        row![
            space().width(Length::Fill),
            button("취소")
                .style(theme::secondary)
                .on_press(msg(JumpMessage::Cancel)),
            button("이 월로 이동").on_press(msg(JumpMessage::Apply))
        ]
        .spacing(8),
        text("날짜 직접 입력").size(12),
        row![
            text_input("YYYY-MM-DD", &jump.date)
                .padding(8)
                .on_input(|v| msg(JumpMessage::Date(v)))
                .on_submit(msg(JumpMessage::ApplyDate)),
            button("이동")
                .padding(8)
                .on_press(msg(JumpMessage::ApplyDate))
        ]
        .spacing(8),
        text(&jump.error).size(12).style(theme::danger),
    ]
    .spacing(12);
    container(content)
        .padding(20)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::root)
        .into()
}
