#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod draft;
mod history;
mod windows_probe;
use draft::{CloseIntent, Draft};
use iced::widget::{
    button, column, container, mouse_area, opaque, row, scrollable, stack, text, text_editor,
    text_input, Column,
};
use iced::{window, Color, Element, Font, Length, Size, Subscription, Task, Theme};
use windows_probe::Action as NativeAction;
const DAYS: [&str; 7] = [
    "26  MON", "27  TUE", "28  WED", "29  THU", "30  FRI", "31  SAT", "01  SUN",
];

fn main() -> iced::Result {
    iced::daemon(App::boot, App::update, App::view)
        .title(|_: &App, _| String::from("TimeSchedule · Rust UI Prototype"))
        .default_font(Font::with_name("Malgun Gothic"))
        .theme(|_: &App, _| Theme::Light)
        .subscription(App::subscription)
        .run()
}
struct App {
    manager: window::Id,
    widget: Option<window::Id>,
    records: Vec<Vec<String>>,
    expanded: std::collections::HashSet<(usize, usize)>,
    editor: Option<usize>,
    content: text_editor::Content,
    draft: Draft,
    query: String,
    notice: String,
    native_busy: bool,
    locked: bool,
    history: history::History<(String, text_editor::Cursor)>,
    composing: bool,
}
#[derive(Debug, Clone)]
enum Message {
    Ready(window::Id),
    WidgetReady(window::Id),
    Toggle(usize, usize),
    Edit(usize),
    EditAction(text_editor::Action),
    Save,
    CloseEditor,
    CloseWindow(window::Id),
    Continue,
    Discard,
    Query(String),
    Native(NativeAction),
    NativeDone(Result<windows_probe::Report, String>),
    ShowManager,
    Escape,
    Undo,
    Redo,
    DragWidget,
    Ignore,
    Composition(window::Id, bool),
}
impl App {
    fn boot() -> (Self, Task<Message>) {
        let desktop = std::env::args().any(|s| s == "--probe=desktop");
        let (manager, open) = window::open(window::Settings {
            size: Size::new(1080.0, 760.0),
            min_size: Some(Size::new(800.0, 620.0)),
            position: window::Position::Centered,
            exit_on_close_request: false,
            ..Default::default()
        });
        let (widget, widget_task) = if desktop {
            let (id, open) = window::open(window::Settings {
                size: Size::new(340.0, 280.0),
                position: window::Position::Specific(iced::Point::new(40.0, 40.0)),
                decorations: false,
                resizable: false,
                exit_on_close_request: false,
                ..Default::default()
            });
            (Some(id), open.map(Message::WidgetReady))
        } else {
            (None, Task::none())
        };
        (
            Self {
                manager,
                widget,
                records: (0..7)
                    .map(|day| {
                        (1..=100)
                            .map(|i| format!("{} · 기록 {i:03} · 커밋 / 트러블슈팅", DAYS[day]))
                            .collect()
                    })
                    .collect(),
                expanded: Default::default(),
                editor: None,
                content: text_editor::Content::new(),
                draft: Draft::default(),
                query: String::new(),
                notice: "시제품 · 700개 테스트 기록 · 저장은 실행 중 메모리에만 유지됩니다".into(),
                native_busy: false,
                locked: true,
                history: Default::default(),
                composing: false,
            },
            Task::batch([open.map(Message::Ready), widget_task]),
        )
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            window::close_requests().map(Message::CloseWindow),
            iced::event::listen_with(|event, status, id| {
                if let iced::Event::InputMethod(event) = &event {
                    match event {
                        iced::advanced::input_method::Event::Preedit(value, _) => {
                            return Some(Message::Composition(id, !value.is_empty()))
                        }
                        iced::advanced::input_method::Event::Commit(_)
                        | iced::advanced::input_method::Event::Closed => {
                            return Some(Message::Composition(id, false))
                        }
                        _ => {}
                    }
                }
                if status == iced::event::Status::Ignored {
                    if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                        ..
                    }) = event
                    {
                        return Some(Message::Escape);
                    }
                }
                None
            }),
        ])
    }
    fn close(&mut self, intent: CloseIntent) -> Task<Message> {
        if self.editor.is_some() && self.composing {
            self.notice = "한글 조합을 완료한 뒤 다시 닫아 주세요. 작성 내용은 유지됩니다.".into();
            return iced::widget::operation::focus("diary-editor");
        }
        if self.editor.is_some() && !self.draft.request_close(&self.content.text(), intent) {
            return Task::none();
        }
        self.finish_close(intent)
    }
    fn finish_close(&mut self, intent: CloseIntent) -> Task<Message> {
        self.editor = None;
        self.composing = false;
        self.draft = Draft::default();
        match intent {
            CloseIntent::Editor => Task::none(),
            CloseIntent::Application => iced::exit(),
        }
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        if self.draft.pending.is_some()
            && !matches!(
                message,
                Message::Continue | Message::Discard | Message::Escape | Message::NativeDone(_)
            )
        {
            return Task::none();
        }
        match message {
            Message::Composition(id, active) if id == self.manager && self.editor.is_some() => {
                self.composing = active
            }
            Message::Ready(id) => {
                debug_assert_eq!(id, self.manager);
            }
            Message::WidgetReady(id) => {
                return window::run(id, |w| windows_probe::run(w, NativeAction::Inspect))
                    .map(Message::NativeDone)
            }
            Message::Toggle(day, record) if self.editor.is_none() => {
                if !self.expanded.insert((day, record)) {
                    self.expanded.remove(&(day, record));
                }
            }
            Message::Edit(day) if self.editor.is_none() => {
                self.content = text_editor::Content::new();
                self.draft = Draft::default();
                self.editor = Some(day);
                self.draft.saved = self.content.text();
                self.history = Default::default();
                self.composing = false;
                return iced::widget::operation::focus("diary-editor");
            }
            Message::EditAction(action) if self.editor.is_some() => {
                if action.is_edit() {
                    let previous = (self.content.text(), self.content.cursor());
                    self.content.perform(action);
                    if self.content.text().len() > 300_000 {
                        self.content = text_editor::Content::with_text(&previous.0);
                        self.content.move_to(previous.1);
                        self.notice = "시제품 본문은 UTF-8 300KB까지 입력할 수 있습니다.".into();
                    } else if self.content.text() != previous.0 {
                        self.history.record(previous);
                    }
                } else {
                    self.content.perform(action);
                }
            }
            Message::Undo | Message::Redo if self.editor.is_some() && !self.composing => {
                let current = (self.content.text(), self.content.cursor());
                let target = if matches!(message, Message::Undo) {
                    self.history.undo(current)
                } else {
                    self.history.redo(current)
                };
                if let Some((text, cursor)) = target {
                    self.content = text_editor::Content::with_text(&text);
                    self.content.move_to(cursor);
                }
                return iced::widget::operation::focus("diary-editor");
            }
            Message::Save if !self.composing => {
                if let Some(day) = self.editor {
                    let value = self.content.text();
                    if value.trim().is_empty() {
                        self.notice = "내용을 입력해 주세요.".into();
                    } else {
                        self.records[day].insert(0, value);
                        self.expanded.retain(|(d, _)| *d != day);
                        self.notice =
                            "테스트 기록을 메모리에 저장했습니다. 시제품 종료 시 사라집니다."
                                .into();
                        return self.finish_close(CloseIntent::Editor);
                    }
                }
            }
            Message::CloseEditor => return self.close(CloseIntent::Editor),
            Message::CloseWindow(id) => {
                if Some(id) == self.widget {
                    self.widget = None;
                    return window::close(id);
                }
                return self.close(CloseIntent::Application);
            }
            Message::Continue => {
                self.draft.keep_editing();
                return iced::widget::operation::focus("diary-editor");
            }
            Message::Discard => {
                if let Some(intent) = self.draft.discard() {
                    return self.finish_close(intent);
                }
            }
            Message::Escape => {
                if self.draft.pending.is_some() {
                    self.draft.keep_editing();
                    return iced::widget::operation::focus("diary-editor");
                } else if self.editor.is_some() {
                    return self.close(CloseIntent::Editor);
                }
            }
            Message::Query(value) if self.editor.is_none() => self.query = value,
            Message::Native(action) if !self.native_busy && self.editor.is_none() => {
                if let Some(id) = self.widget {
                    self.native_busy = true;
                    return window::run(id, move |w| windows_probe::run(w, action))
                        .map(Message::NativeDone);
                }
            }
            Message::NativeDone(result) => {
                self.native_busy = false;
                match result {
                    Ok(report) => {
                        self.locked = report.locked;
                        self.notice = report.message;
                    }
                    Err(e) => self.notice = format!("연결 진단: {e}"),
                }
            }
            Message::ShowManager if self.editor.is_none() => {
                return window::gain_focus(self.manager)
            }
            Message::DragWidget if self.editor.is_none() => {
                if let Some(id) = self.widget {
                    return window::drag(id);
                }
            }
            _ => {}
        }
        Task::none()
    }
    fn day(&self, day: usize) -> Element<'_, Message> {
        let mut entries = Column::new().spacing(6).width(Length::Fill);
        for (index, value) in self.records[day]
            .iter()
            .enumerate()
            .filter(|(_, s)| self.query.is_empty() || s.contains(&self.query))
        {
            let expanded = self.expanded.contains(&(day, index));
            let summary: String = value
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(45)
                .collect();
            entries = entries.push(
                button(text(format!("{} {summary}", if expanded { "−" } else { "+" })).size(13))
                    .style(button::text)
                    .on_press(Message::Toggle(day, index))
                    .width(Length::Fill),
            );
            if expanded {
                entries = entries.push(container(text(format!("{value}\n\n문제: 한글 입력 확인\n해결: 저장 전 조합 상태 점검\n근거: 테스트용 예시 기록")).size(13)).padding(8).style(container::rounded_box));
            }
        }
        paper(
            column![
                row![
                    text(DAYS[day]).size(17),
                    iced::widget::space().width(Length::Fill),
                    button("+ 추가")
                        .on_press(Message::Edit(day))
                        .style(button::text)
                ]
                .align_y(iced::Alignment::Center),
                scrollable(entries)
                    .id(format!("day-{day}"))
                    .height(Length::Fill),
            ]
            .spacing(8)
            .height(Length::Fill)
            .into(),
        )
    }
    fn view(&self, id: window::Id) -> Element<'_, Message> {
        if Some(id) == self.widget {
            return container(
                column![
                    mouse_area(text("2026  /  10   ↔").size(28)).on_press(Message::DragWidget),
                    text("만년 다이어리 · Rust 위젯").size(16),
                    text("MON   TUE   WED   THU   FRI   SAT   SUN").size(11),
                    text(" 26      27      28      29      30      31      01").size(13),
                    button("주간 다이어리 열기").on_press(Message::ShowManager),
                    text("연결·투명도·잠금은 상세창에서 시험").size(12),
                ]
                .spacing(16),
            )
            .padding(20)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(paper_style)
            .into();
        }
        let overview = paper(
            column![
                text("OCTOBER").size(26),
                text("2026.10.26 — 11.01").size(15),
                text("이번 주 계획").size(19),
                text("• 한글 편집 확인\n• 날짜별 스크롤\n• 기록 펼치기\n• 미저장 확인 팝업")
                    .size(15),
                text("각 날짜에 100개 기록\n이 화면은 성능 비교 전 시제품입니다.").size(12)
            ]
            .spacing(15)
            .into(),
        );
        let weeks = column![
            row![overview, self.day(0), self.day(1), self.day(2)]
                .spacing(2)
                .height(Length::Fill),
            row![self.day(3), self.day(4), self.day(5), self.day(6)]
                .spacing(2)
                .height(Length::Fill)
        ]
        .spacing(2)
        .height(Length::Fill);
        let mut body = column![
            row![
                text("Weekly Diary").size(28),
                iced::widget::space().width(Length::Fill),
                text_input("테스트 기록 검색", &self.query)
                    .on_input(Message::Query)
                    .width(230)
            ],
            text(&self.notice).size(12),
        ]
        .spacing(10);
        if self.widget.is_some() {
            let control = |label: &'static str, action| {
                let b = button(label);
                if self.native_busy {
                    b
                } else {
                    b.on_press(Message::Native(action))
                }
            };
            body = body.push(
                row![
                    control("진단", NativeAction::Inspect),
                    control("바탕화면 연결", NativeAction::Attach),
                    control("분리", NativeAction::Detach),
                    control("40%", NativeAction::Opacity(40)),
                    control("82%", NativeAction::Opacity(82)),
                    control("100%", NativeAction::Opacity(100)),
                    control(
                        if self.locked {
                            "잠금 해제"
                        } else {
                            "잠금"
                        },
                        NativeAction::Lock(!self.locked)
                    )
                ]
                .spacing(5),
            );
        }
        let base: Element<'_, Message> = container(body.push(weeks))
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style::default().background(Color::from_rgb8(244, 239, 218)))
            .into();
        if self.draft.pending.is_some() {
            let dialog = column![
                text("저장하지 않고 닫을까요?").size(23),
                text("작성 중인 테스트 기록이 사라집니다."),
                row![
                    button("계속 작성").on_press(Message::Continue),
                    button("저장하지 않고 닫기")
                        .on_press(Message::Discard)
                        .style(button::danger)
                ]
                .spacing(12)
            ]
            .spacing(22);
            return modal(base, dialog.into(), None);
        }
        if let Some(day) = self.editor {
            let dialog = column![
                text(format!("{} · 기록 추가", DAYS[day])).size(22),
                text("한글 조합 · Ctrl+Z / Ctrl+Y · 최근 편집 32회 복원").size(13),
                text_editor(&self.content)
                    .id("diary-editor")
                    .placeholder("만년 다이어리 일정 기록")
                    .on_action(Message::EditAction)
                    .key_binding(editor_binding)
                    .height(280),
                row![
                    button("닫기").on_press(Message::CloseEditor),
                    button("테스트 기록 저장").on_press(Message::Save)
                ]
                .spacing(12)
            ]
            .spacing(15);
            return modal(base, dialog.into(), Some(Message::CloseEditor));
        }
        base
    }
}
fn paper_style(_: &Theme) -> container::Style {
    container::Style::default()
        .background(Color::from_rgb8(255, 253, 246))
        .border(iced::Border {
            color: Color::from_rgb8(217, 214, 203),
            width: 1.0,
            radius: 2.0.into(),
        })
}
fn paper(content: Element<'_, Message>) -> Element<'_, Message> {
    container(content)
        .padding(12)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(paper_style)
        .into()
}
fn modal<'a>(
    base: Element<'a, Message>,
    content: Element<'a, Message>,
    backdrop: Option<Message>,
) -> Element<'a, Message> {
    let shade = container(iced::widget::space())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| {
            container::Style::default().background(Color::from_rgba(0.12, 0.14, 0.14, 0.42))
        });
    let shade: Element<'a, Message> = mouse_area(shade)
        .on_press(backdrop.unwrap_or(Message::Ignore))
        .on_scroll(|_| Message::Ignore)
        .into();
    let dialog = opaque(container(content).padding(24).width(580).style(paper_style));
    stack![
        base,
        shade,
        container(dialog)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
    ]
    .into()
}

