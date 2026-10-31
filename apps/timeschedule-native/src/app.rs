use crate::{
    editor::{CloseIntent, Confirmation, Editor, Form, Kind},
    worker::{self, Scope, Snapshot},
};
use chrono::{Datelike, Duration, Local, NaiveDate};
use iced::widget::text_editor;
use iced::{window, Element, Size, Subscription, Task};
use raw_window_handle::RawWindowHandle;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};
use timeschedule_core::{
    dates::{calendar_days, shift_month, week_after_sunday, week_containing},
    diary::{Entry, EntrySummary, ImportResult, SearchHit},
    storage::Storage,
};
use timeschedule_desktop::{
    paths::{self, InstanceLock},
    preferences::{self, AppTheme, Preferences},
    tray::{NativeTray, TrayAction},
    windows,
};

fn fit_widget_before_attach(
    window: &windows::NativeWindow,
    prefs: &Preferences,
) -> Result<(), String> {
    let (x, y) = match (prefs.widget_x, prefs.widget_y) {
        (Some(x), Some(y)) => (x, y),
        _ => windows::position(window)?,
    };
    // Also clamp first-run geometry, whose logical size may exceed the physical
    // work area at high DPI. This same path permits retry after a failed attach.
    windows::restore_position(window, x, y)
}
fn widget_window_settings() -> window::Settings {
    window::Settings {
        size: Size::new(500.0, 620.0),
        // Native fitting may shrink this on small/high-DPI displays.
        min_size: None,
        decorations: false,
        resizable: false,
        visible: false,
        // Saved physical pixels are restored by the native adapter, never
        // reinterpreted here as Iced logical coordinates.
        position: window::Position::Specific(iced::Point::new(30.0, 40.0)),
        exit_on_close_request: false,
        platform_specific: window::settings::PlatformSpecific {
            skip_taskbar: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Month,
    Week,
    Projects,
    Records,
    Tasks,
    Notes,
    Settings,
}
impl Page {
    pub fn label(self) -> &'static str {
        match self {
            Self::Month => "월간 다이어리",
            Self::Week => "주간 다이어리",
            Self::Projects => "프로젝트",
            Self::Records => "기록 보관함",
            Self::Tasks => "할 일",
            Self::Notes => "메모",
            Self::Settings => "설정",
        }
    }
}
#[derive(Clone, Debug)]
pub enum Field {
    Title(String),
    Date(String),
    Project(Option<String>),
    EntryKind(String),
    Source(String),
    Repo(String),
    Start(String),
    End(String),
    Color(String),
    AllDay(bool),
    Completed(bool),
}
#[derive(Clone, Copy, Debug)]
pub enum NativeAction {
    Attach,
    Show,
    Hide,
    Recover,
    Lock(bool),
    Opacity(u8),
    Position,
}
#[derive(Clone, Debug)]
pub struct NativeReport {
    pub attached: bool,
    pub message: String,
    pub locked: bool,
    pub opacity: Option<u8>,
    pub visible: Option<bool>,
    pub position: Option<(i32, i32)>,
}
#[derive(Clone, Debug)]
pub enum Message {
    ToggleTheme,
    Resized(window::Id, Size),
    Ready(window::Id),
    WidgetReady(window::Id),
    CloseWindow(window::Id),
    WindowClosed(window::Id),
    Tick,
    Navigate(Page),
    Month(i32),
    Week(i64),
    Today,
    OpenWeek(NaiveDate),
    HoverWeek(Option<NaiveDate>),
    SelectDay(NaiveDate),
    WidgetMonth(i32),
    WidgetToday,
    WidgetDate(NaiveDate),
    Project(Option<String>),
    New(Kind, Option<String>),
    EditForm(Form),
    FetchForm(SearchHit),
    FormLoaded(u64, u64, Result<Form, String>),
    Load,
    Loaded(u64, Result<Snapshot, String>),
    More,
    MoreLoaded(u64, Result<Vec<EntrySummary>, String>),
    ToggleEntry(String),
    EntryLoaded(u64, String, Result<Entry, String>),
    ToggleEvent(String),
    Source(String),
    SourceDone(Result<(), String>),
    Field(Field),
    Body(text_editor::Action),
    Undo,
    Redo,
    Save,
    Delete,
    ConfirmDelete,
    Keep,
    Discard,
    CloseEditor,
    Escape,
    Composition(window::Id, bool),
    Saved(u64, Result<(), String>),
    ToggleTask(String),
    MutationDone(Result<(), String>),
    Search(String),
    SearchDue(u64, String),
    SearchDone(u64, Result<Vec<SearchHit>, String>),
    Import,
    Imported(Result<ImportResult, String>),
    Draft(bool),
    Drafted(u64, u64, Result<Form, String>),
    Native(NativeAction),
    NativeDone(NativeAction, Result<NativeReport, String>),
    PrefsDone(Result<(), String>),
    ShowManager,
    DragWidget,
    Quit,
    Ignore,
}

pub struct App {
    pub manager: window::Id,
    pub manager_size: Size,
    pub widget: Option<window::Id>,
    pub store: Option<Arc<Storage>>,
    pub data_dir: PathBuf,
    pub preferences: Preferences,
    pub snapshot: Snapshot,
    pub page: Page,
    pub month: NaiveDate,
    pub selected: NaiveDate,
    pub week: NaiveDate,
    pub widget_month: NaiveDate,
    pub hovered_week: Option<NaiveDate>,
    pub project: Option<String>,
    pub expanded: HashSet<String>,
    pub expanded_events: HashSet<String>,
    pub bodies: HashMap<String, Entry>,
    pub entry_errors: HashMap<String, String>,
    pub editor: Option<Editor>,
    pub editor_generation: u64,
    pub view_generation: u64,
    pub search_generation: u64,
    pub search: String,
    pub search_results: Vec<SearchHit>,
    pub searching: bool,
    pub loading: bool,
    pub more: bool,
    pub paging: bool,
    pub busy: bool,
    pub native_busy: bool,
    pub notice: String,
    pub error: String,
    pub desktop_message: String,
    pub widget_attached: bool,
    pub tray: Option<NativeTray>,
    _instance: Option<InstanceLock>,
    ticks: u32,
    pub manager_visible: bool,
    prefs_saving: bool,
    prefs_pending: bool,
    quit_when_idle: bool,
    native_pending: Option<NativeAction>,
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let today = Local::now().date_naive();
        let month = today.with_day(1).unwrap();
        let mut errors = Vec::new();
        let data_dir = match paths::resolve_data_dir(
            std::env::var_os("TIMESCHEDULE_DATA_DIR").map(PathBuf::from),
            true,
        ) {
            Ok(p) => p,
            Err(e) => {
                errors.push(e);
                PathBuf::new()
            }
        };
        let instance = if data_dir.as_os_str().is_empty() {
            None
        } else {
            match InstanceLock::acquire(&data_dir) {
                Ok(lock) => Some(lock),
                Err(e) => {
                    errors.push(e);
                    None
                }
            }
        };
        let prefs = if instance.is_some() {
            preferences::load(&data_dir).unwrap_or_else(|e| {
                errors.push(e);
                Preferences::default()
            })
        } else {
            Preferences::default()
        };
        let store = if instance.is_some() {
            match Storage::open(data_dir.join("schedule.db")) {
                Ok(s) => Some(Arc::new(s)),
                Err(e) => {
                    errors.push(e);
                    None
                }
            }
        } else {
            None
        };
        let tray = match NativeTray::new() {
            Ok(t) => Some(t),
            Err(e) => {
                errors.push(format!("트레이: {e}"));
                None
            }
        };
        let (manager, open) = window::open(window::Settings {
            size: Size::new(1160.0, 820.0),
            min_size: Some(Size::new(800.0, 620.0)),
            position: window::Position::Centered,
            exit_on_close_request: false,
            ..Default::default()
        });
        if let Some(error) = crate::DPI_INIT_ERROR.get() {
            errors.push(error.clone());
        }
        let mut app = Self {
            manager,
            manager_size: Size::new(1160.0, 820.0),
            widget: None,
            store,
            data_dir,
            preferences: prefs,
            snapshot: Snapshot::default(),
            page: Page::Month,
            month,
            selected: today,
            week: week_containing(today).from,
            widget_month: month,
            hovered_week: None,
            project: None,
            expanded: HashSet::new(),
            expanded_events: HashSet::new(),
            bodies: HashMap::new(),
            entry_errors: HashMap::new(),
            editor: None,
            editor_generation: 0,
            view_generation: 0,
            search_generation: 0,
            search: String::new(),
            search_results: vec![],
            searching: false,
            loading: false,
            more: false,
            paging: false,
            busy: false,
            native_busy: false,
            notice: "내 컴퓨터에 보관하는 만년 다이어리 · Rust 네이티브".into(),
            error: errors.join("\n"),
            desktop_message: "위젯을 준비하고 있습니다.".into(),
            widget_attached: false,
            tray,
            _instance: instance,
            ticks: 0,
            manager_visible: true,
            prefs_saving: false,
            prefs_pending: false,
            quit_when_idle: false,
            native_pending: None,
        };
        app.log(&format!(
            "startup data={} storage={} errors={}",
            app.data_dir.display(),
            app.store.is_some(),
            app.error
        ));
        let widget = app.open_widget();
        let load = app.refresh();
        (app, Task::batch([open.map(Message::Ready), widget, load]))
    }
    pub fn title(&self, id: window::Id) -> String {
        if Some(id) == self.widget {
            "TimeSchedule Native · 바탕화면".into()
        } else {
            "TimeSchedule Native · 만년 다이어리".into()
        }
    }
    pub fn log(&self, value: &str) {
        use std::io::Write;
        if self.data_dir.as_os_str().is_empty() || self._instance.is_none() {
            return;
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.data_dir.join("native.log"))
        {
            let _ = writeln!(f, "{} {value}", Local::now().to_rfc3339());
        }
    }
    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            window::close_requests().map(Message::CloseWindow),
            window::close_events().map(Message::WindowClosed),
            iced::time::every(std::time::Duration::from_millis(500)).map(|_| Message::Tick),
            iced::event::listen_with(|event, status, id| {
                if let iced::Event::Window(window::Event::Resized(size)) = &event {
                    return Some(Message::Resized(id, *size));
                }
                if let iced::Event::InputMethod(ime) = &event {
                    match ime {
                        iced::advanced::input_method::Event::Preedit(s, _) => {
                            return Some(Message::Composition(id, !s.is_empty()))
                        }
                        iced::advanced::input_method::Event::Commit(_)
                        | iced::advanced::input_method::Event::Closed => {
                            return Some(Message::Composition(id, false))
                        }
                        _ => {}
                    }
                }
                if status == iced::event::Status::Ignored
                    && matches!(
                        event,
                        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                            ..
                        })
                    )
                {
                    return Some(Message::Escape);
                }
                None
            }),
        ])
    }
    pub fn scope(&self) -> Scope {
        let days = calendar_days(self.month);
        let widget_days = calendar_days(self.widget_month);
        let (event_from, event_to) = if self.page == Page::Week {
            (self.week, self.week + Duration::days(6))
        } else {
            (days[0], days[41])
        };
        let (from, to) = if matches!(self.page, Page::Projects | Page::Records) {
            ("1900-01-01".into(), "9999-12-31".into())
        } else {
            (event_from.to_string(), event_to.to_string())
        };
        Scope {
            from,
            to,
            event_from: event_from.to_string(),
            event_to: event_to.to_string(),
            widget_from: widget_days[0].to_string(),
            widget_to: widget_days[41].to_string(),
            today: Local::now().date_naive().to_string(),
            project: if self.page == Page::Projects {
                self.project.clone()
            } else {
                None
            },
        }
    }
    pub fn refresh(&mut self) -> Task<Message> {
        self.view_generation += 1;
        let generation = self.view_generation;
        self.expanded.clear();
        self.bodies.clear();
        self.entry_errors.clear();
        self.paging = false;
        // The previous snapshot may remain visible while loading, but it cannot
        // provide an offset for the new scope, including after a failed load.
        self.more = false;
        self.loading = false;
        let Some(store) = self.store.clone() else {
            return Task::none();
        };
        self.loading = true;
        let scope = self.scope();
        worker::job(
            store,
            move |s| worker::snapshot(s, &scope),
            move |result| Message::Loaded(generation, result),
        )
    }
    fn open_widget(&mut self) -> Task<Message> {
        if self.widget.is_some() {
            return Task::none();
        }
        let (id, open) = window::open(widget_window_settings());
        self.widget = Some(id);
        open.map(Message::WidgetReady)
    }
    fn native(&mut self, action: NativeAction) -> Task<Message> {
        if self.native_busy {
            if !matches!(action, NativeAction::Position) {
                self.native_pending = Some(action);
            }
            return Task::none();
        }
        let Some(id) = self.widget else {
            self.native_pending = Some(action);
            return self.open_widget();
        };
        self.native_busy = true;
        let prefs = self.preferences.clone();
        window::run(id, move |window| {
            let handle = window.window_handle().map_err(|e| e.to_string())?;
            let RawWindowHandle::Win32(raw) = handle.as_raw() else {
                return Err("Windows 전용 바탕화면 기능입니다.".into());
            };
            let owned = unsafe { windows::NativeWindow::iced_widget(raw.hwnd.get()) };
            let mut visible = None;
            match action {
                NativeAction::Attach => {
                    fit_widget_before_attach(&owned, &prefs)?;
                    windows::attach(&owned)?;
                    windows::set_opacity(&owned, prefs.widget_opacity)?;
                    windows::set_locked(&owned, prefs.widget_locked)?;
                    windows::set_visible(&owned, prefs.widget_visible)?;
                    visible = Some(prefs.widget_visible);
                }
                NativeAction::Show => {
                    if !windows::status(&owned)?.attached {
                        fit_widget_before_attach(&owned, &prefs)?;
                        windows::attach(&owned)?;
                        windows::set_opacity(&owned, prefs.widget_opacity)?;
                        windows::set_locked(&owned, prefs.widget_locked)?;
                    }
                    windows::set_visible(&owned, true)?;
                    visible = Some(true);
                }
                NativeAction::Hide => {
                    windows::set_visible(&owned, false)?;
                    visible = Some(false);
                }
                NativeAction::Recover => {
                    if !windows::status(&owned)?.attached {
                        fit_widget_before_attach(&owned, &prefs)?;
                        windows::attach(&owned)?;
                        windows::set_opacity(&owned, prefs.widget_opacity)?;
                        windows::set_locked(&owned, prefs.widget_locked)?;
                    }
                    windows::recover_position(&owned)?;
                    windows::set_visible(&owned, true)?;
                    visible = Some(true);
                }
                NativeAction::Lock(value) => windows::set_locked(&owned, value)?,
                NativeAction::Opacity(value) => windows::set_opacity(&owned, value)?,
                NativeAction::Position => {}
            }
            let info = windows::status(&owned)?;
            let position = if info.attached {
                windows::position(&owned).ok()
            } else {
                None
            };
            Ok(NativeReport {
                attached: info.attached,
                message: info.message,
                locked: info.locked,
                opacity: info.opacity_percent,
                visible,
                position,
            })
        })
        .map(move |result| Message::NativeDone(action, result))
    }
    fn save_preferences(&mut self) -> Task<Message> {
        if self._instance.is_none() {
            return Task::none();
        }
        if self.prefs_saving {
            self.prefs_pending = true;
            return Task::none();
        }
        self.prefs_saving = true;
        self.prefs_pending = false;
        let dir = self.data_dir.clone();
        let prefs = self.preferences.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || preferences::save(&dir, &prefs))
                    .await
                    .map_err(|e| e.to_string())?
            },
            Message::PrefsDone,
        )
    }
    fn open_form(&mut self, form: Form) -> Task<Message> {
        if self.editor.is_some() {
            self.notice = "현재 편집을 먼저 마쳐 주세요.".into();
            return Task::none();
        }
        self.editor_generation += 1;
        self.editor = Some(Editor::new(form, self.editor_generation));
        iced::widget::operation::focus("editor-title")
    }
    fn request_close(&mut self, intent: CloseIntent) -> Task<Message> {
        if let Some(editor) = &mut self.editor {
            if !editor.request_close(intent) {
                return Task::none();
            }
        }
        self.finish_close(intent)
    }
    fn finish_close(&mut self, intent: CloseIntent) -> Task<Message> {
        self.editor = None;
        self.editor_generation += 1;
        match intent {
            CloseIntent::Editor => Task::none(),
            CloseIntent::Manager => {
                if self.tray.is_none() {
                    return self.exit_when_idle();
                }
                self.manager_visible = false;
                window::set_mode(self.manager, window::Mode::Hidden)
            }
            CloseIntent::Application => self.exit_when_idle(),
        }
    }
    fn exit_when_idle(&mut self) -> Task<Message> {
        if self.busy || self.prefs_saving || self.native_busy {
            self.quit_when_idle = true;
            self.notice = "진행 중인 저장·Git·설정 작업을 마친 뒤 종료합니다.".into();
            return Task::none();
        }
        self.log("clean exit");
        iced::exit()
    }
    fn show_manager(&mut self) -> Task<Message> {
        self.manager_visible = true;
        Task::batch([
            window::set_mode(self.manager, window::Mode::Windowed),
            window::minimize(self.manager, false),
        ])
        .chain(window::gain_focus(self.manager))
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Background completions are allowed during modals; all unrelated user edits are blocked.
        if self
            .editor
            .as_ref()
            .is_some_and(|e| e.confirmation.is_some())
            && !matches!(
                message,
                Message::Keep
                    | Message::Discard
                    | Message::ConfirmDelete
                    | Message::Escape
                    | Message::Tick
                    | Message::NativeDone(..)
                    | Message::PrefsDone(_)
                    | Message::Loaded(..)
                    | Message::SearchDone(..)
                    | Message::Saved(..)
                    | Message::MutationDone(_)
                    | Message::Imported(_)
                    | Message::Drafted(..)
                    | Message::EntryLoaded(..)
                    | Message::FormLoaded(..)
                    | Message::Composition(..)
                    | Message::WindowClosed(_)
                    | Message::Resized(..)
            )
        {
            return Task::none();
        }
        match message {
            Message::ToggleTheme => {
                self.preferences.theme = match self.preferences.theme {
                    AppTheme::Midnight => AppTheme::Porcelain,
                    AppTheme::Porcelain => AppTheme::Midnight,
                };
                return self.save_preferences();
            }
            Message::Resized(id, size) if id == self.manager => self.manager_size = size,
            Message::Ready(id) => self.log(&format!("manager ready {id:?}")),
            Message::WidgetReady(id) => {
                self.log(&format!("widget ready {id:?}"));
                return self.native(NativeAction::Attach);
            }
            Message::WindowClosed(id) => {
                if Some(id) == self.widget {
                    self.widget = None;
                    self.native_busy = false;
                    self.desktop_message =
                        "위젯 창이 종료되었습니다. 설정에서 다시 표시하세요.".into();
                }
            }
            Message::CloseWindow(id) => {
                return if Some(id) == self.widget {
                    self.native(NativeAction::Hide)
                } else {
                    self.request_close(CloseIntent::Manager)
                }
            }
            Message::Tick => {
                if self.quit_when_idle && !self.busy && !self.prefs_saving && !self.native_busy {
                    self.quit_when_idle = false;
                    return self.request_close(CloseIntent::Application);
                }
                self.ticks += 1;
                let action = self.tray.as_ref().and_then(|tray| tray.try_recv());
                if let Some(action) = action {
                    return match action {
                        TrayAction::OpenDiary => self.show_manager(),
                        TrayAction::ShowWidget => self.native(NativeAction::Show),
                        TrayAction::HideWidget => self.native(NativeAction::Hide),
                        TrayAction::RecoverWidget => self.native(NativeAction::Recover),
                        TrayAction::Quit => {
                            let show = self.show_manager();
                            let close = self.request_close(CloseIntent::Application);
                            Task::batch([show, close])
                        }
                    };
                }
                if self.ticks % 10 == 0 && !self.native_busy && self.widget.is_some() {
                    return self.native(NativeAction::Position);
                }
            }
            Message::Navigate(page) if self.editor.is_none() => {
                self.page = page;
                self.search.clear();
                self.search_generation += 1;
                self.search_results.clear();
                return self.refresh();
            }
            Message::Month(delta) if self.editor.is_none() => {
                self.month = shift_month(self.month, delta);
                self.hovered_week = None;
                return self.refresh();
            }
            Message::Week(delta) if self.editor.is_none() => {
                if let Some(date) = self.week.checked_add_signed(Duration::days(delta * 7)) {
                    if (1900..=9998).contains(&date.year()) {
                        self.week = date;
                        self.selected = date;
                        return self.refresh();
                    }
                }
            }
            Message::Today if self.editor.is_none() => {
                self.selected = Local::now().date_naive();
                self.month = self.selected.with_day(1).unwrap();
                self.week = week_containing(self.selected).from;
                self.widget_month = self.month;
                return self.refresh();
            }
            Message::OpenWeek(sunday) if self.editor.is_none() => match week_after_sunday(sunday) {
                Ok(range) => {
                    self.week = range.from;
                    self.selected = range.from;
                    self.page = Page::Week;
                    self.hovered_week = None;
                    return self.refresh();
                }
                Err(e) => self.error = e,
            },
            Message::HoverWeek(value) if self.editor.is_none() => self.hovered_week = value,
            Message::SelectDay(date) if self.editor.is_none() => {
                self.selected = date;
            }
            Message::WidgetMonth(delta) => {
                self.widget_month = shift_month(self.widget_month, delta);
                return self.refresh();
            }
            Message::WidgetToday => {
                self.widget_month = Local::now().date_naive().with_day(1).unwrap();
                return self.refresh();
            }
            Message::WidgetDate(date) => {
                let show = self.show_manager();
                if self.editor.is_some() {
                    self.notice = "편집을 마친 뒤 날짜를 다시 선택해 주세요.".into();
                    return show;
                }
                self.selected = date;
                self.month = date.with_day(1).unwrap();
                self.week = week_containing(date).from;
                self.page = Page::Month;
                let load = self.refresh();
                let edit = self.open_form(Form::new(Kind::Event, date.to_string(), None));
                return Task::batch([show, load, edit]);
            }
            Message::Project(id) if self.editor.is_none() => {
                self.project = id;
                self.page = Page::Projects;
                self.search.clear();
                self.search_generation += 1;
                return self.refresh();
            }
            Message::New(kind, date) if self.editor.is_none() => {
                let date = date.unwrap_or_else(|| self.selected.to_string());
                let mut form = Form::new(
                    kind,
                    date,
                    if self.page == Page::Projects {
                        self.project.clone()
                    } else {
                        None
                    },
                );
                if kind == Kind::Note || kind == Kind::Project {
                    form.date.clear();
                }
                return self.open_form(form);
            }
            Message::EditForm(form) => return self.open_form(form),
            Message::FetchForm(hit) if self.editor.is_none() => {
                let Some(store) = self.store.clone() else {
                    return Task::none();
                };
                self.editor_generation += 1;
                let eg = self.editor_generation;
                let vg = self.view_generation;
                return worker::job(
                    store,
                    move |s| worker::fetch_form(s, &hit),
                    move |r| Message::FormLoaded(vg, eg, r),
                );
            }
            Message::FormLoaded(vg, eg, result)
                if vg == self.view_generation
                    && eg == self.editor_generation
                    && self.editor.is_none() =>
            {
                match result {
                    Ok(form) => {
                        self.search.clear();
                        self.search_generation += 1;
                        return self.open_form(form);
                    }
                    Err(e) => self.error = e,
                }
            }
            Message::Load => return self.refresh(),
            Message::Loaded(generation, result) if generation == self.view_generation => {
                self.loading = false;
                match result {
                    Ok(snapshot) => {
                        self.log(&format!(
                            "snapshot generation={generation} entries={} events={} projects={}",
                            snapshot.entries.len(),
                            snapshot.events.len(),
                            snapshot.projects.len()
                        ));
                        self.more = snapshot.entries.len() == 200;
                        self.snapshot = snapshot;
                        if self
                            .project
                            .as_ref()
                            .is_some_and(|id| !self.snapshot.projects.iter().any(|p| &p.id == id))
                        {
                            self.project = None;
                        }
                    }
                    Err(e) => self.error = e,
                }
            }
            Message::More if self.more && !self.loading && !self.paging => {
                let Some(store) = self.store.clone() else {
                    return Task::none();
                };
                self.paging = true;
                let g = self.view_generation;
                let scope = self.scope();
                let offset = self.snapshot.entries.len() as u32;
                return worker::job(
                    store,
                    move |s| {
                        s.list_entries(&scope.from, &scope.to, scope.project.as_deref(), offset)
                    },
                    move |r| Message::MoreLoaded(g, r),
                );
            }
            Message::MoreLoaded(g, result) if g == self.view_generation && self.paging => {
                self.paging = false;
                match result {
                    Ok(rows) => {
                        self.more = rows.len() == 200;
                        self.snapshot.entries.extend(rows)
                    }
                    Err(e) => self.error = e,
                }
            }
            Message::ToggleEntry(id) if self.editor.is_none() => {
                if !self.expanded.insert(id.clone()) {
                    self.expanded.remove(&id);
                    return Task::none();
                }
                let Some(store) = self.store.clone() else {
                    return Task::none();
                };
                let g = self.view_generation;
                let key = id.clone();
                return worker::job(
                    store,
                    move |s| s.get_entry(&id),
                    move |r| Message::EntryLoaded(g, key.clone(), r),
                );
            }
            Message::EntryLoaded(g, id, result) if g == self.view_generation => match result {
                Ok(entry) => {
                    self.bodies.insert(id, entry);
                }
                Err(e) => {
                    self.entry_errors.insert(id, e);
                }
            },
            Message::ToggleEvent(id) if self.editor.is_none() => {
                if !self.expanded_events.insert(id.clone()) {
                    self.expanded_events.remove(&id);
                }
            }
            Message::Source(url) => {
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            timeschedule_desktop::open_source_url(&url)
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    Message::SourceDone,
                )
            }
            Message::SourceDone(Err(e)) => self.error = e,
            Message::Field(field) => {
                if let Some(e) = self.editor.as_mut().filter(|e| !e.saving) {
                    match field {
                        Field::Title(v) => e.form.title = v,
                        Field::Date(v) => e.form.date = v,
                        Field::Project(v) => e.form.project_id = v,
                        Field::EntryKind(v) => e.form.entry_kind = v,
                        Field::Source(v) => e.form.source_url = v,
                        Field::Repo(v) => e.form.repo_path = v,
                        Field::Start(v) => e.form.start = v,
                        Field::End(v) => e.form.end = v,
                        Field::Color(v) => e.form.color = v,
                        Field::AllDay(v) => e.form.all_day = v,
                        Field::Completed(v) => e.form.completed = v,
                    }
                }
            }
            Message::Body(action) => {
                if let Some(e) = &mut self.editor {
                    e.body_action(action)
                }
            }
            Message::Undo | Message::Redo => {
                let redo = matches!(message, Message::Redo);
                if let Some(e) = &mut self.editor {
                    e.history(redo);
                }
                // The editor stays mounted; keep its existing focus and selection.
                return Task::none();
            }
            Message::Composition(id, active) if id == self.manager => {
                if let Some(e) = &mut self.editor {
                    e.composing = active;
                }
            }
            Message::Save => return self.persist_editor(false),
            Message::Delete => {
                if let Some(e) = self
                    .editor
                    .as_mut()
                    .filter(|e| !e.saving && !e.composing && e.form.id.is_some())
                {
                    e.confirmation = Some(Confirmation::Delete);
                }
            }
            Message::ConfirmDelete => return self.persist_editor(true),
            Message::Keep => {
                if let Some(e) = &mut self.editor {
                    e.confirmation = None;
                }
                return iced::widget::operation::focus("editor-body");
            }
            Message::Discard => {
                if let Some(Confirmation::Discard(intent)) =
                    self.editor.as_ref().and_then(|e| e.confirmation)
                {
                    return self.finish_close(intent);
                }
            }
            Message::CloseEditor => return self.request_close(CloseIntent::Editor),
            Message::Escape => {
                if let Some(e) = &mut self.editor {
                    if e.confirmation.is_some() {
                        e.confirmation = None;
                        return Task::none();
                    }
                    return self.request_close(CloseIntent::Editor);
                }
                self.search.clear();
                self.search_generation += 1;
            }
            Message::Saved(g, result)
                if self.editor.as_ref().is_some_and(|e| e.generation == g) =>
            {
                match result {
                    Ok(()) => {
                        if let Some(editor) = &self.editor {
                            if matches!(editor.form.kind, Kind::Event | Kind::Entry) {
                                if let Ok(date) =
                                    NaiveDate::parse_from_str(&editor.form.date, "%Y-%m-%d")
                                {
                                    self.selected = date;
                                }
                            }
                        }
                        self.editor = None;
                        self.editor_generation += 1;
                        self.notice = "내 컴퓨터에 저장했습니다.".into();
                        return self.refresh();
                    }
                    Err(error) => {
                        if let Some(e) = &mut self.editor {
                            e.saving = false;
                            e.confirmation = None;
                            e.error = error;
                        }
                    }
                }
            }
            Message::ToggleTask(id) if !self.busy && self.editor.is_none() => {
                if let Some(t) = self.snapshot.tasks.iter().find(|t| t.id == id) {
                    let mut form = Form::task(t.clone());
                    form.completed = !form.completed;
                    let Some(store) = self.store.clone() else {
                        return Task::none();
                    };
                    self.busy = true;
                    return worker::job(store, move |s| form.save(s), Message::MutationDone);
                }
            }
            Message::MutationDone(result) => {
                self.busy = false;
                match result {
                    Ok(()) => return self.refresh(),
                    Err(e) => self.error = e,
                }
            }
            Message::Search(value) if self.editor.is_none() => {
                self.search = value.clone();
                self.search_generation += 1;
                self.search_results.clear();
                self.searching = !value.trim().is_empty();
                let g = self.search_generation;
                if self.searching {
                    return Task::perform(
                        async move {
                            tokio::time::sleep(std::time::Duration::from_millis(220)).await;
                            (g, value)
                        },
                        |(g, q)| Message::SearchDue(g, q),
                    );
                }
            }
            Message::SearchDue(g, query) if g == self.search_generation => {
                let Some(store) = self.store.clone() else {
                    return Task::none();
                };
                return worker::job(
                    store,
                    move |s| s.search_diary(&query, 50),
                    move |r| Message::SearchDone(g, r),
                );
            }
            Message::SearchDone(g, result) if g == self.search_generation => {
                self.searching = false;
                match result {
                    Ok(rows) => self.search_results = rows,
                    Err(e) => self.error = e,
                }
            }
            Message::Import if !self.busy => {
                if let (Some(id), Some(store)) = (self.project.clone(), self.store.clone()) {
                    self.busy = true;
                    return worker::job(store, move |s| s.import_git(&id), Message::Imported);
                }
            }
            Message::Imported(result) => {
                self.busy = false;
                match result {
                    Ok(r) => {
                        self.notice = format!(
                            "커밋 {}개 확인 · {}개 추가 · {}개 중복 보존{}",
                            r.scanned,
                            r.imported,
                            r.scanned - r.imported,
                            if r.limited {
                                " (최근 500개 제한)"
                            } else {
                                ""
                            }
                        );
                        return self.refresh();
                    }
                    Err(e) => self.error = e,
                }
            }
            Message::Draft(portfolio) if !self.busy && self.editor.is_none() => {
                let Some(store) = self.store.clone() else {
                    return Task::none();
                };
                self.busy = true;
                let vg = self.view_generation;
                let eg = self.editor_generation;
                let project_mode = self.page == Page::Projects;
                let mut scope = self.scope();
                if !project_mode {
                    scope.from = self.week.to_string();
                    scope.to = (self.week + Duration::days(6)).to_string();
                    scope.project = None;
                }
                let selected = self.selected.to_string();
                return worker::job(
                    store,
                    move |s| worker::draft(s, &scope, selected, project_mode, portfolio),
                    move |r| Message::Drafted(vg, eg, r),
                );
            }
            Message::Drafted(vg, eg, result) => {
                self.busy = false;
                if vg == self.view_generation
                    && eg == self.editor_generation
                    && self.editor.is_none()
                {
                    match result {
                        Ok(form) => return self.open_form(form),
                        Err(e) => self.error = e,
                    }
                }
            }
            Message::Native(action) => return self.native(action),
            Message::NativeDone(action, result) => {
                self.native_busy = false;
                let mut save = Task::none();
                match result {
                    Ok(report) => {
                        if !matches!(action, NativeAction::Position) {
                            self.log(&format!(
                                "widget action={action:?} attached={} visible={:?} position={:?}",
                                report.attached, report.visible, report.position
                            ));
                        }
                        let before = (
                            self.preferences.widget_x,
                            self.preferences.widget_y,
                            self.preferences.widget_locked,
                            self.preferences.widget_visible,
                            self.preferences.widget_opacity,
                        );
                        self.desktop_message = report.message;
                        self.widget_attached = report.attached;
                        if report.attached {
                            self.preferences.widget_locked = report.locked;
                        }
                        if let Some(value) = report.opacity {
                            self.preferences.widget_opacity = value;
                        }
                        if let Some(value) = report.visible {
                            self.preferences.widget_visible = value;
                        }
                        if let Some((x, y)) = report.position {
                            self.preferences.widget_x = Some(x);
                            self.preferences.widget_y = Some(y);
                        }
                        let after = (
                            self.preferences.widget_x,
                            self.preferences.widget_y,
                            self.preferences.widget_locked,
                            self.preferences.widget_visible,
                            self.preferences.widget_opacity,
                        );
                        if before != after {
                            save = self.save_preferences();
                        }
                    }
                    Err(e) => {
                        self.desktop_message = e.clone();
                        if !matches!(action, NativeAction::Position) {
                            self.error = format!("바탕화면 위젯: {e}");
                            self.log(&self.error);
                        }
                    }
                }
                if let Some(next) = self.native_pending.take() {
                    return Task::batch([save, self.native(next)]);
                }
                return save;
            }
            Message::PrefsDone(result) => {
                self.prefs_saving = false;
                if let Err(e) = result {
                    self.error = format!("설정 저장 실패: {e}");
                }
                if self.prefs_pending {
                    return self.save_preferences();
                }
            }
            Message::ShowManager => return self.show_manager(),
            Message::DragWidget => {
                if let Some(id) = self.widget {
                    return window::drag(id);
                }
            }
            Message::Quit => return self.request_close(CloseIntent::Application),
            _ => {}
        }
        Task::none()
    }
    fn persist_editor(&mut self, delete: bool) -> Task<Message> {
        let Some(store) = self.store.clone() else {
            self.error = "저장소가 열리지 않았습니다.".into();
            return Task::none();
        };
        let Some(editor) = self.editor.as_mut() else {
            return Task::none();
        };
        if editor.saving || editor.composing {
            return Task::none();
        }
        if delete && editor.confirmation != Some(Confirmation::Delete) {
            return Task::none();
        }
        editor.saving = true;
        editor.error.clear();
        let form = editor.form.clone();
        let generation = editor.generation;
        worker::job(
            store,
            move |s| if delete { form.delete(s) } else { form.save(s) },
            move |r| Message::Saved(generation, r),
        )
    }
    pub fn view(&self, id: window::Id) -> Element<'_, Message> {
        crate::views::view(self, id)
    }
    pub fn theme(&self, _: window::Id) -> iced::Theme {
        crate::theme::iced_theme(self.preferences.theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[path = "visual.rs"]
    mod visual;
    fn fixture() -> App {
        let date = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        App {
            manager: window::Id::unique(),
            manager_size: Size::new(1160.0, 820.0),
            widget: None,
            store: Some(Arc::new(Storage::open(":memory:").unwrap())),
            data_dir: PathBuf::new(),
            preferences: Preferences::default(),
            snapshot: Snapshot::default(),
            page: Page::Month,
            month: date.with_day(1).unwrap(),
            selected: date,
            week: week_containing(date).from,
            widget_month: date.with_day(1).unwrap(),
            hovered_week: None,
            project: None,
            expanded: HashSet::new(),
            expanded_events: HashSet::new(),
            bodies: HashMap::new(),
            entry_errors: HashMap::new(),
            editor: None,
            editor_generation: 0,
            view_generation: 0,
            search_generation: 0,
            search: String::new(),
            search_results: vec![],
            searching: false,
            loading: false,
            more: false,
            paging: false,
            busy: false,
            native_busy: false,
            notice: String::new(),
            error: String::new(),
            desktop_message: String::new(),
            widget_attached: false,
            tray: None,
            _instance: None,
            ticks: 0,
            manager_visible: true,
            prefs_saving: false,
            prefs_pending: false,
            quit_when_idle: false,
            native_pending: None,
        }
    }
    fn hit(title: &str) -> SearchHit {
        SearchHit {
            id: "test".into(),
            kind: "entry".into(),
            title: title.into(),
            excerpt: String::new(),
            date: None,
            project_id: None,
        }
    }
    #[test]
    fn selecting_calendar_day_shows_details_without_opening_editor() {
        let mut app = fixture();
        let date = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
        let _ = app.update(Message::SelectDay(date));
        assert_eq!(app.selected, date);
        assert!(
            app.editor.is_none(),
            "date selection should show the inspector"
        );
        let _ = app.update(Message::New(Kind::Event, Some(date.to_string())));
        assert_eq!(app.editor.as_ref().unwrap().form.date, date.to_string());
    }
    #[test]
    fn dense_calendar_keeps_chips_within_short_rows() {
        use iced::advanced::{layout, widget::Tree};
        fn check(node: &layout::Node) {
            for child in node.children() {
                let b = child.bounds();
                assert!(
                    b.x >= -0.1 && b.x + b.width <= node.size().width + 0.1,
                    "child {b:?} exceeds {:?}",
                    node.size()
                );
                assert!(
                    b.y >= -0.1 && b.y + b.height <= node.size().height + 0.1,
                    "child {b:?} exceeds {:?}",
                    node.size()
                );
                check(child);
            }
        }
        let mut app = fixture();
        for day in timeschedule_core::dates::calendar_days(app.month) {
            for _ in 0..3 {
                let mut form = Form::new(Kind::Event, day.to_string(), None);
                form.title = "일정 제목과 긴 영문 WWWWWWWWWWW".into();
                form.save(app.store.as_ref().unwrap()).unwrap();
            }
        }
        app.snapshot = worker::snapshot(app.store.as_ref().unwrap(), &app.scope()).unwrap();
        let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        for size in [
            Size::new(584.0, 360.0),
            Size::new(644.0, 420.0),
            Size::new(644.0, 620.0),
            Size::new(930.0, 700.0),
        ] {
            let mut calendar = crate::shell::calendar(&app);
            let mut tree = Tree::new(calendar.as_widget());
            let node = calendar.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            check(&node);
        }
    }
    #[test]
    fn themed_pages_keep_content_inside_window_width() {
        use iced::advanced::{layout, widget::Tree};
        fn check(node: &layout::Node, parent_x: f32, width: f32, context: &str) {
            let bounds = node.bounds();
            let x = parent_x + bounds.x;
            assert!(
                x.is_finite() && bounds.width.is_finite(),
                "{context}: non-finite layout"
            );
            assert!(
                x + bounds.width <= width + 1.0,
                "{context}: horizontal overflow: x={x}, width={}, limit={width}",
                bounds.width
            );
            for child in node.children() {
                check(child, x, width, context);
            }
        }
        let mut app = fixture();
        let mut form = Form::new(Kind::Event, app.selected.to_string(), None);
        form.title = "긴 일정 제목과 공백 없는 영문 abcdefghijklmnopqrstuvwxyz".repeat(2);
        form.save(app.store.as_ref().unwrap()).unwrap();
        app.snapshot = worker::snapshot(app.store.as_ref().unwrap(), &app.scope()).unwrap();
        let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        for size in [
            Size::new(800.0, 620.0),
            Size::new(1160.0, 620.0),
            Size::new(1160.0, 820.0),
            Size::new(1440.0, 900.0),
        ] {
            app.manager_size = size;
            for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
                app.preferences.theme = mode;
                for page in [
                    Page::Month,
                    Page::Week,
                    Page::Projects,
                    Page::Records,
                    Page::Tasks,
                    Page::Notes,
                    Page::Settings,
                ] {
                    app.page = page;
                    let mut element = app.view(app.manager);
                    let mut tree = Tree::new(element.as_widget());
                    let node = element.as_widget_mut().layout(
                        &mut tree,
                        &renderer,
                        &layout::Limits::new(Size::ZERO, size),
                    );
                    assert_eq!(node.size(), size);
                    check(
                        &node,
                        0.0,
                        size.width,
                        &format!("{page:?}/{mode:?}/{size:?}"),
                    );
                }
            }
        }
    }
    #[test]
    fn theme_toggle_keeps_editor_and_queues_latest_preference() {
        let mut app = fixture();
        let dir = std::env::temp_dir().join(format!("timeschedule-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        app._instance = Some(InstanceLock::acquire(&dir).unwrap());
        app.data_dir = dir.clone();
        let _ = app.open_form(Form::new(Kind::Entry, app.selected.to_string(), None));
        app.editor.as_mut().unwrap().form.title = "보존할 초안".into();
        let generation = app.editor_generation;
        let _ = app.update(Message::ToggleTheme);
        assert_eq!(app.preferences.theme, AppTheme::Porcelain);
        assert!(app.prefs_saving);
        let _ = app.update(Message::ToggleTheme);
        assert_eq!(app.preferences.theme, AppTheme::Midnight);
        assert!(
            app.prefs_pending,
            "last choice must be saved after the in-flight write"
        );
        assert_eq!(app.editor_generation, generation);
        assert_eq!(app.editor.as_ref().unwrap().form.title, "보존할 초안");
        let _ = app.update(Message::PrefsDone(Ok(())));
        assert!(app.prefs_saving);
        assert!(!app.prefs_pending);
        app._instance = None;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn stale_search_does_not_replace_latest_results() {
        let mut app = fixture();
        app.search_generation = 2;
        app.search_results = vec![hit("최신")];
        let _ = app.update(Message::SearchDone(1, Ok(vec![hit("과거")])));
        assert_eq!(app.search_results[0].title, "최신");
        let _ = app.update(Message::SearchDone(2, Ok(vec![hit("일치")])));
        assert_eq!(app.search_results[0].title, "일치");
    }
    #[test]
    fn stale_load_does_not_clear_current_snapshot_or_loading() {
        let mut app = fixture();
        app.view_generation = 3;
        app.loading = true;
        let _ = app.update(Message::Loaded(2, Ok(Snapshot::default())));
        assert!(app.loading);
        let _ = app.update(Message::Loaded(3, Err("load failure".into())));
        assert!(!app.loading);
        assert_eq!(app.error, "load failure");
    }
    #[test]
    fn stale_editor_and_draft_cannot_replace_new_draft() {
        let mut app = fixture();
        let _ = app.open_form(Form::new(Kind::Note, String::new(), None));
        app.editor.as_mut().unwrap().form.title = "작성 중".into();
        let old = app.editor_generation.saturating_sub(1);
        let _ = app.update(Message::FormLoaded(
            app.view_generation,
            old,
            Ok(Form::new(Kind::Task, String::new(), None)),
        ));
        let _ = app.update(Message::Drafted(
            app.view_generation,
            old,
            Ok(Form::new(Kind::Entry, String::new(), None)),
        ));
        assert_eq!(app.editor.as_ref().unwrap().form.title, "작성 중");
        assert_eq!(app.editor.as_ref().unwrap().form.kind, Kind::Note);
    }
    #[test]
    fn persistence_failure_retains_the_exact_editor_and_allows_retry() {
        let mut app = fixture();
        let _ = app.open_form(Form::new(Kind::Entry, "2026-10-29".into(), None));
        let editor = app.editor.as_mut().unwrap();
        editor.form.title = "트러블슈팅".into();
        editor.form.body = "원문 유지".into();
        editor.saving = true;
        let generation = editor.generation;
        let expected = editor.form.clone();
        let _ = app.update(Message::Saved(generation, Err("disk full".into())));
        let editor = app.editor.as_ref().unwrap();
        assert_eq!(editor.form, expected);
        assert!(!editor.saving);
        assert_eq!(editor.error, "disk full");
    }
    #[test]
    fn manager_close_records_intent_and_keep_preserves_changes() {
        let mut app = fixture();
        let _ = app.open_form(Form::new(Kind::Note, String::new(), None));
        app.editor.as_mut().unwrap().form.title = "미저장".into();
        let _ = app.update(Message::CloseWindow(app.manager));
        assert_eq!(
            app.editor.as_ref().unwrap().confirmation,
            Some(Confirmation::Discard(CloseIntent::Manager))
        );
        let _ = app.update(Message::Keep);
        assert_eq!(app.editor.as_ref().unwrap().form.title, "미저장");
        assert_eq!(app.editor.as_ref().unwrap().confirmation, None);
        assert!(app.manager_visible);
    }
    #[test]
    fn modal_blocks_background_navigation_and_saving_blocks_close() {
        let mut app = fixture();
        let _ = app.open_form(Form::new(Kind::Note, String::new(), None));
        app.editor.as_mut().unwrap().form.title = "미저장".into();
        let _ = app.update(Message::CloseEditor);
        let _ = app.update(Message::Navigate(Page::Projects));
        assert_eq!(app.page, Page::Month);
        let _ = app.update(Message::Keep);
        app.editor.as_mut().unwrap().saving = true;
        let _ = app.update(Message::CloseEditor);
        assert!(app.editor.is_some());
        assert_eq!(app.editor.as_ref().unwrap().confirmation, None);
    }
    #[test]
    fn whole_week_selection_preserves_month_for_return() {
        let mut app = fixture();
        let old = app.month;
        let _ = app.update(Message::OpenWeek(
            NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
        ));
        assert_eq!(app.week.to_string(), "2026-11-02");
        assert_eq!(app.page, Page::Week);
        assert_eq!(app.month, old);
        let _ = app.update(Message::Navigate(Page::Month));
        assert_eq!(app.month, old);
    }
    #[test]
    fn quit_is_deferred_until_mutating_work_finishes() {
        let mut app = fixture();
        app.busy = true;
        let _ = app.exit_when_idle();
        assert!(app.quit_when_idle);
        app.busy = false;
        let _ = app.update(Message::Tick);
        assert!(!app.quit_when_idle);
    }
    #[test]
    fn saved_result_from_another_editor_cannot_close_current_editor() {
        let mut app = fixture();
        let _ = app.open_form(Form::new(Kind::Note, String::new(), None));
        let generation = app.editor.as_ref().unwrap().generation;
        let _ = app.update(Message::Saved(generation + 1, Ok(())));
        assert!(app.editor.is_some());
    }
    #[test]
    fn changing_scope_blocks_more_during_load_and_after_failure() {
        let mut app = fixture();
        app.more = true;
        let _ = app.update(Message::Navigate(Page::Records));
        let generation = app.view_generation;
        let _ = app.update(Message::More);
        assert!(
            !app.paging,
            "must not paginate the old snapshot during a new scope load"
        );
        let _ = app.update(Message::Loaded(generation, Err("database busy".into())));
        let _ = app.update(Message::More);
        assert!(
            !app.paging,
            "failed first page must not expose old-scope pagination"
        );
    }
    #[test]
    fn discard_confirmation_preserves_mounted_input_selection_and_focus() {
        use iced::advanced::{
            text::Renderer as TextRenderer,
            widget::{tree::Tag, Tree},
        };
        type InputState =
            iced::widget::text_input::State<<iced::Renderer as TextRenderer>::Paragraph>;
        fn inputs(
            tree: &mut Tree,
            initialize: bool,
            result: &mut Vec<(bool, iced::widget::text_input::Cursor)>,
        ) {
            if tree.tag == Tag::of::<InputState>() {
                let state = tree.state.downcast_mut::<InputState>();
                if initialize {
                    state.focus();
                    state.select_range(1, 3);
                }
                result.push((state.is_focused(), state.cursor()));
            }
            for child in &mut tree.children {
                inputs(child, initialize, result);
            }
        }
        let mut app = fixture();
        let _ = app.open_form(Form::new(Kind::Entry, "2026-10-29".into(), None));
        app.editor.as_mut().unwrap().form.title = "작성 중인 제목".into();
        let mut tree = Tree::new(crate::views::view(&app, app.manager).as_widget());
        let mut expected = vec![];
        inputs(&mut tree, true, &mut expected);
        let _ = app.update(Message::CloseEditor);
        tree.diff(crate::views::view(&app, app.manager).as_widget());
        let mut during = vec![];
        inputs(&mut tree, false, &mut during);
        assert_eq!(
            during, expected,
            "confirmation must keep the same editor widget tree"
        );
        {
            use iced::advanced::{clipboard, layout, mouse, Layout, Shell};
            let renderer = iced::Renderer::new(iced::Font::default(), 16.into());
            let size = Size::new(1160.0, 820.0);
            let mut element = crate::views::view(&app, app.manager);
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, size),
            );
            assert_eq!(
                node.children()[1].size(),
                size,
                "confirmation backdrop covers the entire editor"
            );
            let mut messages = vec![];
            element.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Layout::new(&node),
                mouse::Cursor::Available(iced::Point::new(10.0, 10.0)),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
            assert!(
                matches!(&messages[..], [Message::Ignore]),
                "outside click must be consumed without reaching the editor or navigation"
            );
        }
        let _ = app.update(Message::Keep);
        tree.diff(crate::views::view(&app, app.manager).as_widget());
        let mut after = vec![];
        inputs(&mut tree, false, &mut after);
        assert_eq!(after, expected);
    }
    #[cfg(windows)]
    #[test]
    fn first_run_and_retry_fit_oversized_owned_window_without_saved_coordinates() {
        assert!(
            widget_window_settings().min_size.is_none(),
            "Iced must permit native monitor fitting below the nominal size"
        );
        // Hidden test-owned top-level window only: this never attaches to Explorer.
        #[repr(C)]
        #[derive(Default)]
        struct Rect {
            left: i32,
            top: i32,
            right: i32,
            bottom: i32,
        }
        #[repr(C)]
        struct MonitorInfo {
            size: u32,
            monitor: Rect,
            work: Rect,
            flags: u32,
        }
        #[link(name = "user32")]
        extern "system" {
            fn CreateWindowExW(
                ex: u32,
                class: *const u16,
                name: *const u16,
                style: u32,
                x: i32,
                y: i32,
                w: i32,
                h: i32,
                parent: isize,
                menu: isize,
                instance: isize,
                param: *const std::ffi::c_void,
            ) -> isize;
            fn DestroyWindow(hwnd: isize) -> i32;
            fn GetWindowRect(hwnd: isize, rect: *mut Rect) -> i32;
            fn MonitorFromWindow(hwnd: isize, flags: u32) -> isize;
            fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
        }
        struct Owned(isize);
        impl Drop for Owned {
            fn drop(&mut self) {
                unsafe {
                    DestroyWindow(self.0);
                }
            }
        }
        let class: Vec<u16> = "STATIC".encode_utf16().chain(Some(0)).collect();
        let name: Vec<u16> = "TimeSchedule isolated geometry test"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let owned = Owned(unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                name.as_ptr(),
                0x80000000,
                100_000,
                -100_000,
                6000,
                5000,
                0,
                0,
                0,
                std::ptr::null(),
            )
        });
        assert_ne!(owned.0, 0);
        let window = unsafe { windows::NativeWindow::iced_widget(owned.0) };
        let prefs = Preferences::default();
        assert_eq!((prefs.widget_x, prefs.widget_y), (None, None));
        fit_widget_before_attach(&window, &prefs).unwrap();
        // The same pre-attach path is used by Show/Recover after an attach failure.
        fit_widget_before_attach(&window, &prefs).unwrap();
        let mut rect = Rect::default();
        let mut info = MonitorInfo {
            size: std::mem::size_of::<MonitorInfo>() as u32,
            monitor: Rect::default(),
            work: Rect::default(),
            flags: 0,
        };
        unsafe {
            assert_ne!(GetWindowRect(owned.0, &mut rect), 0);
            assert_ne!(GetMonitorInfoW(MonitorFromWindow(owned.0, 2), &mut info), 0);
        }
        assert!(rect.left >= info.work.left && rect.top >= info.work.top);
        assert!(rect.right <= info.work.right && rect.bottom <= info.work.bottom);
        assert!(rect.right > rect.left && rect.bottom > rect.top);
    }
}
