//! A day-sized editor sharing the manager's guarded Form/Editor and storage.
use crate::{
    app::{App, Message},
    editor::{Form, Kind},
    theme, worker,
};
use chrono::NaiveDate;
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{window, Element, Length, Size, Task};

impl App {
    /// Open the day window for `date`, placed over `source` (the widget or
    /// the diary window it was opened from).
    pub(crate) fn open_quick(&mut self, date: NaiveDate, source: window::Id) -> Task<Message> {
        if self.editor.is_some() {
            self.notice = "작성 중인 내용을 저장하거나 닫은 뒤 다른 일정을 선택하세요.".into();
            return if self.editor_in_quick {
                self.focus_quick()
            } else {
                self.show_manager()
            };
        }
        self.quick_date = Some(date);
        self.quick_source = Some(source);
        self.calendar.quick = Default::default();
        self.calendar.offset = 0;
        self.calendar.show_completed = false;
        self.calendar.completed_page = Default::default();
        self.calendar.completed_offset = 0;
        self.calendar.completed_loading = false;
        self.calendar.detail = None;
        self.calendar.generation += 1;

        self.error.clear();
        let open = if let Some(id) = self.quick_window {
            self.place_popup(source, id)
        } else {
            let (id, open) = window::open(window::Settings {
                size: crate::views::EDITOR_SIZE,
                min_size: Some(Size::new(350.0, 330.0)),
                visible: false,
                exit_on_close_request: false,
                ..Default::default()
            });
            self.quick_window = Some(id);
            self.placing_popups.insert(id, window::Id::unique());
            open.map(Message::QuickReady)
        };
        Task::batch([open, self.load_quick(true)])
    }

    pub(crate) fn focus_quick(&self) -> Task<Message> {
        self.quick_window.map_or_else(Task::none, |id| {
            if self.placing_popups.contains_key(&id) {
                return Task::none();
            }
            window::minimize(id, false).chain(window::gain_focus(id))
        })
    }

    pub(crate) fn load_quick(&mut self, initial: bool) -> Task<Message> {
        let (Some(date), Some(store), Some(_)) =
            (self.quick_date, self.store.clone(), self.quick_window)
        else {
            return Task::none();
        };
        self.quick_generation += 1;
        let generation = self.quick_generation;
        let offset = self.calendar.offset;
        self.quick_loading = true;
        let page = worker::job(
            store,
            move |s| s.calendar_section(&date.to_string(), offset, false),
            move |result| {
                crate::calendar_ui::message(crate::calendar_ui::Action::QuickPage(
                    generation, initial, result,
                ))
            },
        );
        if self.calendar.show_completed {
            Task::batch([page, crate::calendar_ui::load_completed(self)])
        } else {
            page
        }
    }

    pub(crate) fn edit_quick(&mut self) -> Task<Message> {
        if self.editor.is_some() || self.quick_loading || self.busy || self.quick_window.is_none() {
            return Task::none();
        }
        let Some(date) = self.quick_date else {
            return Task::none();
        };
        let form = Form::new(Kind::Event, date.to_string(), None);
        let edit = self.open_form(form);
        self.editor_in_quick = self.editor.is_some();
        Task::batch([edit, self.focus_quick()])
    }

    pub(crate) fn close_quick(&mut self) -> Task<Message> {
        self.calendar.detail = None;
        self.calendar.generation += 1;
        self.quick_date = None;

        self.quick_loading = false;
        self.quick_generation += 1;
        self.quick_window.take().map_or_else(Task::none, |id| {
            self.placing_popups.remove(&id);
            window::close(id)
        })
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    use crate::calendar_ui::{message, Action};
    let mut list = column![
        text(
            app.quick_date
                .map(|d| d.format("%Y년 %m월 %d일").to_string())
                .unwrap_or_default()
        )
        .size(20),
        row![
            button("+ 일정").on_press(message(Action::New(Kind::Event, true))),
            button("+ 기록").on_press(message(Action::New(Kind::Entry, true)))
        ]
        .spacing(8)
    ]
    .spacing(10);
    if app.quick_loading {
        list = list.push(text("불러오는 중…").size(13));
    }
    if !app.error.is_empty() {
        list = list.push(text(&app.error).style(theme::danger));
    }
    let page = &app.calendar.quick;
    for item in page.items.iter().filter(|i| !i.completed) {
        list = list.push(crate::calendar_ui::item_view(app, item, 14.0, true));
    }
    if page.completed > 0 {
        list = list.push(
            button(text(format!(
                "{} 완료 {}개",
                if app.calendar.show_completed {
                    "▾"
                } else {
                    "▸"
                },
                page.completed
            )))
            .style(theme::text_button)
            .on_press(message(Action::ToggleCompleted)),
        );
        if app.calendar.show_completed {
            for item in &app.calendar.completed_page.items {
                list = list.push(crate::calendar_ui::item_view(app, item, 14.0, true));
            }
            if app.calendar.completed_loading {
                list = list.push(text("완료 항목 불러오는 중…").size(12));
            }
            list = list.push(
                row![
                    button("‹ 완료 이전").on_press_maybe(
                        (app.calendar.completed_offset > 0 && !app.calendar.completed_loading)
                            .then_some(message(Action::CompletedPage(-1)))
                    ),
                    button("완료 다음 ›").on_press_maybe(
                        (app.calendar.completed_page.has_more && !app.calendar.completed_loading)
                            .then_some(message(Action::CompletedPage(1)))
                    )
                ]
                .spacing(8),
            );
        }
    }
    let pager = row![
        button("‹ 이전").on_press_maybe(
            (app.calendar.offset > 0 && !app.quick_loading).then_some(message(Action::Page(-1)))
        ),
        text(format!(
            "전체 {}개 · {}쪽",
            page.total,
            app.calendar.offset / 50 + 1
        ))
        .size(12),
        button("다음 ›").on_press_maybe(
            (page.has_more && !app.quick_loading).then_some(message(Action::Page(1)))
        )
    ]
    .spacing(12);
    container(column![scrollable(list).height(Length::Fill), pager].spacing(12))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::root)
        .into()
}
