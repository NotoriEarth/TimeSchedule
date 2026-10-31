//! A day-sized editor sharing the manager's guarded Form/Editor and storage.
use crate::{
    app::{App, Message},
    editor::{Form, Kind},
    theme, worker,
};
use chrono::NaiveDate;
use iced::widget::{button, column, container, row, scrollable, space, text};
use iced::{window, Element, Length, Size, Task};

impl App {
    pub(crate) fn open_quick(&mut self, date: NaiveDate) -> Task<Message> {
        if self.editor.is_some() {
            self.notice = "작성 중인 내용을 저장하거나 닫은 뒤 다른 일정을 선택하세요.".into();
            return if self.editor_in_quick {
                self.focus_quick()
            } else {
                self.show_manager()
            };
        }
        self.quick_date = Some(date);
        self.quick_events.clear();
        self.error.clear();
        let open = if let Some(id) = self.quick_window {
            self.place_popup(self.widget.unwrap_or(self.manager), id)
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
        self.quick_loading = true;
        worker::job(
            store,
            move |s| s.list_events(&date.to_string(), &date.to_string()),
            move |result| Message::QuickLoaded(generation, initial, result),
        )
    }

    pub(crate) fn edit_quick(&mut self, id: Option<String>) -> Task<Message> {
        if self.editor.is_some() || self.quick_loading || self.busy || self.quick_window.is_none() {
            return Task::none();
        }
        let Some(date) = self.quick_date else {
            return Task::none();
        };
        let form = if let Some(id) = id {
            let Some(event) = self.quick_events.iter().find(|e| e.id == id) else {
                return Task::none();
            };
            Form::event(event.clone())
        } else {
            Form::new(Kind::Event, date.to_string(), None)
        };
        let edit = self.open_form(form);
        self.editor_in_quick = self.editor.is_some();
        Task::batch([edit, self.focus_quick()])
    }

    pub(crate) fn close_quick(&mut self) -> Task<Message> {
        self.quick_date = None;
        self.quick_events.clear();
        self.quick_loading = false;
        self.quick_generation += 1;
        self.quick_window.take().map_or_else(Task::none, |id| {
            self.placing_popups.remove(&id);
            window::close(id)
        })
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    let mut list = column![row![
        text(
            app.quick_date
                .map(|d| d.format("%Y년 %m월 %d일").to_string())
                .unwrap_or_default()
        )
        .size(18),
        space().width(Length::Fill),
        button("+ 일정").on_press_maybe(
            (!app.quick_loading && app.editor.is_none() && !app.busy)
                .then_some(Message::QuickEdit(None))
        )
    ]
    .spacing(8)]
    .spacing(10);
    if app.quick_loading {
        list = list.push(text("일정을 불러오는 중…").size(13));
    }
    if !app.error.is_empty() {
        list = list.push(text(&app.error).style(theme::danger));
    }
    if app.editor.is_some() {
        list = list.push(text("열려 있는 편집을 마친 뒤 선택해 주세요.").size(13));
    }
    for event in &app.quick_events {
        let time = if event.all_day {
            "하루 종일".into()
        } else {
            format!(
                "{}–{}",
                event.start_time.as_deref().unwrap_or(""),
                event.end_time.as_deref().unwrap_or("")
            )
        };
        list = list.push(
            button(
                column![
                    text(&event.title).size(15),
                    text(format!(
                        "{} · {}",
                        time,
                        if event.completed {
                            "완료"
                        } else {
                            "진행 전"
                        }
                    ))
                    .size(11)
                ]
                .spacing(4),
            )
            .width(Length::Fill)
            .padding(10)
            .style(theme::text_button)
            .on_press_maybe(
                (app.editor.is_none() && !app.quick_loading && !app.busy)
                    .then(|| Message::QuickEdit(Some(event.id.clone()))),
            ),
        );
    }
    container(scrollable(list))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::root)
        .into()
}
