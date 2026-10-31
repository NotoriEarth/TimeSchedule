//! Shared Windows-like year/month picker. Browsing only changes its local draft.
use crate::{
    app::{App, Message},
    theme,
};
use chrono::{Datelike, NaiveDate};
use iced::widget::{
    button, column, container, operation, row, space, text, text_input, tooltip, Column, Row,
};
use iced::{window, Border, Element, Length, Size, Task};

pub const PICKER_SIZE: Size = Size::new(360.0, 420.0);
pub const PICKER_MIN_SIZE: Size = Size::new(320.0, 400.0);
const FIRST_YEAR: i32 = 1900;
const LAST_YEAR: i32 = 9998;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpField {
    Year,
    Month,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpTarget {
    Widget,
    Diary,
}
impl JumpTarget {
    pub fn source(self, app: &App) -> window::Id {
        match self {
            Self::Widget => app.widget.unwrap_or(app.manager),
            Self::Diary => app.manager,
        }
    }
}
#[derive(Clone, Debug)]
pub enum JumpMessage {
    Open(JumpTarget, JumpField),
    Ready(window::Id),
    PickYear(i32),
    PickMonth(u32),
    Page(i32),
    ShowYears,
    ToggleDate,
    Date(String),
    ApplyDate,
    Cancel,
}
pub struct MonthJump {
    pub window: window::Id,
    pub target: JumpTarget,
    pub level: JumpField,
    pub year: i32,
    pub month: u32,
    pub decade: i32,
    pub direct: bool,
    pub date: String,
    pub error: String,
}
impl MonthJump {
    pub fn new(window: window::Id, target: JumpTarget, level: JumpField, date: NaiveDate) -> Self {
        let year = date.year().clamp(FIRST_YEAR, LAST_YEAR);
        Self {
            window,
            target,
            level,
            year,
            month: date.month(),
            decade: year / 10 * 10,
            direct: false,
            date: String::new(),
            error: String::new(),
        }
    }
    fn can_page(&self, delta: i32) -> bool {
        match (self.level, delta) {
            (JumpField::Year, -1) => self.decade > FIRST_YEAR,
            (JumpField::Year, 1) => self.decade < LAST_YEAR / 10 * 10,
            (JumpField::Month, -1) => self.year > FIRST_YEAR,
            (JumpField::Month, 1) => self.year < LAST_YEAR,
            _ => false,
        }
    }
}
impl App {
    pub fn jump_message(&mut self, message: JumpMessage) -> Task<Message> {
        match message {
            JumpMessage::Open(target, level) => {
                let old = if self
                    .widget_ui
                    .jump
                    .as_ref()
                    .is_some_and(|j| j.target != target)
                {
                    self.jump_message(JumpMessage::Cancel)
                } else {
                    Task::none()
                };
                if let Some(jump) = &mut self.widget_ui.jump {
                    jump.level = level;
                    if level == JumpField::Year {
                        jump.decade = jump.year / 10 * 10;
                    }
                    if self.placing_popups.contains_key(&jump.window) {
                        return Task::none();
                    }
                    let id = jump.window;
                    return self.place_popup(target.source(self), id);
                }
                let (id, open) = window::open(window::Settings {
                    size: PICKER_SIZE,
                    min_size: Some(PICKER_MIN_SIZE),
                    visible: false,
                    exit_on_close_request: false,
                    ..Default::default()
                });
                self.placing_popups.insert(id, window::Id::unique());
                let date = match target {
                    JumpTarget::Widget => self.widget_month,
                    JumpTarget::Diary => self.month,
                };
                self.widget_ui.jump = Some(MonthJump::new(id, target, level, date));
                Task::batch([old, open.map(|id| Message::Jump(JumpMessage::Ready(id)))])
            }
            JumpMessage::Cancel => self.widget_ui.jump.take().map_or_else(Task::none, |j| {
                self.placing_popups.remove(&j.window);
                window::close(j.window)
            }),
            JumpMessage::Ready(id) => {
                let Some(jump) = self.widget_ui.jump.as_ref().filter(|j| j.window == id) else {
                    return Task::none();
                };
                self.place_popup(jump.target.source(self), id)
            }
            other => {
                let Some(jump) = &mut self.widget_ui.jump else {
                    return Task::none();
                };
                let mut chosen = None;
                match other {
                    JumpMessage::PickYear(year) if jump.level == JumpField::Year => {
                        if (FIRST_YEAR..=LAST_YEAR).contains(&year) {
                            jump.year = year;
                            jump.level = JumpField::Month;
                            jump.error.clear();
                        }
                    }
                    JumpMessage::PickMonth(month) if jump.level == JumpField::Month => {
                        chosen = NaiveDate::from_ymd_opt(jump.year, month, 1)
                            .filter(|d| (FIRST_YEAR..=LAST_YEAR).contains(&d.year()));
                        if chosen.is_none() {
                            jump.error = "1~12월 중에서 선택하세요.".into();
                        }
                    }
                    JumpMessage::Page(delta) if jump.can_page(delta) => {
                        if jump.level == JumpField::Year {
                            jump.decade += delta * 10;
                        } else {
                            jump.year += delta;
                        }
                        jump.error.clear();
                    }
                    JumpMessage::ShowYears => {
                        jump.level = JumpField::Year;
                        jump.decade = jump.year / 10 * 10;
                        jump.error.clear();
                    }
                    JumpMessage::ToggleDate => {
                        jump.direct = !jump.direct;
                        jump.error.clear();
                        if jump.direct {
                            return operation::focus("jump-date")
                                .chain(operation::select_all("jump-date"));
                        }
                    }
                    JumpMessage::Date(value) => {
                        jump.date = value.chars().take(10).collect();
                        jump.error.clear();
                    }
                    JumpMessage::ApplyDate => {
                        chosen = NaiveDate::parse_from_str(jump.date.trim(), "%Y-%m-%d")
                            .ok()
                            .filter(|d| (FIRST_YEAR..=LAST_YEAR).contains(&d.year()));
                        if chosen.is_none() {
                            jump.direct = true;
                            jump.error = "1900~9998년의 올바른 날짜를 입력하세요.".into();
                        }
                    }
                    _ => {}
                }
                if let Some(date) = chosen {
                    let target = jump.target;
                    let close = self.jump_message(JumpMessage::Cancel);
                    let navigate = match target {
                        JumpTarget::Widget => self.jump_widget(date, false),
                        JumpTarget::Diary => {
                            self.month = date.with_day(1).unwrap();
                            self.selected = date;
                            self.week = timeschedule_core::dates::week_containing(date).from;
                            self.hovered_week = None;
                            self.search.clear();
                            self.search_results.clear();
                            self.search_generation += 1;
                            self.refresh()
                        }
                    };
                    return Task::batch([close, navigate]);
                }
                Task::none()
            }
        }
    }
}
fn msg(value: JumpMessage) -> Message {
    Message::Jump(value)
}
fn tile(
    label: String,
    selected: bool,
    outside: bool,
    action: Option<JumpMessage>,
) -> Element<'static, Message> {
    button(
        container(text(label).size(15))
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .padding(0)
    .width(Length::Fill)
    .height(Length::Fill)
    .on_press_maybe(action.map(msg))
    .style(move |theme, status| {
        let p = theme::colors(theme);
        let disabled = status == button::Status::Disabled;
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: if selected {
                Some(p.accent.into())
            } else if hovered {
                Some(p.raised.into())
            } else {
                None
            },
            text_color: if selected {
                p.on_accent
            } else if outside || disabled {
                p.muted
            } else {
                p.text
            },
            border: Border {
                color: p.line,
                width: if hovered && !selected { 1.0 } else { 0.0 },
                radius: 2.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}
pub fn view(app: &App) -> Element<'_, Message> {
    let jump = app.widget_ui.jump.as_ref().unwrap();
    let years = jump.level == JumpField::Year;
    let heading: Element<'_, Message> = if years {
        container(
            text(format!(
                "{} – {}",
                jump.decade,
                (jump.decade + 9).min(LAST_YEAR)
            ))
            .size(16),
        )
        .padding([4, 6])
        .into()
    } else {
        tooltip(
            button(text(format!("{}년", jump.year)).size(16))
                .padding([4, 6])
                .style(theme::text_button)
                .on_press(msg(JumpMessage::ShowYears)),
            text("연도 선택").size(12),
            tooltip::Position::Bottom,
        )
        .into()
    };
    let previous = button(text("↑").size(20))
        .padding([0, 10])
        .style(theme::text_button)
        .on_press_maybe(jump.can_page(-1).then(|| msg(JumpMessage::Page(-1))));
    let next = button(text("↓").size(20))
        .padding([0, 10])
        .style(theme::text_button)
        .on_press_maybe(jump.can_page(1).then(|| msg(JumpMessage::Page(1))));
    let header = row![
        heading,
        space().width(Length::Fill),
        tooltip(
            previous,
            text(if years {
                "이전 10년"
            } else {
                "이전 연도"
            })
            .size(12),
            tooltip::Position::Bottom
        ),
        tooltip(
            next,
            text(if years {
                "다음 10년"
            } else {
                "다음 연도"
            })
            .size(12),
            tooltip::Position::Bottom
        )
    ]
    .align_y(iced::Alignment::Center)
    .spacing(4);
    let mut grid = Column::new().spacing(5).height(Length::Fill);
    for r in 0..if years { 4 } else { 3 } {
        let mut cells = Row::new().spacing(5).height(Length::Fill);
        for c in 0..4 {
            let index = r * 4 + c;
            let cell = if years {
                let year = jump.decade - 2 + index;
                tile(
                    year.to_string(),
                    year == jump.year,
                    year < jump.decade || year > jump.decade + 9,
                    (FIRST_YEAR..=LAST_YEAR)
                        .contains(&year)
                        .then_some(JumpMessage::PickYear(year)),
                )
            } else {
                let month = index as u32 + 1;
                tile(
                    format!("{month}월"),
                    month == jump.month,
                    false,
                    Some(JumpMessage::PickMonth(month)),
                )
            };
            cells = cells.push(cell);
        }
        grid = grid.push(cells);
    }
    let mut content = column![
        header,
        grid,
        text(if years {
            "연도를 선택하면 월을 고를 수 있어요."
        } else {
            "월을 선택하면 바로 이동합니다."
        })
        .size(12)
        .style(theme::muted),
        row![
            button(
                text(if jump.direct {
                    "날짜 입력 닫기"
                } else {
                    "날짜 직접 입력"
                })
                .size(12)
            )
            .padding([5, 6])
            .style(theme::text_button)
            .on_press(msg(JumpMessage::ToggleDate)),
            space().width(Length::Fill),
            button(text("취소").size(12))
                .padding([5, 10])
                .style(theme::text_button)
                .on_press(msg(JumpMessage::Cancel))
        ]
        .align_y(iced::Alignment::Center),
    ]
    .spacing(12)
    .height(Length::Fill);
    if jump.direct {
        content = content.push(
            row![
                text_input("YYYY-MM-DD", &jump.date)
                    .id("jump-date")
                    .padding(8)
                    .on_input(|v| msg(JumpMessage::Date(v)))
                    .on_submit(msg(JumpMessage::ApplyDate)),
                button("이동")
                    .padding(8)
                    .on_press(msg(JumpMessage::ApplyDate))
            ]
            .spacing(8),
        );
    }
    if !jump.error.is_empty() {
        content = content.push(text(&jump.error).size(12).style(theme::danger));
    }
    container(content)
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::root)
        .into()
}
