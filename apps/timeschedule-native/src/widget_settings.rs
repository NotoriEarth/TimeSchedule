//! Widget preferences UI and drafts; dates and appearance live in their modules.
use crate::{
    app::{App, Message, NativeAction, Page},
    month_jump::MonthJump,
    widget_appearance::{self, Appearance},
};
use chrono::NaiveDate;
use iced::widget::{button, checkbox, column, container, pick_list, row, slider, text, text_input};
use iced::{Element, Length, Size, Task};
use std::time::{Duration, Instant};
use timeschedule_desktop::{
    preferences::{AppTheme, Preferences},
    widget_preferences::{ColorOverrides, WidgetPreferences},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorRole {
    Background,
    Text,
    Today,
    Sunday,
    Saturday,
    Border,
}
impl std::fmt::Display for ColorRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Background => "배경",
            Self::Text => "글자",
            Self::Today => "오늘 강조",
            Self::Sunday => "일요일",
            Self::Saturday => "토요일",
            Self::Border => "테두리",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    Compact,
    Default,
    Comfortable,
}
impl std::fmt::Display for Density {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Compact => "촘촘하게",
            Self::Default => "기본",
            Self::Comfortable => "여유롭게",
        })
    }
}
#[derive(Clone, Debug)]
pub enum Change {
    Open,
    Width(String),
    Height(String),
    Resize,
    Rolling(bool),
    Weeks(u8),
    Past(u8),
    Font(String),
    DateSize(u8),
    EventSize(u8),
    CellGap(u8),
    Padding(u8),
    LineGap(u8),
    Density(Density),
    ColorRole(ColorRole),
    ColorText(String),
    ApplyColor,
    ResetColors,
    ResetAppearance,
}
pub struct State {
    pub anchor: NaiveDate,
    pub today: NaiveDate,
    pub follow_today: bool,
    pub selected: Option<NaiveDate>,
    pub jump: Option<MonthJump>,
    pub width: String,
    pub height: String,
    pub actual_size: Size,
    pub color_role: ColorRole,
    pub color: String,
    pub error: String,
    pub geometry_error: Option<String>,
    pub native_error: Option<String>,
    pub display_observer_error: Option<String>,
    pub save_due: Option<Instant>,
}
impl State {
    pub fn new(prefs: &Preferences, today: NaiveDate) -> Self {
        Self {
            anchor: today,
            today,
            follow_today: true,
            selected: None,
            jump: None,
            width: prefs.widget.width.to_string(),
            height: prefs.widget.height.to_string(),
            actual_size: Size::new(prefs.widget.width as f32, prefs.widget.height as f32),
            color_role: ColorRole::Background,
            color: widget_appearance::hex(Appearance::resolve(prefs).colors.background),
            error: String::new(),
            geometry_error: None,
            native_error: None,
            display_observer_error: None,
            save_due: None,
        }
    }
}
impl App {
    pub(crate) fn widget_errors(&self) -> String {
        [
            self.widget_ui.geometry_error.as_deref(),
            self.widget_ui.native_error.as_deref(),
            self.widget_ui.display_observer_error.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n")
    }
    pub(crate) fn widget_status_text(&self) -> String {
        let errors = self.widget_errors();
        if errors.is_empty() {
            self.desktop_message.clone()
        } else {
            errors
        }
    }

    pub(crate) fn sync_color_draft(&mut self) {
        let p = Appearance::resolve(&self.preferences).colors;
        self.widget_ui.color = widget_appearance::hex(match self.widget_ui.color_role {
            ColorRole::Background => p.background,
            ColorRole::Text => p.text,
            ColorRole::Today => p.accent,
            ColorRole::Sunday => p.rose,
            ColorRole::Saturday => p.blue,
            ColorRole::Border => p.line,
        });
    }
    pub(crate) fn widget_setting(&mut self, change: Change) -> Task<Message> {
        self.widget_ui.error.clear();
        let mut reload = false;
        let mut save = true;
        let w = &mut self.preferences.widget;
        match change {
            Change::Open => {
                if self.editor.is_some() {
                    self.notice =
                        "작성 중인 내용을 저장하거나 닫으면 설정을 열 수 있습니다.".into();
                    return if self.editor_in_quick {
                        self.focus_quick()
                    } else {
                        self.show_manager()
                    };
                }
                self.page = Page::Settings;
                self.widget_ui.width = w.width.to_string();
                self.widget_ui.height = w.height.to_string();
                self.sync_color_draft();
                return self.show_manager();
            }
            Change::Width(s) => {
                self.widget_ui.width = s.chars().take(4).collect();
                save = false;
            }
            Change::Height(s) => {
                self.widget_ui.height = s.chars().take(4).collect();
                save = false;
            }
            Change::Resize => {
                if self.preferences.widget_locked || self.native_busy {
                    return Task::none();
                }
                let dims = self
                    .widget_ui
                    .width
                    .parse::<u32>()
                    .ok()
                    .zip(self.widget_ui.height.parse::<u32>().ok());
                if let Some((width, height)) =
                    dims.filter(|(w, h)| (320..=3840).contains(w) && (220..=2160).contains(h))
                {
                    return self.native(NativeAction::Resize(width, height));
                }
                self.widget_ui.error = "가로 320~3840, 세로 220~2160으로 입력하세요.".into();
                save = false;
            }
            Change::Rolling(value) => {
                w.rolling = value;
                reload = true;
            }
            Change::Weeks(value) => {
                w.weeks = value.clamp(2, 8);
                w.past_weeks = w.past_weeks.min(w.weeks - 1);
                reload = true;
            }
            Change::Past(value) => {
                w.past_weeks = value.min(2).min(w.weeks - 1);
                reload = true;
            }
            Change::Font(value) => {
                if widget_appearance::fonts().iter().any(|name| *name == value) {
                    w.font = value;
                }
            }
            Change::DateSize(v) => w.date_size = v.clamp(10, 32),
            Change::EventSize(v) => w.event_size = v.clamp(9, 24),
            Change::CellGap(v) => w.cell_gap = v.min(16),
            Change::Padding(v) => w.cell_padding = v.min(20),
            Change::LineGap(v) => w.line_gap = v.min(12),
            Change::Density(v) => {
                let (date, event, gap, padding, line) = match v {
                    Density::Compact => (12, 11, 0, 2, 0),
                    Density::Default => (13, 13, 1, 4, 1),
                    Density::Comfortable => (16, 15, 3, 8, 3),
                };
                w.date_size = date;
                w.event_size = event;
                w.cell_gap = gap;
                w.cell_padding = padding;
                w.line_gap = line;
            }
            Change::ColorRole(role) => {
                self.widget_ui.color_role = role;
                self.sync_color_draft();
                save = false;
            }
            Change::ColorText(s) => {
                self.widget_ui.color = s.chars().take(7).collect();
                save = false;
            }
            Change::ApplyColor => {
                if widget_appearance::rgb(&self.widget_ui.color).is_none() {
                    self.widget_ui.error = "색상은 #RRGGBB로 입력하세요. 예: #202632".into();
                    return Task::none();
                }
                let c = if self.preferences.theme == AppTheme::Midnight {
                    &mut w.dark
                } else {
                    &mut w.light
                };
                let slot = match self.widget_ui.color_role {
                    ColorRole::Background => &mut c.background,
                    ColorRole::Text => &mut c.text,
                    ColorRole::Today => &mut c.today,
                    ColorRole::Sunday => &mut c.sunday,
                    ColorRole::Saturday => &mut c.saturday,
                    ColorRole::Border => &mut c.border,
                };
                *slot = Some(self.widget_ui.color.to_uppercase());
            }
            Change::ResetColors => {
                if self.preferences.theme == AppTheme::Midnight {
                    w.dark = ColorOverrides::default()
                } else {
                    w.light = ColorOverrides::default()
                }
                self.sync_color_draft();
            }
            Change::ResetAppearance => {
                let d = WidgetPreferences::default();
                w.font = d.font;
                w.date_size = d.date_size;
                w.event_size = d.event_size;
                w.cell_gap = d.cell_gap;
                w.cell_padding = d.cell_padding;
                w.line_gap = d.line_gap;
                w.dark = d.dark;
                w.light = d.light;
                self.sync_color_draft();
            }
        }
        if save {
            self.widget_ui.save_due = Some(Instant::now() + Duration::from_millis(400));
        }
        if reload {
            self.refresh()
        } else {
            Task::none()
        }
    }
}
fn metric(
    label: String,
    range: std::ops::RangeInclusive<u8>,
    value: u8,
    message: fn(u8) -> Change,
) -> Element<'static, Message> {
    column![
        text(label).size(13),
        slider(range, value, move |v| Message::WidgetSetting(message(v)))
    ]
    .spacing(6)
    .into()
}
pub fn view(app: &App) -> Element<'_, Message> {
    use crate::views::{quiet, subtitle};
    let p = &app.preferences.widget;
    fn msg(value: Change) -> Message {
        Message::WidgetSetting(value)
    }
    let mut width = text_input("가로", &app.widget_ui.width).padding(8);
    let mut height = text_input("세로", &app.widget_ui.height).padding(8);
    let editable = !app.preferences.widget_locked && !app.native_busy;
    if editable {
        width = width.on_input(|v| msg(Change::Width(v)));
        height = height.on_input(|v| msg(Change::Height(v)));
    }
    let actual = app.widget_ui.actual_size;
    let mut c = column![
        subtitle("크기와 표시 기간"),
        quiet(format!(
            "현재 {:.0} × {:.0} · 잠금을 풀면 오른쪽 아래 모서리를 끌어 조절할 수 있습니다.",
            actual.width, actual.height
        )),
        row![
            column![quiet("가로"), width],
            column![quiet("세로"), height],
            button("크기 적용")
                .padding(8)
                .on_press_maybe(editable.then_some(msg(Change::Resize)))
        ]
        .spacing(8)
        .align_y(iced::Alignment::End),
        checkbox(p.rolling)
            .label("기간 보기 · 이번 주를 기준으로 표시")
            .on_toggle(|v| msg(Change::Rolling(v))),
    ]
    .spacing(14);
    if p.rolling {
        c = c.push(
            row![
                column![
                    quiet("전체 표시 주 수"),
                    pick_list((2..=8).collect::<Vec<u8>>(), Some(p.weeks), |v| msg(
                        Change::Weeks(v)
                    ))
                ],
                column![
                    quiet("지난주 포함"),
                    pick_list(
                        (0..=2.min(p.weeks - 1)).collect::<Vec<u8>>(),
                        Some(p.past_weeks),
                        |v| msg(Change::Past(v))
                    )
                ]
            ]
            .spacing(16),
        );
    } else {
        c = c.push(quiet("월간 보기 · 해당 월 전체를 4~6주로 표시합니다."));
    }
    let selected = widget_appearance::fonts()
        .iter()
        .copied()
        .find(|name| *name == p.font);
    c = c
        .push(subtitle("글꼴과 간격"))
        .push(
            row![
                pick_list(
                    [Density::Compact, Density::Default, Density::Comfortable],
                    None::<Density>,
                    |v| msg(Change::Density(v))
                )
                .placeholder("밀도 프리셋"),
                pick_list(widget_appearance::fonts(), selected, |v| msg(Change::Font(
                    v.to_string()
                )))
                .placeholder("기본 글꼴")
                .width(Length::Fill)
            ]
            .spacing(10),
        )
        .push(
            row![
                metric(
                    format!("날짜 글자 {}", p.date_size),
                    10..=32,
                    p.date_size,
                    Change::DateSize
                ),
                metric(
                    format!("일정 글자 {}", p.event_size),
                    9..=24,
                    p.event_size,
                    Change::EventSize
                )
            ]
            .spacing(20),
        )
        .push(
            row![
                metric(
                    format!("셀 사이 {}", p.cell_gap),
                    0..=16,
                    p.cell_gap,
                    Change::CellGap
                ),
                metric(
                    format!("셀 안쪽 {}", p.cell_padding),
                    0..=20,
                    p.cell_padding,
                    Change::Padding
                ),
                metric(
                    format!("일정 줄 간격 {}", p.line_gap),
                    0..=12,
                    p.line_gap,
                    Change::LineGap
                )
            ]
            .spacing(20),
        );
    let swatch = widget_appearance::rgb(&app.widget_ui.color).unwrap_or(iced::Color::TRANSPARENT);
    c = c
        .push(subtitle("위젯 색상"))
        .push(quiet(if app.preferences.theme == AppTheme::Midnight {
            "어두운 모드 색상 · 밝은 모드와 별도로 저장됩니다."
        } else {
            "밝은 모드 색상 · 어두운 모드와 별도로 저장됩니다."
        }))
        .push(
            row![
                pick_list(
                    [
                        ColorRole::Background,
                        ColorRole::Text,
                        ColorRole::Today,
                        ColorRole::Sunday,
                        ColorRole::Saturday,
                        ColorRole::Border
                    ],
                    Some(app.widget_ui.color_role),
                    |v| msg(Change::ColorRole(v))
                ),
                container(iced::widget::space())
                    .width(30)
                    .height(30)
                    .style(move |_| container::Style::default().background(swatch)),
                text_input("#RRGGBB", &app.widget_ui.color)
                    .on_input(|v| msg(Change::ColorText(v)))
                    .on_submit(msg(Change::ApplyColor))
                    .width(110)
                    .padding(7),
                button("적용").padding(7).on_press(msg(Change::ApplyColor))
            ]
            .spacing(8),
        )
        .push(
            row![
                button("현재 모드 색상 초기화")
                    .style(crate::theme::secondary)
                    .on_press(msg(Change::ResetColors)),
                button("글꼴·간격·색상 초기화")
                    .style(crate::theme::secondary)
                    .on_press(msg(Change::ResetAppearance))
            ]
            .spacing(8),
        )
        .push(
            text(&app.widget_ui.error)
                .size(13)
                .style(crate::theme::danger),
        )
        .push(quiet(
            "설정은 위젯에 바로 반영됩니다. 칸에 다 들어가지 않는 일정은 마지막 줄에 제목으로 이어 씁니다.",
        ));
    c.into()
}