fn editor_binding(key: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    if matches!(key.status, text_editor::Status::Focused { .. }) && key.modifiers.command() {
        match key.key.to_latin(key.physical_key) {
            Some('z') => {
                return Some(text_editor::Binding::Custom(if key.modifiers.shift() {
                    Message::Redo
                } else {
                    Message::Undo
                }))
            }
            Some('y') => return Some(text_editor::Binding::Custom(Message::Redo)),
            _ => {}
        }
    }
    text_editor::Binding::from_key_press(key)
}

#[cfg(test)]
mod app_tests {
    use super::*;
    #[test]
    fn close_during_composition_preserves_editor_until_commit() {
        let (mut app, _) = App::boot();
        let _ = app.update(Message::Edit(0));
        let _ = app.update(Message::Composition(app.manager, true));
        let _ = app.update(Message::CloseEditor);
        assert_eq!(app.editor, Some(0));
        assert!(app.draft.pending.is_none());
    }
    #[test]
    fn actual_editor_undo_redo_restores_korean_text() {
        let (mut app, _) = App::boot();
        let _ = app.update(Message::Edit(0));
        let initial = app.content.text();
        let _ = app.update(Message::EditAction(text_editor::Action::Edit(
            text_editor::Edit::Paste(std::sync::Arc::new("만년 다이어리".into())),
        )));
        let typed = app.content.text();
        assert!(typed.contains("만년 다이어리"));
        let _ = app.update(Message::Undo);
        assert_eq!(app.content.text(), initial);
        let _ = app.update(Message::Redo);
        assert_eq!(app.content.text(), typed);
    }
    #[test]
    fn untouched_editor_closes_without_confirmation() {
        let (mut app, _) = App::boot();
        let _ = app.update(Message::Edit(0));
        let _ = app.update(Message::CloseEditor);
        assert!(app.editor.is_none());
        assert!(app.draft.pending.is_none());
    }
    #[test]
    fn native_failure_does_not_change_confirmed_lock_state() {
        let (mut app, _) = App::boot();
        app.locked = true;
        let _ = app.update(Message::NativeDone(Err("not attached".into())));
        assert!(app.locked);
        let _ = app.update(Message::NativeDone(Ok(windows_probe::Report {
            message: "unlocked".into(),
            locked: false,
        })));
        assert!(!app.locked);
    }
}
