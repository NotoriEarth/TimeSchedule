use crate::{
    editor::{CloseIntent, Confirmation, Editor, EditorPicker, Form, Kind},
    worker::{self, Scope, Snapshot},
};
use chrono::{Datelike, Duration, Local, NaiveDate};
use iced::widget::text_editor;
use iced::{window, Element, Size, Subscription, Task};
use raw_window_handle::RawWindowHandle;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};
use timeschedule_core::{
    dates::{calendar_days, shift_month, week_after_sunday, week_containing},
    diary::{Entry, EntrySummary, SearchHit},
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
fn configure_widget_geometry(
    window: &windows::NativeWindow,
    prefs: &Preferences,
) -> Result<(), String> {
    let (x, y) = match (prefs.widget_x, prefs.widget_y) {
        (Some(x), Some(y)) => (x, y),
        _ => windows::position(window)?,
    };
    windows::configure_geometry(
        window,
        windows::WidgetGeometry {
            x,
            y,
            width: prefs.widget.width,
            height: prefs.widget.height,
        },
    )
}
fn widget_window_settings() -> window::Settings {
    window::Settings {
        size: Size::new(960.0, 640.0),
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
            Self::Tasks => "날짜 미정",
            Self::Notes => "메모",
            Self::Settings => "설정",
        }
    }
}
/// Scrolls the project screen back to its top.
fn projects_top() -> Task<Message> {
    iced::widget::operation::snap_to(
        "projects-page",
        iced::widget::operation::RelativeOffset::START,
    )
}
/// The tab on show under a project's info rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProjectTab {
    #[default]
    Records,
    Review,
    Portfolio,
}
/// The part of the settings screen on show; the buttons above it pick one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsSection {
    #[default]
    General,
    Records,
    Widget,
    Mcp,
}
impl SettingsSection {
    pub const ALL: [Self; 4] = [Self::General, Self::Records, Self::Widget, Self::Mcp];
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "일반",
            Self::Records => "기록과 하루 정리",
            Self::Widget => "바탕화면 위젯",
            Self::Mcp => "AI 연결 (MCP)",
        }
    }
}
#[derive(Clone, Debug)]
pub enum Field {
    Category(String),
    ShortTitle(String),
    /// A record's typed time of day, `HH:mm` in KST.
    Time(String),
    ProjectShortName(String),
    Title(String),
    Date(String),
    Project(Option<String>),
    EntryKind(String),
    Source(String),
    Repo(String),
    Start(String),
    End(String),

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
    Resize(u32, u32),
}
#[derive(Clone, Debug)]
pub struct NativeReport {
    pub attached: bool,
    pub message: String,
    pub locked: bool,
    pub opacity: Option<u8>,
    pub visible: Option<bool>,
    pub position: Option<(i32, i32)>,
    pub size: Option<(u32, u32)>,
}
/// What the running daily record job was started for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DailyKind {
    /// The automatic mode's finished days.
    Auto,
    /// The selected day's button.
    Button,
    /// The dry run that says which past days of this project (ID) would ask the model.
    PastCheck(String),
    /// The past days of this project (ID) the owner confirmed.
    Past(String),
}

impl DailyKind {
    /// The project (ID) a past-days job is for.
    fn past_project(&self) -> Option<&str> {
        match self {
            Self::PastCheck(project) | Self::Past(project) => Some(project),
            Self::Auto | Self::Button => None,
        }
    }
}

/// What makes the record of a day in a daily job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Maker {
    /// Asks the model and keeps the record.
    Real,
    /// Says what would happen; asks and writes nothing.
    Dry,
}

impl Maker {
    /// Only the check of the past days is dry: the other kinds make the records.
    fn of(kind: &DailyKind) -> Self {
        match kind {
            DailyKind::PastCheck(_) => Self::Dry,
            DailyKind::Auto | DailyKind::Button | DailyKind::Past(_) => Self::Real,
        }
    }
    fn make(
        self,
        vault: &std::path::Path,
        transcript: &timeschedule_core::daily_record::DayTranscript,
    ) -> Result<worker::Made, crate::record_cli::DailyError> {
        let model = timeschedule_core::terra::MODEL;
        match self {
            Self::Real => {
                crate::record_cli::summarize_day(vault, transcript, model).map(worker::Made::from)
            }
            Self::Dry => crate::record_cli::would_summarize_day(vault, transcript, model),
        }
    }
}

/// Where the making of one project's past days stands. A project not in
/// `App::past_days` is idle. At most one project is `Checking` or `Making`, because one
/// daily job runs at a time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Past {
    /// A check is owed: it starts when no chat scan and no daily job runs.
    Due,
    /// The dry run is running.
    Checking,
    /// The owner is asked: these days would each ask the model once. They were counted in
    /// the chats of the folder with this key, and are made only while it is the
    /// project's folder.
    Ask(Vec<NaiveDate>, String),
    /// The confirmed days are being made.
    Making,
}

impl Past {
    /// A daily job runs for this state.
    fn running(&self) -> bool {
        matches!(self, Self::Checking | Self::Making)
    }
}

/// What a daily record job is asked to make.
enum DailyScope {
    Days(Vec<NaiveDate>),
    Past {
        project: String,
        before: NaiveDate,
    },
    ProjectDays {
        project: String,
        days: Vec<NaiveDate>,
        root: String,
    },
}

impl DailyScope {
    fn which(&self) -> worker::Which<'_> {
        match self {
            Self::Days(days) => worker::Which::Days(days),
            Self::Past { project, before } => worker::Which::Past {
                project,
                before: *before,
            },
            Self::ProjectDays {
                project,
                days,
                root,
            } => worker::Which::ProjectDays {
                project,
                days,
                root,
            },
        }
    }
}

/// One tick is 500 ms. The record inbox is polled every minute.
const RECORDS_POLL_TICKS: u32 = 120;
/// Chat files are read and kept every hour, and when the projects screen opens.
const CHAT_SCAN_TICKS: u32 = 7200;

#[derive(Clone, Debug)]
pub enum Message {
    Calendar(crate::calendar_ui::Action),
    Category(crate::category_ui::Action),
    McpSetup(crate::mcp_setup_ui::Action),
    SetMcpEnabled(bool),
    WidgetSetting(crate::widget_settings::Change),
    Jump(crate::month_jump::JumpMessage),
    ResizeWidget,
    ToggleTheme,
    Resized(window::Id, Size),
    Ready(window::Id),
    WidgetReady(window::Id),
    CloseWindow(window::Id),
    WindowClosed(window::Id),
    Tick,
    RecordsDue,
    SetChatCapture(String, bool),
    ScanChats,
    ChatsScanned(Result<worker::ChatScan, String>),
    SetDailyMode(preferences::DailyRecordMode),
    SettingsSection(SettingsSection),
    /// Opens or closes the advanced part of the AI connection section.
    McpAdvanced(bool),
    ProjectTab(ProjectTab),
    SetTheme(AppTheme),
    /// Make the daily record of this day now (the selected day's button).
    MakeDaily(NaiveDate),
    DailyMade(Result<worker::DailyMade, String>),
    /// Look for the past days of this project (ID) whose daily record is due.
    CheckPastDays(String),
    /// Make the past days the owner was asked about for this project (ID).
    MakePastDays(String),
    DismissPastDays(String),
    /// End the making of this project's (ID) past days after the day it is on.
    StopPastDays(String),
    /// Open the folder dialog for the project editor's Git folder.
    PickRepoFolder,
    /// The answer of the folder dialog opened by the editor of this generation.
    RepoFolderPicked(u64, Option<PathBuf>),
    RecordsDrained(Result<crate::record_inbox::DrainReport, String>),
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
    /// Open the day window from the diary, placed over the diary.
    ManagerDate(NaiveDate),
    QuickReady(window::Id),
    PopupPositioned(window::Id, window::Id, Result<(), String>),

    Project(Option<String>),
    New(Kind, Option<String>),
    EditForm(Form),
    FetchForm(SearchHit),
    FormLoaded(u64, u64, Result<Form, String>),
    Load,
    Loaded(u64, Result<Snapshot, String>),
    More,
    MoreLoaded(
        u64,
        Result<(Vec<EntrySummary>, HashMap<String, u32>), String>,
    ),
    ToggleEntry(String),
    EntryLoaded(
        u64,
        String,
        Result<(Entry, timeschedule_core::timeline::EntryTimeline), String>,
    ),

    Source(String),
    /// Open an automatic record's original `record.md` by its work ID.
    OpenOriginal(String),
    SourceDone(Result<(), String>),
    Field(Field),
    DatePicker(crate::date_choices::Part),
    BrowseDatePicker(i32),
    PickEditorDate(NaiveDate),
    OpenTimePicker(crate::time_input::Field),
    PickEditorTime(crate::time_input::Field, String),
    DismissTimePicker(crate::time_input::Field),
    DismissDatePicker,
    DismissEditorPicker,
    EditorTab(window::Id, bool),
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
    EscapeWindow(window::Id),
    Composition(window::Id, bool),
    Saved(u64, Result<(), String>),
    ToggleTask(String),
    MutationDone(Result<(), String>),
    Search(String),
    SearchDue(u64, String),
    SearchDone(u64, Result<Vec<SearchHit>, String>),
    Portfolio(crate::portfolio_ui::Action),
    ChatReview(crate::chat_review_ui::Action),
    Draft(bool),
    Drafted(u64, u64, Result<Form, String>),
    Native(NativeAction),
    NativeDone(NativeAction, Result<NativeReport, String>),
    DisplayObserverReady(Result<(), String>),
    PrefsDone(Result<(), String>),
    ShowManager,
    DragWidget,
    Quit,
    Ignore,
}

pub struct App {
    pub calendar: crate::calendar_ui::State,
    pub categories: crate::category_ui::State,
    pub widget_ui: crate::widget_settings::State,
    pub manager: window::Id,
    pub manager_size: Size,
    pub widget: Option<window::Id>,
    pub quick_window: Option<window::Id>,
    pub placing_popups: HashMap<window::Id, window::Id>,
    pub quick_date: Option<NaiveDate>,
    /// The window the day window was opened from; it is placed over it.
    pub quick_source: Option<window::Id>,

    pub quick_generation: u64,
    pub quick_loading: bool,
    pub editor_in_quick: bool,
    pub store: Option<Arc<Storage>>,
    _mcp: Option<crate::mcp::LocalMcp>,
    mcp_changed: Arc<std::sync::atomic::AtomicBool>,
    pub mcp_status: String,
    mcp_enabled: Option<bool>,
    pub mcp_setup: crate::mcp_setup_ui::State,
    pub data_dir: PathBuf,
    pub preferences: Preferences,
    pub snapshot: Snapshot,
    pub chat_review: crate::chat_review_ui::State,
    pub portfolio_writer: fn(&str) -> Result<String, String>,
    pub portfolio_selection:
        std::collections::BTreeMap<String, Option<std::collections::BTreeSet<String>>>,
    pub portfolio_editor: Option<crate::portfolio_ui::AnswerEdit>,
    pub portfolio_feedback: Option<(String, String)>,
    pub page: Page,
    pub month: NaiveDate,
    pub selected: NaiveDate,
    pub week: NaiveDate,
    pub widget_month: NaiveDate,
    pub hovered_week: Option<NaiveDate>,
    pub project: Option<String>,
    /// The project whose full list `snapshot.entries` holds, set only when a project
    /// screen load succeeds: a failed load never passes the previous rows off as these.
    pub entries_project: Option<String>,
    /// Whose rows are on screen: `Some(None)` for 모든 프로젝트, `Some(Some(id))` for one
    /// project, `None` for rows read by another screen (a calendar's range). Kept through
    /// a refresh and replaced only by a successful load, so rows read for one choice are
    /// never shown under another.
    pub rows_project: Option<Option<String>>,
    /// Whether the rows on screen have a next page; unlike `more`, kept through a refresh.
    pub rows_more: bool,
    pub settings_section: SettingsSection,
    /// Whether the owner opened (true) or closed (false) the advanced part of the
    /// AI connection section; until then it opens itself when it needs attention.
    pub mcp_advanced: Option<bool>,
    /// Back to the records tab whenever another project is opened.
    pub project_tab: ProjectTab,
    pub expanded: HashSet<String>,

    record_sync: crate::record_sync::Runner,
    pub bodies: HashMap<String, Entry>,
    /// Times and evidence of the records unfolded in the lists, by ID.
    pub timelines: HashMap<String, timeschedule_core::timeline::EntryTimeline>,
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
    records_due: bool,
    chat_scanning: bool,
    /// The hourly chat scan is owed. It stays owed until a scan can start.
    chat_scan_due: bool,
    /// Daily records are being made.
    pub daily_making: bool,
    /// In the automatic mode, the finished days are to be made after the next chat
    /// scan: at start and every hour, not each time the project screen opens.
    daily_auto_due: bool,
    /// An automatic job met projects it could not keep whole (another run was keeping
    /// them). It is tried again when a chat scan ends, and only then.
    daily_retry_after_scan: bool,
    /// What the running job is for. The button's and the past days' notice lines say what
    /// happened even when nothing was made; the automatic mode's does not.
    daily_kind: DailyKind,
    /// Where the past days of a project stand (project screen).
    pub past_days: BTreeMap<String, Past>,
    /// Set to end the making of the past days after the day it is on.
    past_stop: Arc<std::sync::atomic::AtomicBool>,
    /// Inputs that failed; the automatic mode does not send them again until the
    /// dialogue changes or the app starts again.
    daily_failed: std::collections::HashSet<worker::DailyInput>,
    /// Whether the automatic mode is chosen, read by a run between its days.
    daily_auto: Arc<std::sync::atomic::AtomicBool>,
    records_processing: bool,
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
        if let Some(ignored) = crate::IGNORED_ARGUMENT.get() {
            errors.push(crate::launch::notice(ignored));
        }
        let prefs = if instance.is_some() {
            // One unusable value goes back to its default; the rest stays.
            match preferences::load_or_repair(&data_dir) {
                Ok(loaded) => {
                    errors.extend(loaded.notice());
                    loaded.preferences
                }
                Err(e) => {
                    errors.push(e);
                    Preferences::default()
                }
            }
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
        let mcp_changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
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
        let daily_auto = prefs.daily_record == preferences::DailyRecordMode::Auto;
        let mut app = Self {
            widget_ui: crate::widget_settings::State::new(&prefs, today),
            calendar: Default::default(),
            categories: Default::default(),
            manager,
            manager_size: Size::new(1160.0, 820.0),
            widget: None,
            quick_window: None,
            placing_popups: HashMap::new(),
            quick_date: None,
            quick_source: None,

            quick_generation: 0,
            quick_loading: false,
            editor_in_quick: false,
            store,
            _mcp: None,
            mcp_changed,
            mcp_status: "꺼짐 · 데이터 폴더를 사용할 수 없습니다.".into(),
            mcp_enabled: None,
            mcp_setup: Default::default(),
            data_dir,
            preferences: prefs,
            snapshot: Snapshot::default(),
            chat_review: Default::default(),
            portfolio_writer: timeschedule_core::terra::generate,
            portfolio_feedback: None,
            portfolio_selection: Default::default(),
            portfolio_editor: None,
            page: Page::Month,
            month,
            selected: today,
            week: week_containing(today).from,
            widget_month: month,
            hovered_week: None,
            project: None,
            entries_project: None,
            rows_project: None,
            rows_more: false,
            settings_section: SettingsSection::General,
            mcp_advanced: None,
            project_tab: ProjectTab::Records,
            expanded: HashSet::new(),

            record_sync: crate::record_sync::Runner::default(),
            bodies: HashMap::new(),
            timelines: HashMap::new(),
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
            records_due: false,
            chat_scanning: false,
            chat_scan_due: false,
            daily_making: false,
            daily_auto_due: true,
            daily_retry_after_scan: false,
            daily_kind: DailyKind::Auto,
            past_days: BTreeMap::new(),
            past_stop: Default::default(),
            daily_failed: Default::default(),
            daily_auto: Arc::new(std::sync::atomic::AtomicBool::new(daily_auto)),
            records_processing: false,
            manager_visible: true,
            prefs_saving: false,
            prefs_pending: false,
            quit_when_idle: false,
            native_pending: None,
        };
        app.log(&format!(
            "startup version={} data={} storage={} errors={}",
            env!("CARGO_PKG_VERSION"),
            app.data_dir.display(),
            app.store.is_some(),
            app.error
        ));
        if app._instance.is_some() {
            match crate::mcp_power::Power::initialize(&app.data_dir) {
                Ok(power) => app.apply_mcp_power(power.enabled),
                Err(error) => app.mcp_status = error,
            }
        }
        let widget = app.open_widget();
        let load = app.refresh();
        (
            app,
            Task::batch([
                open.map(Message::Ready),
                widget,
                load,
                Task::done(Message::RecordsDue),
                Task::done(Message::ScanChats),
            ]),
        )
    }
    pub fn title(&self, id: window::Id) -> String {
        if Some(id) == self.widget {
            format!("TimeSchedule {} · 바탕화면", env!("CARGO_PKG_VERSION"))
        } else if Some(id) == self.quick_window {
            format!(
                "{} · 빠른 일정",
                self.quick_date.map(|d| d.to_string()).unwrap_or_default()
            )
        } else {
            if self.widget_ui.jump.as_ref().is_some_and(|j| j.window == id) {
                "TimeSchedule · 연·월 이동".into()
            } else {
                format!("TimeSchedule {} · 만년 다이어리", env!("CARGO_PKG_VERSION"))
            }
        }
    }
    pub fn can_set_mcp(&self, enabled: bool) -> bool {
        self._instance.is_some()
            && !self.mcp_setup.busy
            && if enabled {
                self.store.is_some() && self.mcp_endpoint().is_none()
            } else {
                self.mcp_enabled != Some(false)
            }
    }

    fn apply_mcp_power(&mut self, enabled: bool) {
        self._mcp.take();
        self.mcp_enabled = Some(enabled);
        self.mcp_status = if !enabled {
            "꺼짐".into()
        } else if let Some(store) = self.store.clone() {
            let started =
                crate::mcp::LocalMcp::start(store, self.mcp_changed.clone(), &self.data_dir);
            if let Some(reason) = started.recovered {
                self.log(&format!("mcp token file unusable ({reason}); reissued"));
                self.mcp_setup.token = crate::mcp_setup_ui::TokenPanel::Recovered;
                // A new warning opens the advanced part, even if the owner had closed it.
                self.mcp_advanced = Some(true);
            }
            match started.server {
                Ok(server) => {
                    let status = server.status.clone();
                    self._mcp = Some(server);
                    status
                }
                Err(error) => format!("시작하지 못함 · {error}"),
            }
        } else {
            "시작하지 못함 · 데이터 저장소를 사용할 수 없습니다.".into()
        };
        self.mcp_setup.token_control =
            crate::mcp::TokenControl::for_app(self._mcp.as_ref(), self._instance.is_some());
    }

    /// The running MCP server's endpoint, used to judge whether each AI app's
    /// registration points at it.
    pub fn mcp_endpoint(&self) -> Option<String> {
        self._mcp
            .as_ref()
            .filter(|server| server.is_running())
            .map(crate::mcp::LocalMcp::endpoint)
    }
    /// Which AI apps have connected, newest first. Empty while MCP is off.
    pub fn mcp_clients(&self) -> Vec<crate::mcp_clients::Seen> {
        self._mcp
            .as_ref()
            .map(|server| server.clients.snapshot())
            .unwrap_or_default()
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
                if status == iced::event::Status::Ignored {
                    if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(key),
                        modifiers,
                        ..
                    }) = event
                    {
                        use iced::keyboard::key::Named;
                        return match key {
                            Named::Escape => Some(Message::EscapeWindow(id)),
                            Named::Tab
                                if !modifiers.command()
                                    && !modifiers.alt()
                                    && !modifiers.logo() =>
                            {
                                Some(Message::EditorTab(id, modifiers.shift()))
                            }
                            _ => None,
                        };
                    }
                }
                None
            }),
        ])
    }
    pub fn scope(&self) -> Scope {
        let days = calendar_days(self.month);
        let widget_range = self.widget_range();
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
            widget_from: widget_range.from.to_string(),
            widget_to: widget_range.to.to_string(),
            today: Local::now().date_naive().to_string(),
            project: if self.page == Page::Projects {
                self.project.clone()
            } else {
                None
            },
        }
    }
    /// Makes the daily records `scope` names for a job of `kind`, unless records are
    /// being made. `daily_making` says whether the job started.
    fn make_daily(&mut self, kind: DailyKind, scope: DailyScope) -> Task<Message> {
        if self.daily_making {
            return Task::none();
        }
        let Some(store) = self
            .store
            .clone()
            .filter(|_| self._instance.is_some() && !self.quit_when_idle)
        else {
            return Task::none();
        };
        // Only an automatic job is the automatic mode's turn.
        if kind == DailyKind::Auto {
            self.daily_auto_due = false;
            self.daily_retry_after_scan = false;
        }
        self.daily_making = true;
        self.daily_kind = kind.clone();
        self.past_stop
            .store(false, std::sync::atomic::Ordering::Release);
        let data_dir = self.data_dir.clone();
        // The button and the past days try every time; the automatic mode leaves failed
        // inputs alone and stops between days once it is no longer chosen.
        let skip = if kind == DailyKind::Auto {
            self.daily_failed.clone()
        } else {
            Default::default()
        };
        let auto = self.daily_auto.clone();
        let stop = self.past_stop.clone();
        worker::job(
            store,
            move |s| {
                worker::daily_job(
                    s,
                    || crate::record_original::originals_vault(&data_dir),
                    worker::keep_originals,
                    scope.which(),
                    worker::DailyRun {
                        make: |vault: &std::path::Path, transcript: &_| {
                            Maker::of(&kind).make(vault, transcript)
                        },
                        key: crate::record_cli::daily_input_key,
                        skip: &skip,
                        keep_going: || match &kind {
                            DailyKind::Auto => auto.load(std::sync::atomic::Ordering::Acquire),
                            // The check reads every past day and the making asks the
                            // model for each: both end early when asked to.
                            DailyKind::PastCheck(_) | DailyKind::Past(_) => {
                                !stop.load(std::sync::atomic::Ordering::Acquire)
                            }
                            DailyKind::Button => true,
                        },
                    },
                )
            },
            Message::DailyMade,
        )
    }
    /// Whether a past-days job can start now: no chat scan keeps originals and no daily
    /// job runs.
    pub(crate) fn past_days_can_start(&self) -> bool {
        !self.chat_scanning && !self.daily_making
    }
    /// Forgets where the past days of `project` stand and ends the job running for it
    /// after the day in progress. For a project whose capture was turned off or that was
    /// deleted.
    fn forget_past_days(&mut self, project: &str) {
        self.past_days.remove(project);
        if self.daily_making && self.daily_kind.past_project() == Some(project) {
            self.past_stop
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
    /// Starts the check of the first project whose check is owed, when no chat scan and no
    /// daily job runs. `None` when none is owed or it cannot start now; it stays owed.
    fn start_past_check(&mut self) -> Option<Task<Message>> {
        if !self.past_days_can_start() {
            return None;
        }
        let project = self
            .past_days
            .iter()
            .find_map(|(id, state)| (*state == Past::Due).then(|| id.clone()))?;
        let task = self.make_daily(
            DailyKind::PastCheck(project.clone()),
            DailyScope::Past {
                project: project.clone(),
                before: kst_today(),
            },
        );
        if !self.daily_making {
            return None;
        }
        self.past_days.insert(project, Past::Checking);
        Some(task)
    }
    /// Turns what a check of `project` found into its state. `checking` says the project
    /// was still being checked: capture turned off meanwhile removed it.
    fn past_check_done(&mut self, project: String, checking: bool, made: &worker::DailyMade) {
        if !checking || made.stopped {
            return;
        }
        if made.due.is_empty() {
            if made.failed.is_empty() {
                self.notice = if made.unkept.is_empty() {
                    "정리할 지난 대화가 없습니다.".into()
                } else {
                    "채팅 원문을 다 보관하지 못해 지난 대화를 확인하지 못했습니다. 잠시 뒤 다시 눌러 주세요."
                        .into()
                };
                // Their days are not in the count.
                if made.unplaced > 0 {
                    self.notice.push_str(&unplaced_note(made.unplaced));
                }
            }
            return;
        }
        let title = self
            .snapshot
            .projects
            .iter()
            .find(|p| p.id == project)
            .map_or("프로젝트", |p| p.title.as_str());
        self.notice = format!(
            "{title}: 지난 대화 {}일을 정리할 수 있습니다. 이 프로젝트의 ‘경험 검토’ 탭에서 확인해 주세요.",
            made.due.len()
        );
        // The folder the days were counted in goes with them.
        let Some(root) = made.root.clone() else {
            return;
        };
        self.past_days
            .insert(project, Past::Ask(made.due.clone(), root));
    }
    /// Reads the AI apps' chat files for the projects that capture chats, unless a scan runs.
    fn scan_chats(&mut self) -> Task<Message> {
        if self.chat_scanning {
            // The running scan may have listed its projects and files already.
            self.chat_scan_due = true;
            return Task::none();
        }
        let Some(store) = self
            .store
            .clone()
            .filter(|_| self._instance.is_some() && !self.quit_when_idle)
        else {
            return Task::none();
        };
        self.chat_scan_due = false;
        self.chat_scanning = true;
        let data_dir = self.data_dir.clone();
        worker::job(
            store,
            move |s| Ok(worker::scan_chats(s, &data_dir, false)),
            Message::ChatsScanned,
        )
    }

    /// Shows one section of the settings screen from its top. A search would cover it,
    /// so the search ends, with any editor it was still loading; the AI connection
    /// section reads every AI app's registration again, as opening settings from the
    /// menu does.
    pub(crate) fn open_settings_section(&mut self, section: SettingsSection) -> Task<Message> {
        self.page = Page::Settings;
        self.settings_section = section;
        self.search.clear();
        self.search_generation += 1;
        self.search_results.clear();
        // An editor still loading from the screen left behind must not open over this one.
        if self.editor.is_none() {
            self.editor_generation += 1;
        }
        let top = iced::widget::operation::snap_to(
            crate::views::SETTINGS_SCROLL,
            iced::widget::operation::RelativeOffset::START,
        );
        if section != SettingsSection::Mcp {
            return top;
        }
        let endpoint = self.mcp_endpoint();
        let board = self.mcp_setup.update(
            crate::mcp_setup_ui::Action::Refresh,
            self.data_dir.clone(),
            endpoint,
        );
        Task::batch([top, board])
    }
    pub fn refresh(&mut self) -> Task<Message> {
        self.calendar.generation += 1;
        self.view_generation += 1;
        let generation = self.view_generation;
        self.expanded.clear();
        self.bodies.clear();
        self.timelines.clear();
        self.entry_errors.clear();
        self.paging = false;
        // The previous snapshot may remain visible while loading, but it cannot
        // provide an offset for the new scope, including after a failed load.
        self.more = false;
        // Until this load succeeds, nothing on screen is known to be its rows.
        self.entries_project = None;
        self.loading = false;
        let Some(store) = self.store.clone() else {
            return Task::none();
        };
        self.loading = true;
        let scope = self.scope();
        let detail = if let Some(form) = self.calendar.detail.clone() {
            let g = self.calendar.generation;
            let quick = self.calendar.detail_quick;
            worker::job(
                store.clone(),
                move |s| worker::hydrate_form(s, form),
                move |r| {
                    crate::calendar_ui::message(crate::calendar_ui::Action::Detail(g, quick, r))
                },
            )
        } else {
            Task::none()
        };
        let snapshot = worker::job(
            store,
            move |s| worker::snapshot(s, &scope),
            move |result| Message::Loaded(generation, result),
        );
        // Cards follow edits, deletions and newly delivered records.
        let group = self.calendar.group.as_ref().map_or_else(Task::none, |g| {
            crate::calendar_ui::load_group(self, g.date.clone(), g.project_id.clone(), g.quick)
        });
        Task::batch([snapshot, detail, group, self.load_quick(false)])
    }
    fn poll_record_inbox(&mut self) -> Task<Message> {
        if !self.records_due
            || self.records_processing
            || self.busy
            || self.loading
            || self.editor.is_some()
            || self.quit_when_idle
            || self.data_dir.as_os_str().is_empty()
        {
            return Task::none();
        }
        let Some(store) = self.store.clone() else {
            return Task::none();
        };
        self.records_due = false;
        self.records_processing = true;
        self.busy = true;
        let dir = self.data_dir.clone();
        worker::job(
            store,
            move |s| crate::record_inbox::drain(&dir, s),
            Message::RecordsDrained,
        )
    }
    fn open_widget(&mut self) -> Task<Message> {
        if self.widget.is_some() {
            return Task::none();
        }
        let mut settings = widget_window_settings();
        settings.size = Size::new(
            self.preferences.widget.width as f32,
            self.preferences.widget.height as f32,
        );
        let (id, open) = window::open(settings);
        self.widget = Some(id);
        open.map(Message::WidgetReady)
    }
    pub(crate) fn native(&mut self, action: NativeAction) -> Task<Message> {
        if let NativeAction::Opacity(value) = action {
            if (40..=100).contains(&value) {
                self.preferences.widget_opacity = value;
                return self.save_preferences();
            }
            return Task::none();
        }
        if self.native_busy {
            self.native_pending = Some(action);
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
                    configure_widget_geometry(&owned, &prefs)?;
                    windows::enable_per_pixel_alpha(&owned)?;
                    windows::set_locked(&owned, prefs.widget_locked)?;
                    windows::set_visible(&owned, prefs.widget_visible)?;
                    visible = Some(prefs.widget_visible);
                }
                NativeAction::Show => {
                    if !windows::status(&owned)?.attached {
                        fit_widget_before_attach(&owned, &prefs)?;
                        windows::attach(&owned)?;
                        configure_widget_geometry(&owned, &prefs)?;
                        windows::enable_per_pixel_alpha(&owned)?;
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
                        configure_widget_geometry(&owned, &prefs)?;
                        windows::enable_per_pixel_alpha(&owned)?;
                        windows::set_locked(&owned, prefs.widget_locked)?;
                    }
                    windows::recover_position(&owned)?;
                    windows::set_visible(&owned, true)?;
                    visible = Some(true);
                }
                NativeAction::Lock(value) => windows::set_locked(&owned, value)?,
                NativeAction::Opacity(_) => unreachable!("background opacity is a UI preference"),
                NativeAction::Resize(width, height) => {
                    windows::resize_logical(&owned, width, height)?
                }
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
                size: if info.attached {
                    windows::logical_size(&owned).ok().flatten()
                } else {
                    None
                },
            })
        })
        .map(move |result| Message::NativeDone(action, result))
    }
    pub(crate) fn save_preferences(&mut self) -> Task<Message> {
        self.widget_ui.save_due = None;
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
    pub(crate) fn open_form(&mut self, form: Form) -> Task<Message> {
        if self.busy || self.loading {
            self.notice = "저장된 내용을 갱신하는 중입니다. 잠시 후 다시 선택하세요.".into();
            return Task::none();
        }
        if self.editor.is_some() {
            self.notice = "현재 편집을 먼저 마쳐 주세요.".into();
            return Task::none();
        }
        self.editor_generation += 1;
        self.editor = Some(Editor::new(form, self.editor_generation));
        iced::widget::operation::focus("editor-title")
    }
    pub(crate) fn editor_date_context(&self) -> NaiveDate {
        self.quick_date
            .filter(|_| self.editor_in_quick)
            .unwrap_or(self.selected)
    }
    fn editor_window(&self) -> Option<window::Id> {
        if self.editor_in_quick {
            self.quick_window
        } else {
            Some(self.manager)
        }
    }
    fn request_close(&mut self, intent: CloseIntent) -> Task<Message> {
        if let Some(editor) = &mut self.editor {
            if !editor.request_close(intent) {
                return if self.editor_in_quick {
                    self.focus_quick()
                } else {
                    Task::none()
                };
            }
        }
        self.finish_close(intent)
    }
    fn finish_close(&mut self, intent: CloseIntent) -> Task<Message> {
        self.editor = None;
        self.editor_in_quick = false;
        self.editor_generation += 1;
        match intent {
            CloseIntent::Editor => Task::none(),
            CloseIntent::Quick => self.close_quick(),
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
        // Quitting does not wait for dozens of days: the check of the past days and their
        // making stop after the day in progress.
        if self.daily_making && self.daily_kind.past_project().is_some() {
            self.past_stop
                .store(true, std::sync::atomic::Ordering::Release);
        }
        if self.widget_ui.save_due.is_some() {
            self.quit_when_idle = true;
            return self.save_preferences();
        }
        if self.busy
            || self.prefs_saving
            || self.native_busy
            || self.mcp_setup.busy
            || self.chat_scanning
            || self.daily_making
        {
            self.quit_when_idle = true;
            self.notice = "진행 중인 저장·Git·설정 작업을 마친 뒤 종료합니다.".into();
            return Task::none();
        }
        self.log("clean exit");
        iced::exit()
    }
    pub(crate) fn show_manager(&mut self) -> Task<Message> {
        self.manager_visible = true;
        Task::batch([
            window::set_mode(self.manager, window::Mode::Windowed),
            window::minimize(self.manager, false),
        ])
        .chain(window::gain_focus(self.manager))
    }
    fn apply_geometry_event(&mut self, event: windows::GeometryEvent) {
        self.widget_ui.actual_size =
            Size::new(event.actual_size.0 as f32, event.actual_size.1 as f32);
        let recovered = self.widget_ui.geometry_error.is_some() && event.error.is_none();
        self.widget_ui.geometry_error = event
            .error
            .map(|error| format!("위젯 화면 보정 실패: {error}"));
        if recovered {
            self.desktop_message = "위젯 화면 보정 완료 · 현재 작업 영역에 맞게 표시합니다.".into();
        }
        if let Some(p) = event.preferred {
            let before = (
                self.preferences.widget_x,
                self.preferences.widget_y,
                self.preferences.widget.width,
                self.preferences.widget.height,
            );
            let after = (Some(p.x), Some(p.y), p.width, p.height);
            if before != after {
                self.preferences.widget_x = Some(p.x);
                self.preferences.widget_y = Some(p.y);
                self.preferences.widget.width = p.width;
                self.preferences.widget.height = p.height;
                self.widget_ui.width = p.width.to_string();
                self.widget_ui.height = p.height.to_string();
                self.widget_ui.save_due = Some(std::time::Instant::now());
            }
        }
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Queue-only drain: no desktop query, busy transition or task launch.
        // Do this before Quit/Lock too, so a just-finished move cannot be lost.
        for event in windows::take_geometry_events() {
            if self.widget.is_some() {
                self.apply_geometry_event(event);
            }
        }

        if let Message::CloseWindow(id) | Message::EscapeWindow(id) = &message {
            if self
                .widget_ui
                .jump
                .as_ref()
                .is_some_and(|j| j.window == *id)
            {
                return self.jump_message(crate::month_jump::JumpMessage::Cancel);
            }
        }
        if let Message::EscapeWindow(id) = &message {
            if self.categories.open && *id == self.manager {
                return crate::category_ui::update(self, crate::category_ui::Action::Close);
            }
            if self.calendar.detail.is_some()
                && ((*id == self.manager && !self.calendar.detail_quick)
                    || (Some(*id) == self.quick_window && self.calendar.detail_quick))
            {
                return crate::calendar_ui::update(self, crate::calendar_ui::Action::CloseDetail);
            }
            if self.editor.is_none()
                && self.calendar.group.as_ref().is_some_and(|g| {
                    (*id == self.manager && !g.quick) || (Some(*id) == self.quick_window && g.quick)
                })
            {
                return crate::calendar_ui::update(self, crate::calendar_ui::Action::CloseGroup);
            }
        }
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
                    | Message::EscapeWindow(_)
                    | Message::Calendar(crate::calendar_ui::Action::QuickPage(..))
                    | Message::Calendar(crate::calendar_ui::Action::GroupLoaded(..))
                    | Message::Calendar(crate::calendar_ui::Action::GroupMoreLoaded(..))
                    | Message::QuickReady(_)
                    | Message::PopupPositioned(..)
                    | Message::Jump(crate::month_jump::JumpMessage::Ready(_))
                    | Message::Tick
                    | Message::RecordsDue
                    | Message::RecordsDrained(_)
                    | Message::ChatsScanned(_)
                    | Message::DailyMade(_)
                    | Message::RepoFolderPicked(..)
                    | Message::NativeDone(..)
                    | Message::McpSetup(crate::mcp_setup_ui::Action::Finished(_))
                    | Message::McpSetup(crate::mcp_setup_ui::Action::Loaded(..))
                    | Message::McpSetup(crate::mcp_setup_ui::Action::Reissued(..))
                    | Message::Category(
                        crate::category_ui::Action::Deleted(_)
                            | crate::category_ui::Action::DeletePreviewed { .. }
                            | crate::category_ui::Action::Saved(_)
                            | crate::category_ui::Action::Reordered(_)
                    )
                    | Message::DisplayObserverReady(..)
                    | Message::PrefsDone(_)
                    | Message::Loaded(..)
                    | Message::SearchDone(..)
                    | Message::Saved(..)
                    | Message::MutationDone(_)
                    | Message::Drafted(..)
                    | Message::ChatReview(
                        crate::chat_review_ui::Action::Read(..)
                            | crate::chat_review_ui::Action::Finished(..)
                    )
                    | Message::Portfolio(
                        crate::portfolio_ui::Action::Answered(..)
                            | crate::portfolio_ui::Action::Exported(..)
                    )
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
            Message::Calendar(action) => return crate::calendar_ui::update(self, action),
            Message::Category(action) => return crate::category_ui::update(self, action),
            Message::McpSetup(action) => {
                use crate::mcp_setup_ui::Action;
                // What the owner starts, and a failed reissue, shows in the advanced part,
                // so it opens and stays open until the owner closes it. Results that only
                // arrive (a board reading, a finished check) leave the owner's choice alone.
                let opens = !matches!(
                    action,
                    Action::Refresh
                        | Action::Loaded(..)
                        | Action::Finished(_)
                        | Action::Reissued(_, Ok(()))
                );
                let endpoint = self.mcp_endpoint();
                let task = self
                    .mcp_setup
                    .update(action, self.data_dir.clone(), endpoint);
                if opens {
                    self.mcp_advanced = Some(true);
                } else if self.mcp_advanced.is_none()
                    && crate::mcp_setup_ui::advanced_needed(&self.mcp_setup)
                {
                    // Opened by itself once, it no longer folds while the state moves on.
                    self.mcp_advanced = Some(true);
                }
                return task;
            }
            Message::SetMcpEnabled(enabled) => {
                if !self.can_set_mcp(enabled) {
                    return Task::none();
                }
                if let Err(error) = crate::mcp_power::Power::set(&self.data_dir, enabled) {
                    self.error = error;
                    return Task::none();
                }
                self.mcp_setup.preview = None;
                self.mcp_setup.status.clear();
                self.mcp_setup.token = Default::default();
                self.apply_mcp_power(enabled);
                let endpoint = self.mcp_endpoint();
                return self.mcp_setup.update(
                    crate::mcp_setup_ui::Action::Refresh,
                    self.data_dir.clone(),
                    endpoint,
                );
            }
            Message::SetChatCapture(id, enabled) => {
                if let Some(store) = self
                    .store
                    .as_ref()
                    .filter(|_| self._instance.is_some() && self.project.as_ref() == Some(&id))
                {
                    match store.set_chat_capture(&id, enabled) {
                        Ok(()) => {
                            if enabled {
                                // The sessions already kept for the folder are checked
                                // once the scan has kept the newest ones. What is being
                                // checked or made is left alone.
                                if !self.past_days.get(&id).is_some_and(Past::running) {
                                    self.past_days.insert(id.clone(), Past::Due);
                                }
                            } else {
                                self.forget_past_days(&id);
                            }
                            return Task::batch([self.refresh(), Task::done(Message::ScanChats)]);
                        }
                        Err(e) => self.error = e,
                    }
                }
            }
            Message::ScanChats => return self.scan_chats(),
            Message::ChatsScanned(result) => {
                self.chat_scanning = false;
                let mut tasks = Vec::new();
                match result {
                    Ok(scan) => {
                        let problems = scan.problems();
                        if !problems.is_empty() {
                            self.error = problems.join(" ");
                        }
                        let notices = scan.notices();
                        if !notices.is_empty() {
                            self.notice = notices.join(" ");
                            tasks.push(self.refresh());
                        }
                    }
                    Err(e) => self.error = format!("로컬 채팅 수집 실패 · {e}"),
                }
                // At start and every hour, and a job that waits for a scan. The job
                // keeps the newest originals itself.
                // The scan a retry waited for has ended: the retry is a turn now, so it is
                // still owed when a job that runs at this moment ends.
                if self.daily_retry_after_scan {
                    self.daily_retry_after_scan = false;
                    self.daily_auto_due = true;
                }
                // The automatic turn goes first; a check of the past days that is owed
                // starts when no automatic turn does.
                if self.daily_auto_due
                    && self.preferences.daily_record == preferences::DailyRecordMode::Auto
                {
                    tasks.push(self.make_daily(
                        DailyKind::Auto,
                        DailyScope::Days(worker::finished_days(kst_today())),
                    ));
                }
                if let Some(check) = self.start_past_check() {
                    tasks.push(check);
                }
                return Task::batch(tasks);
            }
            Message::SettingsSection(section) => return self.open_settings_section(section),
            Message::McpAdvanced(open) => self.mcp_advanced = Some(open),
            Message::ProjectTab(tab) => {
                let changed = tab != self.project_tab;
                self.project_tab = tab;
                if changed {
                    return projects_top();
                }
            }
            Message::SetTheme(theme) if theme != self.preferences.theme => {
                return self.update(Message::ToggleTheme);
            }
            Message::SetDailyMode(mode) => {
                if self.preferences.daily_record != mode {
                    self.preferences.daily_record = mode;
                    self.daily_auto.store(
                        mode == preferences::DailyRecordMode::Auto,
                        std::sync::atomic::Ordering::Release,
                    );
                    self.daily_auto_due = true;
                    return self.save_preferences();
                }
            }
            Message::MakeDaily(date) => {
                return self.make_daily(DailyKind::Button, DailyScope::Days(vec![date]))
            }
            Message::CheckPastDays(id) => {
                // A check or the making in progress is not replaced.
                if self.past_days.get(&id).is_some_and(Past::running) {
                    return Task::none();
                }
                self.past_days.insert(id, Past::Due);
                if let Some(check) = self.start_past_check() {
                    return check;
                }
            }
            Message::MakePastDays(id) => {
                // Not during a chat scan either: the job would meet the scan at the
                // project's lock, make nothing, and the answer would be used up.
                if let (Some(Past::Ask(days, root)), true) =
                    (self.past_days.get(&id), self.past_days_can_start())
                {
                    let (days, root) = (days.clone(), root.clone());
                    let task = self.make_daily(
                        DailyKind::Past(id.clone()),
                        DailyScope::ProjectDays {
                            project: id.clone(),
                            days,
                            root,
                        },
                    );
                    if self.daily_making {
                        self.past_days.insert(id, Past::Making);
                        return task;
                    }
                }
            }
            Message::DismissPastDays(id) => {
                if matches!(self.past_days.get(&id), Some(Past::Ask(..))) {
                    self.past_days.remove(&id);
                }
            }
            Message::StopPastDays(id) => {
                // Only the making it was pressed for: a press that arrives after that job
                // ended must not stop the job that started since.
                if self.daily_making && self.daily_kind == DailyKind::Past(id) {
                    self.past_stop
                        .store(true, std::sync::atomic::Ordering::Release);
                }
            }
            Message::PickRepoFolder => {
                let Some(e) = self
                    .editor
                    .as_mut()
                    .filter(|e| e.form.kind == Kind::Project && !e.saving && !e.picking_repo)
                else {
                    return Task::none();
                };
                e.picking_repo = true;
                let generation = e.generation;
                let start = PathBuf::from(e.form.repo_path.trim());
                return Task::perform(
                    async move {
                        let mut dialog = rfd::AsyncFileDialog::new();
                        if start.is_dir() {
                            dialog = dialog.set_directory(start);
                        }
                        dialog
                            .pick_folder()
                            .await
                            .map(|folder| folder.path().to_path_buf())
                    },
                    move |folder| Message::RepoFolderPicked(generation, folder),
                );
            }
            Message::RepoFolderPicked(generation, folder) => {
                // Only the editor that asked takes the answer, not one opened since, even
                // when its own dialog is open. A cancelled dialog changes nothing.
                if let Some(e) = self.editor.as_mut().filter(|e| {
                    e.generation == generation && e.picking_repo && e.form.kind == Kind::Project
                }) {
                    e.picking_repo = false;
                    if let Some(folder) = folder {
                        e.form.repo_path = folder.to_string_lossy().into_owned();
                    }
                }
            }
            Message::DailyMade(result) => {
                self.daily_making = false;
                let kind = self.daily_kind.clone();
                // The job is over, however it ended: no project stays checked or made. Capture
                // turned off meanwhile removed the project of the job already.
                let running = kind
                    .past_project()
                    .is_some_and(|id| self.past_days.get(id).is_some_and(Past::running));
                self.past_days.retain(|_, state| !state.running());
                let mut tasks = Vec::new();
                let mut retry_after_scan = false;
                match result {
                    Ok(made) => {
                        // Only the days the automatic mode still looks at are remembered.
                        let today = kst_today();
                        let oldest = today - chrono::Days::new(7);
                        self.daily_failed.extend(made.failed_inputs.iter().cloned());
                        self.daily_failed
                            .retain(|(_, day, _)| *day >= oldest && *day < today);
                        if !made.failed.is_empty() {
                            self.error = format!("하루 정리 실패 · {}", made.failed.join(" / "));
                            if kind != DailyKind::Auto && made.unplaced > 0 {
                                self.error.push_str(&unplaced_note(made.unplaced));
                            }
                        }
                        // An automatic job that met a project another run was keeping
                        // tries again after the next chat scan, not at once.
                        retry_after_scan = kind == DailyKind::Auto && !made.unkept.is_empty();
                        // The question of a project is asked again after days were made: it
                        // must not promise calls for days that were just made.
                        if matches!(kind, DailyKind::Auto | DailyKind::Button)
                            && !made.saved.is_empty()
                        {
                            for state in self.past_days.values_mut() {
                                if matches!(state, Past::Ask(..)) {
                                    *state = Past::Due;
                                }
                            }
                        }
                        match &kind {
                            DailyKind::PastCheck(id) => {
                                self.past_check_done(id.clone(), running, &made)
                            }
                            DailyKind::Past(id) => {
                                if let Some(notice) = past_notice(&made) {
                                    self.notice = notice;
                                }
                                // Originals that could not all be kept (a chat scan
                                // held the project): no day was made, so the answer
                                // is not used up and the days are counted again.
                                if running && !made.unkept.is_empty() {
                                    self.past_days.insert(id.clone(), Past::Due);
                                }
                            }
                            DailyKind::Auto | DailyKind::Button => {
                                if let Some(notice) = daily_notice(&made, kind == DailyKind::Button)
                                {
                                    self.notice = notice;
                                }
                            }
                        }
                        // The job showed its new records in the diary already.
                        if made.shown > 0 {
                            tasks.push(self.refresh());
                        }
                    }
                    Err(e) => {
                        self.error = format!("하루 정리 실패 · {e}");
                    }
                }
                // Set first, so an automatic job started below covers it.
                if retry_after_scan {
                    self.daily_retry_after_scan = true;
                }
                // The automatic turn goes first; a check of the past days that is owed
                // starts when no automatic turn does.
                if self.daily_auto_due
                    // An automatic turn that came while this job ran. Not while a chat scan
                    // keeps originals: the two would meet at a project's lock, and the scan
                    // starts the turn when it ends.
                    && !self.chat_scanning
                    && self.preferences.daily_record == preferences::DailyRecordMode::Auto
                {
                    tasks.push(self.make_daily(
                        DailyKind::Auto,
                        DailyScope::Days(worker::finished_days(kst_today())),
                    ));
                }
                if let Some(check) = self.start_past_check() {
                    tasks.push(check);
                }
                return Task::batch(tasks);
            }
            Message::RecordsDue => {
                self.records_due = true;
                return self.poll_record_inbox();
            }
            Message::RecordsDrained(result) if self.records_processing => {
                self.records_processing = false;
                self.busy = false;
                match result {
                    Ok(report) => {
                        if report.conflicts > 0 {
                            self.error = format!(
                                "자동 기록 {}건은 사용자 수정·삭제를 보존하기 위해 보류했습니다.",
                                report.conflicts
                            );
                        }
                        if !report.errors.is_empty() {
                            self.error = report.errors.join("\n");
                        }
                        if report.saved > 0 {
                            self.notice = format!(
                                "프로젝트 기록 {}건을 다이어리에 반영했습니다.",
                                report.saved
                            );
                            self.log(&format!(
                                "project records saved={} conflicts={} failed={}",
                                report.saved, report.conflicts, report.failed
                            ));
                            return self.refresh();
                        }
                        if report.unchanged > 0 {
                            // A previous delivery may have committed before its
                            // acknowledgement failed. Drop old editable bodies.
                            return self.refresh();
                        }
                    }
                    Err(error) => {
                        self.error = format!("자동 기록 수신 실패: {error}");
                        // Errors can happen after the SQLite transaction commits.
                        // Refresh before allowing any edit of the cached snapshot.
                        return self.refresh();
                    }
                }
            }
            Message::WidgetSetting(change) => return self.widget_setting(change),
            Message::Jump(change) => return self.jump_message(change),
            Message::ResizeWidget if !self.preferences.widget_locked && !self.native_busy => {
                if let Some(id) = self.widget {
                    return window::drag_resize(id, window::Direction::SouthEast);
                }
            }
            Message::ToggleTheme => {
                self.preferences.theme = match self.preferences.theme {
                    AppTheme::Midnight => AppTheme::Porcelain,
                    AppTheme::Porcelain => AppTheme::Midnight,
                };
                self.sync_color_draft();
                return self.save_preferences();
            }
            Message::Resized(id, size) if id == self.manager => self.manager_size = size,
            Message::Resized(id, size) if Some(id) == self.widget => {
                self.widget_ui.actual_size = size;
                // Native move-end notifications alone update authored geometry.
                // Iced resize events also describe temporary automatic corrections.
            }
            Message::Ready(id) => {
                self.log(&format!("manager ready {id:?}"));
                return window::run(id, |window| {
                    let handle = window.window_handle().map_err(|e| e.to_string())?;
                    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
                        return Err("Windows 전용 화면 알림입니다.".into());
                    };
                    let owned = unsafe { windows::NativeWindow::iced_widget(raw.hwnd.get()) };
                    windows::observe_display_changes(&owned)
                })
                .map(Message::DisplayObserverReady);
            }
            Message::DisplayObserverReady(result) => {
                self.widget_ui.display_observer_error = result
                    .err()
                    .map(|error| format!("화면 변경 알림 연결 실패: {error}"));
                if let Some(error) = &self.widget_ui.display_observer_error {
                    self.log(error);
                }
            }
            Message::WidgetReady(id) => {
                self.log(&format!("widget ready {id:?}"));
                return self.native(NativeAction::Attach);
            }
            Message::WindowClosed(id) => {
                self.placing_popups.remove(&id);
                if self.widget_ui.jump.as_ref().is_some_and(|j| j.window == id) {
                    self.widget_ui.jump = None;
                }
                if Some(id) == self.quick_window {
                    // Preserve an unexpected-close draft in the manager.
                    self.quick_window = None;
                    self.quick_date = None;
                    self.quick_generation += 1;
                    self.quick_loading = false;
                    // Cards and a card edit still loading belonged to it.
                    if self.calendar.group.as_ref().is_some_and(|g| g.quick) {
                        self.calendar.group = None;
                        self.calendar.generation += 1;
                    }

                    if self.editor_in_quick {
                        self.editor_in_quick = false;
                        return self.show_manager();
                    }
                }
                if Some(id) == self.widget {
                    self.widget = None;
                    self.native_busy = false;
                    self.desktop_message =
                        "위젯 창이 종료되었습니다. 설정에서 다시 표시하세요.".into();
                }
            }
            Message::CloseWindow(id) => {
                return if Some(id) == self.quick_window {
                    if self.editor_in_quick {
                        self.request_close(CloseIntent::Quick)
                    } else {
                        self.close_quick()
                    }
                } else if Some(id) == self.widget {
                    self.native(NativeAction::Hide)
                } else if self.editor_in_quick {
                    self.manager_visible = false;
                    window::set_mode(self.manager, window::Mode::Hidden)
                } else {
                    self.request_close(CloseIntent::Manager)
                }
            }
            Message::Tick => {
                if self.quit_when_idle
                    && !self.busy
                    && !self.prefs_saving
                    && !self.native_busy
                    && !self.mcp_setup.busy
                    && !self.chat_scanning
                    && !self.daily_making
                {
                    self.quit_when_idle = false;
                    return self.request_close(CloseIntent::Application);
                }
                self.ticks += 1;
                // Set before a tray command can end this tick early.
                if self.ticks % RECORDS_POLL_TICKS == 0 {
                    self.records_due = true;
                }
                if self.ticks % CHAT_SCAN_TICKS == 0 {
                    self.chat_scan_due = true;
                    self.daily_auto_due = true;
                }
                if let Some(message) = self.record_sync.poll(&self.data_dir) {
                    self.log(&message);
                }
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
                let mut tasks = vec![self.widget_today_changed()];
                if self
                    ._mcp
                    .as_ref()
                    .is_some_and(|server| !server.is_running())
                {
                    self.mcp_status = "중지됨 · MCP 켜기로 다시 시작하세요.".into();
                }
                if self
                    .mcp_changed
                    .swap(false, std::sync::atomic::Ordering::AcqRel)
                {
                    tasks.push(self.refresh());

                    self.notice =
                        "외부 도구의 일정 변경을 반영했습니다. 작성 중인 초안은 유지됩니다.".into();
                }
                if self.chat_scan_due {
                    tasks.push(self.scan_chats());
                }
                if self.records_due {
                    tasks.push(self.poll_record_inbox());
                }
                if self
                    .widget_ui
                    .save_due
                    .is_some_and(|due| due <= std::time::Instant::now())
                {
                    tasks.push(self.save_preferences());
                }
                return Task::batch(tasks);
            }
            Message::Navigate(page) if self.editor.is_none() => {
                self.page = page;
                self.search.clear();
                self.search_generation += 1;
                self.search_results.clear();
                if page == Page::Settings {
                    // Opening settings re-reads every AI app's registration.
                    let endpoint = self.mcp_endpoint();
                    let board = self.mcp_setup.update(
                        crate::mcp_setup_ui::Action::Refresh,
                        self.data_dir.clone(),
                        endpoint,
                    );
                    return Task::batch([self.refresh(), board]);
                }
                if page == Page::Projects {
                    return Task::batch([self.refresh(), self.scan_chats()]);
                }
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
                self.widget_ui.anchor = self.selected;
                self.widget_ui.follow_today = true;
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
                let date = if self.preferences.widget.rolling {
                    self.widget_ui
                        .anchor
                        .checked_add_signed(Duration::weeks(i64::from(delta)))
                        .unwrap_or(self.widget_ui.anchor)
                } else {
                    shift_month(self.widget_month, delta)
                };
                return self.jump_widget(date, false);
            }
            Message::WidgetToday => {
                return self.jump_widget(Local::now().date_naive(), true);
            }
            Message::WidgetDate(date) => {
                return self.open_quick(date, self.widget.unwrap_or(self.manager));
            }
            Message::ManagerDate(date) => {
                return self.open_quick(date, self.manager);
            }
            Message::PopupPositioned(id, token, result) => {
                return self.popup_positioned(id, token, result)
            }
            Message::QuickReady(id) if Some(id) == self.quick_window => {
                return self.place_popup(self.quick_anchor(), id);
            }
            Message::Project(id) if self.editor.is_none() => {
                // Another project starts on its records, from the top, with nothing of
                // the previous one's review or portfolio result; the same one keeps them.
                let other = id != self.project;
                if other {
                    self.portfolio_feedback = None;
                    self.chat_review = Default::default();
                    self.project_tab = ProjectTab::Records;
                }
                self.project = id;
                self.page = Page::Projects;
                self.search.clear();
                self.search_generation += 1;
                let top = if other { projects_top() } else { Task::none() };
                return Task::batch([self.refresh(), self.scan_chats(), top]);
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
            Message::EditForm(form) if !self.busy && !self.loading => {
                if form.id.is_some() && !form.hydrated && self.editor.is_none() {
                    if let Some(store) = self.store.clone() {
                        self.editor_generation += 1;
                        let eg = self.editor_generation;
                        let vg = self.view_generation;
                        return worker::job(
                            store,
                            move |s| worker::hydrate_form(s, form),
                            move |r| Message::FormLoaded(vg, eg, r),
                        );
                    }
                } else {
                    return self.open_form(form);
                }
            }
            Message::FetchForm(hit) if self.editor.is_none() => {
                let Some(store) = self.store.clone() else {
                    return Task::none();
                };
                self.editor_generation += 1;
                let eg = self.editor_generation;
                let vg = self.view_generation;
                return worker::job(
                    store,
                    move |s| worker::fetch_form(s, &hit).and_then(|f| worker::hydrate_form(s, f)),
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
                            snapshot.days.iter().map(|d| d.total).sum::<u32>(),
                            snapshot.projects.len()
                        ));
                        self.more = snapshot.entries.len() == 200;
                        self.snapshot = snapshot;
                        // A matching generation means page and project are the ones loaded.
                        self.entries_project = (self.page == Page::Projects)
                            .then(|| self.project.clone())
                            .flatten();
                        self.rows_project =
                            (self.page == Page::Projects).then(|| self.project.clone());
                        self.rows_more = self.more;
                        if self
                            .project
                            .as_ref()
                            .is_some_and(|id| !self.snapshot.projects.iter().any(|p| &p.id == id))
                        {
                            // The open project is gone; its rows were filtered by it, so
                            // read every project's rows for "모든 프로젝트".
                            self.project = None;
                            return self.refresh();
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
                    move |s| worker::entry_page(s, &scope, offset),
                    move |r| Message::MoreLoaded(g, r),
                );
            }
            Message::MoreLoaded(g, result) if g == self.view_generation && self.paging => {
                self.paging = false;
                match result {
                    Ok((rows, totals)) => {
                        self.more = rows.len() == 200;
                        self.rows_more = self.more;
                        // A record moved between pages is listed once.
                        let known: HashSet<String> =
                            self.snapshot.entries.iter().map(|e| e.id.clone()).collect();
                        self.snapshot
                            .entries
                            .extend(rows.into_iter().filter(|e| !known.contains(&e.id)));
                        self.snapshot.entry_totals.extend(totals);
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
                let dir = self.data_dir.clone();
                return worker::job(
                    store,
                    move |s| {
                        worker::complete_evidence(s, &dir, std::slice::from_ref(&id));
                        Ok((s.get_entry(&id)?, s.entry_timeline(&id)?))
                    },
                    move |r| Message::EntryLoaded(g, key.clone(), r),
                );
            }
            Message::EntryLoaded(g, id, result) if g == self.view_generation => match result {
                Ok((entry, timeline)) => {
                    self.timelines.insert(id.clone(), timeline);
                    self.bodies.insert(id, entry);
                }
                Err(e) => {
                    self.entry_errors.insert(id, e);
                }
            },
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
            Message::OpenOriginal(work_id) => {
                let dir = self.data_dir.clone();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            crate::record_original::open(&dir, &work_id)
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    Message::SourceDone,
                );
            }
            Message::SourceDone(Err(e)) => self.error = e,
            Message::DatePicker(part) => {
                let fallback = self.editor_date_context();
                if let Some(e) = self.editor.as_mut().filter(|e| !e.saving) {
                    let date = e.form.date.parse::<NaiveDate>().unwrap_or(fallback);
                    e.picker = Some(EditorPicker::Date(crate::date_choices::Picker::new(
                        part, date,
                    )));
                }
            }
            Message::BrowseDatePicker(direction) => {
                if let Some(EditorPicker::Date(p)) =
                    self.editor.as_mut().and_then(|e| e.picker.as_mut())
                {
                    p.browse(direction);
                }
            }
            Message::PickEditorDate(date) => {
                if let Some(e) = self
                    .editor
                    .as_mut()
                    .filter(|e| !e.saving && matches!(e.picker, Some(EditorPicker::Date(_))))
                {
                    e.form.date = date.to_string();
                    e.picker = None;
                }
            }
            Message::OpenTimePicker(field) => {
                if let Some(e) = self
                    .editor
                    .as_mut()
                    .filter(|e| !e.saving && !e.form.all_day)
                {
                    e.picker = Some(EditorPicker::Time(field));
                }
            }
            Message::PickEditorTime(field, value) => {
                if let Some(e) = self
                    .editor
                    .as_mut()
                    .filter(|e| !e.saving && e.time_picker() == Some(field))
                {
                    match field {
                        crate::time_input::Field::Start => e.form.start = value,
                        crate::time_input::Field::End => e.form.end = value,
                    }
                    e.picker = None;
                }
            }
            Message::DismissTimePicker(field) => {
                if let Some(e) = self
                    .editor
                    .as_mut()
                    .filter(|e| e.time_picker() == Some(field))
                {
                    e.picker = None;
                }
            }
            Message::DismissDatePicker => {
                if let Some(e) = self
                    .editor
                    .as_mut()
                    .filter(|e| matches!(e.picker, Some(EditorPicker::Date(_))))
                {
                    e.picker = None;
                }
            }
            Message::DismissEditorPicker => {
                if let Some(e) = &mut self.editor {
                    e.picker = None;
                }
            }
            Message::EditorTab(id, backwards) if self.editor_window() == Some(id) => {
                if let Some(e) = self.editor.as_mut().filter(|e| !e.saving && !e.composing) {
                    use iced::advanced::widget::{operate, operation};
                    e.picker = None;
                    let id = "editor-focus-fields".into();
                    return if backwards {
                        operate(operation::scope(id, operation::focusable::focus_previous()))
                    } else {
                        operate(operation::scope(id, operation::focusable::focus_next()))
                    };
                }
            }
            Message::Field(field) => {
                if let Some(e) = self.editor.as_mut().filter(|e| !e.saving) {
                    if !matches!(
                        (&field, e.time_picker()),
                        (Field::Start(_), Some(crate::time_input::Field::Start))
                            | (Field::End(_), Some(crate::time_input::Field::End))
                    ) {
                        e.picker = None;
                    }
                    match field {
                        Field::Title(v) => e.form.title = v,
                        Field::Category(v) => e.form.category_id = v,
                        Field::ShortTitle(v) => e.form.short_title = v,
                        Field::Time(v) => e.form.time = v.chars().take(5).collect(),
                        Field::ProjectShortName(v) => e.form.project_short_name = v,
                        Field::Date(v) => e.form.date = v,
                        Field::Project(v) => e.form.project_id = v,
                        Field::EntryKind(v) => e.form.entry_kind = v,
                        Field::Source(v) => e.form.source_url = v,
                        Field::Repo(v) => e.form.repo_path = v,
                        Field::Start(v) => e.form.start = v,
                        Field::End(v) => e.form.end = v,

                        Field::AllDay(v) => e.form.all_day = v,
                        Field::Completed(v) => e.form.completed = v,
                    }
                }
            }
            Message::Body(action) => {
                if let Some(e) = &mut self.editor {
                    e.picker = None;
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
            Message::Composition(id, active)
                if (self.editor_in_quick && Some(id) == self.quick_window)
                    || (!self.editor_in_quick && id == self.manager) =>
            {
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
            Message::EscapeWindow(id) => {
                let editing_here = self.editor_window() == Some(id);
                if self.editor.is_some() && !editing_here {
                    return Task::none();
                }
                if self.editor.is_none() && Some(id) == self.quick_window {
                    return self.close_quick();
                }
                return self.update(Message::Escape);
            }
            Message::Escape => {
                if let Some(e) = &mut self.editor {
                    if e.confirmation.is_some() {
                        e.confirmation = None;
                        return Task::none();
                    }
                    if e.picker.take().is_some() {
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
                            if matches!(editor.form.kind, Kind::Event | Kind::Entry | Kind::Task) {
                                if let Ok(date) =
                                    NaiveDate::parse_from_str(&editor.form.date, "%Y-%m-%d")
                                {
                                    self.selected = date;
                                }
                            }
                        }
                        // A deleted project has no past days to ask about. A saved one
                        // keeps its question: the confirmed days are made only while
                        // the project's folder is the one they were counted in.
                        let project = self
                            .editor
                            .as_ref()
                            .filter(|e| e.form.kind == Kind::Project && e.deleting)
                            .and_then(|e| e.form.id.clone());
                        if let Some(project) = project {
                            self.forget_past_days(&project);
                        }
                        self.editor = None;
                        self.editor_in_quick = false;
                        self.editor_generation += 1;
                        self.notice = "내 컴퓨터에 저장했습니다.".into();
                        return self.refresh();
                    }
                    Err(error) => {
                        if let Some(e) = &mut self.editor {
                            e.saving = false;
                            e.deleting = false;
                            e.confirmation = None;
                            e.error = error;
                        }
                    }
                }
            }
            Message::ToggleTask(id) if !self.busy && self.editor.is_none() => {
                if let Some(t) = self.snapshot.tasks.iter().find(|t| t.id == id) {
                    let completed = !t.completed;
                    let Some(store) = self.store.clone() else {
                        return Task::none();
                    };
                    self.busy = true;
                    return worker::job(
                        store,
                        move |s| {
                            s.set_calendar_completed(
                                timeschedule_core::calendar::CalendarSource::Task,
                                &id,
                                completed,
                            )
                        },
                        Message::MutationDone,
                    );
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
            Message::ChatReview(action) => return crate::chat_review_ui::update(self, action),
            Message::Portfolio(action) => return crate::portfolio_ui::update(self, action),
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
                        self.log(&format!(
                            "widget action={action:?} attached={} visible={:?} position={:?}",
                            report.attached, report.visible, report.position
                        ));
                        let before = (
                            self.preferences.widget_x,
                            self.preferences.widget_y,
                            self.preferences.widget_locked,
                            self.preferences.widget_visible,
                            self.preferences.widget_opacity,
                            self.preferences.widget.width,
                            self.preferences.widget.height,
                        );
                        self.desktop_message = report.message;
                        self.widget_ui.native_error = None;
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
                        if let Some((width, height)) = report.size {
                            self.widget_ui.actual_size = Size::new(width as f32, height as f32);
                        }
                        if let NativeAction::Resize(width, height) = action {
                            // Save the requested logical dimensions, never a work-area clamp.
                            self.preferences.widget.width = width;
                            self.preferences.widget.height = height;
                            self.widget_ui.width = width.to_string();
                            self.widget_ui.height = height.to_string();
                        }
                        let after = (
                            self.preferences.widget_x,
                            self.preferences.widget_y,
                            self.preferences.widget_locked,
                            self.preferences.widget_visible,
                            self.preferences.widget_opacity,
                            self.preferences.widget.width,
                            self.preferences.widget.height,
                        );
                        if before != after {
                            save = self.save_preferences();
                        }
                    }
                    Err(e) => {
                        let error = format!("바탕화면 위젯: {e}");
                        self.log(&error);
                        self.widget_ui.native_error = Some(error);
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
                if !self.preferences.widget_locked {
                    if let Some(id) = self.widget {
                        return window::drag(id);
                    }
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
        editor.picker = None;
        editor.saving = true;
        editor.deleting = delete;
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

/// Today in Korea, where the day transcript counts its days.
fn kst_today() -> NaiveDate {
    chrono::Utc::now()
        .with_timezone(&timeschedule_core::daily_record::kst())
        .date_naive()
}

/// What the notice line says after daily records were made. The automatic mode says
/// only what it made; the button also says why nothing was made.
fn daily_notice(made: &worker::DailyMade, by_button: bool) -> Option<String> {
    use chrono::Datelike;
    if !made.saved.is_empty() {
        let mut dates = made.saved.clone();
        dates.sort();
        dates.dedup();
        let days: Vec<String> = dates
            .iter()
            .map(|d| format!("{}월 {}일", d.month(), d.day()))
            .collect();
        let days = days.join(", ");
        // One record per project and day: two projects on one day are two records.
        let done = if made.saved.len() > dates.len() {
            format!(
                "하루 정리 {}건({days})을 만들었습니다. 곧 다이어리에 보입니다.",
                made.saved.len()
            )
        } else {
            format!("하루 정리 {days}을 만들었습니다. 곧 다이어리에 보입니다.")
        };
        // The button says which projects it could not make yet.
        let mut notice = done;
        if by_button && !made.unkept.is_empty() {
            notice.push_str(&format!(
                " 원문을 다 보관하지 못한 프로젝트({})는 만들지 않았습니다.",
                made.unkept.join(", ")
            ));
        }
        if by_button && made.unplaced > 0 {
            notice.push_str(&unplaced_note(made.unplaced));
        }
        return Some(notice);
    }
    if !by_button || !made.failed.is_empty() {
        return None;
    }
    let reason = if !made.unkept.is_empty() {
        format!(
            "채팅 원문을 다 보관하지 못해 하루 정리를 만들지 않았습니다({}). 잠시 뒤 다시 눌러 주세요.",
            made.unkept.join(", ")
        )
    } else if made.busy > 0 {
        "다른 실행이 이날의 하루 정리를 만들고 있습니다. 잠시 뒤 다시 눌러 주세요.".into()
    } else if made.unchanged > 0 {
        "대화가 바뀌지 않아 하루 정리를 새로 만들지 않았습니다.".into()
    } else if made.empty > 0 {
        "이날은 대화가 없어 하루 정리를 만들지 않았습니다.".into()
    } else {
        "채팅 자동 수집을 켠 프로젝트가 없어 하루 정리를 만들지 않았습니다.".into()
    };
    Some(if made.unplaced > 0 {
        reason + &unplaced_note(made.unplaced)
    } else {
        reason
    })
}

/// What the notice line says after the confirmed past days were made. A job that was
/// stopped with days left says so, and what it made until then.
fn past_notice(made: &worker::DailyMade) -> Option<String> {
    if !made.stopped {
        return daily_notice(made, true);
    }
    let made_so_far = if made.saved.is_empty() {
        None
    } else {
        daily_notice(made, true)
    };
    Some(match made_so_far {
        Some(done) => format!("지난 대화 정리를 멈췄습니다. {done}"),
        None => "지난 대화 정리를 멈췄습니다.".into(),
    })
}

/// Said by the button when files could not be read before it was known whose they are.
fn unplaced_note(count: usize) -> String {
    format!(" 어느 프로젝트의 것인지 모르는 채팅 파일 {count}개는 읽지 못했습니다.")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[path = "visual.rs"]
    mod visual;
    fn fixture() -> App {
        let date = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        App {
            widget_ui: crate::widget_settings::State::new(&Preferences::default(), date),
            calendar: Default::default(),
            categories: Default::default(),
            manager: window::Id::unique(),
            manager_size: Size::new(1160.0, 820.0),
            widget: None,
            quick_window: None,
            placing_popups: HashMap::new(),
            quick_date: None,
            quick_source: None,

            quick_generation: 0,
            quick_loading: false,
            editor_in_quick: false,
            store: Some(Arc::new(Storage::open(":memory:").unwrap())),
            _mcp: None,
            mcp_changed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mcp_status: String::new(),
            mcp_enabled: None,
            mcp_setup: Default::default(),
            data_dir: PathBuf::new(),
            preferences: Preferences::default(),
            snapshot: Snapshot::default(),
            chat_review: Default::default(),
            portfolio_writer: timeschedule_core::terra::generate,
            portfolio_feedback: None,
            portfolio_selection: Default::default(),
            portfolio_editor: None,
            page: Page::Month,
            month: date.with_day(1).unwrap(),
            selected: date,
            week: week_containing(date).from,
            widget_month: date.with_day(1).unwrap(),
            hovered_week: None,
            project: None,
            entries_project: None,
            rows_project: None,
            rows_more: false,
            settings_section: SettingsSection::General,
            mcp_advanced: None,
            project_tab: ProjectTab::Records,
            expanded: HashSet::new(),

            record_sync: crate::record_sync::Runner::default(),
            bodies: HashMap::new(),
            timelines: HashMap::new(),
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
            records_due: false,
            chat_scanning: false,
            chat_scan_due: false,
            daily_making: false,
            daily_auto_due: true,
            daily_retry_after_scan: false,
            daily_kind: DailyKind::Auto,
            past_days: BTreeMap::new(),
            past_stop: Default::default(),
            daily_failed: Default::default(),
            daily_auto: Default::default(),
            records_processing: false,
            manager_visible: true,
            prefs_saving: false,
            prefs_pending: false,
            quit_when_idle: false,
            native_pending: None,
        }
    }

    #[test]
    fn external_calendar_refresh_preserves_open_draft_and_refreshes_quick_list() {
        let mut app = fixture();
        app.widget_ui.today = Local::now().date_naive();
        app.quick_window = Some(window::Id::unique());
        app.quick_date = Some(app.selected);
        let _ = app.update(Message::New(Kind::Event, None));
        let _ = app.update(Message::Field(Field::Title("내 초안".into())));
        let draft = app.editor.as_ref().unwrap().form.clone();
        let view = app.view_generation;
        let quick = app.quick_generation;
        app.mcp_changed
            .store(true, std::sync::atomic::Ordering::Release);
        let _ = app.update(Message::Tick);
        assert_eq!(app.view_generation, view + 1);
        assert_eq!(app.quick_generation, quick + 1);
        assert_eq!(app.editor.as_ref().unwrap().form, draft);
    }

    #[test]
    fn record_queue_waits_for_editor_and_blocks_stale_edits_during_import() {
        let dir = crate::record_inbox::tests::TestDir::new();
        let mut app = fixture();
        app.data_dir = dir.0.clone();
        let _ = app.update(Message::New(Kind::Entry, None));
        let _ = app.update(Message::Field(Field::Title("작성 중인 원문".into())));
        let _ = app.update(Message::RecordsDue);
        assert_eq!(app.editor.as_ref().unwrap().form.title, "작성 중인 원문");
        assert!(!app.busy);
        assert!(app.records_due);
        app.editor = None;
        let _ = app.poll_record_inbox();
        assert!(app.busy);
        let _ = app.update(Message::New(Kind::Entry, None));
        assert!(
            app.editor.is_none(),
            "기록 반영 중 오래된 목록에서 편집기를 열지 않는다"
        );
        let _ = app.update(Message::RecordsDrained(Ok(
            crate::record_inbox::DrainReport {
                saved: 1,
                ..Default::default()
            },
        )));
        assert!(!app.busy);
        assert!(!app.records_processing);
        assert!(app.loading, "반영 후 새 목록을 조회한다");
    }

    #[test]
    fn record_commit_receipt_failure_invalidates_cached_body_before_editing() {
        let dir = crate::record_inbox::tests::TestDir::new();
        let mut app = fixture();
        let store = app.store.clone().unwrap();
        let mut record = crate::record_inbox::tests::sample();
        let id = store
            .apply_project_record(&record)
            .unwrap()
            .entry_id
            .unwrap();
        let old = store.get_entry(&id).unwrap();
        app.bodies.insert(id.clone(), old.clone());
        app.expanded.insert(id.clone());
        record.revision = 2;
        record.summary = "새 검증 결과. 사용자 확인은 남아 있다.".into();
        crate::record_inbox::enqueue(&dir.0, &record).unwrap();
        std::fs::write(dir.0.join("record-receipts"), b"receipt path blocked").unwrap();
        let result = crate::record_inbox::drain(&dir.0, &store);
        assert!(result.is_err());
        assert!(store.get_entry(&id).unwrap().body.contains("새 검증 결과"));
        app.records_processing = true;
        app.busy = true;
        let _ = app.update(Message::RecordsDrained(result));
        assert!(app.loading, "DB commit 이후 수신 결과 실패도 갱신해야 한다");
        assert!(app.bodies.is_empty());
        let _ = app.open_form(Form::entry(old));
        assert!(app.editor.is_none());
        std::fs::remove_file(dir.0.join("record-receipts")).unwrap();
        let replay = crate::record_inbox::drain(&dir.0, &store).unwrap();
        assert_eq!(replay.unchanged, 1);
        app.loading = false;
        app.records_processing = true;
        let _ = app.update(Message::RecordsDrained(Ok(replay)));
        assert!(app.loading, "재시도에서도 이미 커밋된 기록을 다시 읽는다");
    }

    #[test]
    fn customization_range_matches_queries_and_appearance_does_not_reload() {
        let mut app = fixture();
        app.widget_month = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
        assert_eq!(app.scope().widget_to, "2026-02-28");
        let generation = app.view_generation;
        let _ = app.update(Message::WidgetSetting(
            crate::widget_settings::Change::EventSize(18),
        ));
        assert_eq!(app.preferences.widget.event_size, 18);
        assert_eq!(app.view_generation, generation);
        let _ = app.update(Message::WidgetSetting(
            crate::widget_settings::Change::Rolling(true),
        ));
        let _ = app.update(Message::WidgetSetting(
            crate::widget_settings::Change::Weeks(2),
        ));
        let _ = app.update(Message::WidgetSetting(
            crate::widget_settings::Change::Past(2),
        ));
        assert_eq!(app.preferences.widget.past_weeks, 1);
        let days = app.widget_days();
        assert_eq!(days.len(), 14);
        assert_eq!(app.scope().widget_from, days[0].to_string());
        assert_eq!(app.scope().widget_to, days.last().unwrap().to_string());
    }

    #[test]
    fn month_jump_focus_cancel_validation_and_draft_preservation() {
        use crate::month_jump::{JumpField, JumpMessage};
        let mut app = fixture();
        app.widget_month = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let _ = app.update(Message::Jump(JumpMessage::Open(
            crate::month_jump::JumpTarget::Widget,
            JumpField::Month,
        )));
        let jump = app.widget_ui.jump.as_ref().unwrap();
        assert_eq!((jump.year, jump.month), (2025, 9));
        assert_eq!(jump.level, JumpField::Month);
        let _ = app.update(Message::Jump(JumpMessage::PickMonth(13)));
        assert!(app.widget_ui.jump.as_ref().unwrap().error.len() > 0);
        assert_eq!(app.widget_month.month(), 9);
        let _ = app.update(Message::Jump(JumpMessage::Cancel));
        assert!(app.widget_ui.jump.is_none());
        let _ = app.update(Message::New(Kind::Event, None));
        let _ = app.update(Message::Field(Field::Title("미저장 원문".into())));
        let _ = app.update(Message::Jump(JumpMessage::Open(
            crate::month_jump::JumpTarget::Widget,
            JumpField::Year,
        )));
        let _ = app.update(Message::Jump(JumpMessage::PickYear(2028)));
        let _ = app.update(Message::Jump(JumpMessage::PickMonth(2)));
        assert_eq!(
            app.widget_month,
            NaiveDate::from_ymd_opt(2028, 2, 1).unwrap()
        );
        assert!(!app.widget_ui.follow_today);
        assert_eq!(app.editor.as_ref().unwrap().form.title, "미저장 원문");
    }
    #[test]
    fn diary_jump_changes_only_diary_and_replaces_other_origin_popup() {
        use crate::month_jump::{JumpField, JumpMessage, JumpTarget};
        let mut app = fixture();
        app.month = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let widget_month = app.widget_month;
        let _ = app.update(Message::Jump(JumpMessage::Open(
            JumpTarget::Widget,
            JumpField::Year,
        )));
        let old_id = app.widget_ui.jump.as_ref().unwrap().window;
        let _ = app.update(Message::Jump(JumpMessage::Open(
            JumpTarget::Diary,
            JumpField::Month,
        )));
        let jump = app.widget_ui.jump.as_ref().unwrap();
        assert_ne!(jump.window, old_id);
        assert_eq!((jump.year, jump.month), (2025, 9));
        assert_eq!(jump.target.source(&app), app.manager);
        let _ = app.update(Message::New(Kind::Event, None));
        let _ = app.update(Message::Field(Field::Title("기존 초안".into())));
        let _ = app.update(Message::Jump(JumpMessage::Date("2028-02-29".into())));
        let _ = app.update(Message::Jump(JumpMessage::ApplyDate));
        assert_eq!(app.selected.to_string(), "2028-02-29");
        assert_eq!(app.month.to_string(), "2028-02-01");
        assert_eq!(app.widget_month, widget_month);
        assert_eq!(app.editor.as_ref().unwrap().form.title, "기존 초안");
    }
    #[test]
    fn popup_placement_reveals_once_and_ignores_closed_or_stale_requests() {
        let mut app = fixture();
        let id = window::Id::unique();
        app.quick_window = Some(id);
        let _ = app.place_popup(app.manager, id);
        let token = app.placing_popups[&id];
        let _ = app.place_popup(app.manager, id);
        let newer = app.placing_popups[&id];
        assert_eq!(
            app.update(Message::PopupPositioned(id, token, Ok(())))
                .units(),
            0
        );
        assert!(
            app.update(Message::PopupPositioned(
                id,
                newer,
                Err("source closed".into())
            ))
            .units()
                > 0
        );
        assert_eq!(
            app.update(Message::PopupPositioned(id, newer, Ok(())))
                .units(),
            0
        );
        let _ = app.place_popup(app.manager, id);
        let token = app.placing_popups[&id];
        app.quick_window = None;
        assert_eq!(
            app.update(Message::PopupPositioned(id, token, Ok(())))
                .units(),
            0
        );
    }
    #[test]
    fn quick_editor_does_not_show_before_native_placement() {
        let mut app = fixture();
        app.widget = Some(window::Id::unique());
        let date = app.selected;
        let _ = app.open_quick(date, app.manager);
        assert_eq!(
            app.focus_quick().units(),
            0,
            "focus must not restore a hidden popup at the default screen position"
        );
    }
    #[test]
    fn reopening_jump_repositions_without_resetting_its_draft() {
        use crate::month_jump::{JumpField, JumpMessage, JumpTarget};
        let mut app = fixture();
        let _ = app.jump_message(JumpMessage::Open(JumpTarget::Diary, JumpField::Year));
        let id = app.widget_ui.jump.as_ref().unwrap().window;
        app.placing_popups.clear(); // Placement of the first opening completed.
        let _ = app.jump_message(JumpMessage::PickYear(2035));
        let _ = app.jump_message(JumpMessage::Open(JumpTarget::Diary, JumpField::Month));
        assert!(
            app.placing_popups.contains_key(&id),
            "reused popup must recapture the moved diary position"
        );
        let jump = app.widget_ui.jump.as_ref().unwrap();
        assert_eq!(jump.year, 2035);
        assert_eq!(jump.level, JumpField::Month);
    }
    #[test]
    fn picker_browsing_cancels_and_neighbor_years_are_selectable() {
        use crate::month_jump::{JumpField, JumpMessage, JumpTarget};
        let mut app = fixture();
        app.widget_month = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let original = app.widget_month;
        let _ = app.jump_message(JumpMessage::Open(JumpTarget::Widget, JumpField::Year));
        let _ = app.jump_message(JumpMessage::Page(1));
        assert_eq!(app.widget_ui.jump.as_ref().unwrap().decade, 2030);
        assert_eq!(app.widget_ui.jump.as_ref().unwrap().year, 2026);
        assert_eq!(app.widget_month, original);
        let _ = app.jump_message(JumpMessage::Page(-1));
        let _ = app.jump_message(JumpMessage::PickYear(2019));
        let jump = app.widget_ui.jump.as_ref().unwrap();
        assert_eq!(jump.level, JumpField::Month);
        assert_eq!((jump.year, jump.month), (2019, 10));
        let _ = app.jump_message(JumpMessage::Page(1));
        assert_eq!(app.widget_ui.jump.as_ref().unwrap().year, 2020);
        let _ = app.jump_message(JumpMessage::ShowYears);
        assert_eq!(app.widget_ui.jump.as_ref().unwrap().decade, 2020);
        let id = app.widget_ui.jump.as_ref().unwrap().window;
        let _ = app.update(Message::EscapeWindow(id));
        assert!(app.widget_ui.jump.is_none());
        assert_eq!(app.widget_month, original);
    }
    #[test]
    fn picker_pages_respect_supported_years_and_direct_date_validation() {
        use crate::month_jump::{JumpField, JumpMessage, JumpTarget};
        for (year, direction, invalid) in [(1900, -1, 1899), (9998, 1, 9999)] {
            let mut app = fixture();
            app.widget_month = NaiveDate::from_ymd_opt(year, 9, 1).unwrap();
            let _ = app.jump_message(JumpMessage::Open(JumpTarget::Widget, JumpField::Year));
            let decade = app.widget_ui.jump.as_ref().unwrap().decade;
            let _ = app.jump_message(JumpMessage::Page(direction));
            let _ = app.jump_message(JumpMessage::PickYear(invalid));
            assert_eq!(app.widget_ui.jump.as_ref().unwrap().decade, decade);
            assert_eq!(app.widget_ui.jump.as_ref().unwrap().level, JumpField::Year);
            let _ = app.jump_message(JumpMessage::PickYear(year));
            let _ = app.jump_message(JumpMessage::Page(direction));
            assert_eq!(app.widget_ui.jump.as_ref().unwrap().year, year);
            let _ = app.jump_message(JumpMessage::ToggleDate);
            assert!(app.widget_ui.jump.as_ref().unwrap().direct);
            let _ = app.jump_message(JumpMessage::Date("2027-02-29".into()));
            let _ = app.jump_message(JumpMessage::ApplyDate);
            assert!(!app.widget_ui.jump.as_ref().unwrap().error.is_empty());
            assert_eq!(app.widget_month.year(), year);
            let _ = app.jump_message(JumpMessage::Date("2028-02-29".into()));
            let _ = app.jump_message(JumpMessage::ApplyDate);
            assert!(app.widget_ui.jump.is_none());
            assert_eq!(app.widget_ui.selected.unwrap().to_string(), "2028-02-29");
        }
    }
    #[test]
    fn picker_bounds_sources_reached_by_diary_month_arrows() {
        use crate::month_jump::{JumpField, JumpMessage, JumpTarget};
        for (year, month, delta, outside) in [(1900, 1, -1, 1899), (9998, 12, 1, 9999)] {
            let mut app = fixture();
            app.month = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
            let _ = app.update(Message::Month(delta));
            assert_eq!(app.month.year(), outside);
            let _ = app.jump_message(JumpMessage::Open(JumpTarget::Diary, JumpField::Month));
            assert_eq!(
                app.widget_ui.jump.as_ref().unwrap().year,
                year,
                "picker must start in the supported range even if the source has crossed it"
            );
            let _ = app.jump_message(JumpMessage::PickMonth(6));
            assert_eq!(app.month.to_string(), format!("{year}-06-01"));
            assert!(app.widget_ui.jump.is_none());
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
    fn widget_double_click_does_not_open_the_manager() {
        let mut app = fixture();
        app.manager_visible = false;
        let _ = app.update(Message::WidgetDate(app.selected));
        assert!(
            !app.manager_visible,
            "quick editing must not open the manager"
        );
    }

    #[test]
    fn quick_editor_preserves_full_event_and_save_error_then_saves() {
        let mut app = fixture();
        let date = app.selected;
        let mut original = Form::new(Kind::Event, date.to_string(), None);
        original.title = "짧은 편집".into();
        original.body = "위젯 헤더에는 없는 원문\n두 번째 줄".into();
        original.save(app.store.as_ref().unwrap()).unwrap();
        let events = app
            .store
            .as_ref()
            .unwrap()
            .list_events(&date.to_string(), &date.to_string())
            .unwrap();
        let _ = app.update(Message::WidgetDate(date));
        let window = app.quick_window;
        let loaded =
            worker::hydrate_form(app.store.as_ref().unwrap(), Form::event(events[0].clone()))
                .unwrap();
        app.quick_loading = false;
        let _ = app.update(crate::calendar_ui::message(
            crate::calendar_ui::Action::Detail(app.calendar.generation, true, Ok(loaded)),
        ));
        let _ = app.update(crate::calendar_ui::message(
            crate::calendar_ui::Action::EditDetail,
        ));
        assert!(app.editor_in_quick);
        assert_eq!(app.editor.as_ref().unwrap().form.body, original.body);
        let _ = app.update(Message::Field(Field::Title("수정한 제목".into())));
        let generation = app.editor.as_ref().unwrap().generation;
        let _ = app.update(Message::Saved(generation, Err("저장 실패 fixture".into())));
        assert_eq!(app.quick_window, window);
        assert!(app.editor_in_quick);
        assert_eq!(app.editor.as_ref().unwrap().form.title, "수정한 제목");
        app.editor
            .as_ref()
            .unwrap()
            .form
            .save(app.store.as_ref().unwrap())
            .unwrap();
        let _ = app.update(Message::Saved(generation, Ok(())));
        assert!(app.editor.is_none());
        let saved = app
            .store
            .as_ref()
            .unwrap()
            .list_events(&date.to_string(), &date.to_string())
            .unwrap();
        assert_eq!(saved[0].id, events[0].id);
        assert_eq!(saved[0].notes, original.body);
        assert_eq!(saved[0].title, "수정한 제목");
    }

    #[test]
    fn quick_editor_close_protects_dirty_and_composing_text_and_rejects_stale_load() {
        let mut app = fixture();
        let date = app.selected;
        let _ = app.update(Message::WidgetDate(date));
        let id = app.quick_window.unwrap();
        let g = app.quick_generation;
        let _ = app.update(crate::calendar_ui::message(
            crate::calendar_ui::Action::QuickPage(g, true, Ok(Default::default())),
        ));
        let _ = app.update(Message::Field(Field::Title("한글 초안".into())));
        let _ = app.update(Message::Composition(id, true));
        let _ = app.update(Message::CloseWindow(id));
        assert!(app.editor.as_ref().unwrap().confirmation.is_none());
        assert_eq!(app.quick_window, Some(id));
        let _ = app.update(Message::Composition(id, false));
        let _ = app.update(Message::WidgetDate(date + Duration::days(1)));
        assert_eq!(app.quick_date, Some(date));
        assert_eq!(app.editor.as_ref().unwrap().form.title, "한글 초안");
        let _ = app.update(Message::CloseWindow(id));
        assert_eq!(
            app.editor.as_ref().unwrap().confirmation,
            Some(Confirmation::Discard(CloseIntent::Quick))
        );
        let _ = app.update(Message::Keep);
        assert!(app.editor.as_ref().unwrap().confirmation.is_none());
        let _ = app.update(Message::CloseWindow(id));
        let _ = app.update(Message::Discard);
        assert!(app.quick_window.is_none());
        let _ = app.update(crate::calendar_ui::message(
            crate::calendar_ui::Action::QuickPage(g, true, Ok(Default::default())),
        ));
        assert!(app.editor.is_none());
    }

    #[test]
    fn background_opacity_does_not_schedule_a_constant_window_alpha_change() {
        let mut app = fixture();
        app.native_busy = true;
        let _ = app.update(Message::Native(NativeAction::Opacity(40)));
        assert_eq!(app.preferences.widget_opacity, 40);
        assert!(app.native_pending.is_none());
        let _ = app.update(Message::Native(NativeAction::Opacity(0)));
        assert_eq!(app.preferences.widget_opacity, 40);
    }

    #[test]
    fn snapshot_editing_waits_for_completion_write_and_refresh() {
        let mut app = fixture();
        let mut form = Form::new(Kind::Event, app.selected.to_string(), None);
        form.title = "완료 직후 수정".into();
        form.save(app.store.as_ref().unwrap()).unwrap();
        let event = app
            .store
            .as_ref()
            .unwrap()
            .list_events(&form.date, &form.date)
            .unwrap()
            .remove(0);
        app.busy = true;
        let _ = app.update(Message::EditForm(Form::event(event.clone())));
        assert!(
            app.editor.is_none(),
            "a pending completion write must not open stale event data"
        );
        app.busy = false;
        app.loading = true;
        let _ = app.update(Message::EditForm(Form::event(event.clone())));
        assert!(
            app.editor.is_none(),
            "wait for refreshed completion state before editing"
        );
        app.loading = false;
        app.store
            .as_ref()
            .unwrap()
            .set_event_completed(&event.id, true)
            .unwrap();
        let event = app
            .store
            .as_ref()
            .unwrap()
            .list_events(&form.date, &form.date)
            .unwrap()
            .remove(0);
        let form = worker::hydrate_form(app.store.as_ref().unwrap(), Form::event(event)).unwrap();
        let _ = app.update(Message::EditForm(form));
        assert!(app.editor.as_ref().unwrap().form.completed);
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
    fn locked_widget_does_not_start_native_drag() {
        let mut app = fixture();
        app.widget = Some(window::Id::unique());
        assert!(app.preferences.widget_locked);
        assert_eq!(app.update(Message::DragWidget).units(), 0);
        assert_eq!(app.update(Message::ResizeWidget).units(), 0);
        app.preferences.widget_locked = false;
        assert!(app.update(Message::DragWidget).units() > 0);
        assert!(app.update(Message::ResizeWidget).units() > 0);
    }
    #[test]
    fn automatic_reports_preserve_authored_preferences_on_disk_and_restart() {
        let mut app = fixture();
        app.widget = Some(window::Id::unique());
        app.preferences.widget_x = Some(-1400);
        app.preferences.widget_y = Some(75);
        app.preferences.widget.width = 1100;
        app.preferences.widget.height = 750;
        let directory =
            std::env::temp_dir().join(format!("timeschedule-geometry-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        for locked in [false, true] {
            app.preferences.widget_locked = locked;
            let preferred = app.preferences.clone();
            for action in [
                NativeAction::Attach,
                NativeAction::Show,
                NativeAction::Recover,
                NativeAction::Lock(locked),
            ] {
                let _ = app.update(Message::NativeDone(
                    action,
                    Ok(NativeReport {
                        attached: true,
                        message: "temporary small display".into(),
                        locked,
                        opacity: None,
                        visible: None,
                        position: Some((0, 0)),
                        size: Some((500, 350)),
                    }),
                ));
                app.apply_geometry_event(windows::GeometryEvent {
                    preferred: None,
                    actual_size: (500, 350),
                    error: None,
                });
                assert_eq!(app.preferences, preferred);
                assert!(app.widget_ui.save_due.is_none());
                preferences::save(&directory, &app.preferences).unwrap();
                assert_eq!(preferences::load(&directory).unwrap(), preferred);
            }
        }
        let geometry = windows::WidgetGeometry {
            x: -1200,
            y: 80,
            width: 900,
            height: 600,
        };
        let event = windows::GeometryEvent {
            preferred: Some(geometry),
            actual_size: (500, 350),
            error: None,
        };
        app.apply_geometry_event(event.clone());
        assert!(app.widget_ui.save_due.is_some());
        app.widget_ui.save_due = None;
        app.apply_geometry_event(event);
        assert!(
            app.widget_ui.save_due.is_none(),
            "duplicate final geometry must not write twice"
        );
        preferences::save(&directory, &app.preferences).unwrap();
        let restarted = preferences::load(&directory).unwrap();
        assert_eq!(
            (
                restarted.widget_x,
                restarted.widget_y,
                restarted.widget.width,
                restarted.widget.height
            ),
            (Some(-1200), Some(80), 900, 600)
        );
        std::fs::remove_file(directory.join("preferences.json")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn idle_ticks_do_not_query_widget_or_show_native_busy() {
        let mut app = fixture();
        app.widget = Some(window::Id::unique());
        app.widget_attached = true;
        for _ in 0..120 {
            let _ = app.update(Message::Tick);
            assert!(
                !app.native_busy,
                "idle ticks must not invoke native position work"
            );
        }
    }
    #[test]
    fn automatic_resize_does_not_replace_saved_preference() {
        let mut app = fixture();
        let id = window::Id::unique();
        app.widget = Some(id);
        app.widget_attached = true;
        app.preferences.widget_locked = false;
        let expected = (app.preferences.widget.width, app.preferences.widget.height);
        let _ = app.update(Message::Resized(id, Size::new(500., 350.)));
        assert_eq!(
            (app.preferences.widget.width, app.preferences.widget.height),
            expected
        );
        assert_eq!(app.widget_ui.actual_size, Size::new(500., 350.));
    }
    #[test]
    fn final_resize_is_saved_before_immediate_lock_or_quit() {
        let mut app = fixture();
        let id = window::Id::unique();
        app.widget = Some(id);
        app.widget_attached = true;
        app.preferences.widget_locked = false;
        let _ = app.update(Message::Resized(id, Size::new(740., 480.)));
        app.apply_geometry_event(windows::GeometryEvent {
            preferred: Some(windows::WidgetGeometry {
                x: 30,
                y: 40,
                width: 740,
                height: 480,
            }),
            actual_size: (740, 480),
            error: None,
        });
        assert_eq!(
            (app.preferences.widget.width, app.preferences.widget.height),
            (740, 480)
        );
        assert!(app.widget_ui.save_due.is_some());
        // There has been no polling message between the resize and the lock.
        let _ = app.update(Message::NativeDone(
            NativeAction::Lock(true),
            Ok(NativeReport {
                attached: true,
                message: String::new(),
                locked: true,
                opacity: None,
                visible: None,
                position: None,
                size: None,
            }),
        ));
        assert_eq!(
            (app.preferences.widget.width, app.preferences.widget.height),
            (740, 480)
        );
        app.preferences.widget_locked = false;
        let _ = app.update(Message::Resized(id, Size::new(760., 490.)));
        app.apply_geometry_event(windows::GeometryEvent {
            preferred: Some(windows::WidgetGeometry {
                x: 30,
                y: 40,
                width: 760,
                height: 490,
            }),
            actual_size: (760, 490),
            error: None,
        });
        let _ = app.update(Message::Quit);
        assert!(
            app.quit_when_idle,
            "quit waits for the last geometry preference save"
        );
        assert!(app.widget_ui.save_due.is_none());
        assert_eq!(
            (app.preferences.widget.width, app.preferences.widget.height),
            (760, 490)
        );
    }
    #[test]
    fn appearance_overrides_are_per_theme_and_invalid_inputs_do_not_replace_settings() {
        use crate::widget_settings::{Change, ColorRole};
        let mut app = fixture();
        let _ = app.update(Message::WidgetSetting(Change::ColorRole(ColorRole::Text)));
        let _ = app.update(Message::WidgetSetting(Change::ColorText("#ABCDEF".into())));
        let _ = app.update(Message::WidgetSetting(Change::ApplyColor));
        assert_eq!(app.preferences.widget.dark.text.as_deref(), Some("#ABCDEF"));
        let _ = app.update(Message::ToggleTheme);
        assert!(app.preferences.widget.light.text.is_none());
        let _ = app.update(Message::WidgetSetting(Change::ColorText("#12zz00".into())));
        let _ = app.update(Message::WidgetSetting(Change::ApplyColor));
        assert!(app.preferences.widget.light.text.is_none());
        assert!(!app.widget_ui.error.is_empty());
        app.preferences.widget_locked = false;
        let _ = app.update(Message::WidgetSetting(Change::Width("9999".into())));
        assert_eq!(
            app.update(Message::WidgetSetting(Change::Resize)).units(),
            0
        );
        assert_eq!(app.preferences.widget.width, 960);
    }
    #[test]
    fn fitted_widget_keeps_dense_day_content_inside_cells() {
        use iced::advanced::{layout, widget::Tree};
        fn check(node: &layout::Node, x: f32, width: f32) {
            let bounds = node.bounds();
            let x = x + bounds.x;
            assert!(
                x + bounds.width <= width + 0.1,
                "horizontal overflow: {bounds:?} at {x} / {width}"
            );
            for child in node.children() {
                // Calendar-day descendants are narrower than the outer scroll
                // viewport. Their contents must fit vertically even when dense.
                if node.size().width < 160.0 {
                    let b = child.bounds();
                    assert!(
                        b.y + b.height <= node.size().height + 0.1,
                        "day content {b:?} exceeds {:?}",
                        node.size()
                    );
                }
                check(child, x, width);
            }
        }
        let mut app = fixture();
        let widget = window::Id::unique();
        app.widget = Some(widget);
        for day in calendar_days(app.widget_month) {
            for i in 0..6 {
                let mut form = Form::new(Kind::Event, day.to_string(), None);
                form.title = format!("{i} 아주 긴 한글 일정 제목 WWWWWWW");
                form.save(app.store.as_ref().unwrap()).unwrap();
            }
        }
        app.snapshot = worker::snapshot(app.store.as_ref().unwrap(), &app.scope()).unwrap();
        let renderer = iced::Renderer::new(iced::Font::with_name("Malgun Gothic"), 14.into());
        for size in [
            Size::new(320.0, 300.0),
            Size::new(500.0, 620.0),
            Size::new(960.0, 640.0),
        ] {
            for mode in [AppTheme::Midnight, AppTheme::Porcelain] {
                app.preferences.theme = mode;
                let mut element = app.view(widget);
                let mut tree = Tree::new(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                assert_eq!(node.size(), size);
                check(&node, 0.0, size.width);
            }
        }
        // Outside-month 10/1 must fit the selected large font, not a fixed threshold.
        app.widget_month = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        app.preferences.widget.font = "Arial".into();
        app.preferences.widget.date_size = 32;
        app.preferences.widget.cell_gap = 0;
        app.preferences.widget.cell_padding = 0;
        let size = Size::new(400., 500.);
        let mut element = app.view(widget);
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        check(&node, 0., size.width);
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
                    // Settings shows one section at a time; every one must fit.
                    let sections: &[SettingsSection] = if page == Page::Settings {
                        &SettingsSection::ALL
                    } else {
                        &[SettingsSection::General]
                    };
                    for &section in sections {
                        app.settings_section = section;
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
                            &format!("{page:?}/{section:?}/{mode:?}/{size:?}"),
                        );
                    }
                }
            }
        }
    }
    fn scanning_app() -> (crate::record_inbox::tests::TestDir, App) {
        let dir = crate::record_inbox::tests::TestDir::new();
        let mut app = fixture();
        app._instance = Some(InstanceLock::acquire(&dir.0).unwrap());
        app.data_dir = dir.0.clone();
        (dir, app)
    }
    #[test]
    fn the_automatic_mode_makes_the_finished_days_after_the_start_and_hourly_scans_only() {
        use timeschedule_desktop::preferences::DailyRecordMode;
        let (_dir, mut app) = scanning_app();
        // The button mode never makes records after a scan.
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert!(!app.daily_making);
        let _ = app.update(Message::SetDailyMode(DailyRecordMode::Auto));
        assert_eq!(app.preferences.daily_record, DailyRecordMode::Auto);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert!(app.daily_making, "the start's scan makes the finished days");
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        assert!(!app.daily_making);
        // A scan that is not the hourly one (opening the project screen) does not.
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert!(!app.daily_making);
        app.ticks = CHAT_SCAN_TICKS - 1;
        app.loading = false;
        let _ = app.update(Message::Tick);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert!(app.daily_making, "the hourly scan does");
    }
    #[test]
    fn the_button_makes_its_day_once_at_a_time_and_says_what_happened() {
        let (_dir, mut app) = scanning_app();
        let day = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        let _ = app.update(Message::MakeDaily(day));
        // One job keeps the newest originals and makes the day.
        assert!(app.daily_making && app.daily_kind == DailyKind::Button);
        assert!(
            app.daily_auto_due,
            "the button does not take the automatic turn"
        );
        let made = worker::DailyMade {
            unchanged: 1,
            ..Default::default()
        };
        let _ = app.update(Message::DailyMade(Ok(made.clone())));
        assert!(!app.daily_making);
        assert_eq!(
            app.notice,
            "대화가 바뀌지 않아 하루 정리를 새로 만들지 않았습니다."
        );
        // The automatic mode is quiet about days it did not need to make.
        assert_eq!(daily_notice(&made, false), None);
        let saved = worker::DailyMade {
            saved: vec![day, day],
            ..Default::default()
        };
        assert_eq!(
            daily_notice(&saved, false).unwrap(),
            "하루 정리 2건(11월 20일)을 만들었습니다. 곧 다이어리에 보입니다."
        );
        let unkept = worker::DailyMade {
            unkept: vec!["작업".into()],
            ..Default::default()
        };
        assert_eq!(
            daily_notice(&unkept, true).unwrap(),
            "채팅 원문을 다 보관하지 못해 하루 정리를 만들지 않았습니다(작업). 잠시 뒤 다시 눌러 주세요."
        );
    }
    #[test]
    fn a_daily_run_that_ends_under_an_open_confirmation_still_ends() {
        let (_dir, mut app) = scanning_app();
        let day = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        let _ = app.update(Message::MakeDaily(day));
        assert!(app.daily_making);
        let _ = app.open_form(Form::new(Kind::Note, String::new(), None));
        app.editor.as_mut().unwrap().form.title = "미저장".into();
        let _ = app.update(Message::CloseWindow(app.manager));
        assert!(app.editor.as_ref().unwrap().confirmation.is_some());
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        assert!(
            !app.daily_making,
            "the run's end is not dropped by the dialog"
        );
    }
    #[test]
    fn the_automatic_mode_remembers_failed_inputs_of_the_last_week_only() {
        use timeschedule_desktop::preferences::DailyRecordMode;
        let (_dir, mut app) = scanning_app();
        let _ = app.update(Message::SetDailyMode(DailyRecordMode::Auto));
        assert!(app.daily_auto.load(std::sync::atomic::Ordering::Acquire));
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert!(app.daily_making && !app.daily_auto_due);
        let recent = kst_today() - chrono::Days::new(1);
        let input = ("p".to_string(), recent, "fp".to_string());
        app.daily_failed.insert((
            "p".into(),
            kst_today() - chrono::Days::new(30),
            "old".into(),
        ));
        let failed = worker::DailyMade {
            failed: vec!["작업: 답 형식 오류".into()],
            failed_inputs: vec![input.clone()],
            ..Default::default()
        };
        let _ = app.update(Message::DailyMade(Ok(failed)));
        assert_eq!(app.daily_failed.len(), 1);
        assert!(app.daily_failed.contains(&input));
        assert_eq!(app.error, "하루 정리 실패 · 작업: 답 형식 오류");
        // Choosing the button stops a job between its days.
        let _ = app.update(Message::SetDailyMode(DailyRecordMode::Button));
        assert!(!app.daily_auto.load(std::sync::atomic::Ordering::Acquire));
    }
    #[test]
    fn an_automatic_job_that_met_a_held_project_waits_for_the_next_scan() {
        use timeschedule_desktop::preferences::DailyRecordMode;
        let (_dir, mut app) = scanning_app();
        let _ = app.update(Message::SetDailyMode(DailyRecordMode::Auto));
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert!(app.daily_making);
        let held = worker::DailyMade {
            unkept: vec!["작업".into()],
            ..Default::default()
        };
        let _ = app.update(Message::DailyMade(Ok(held.clone())));
        // Not again at once: the project may still be held.
        assert!(!app.daily_making && app.daily_retry_after_scan && !app.daily_auto_due);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert!(app.daily_making && !app.daily_retry_after_scan);
        // Held again: still only after the next scan, never in a loop of jobs.
        let _ = app.update(Message::DailyMade(Ok(held)));
        assert!(!app.daily_making && app.daily_retry_after_scan);
    }
    fn empty_scan() -> worker::ChatScan {
        worker::ChatScan {
            excerpts: Ok(Default::default()),
            originals: Ok(Default::default()),
            daily: Ok(Default::default()),
        }
    }
    /// A project with a folder, open on the project screen, whose capture is turned on.
    fn capturing(dir: &crate::record_inbox::tests::TestDir, app: &mut App) -> String {
        let repo = dir.0.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let store = app.store.clone().unwrap();
        let mut project = Form::new(Kind::Project, String::new(), None);
        project.title = "작업".into();
        project.repo_path = repo.to_string_lossy().into_owned();
        project.save(&store).unwrap();
        let listed = store.list_projects().unwrap();
        let id = listed[0].id.clone();
        // What the project screen has loaded.
        app.snapshot.projects = listed;
        app.project = Some(id.clone());
        let _ = app.update(Message::SetChatCapture(id.clone(), true));
        id
    }
    fn due(days: &[&str]) -> worker::DailyMade {
        worker::DailyMade {
            due: days
                .iter()
                .map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap())
                .collect(),
            root: Some("root".into()),
            ..Default::default()
        }
    }
    /// A second project with a folder, open on the project screen, whose capture is turned on.
    fn capturing_another(dir: &crate::record_inbox::tests::TestDir, app: &mut App) -> String {
        let repo = dir.0.join("repo-two");
        std::fs::create_dir_all(&repo).unwrap();
        let store = app.store.clone().unwrap();
        let mut project = Form::new(Kind::Project, String::new(), None);
        project.title = "둘째".into();
        project.repo_path = repo.to_string_lossy().into_owned();
        project.save(&store).unwrap();
        let listed = store.list_projects().unwrap();
        let id = listed
            .iter()
            .find(|p| p.title == "둘째")
            .unwrap()
            .id
            .clone();
        app.snapshot.projects = listed;
        app.project = Some(id.clone());
        let _ = app.update(Message::SetChatCapture(id.clone(), true));
        id
    }
    fn nothing_runs_for_the_past_days(app: &App) -> bool {
        app.past_days.values().all(|state| !state.running())
    }
    #[test]
    fn turning_capture_on_checks_the_past_days_once_the_scan_has_ended() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        assert_eq!(app.past_days.get(&id), Some(&Past::Due));
        // The scan that keeps the newest sessions runs first.
        let _ = app.update(Message::ScanChats);
        assert!(app.chat_scanning && !app.daily_making);
        assert_eq!(app.past_days.get(&id), Some(&Past::Due));
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert!(app.daily_making && app.daily_kind == DailyKind::PastCheck(id.clone()));
        assert_eq!(app.past_days.get(&id), Some(&Past::Checking));
        // Days are due: the owner is asked, and the notice names the project and the count.
        let _ = app.update(Message::DailyMade(Ok(due(&["2026-11-10", "2026-11-12"]))));
        assert!(!app.daily_making);
        let days = [
            NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(),
            NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(),
        ]
        .to_vec();
        assert_eq!(
            app.past_days.get(&id),
            Some(&Past::Ask(days.clone(), "root".into()))
        );
        assert!(app.notice.contains("작업") && app.notice.contains("2일"));
        assert_eq!(
            crate::views::past_days_question(&days),
            "지난 대화가 2일 있습니다 (2026-11-10 ~ 2026-11-12). 모두 정리하면 모델을 2번 부릅니다."
        );
        // Not now: nothing is made and the question is gone.
        let _ = app.update(Message::DismissPastDays(id));
        assert!(app.past_days.is_empty());
    }
    #[test]
    fn a_check_with_nothing_due_says_so_and_asks_nothing() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert_eq!(app.past_days.get(&id), Some(&Past::Checking));
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        assert!(app.past_days.is_empty());
        assert_eq!(app.notice, "정리할 지난 대화가 없습니다.");
    }
    #[test]
    fn the_confirmed_past_days_are_made_and_can_be_stopped() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        let _ = app.update(Message::DailyMade(Ok(due(&["2026-11-10"]))));
        let _ = app.update(Message::MakePastDays(id.clone()));
        assert!(app.daily_making && app.daily_kind == DailyKind::Past(id.clone()));
        assert_eq!(app.past_days.get(&id), Some(&Past::Making));
        // Stopping is asked for with a flag that the job reads between its days.
        assert!(!app.past_stop.load(std::sync::atomic::Ordering::Acquire));
        // A press meant for another project's making does not stop this one.
        let _ = app.update(Message::StopPastDays("another".into()));
        assert!(!app.past_stop.load(std::sync::atomic::Ordering::Acquire));
        let _ = app.update(Message::StopPastDays(id.clone()));
        assert!(app.past_stop.load(std::sync::atomic::Ordering::Acquire));
        let stopped = worker::DailyMade {
            saved: vec![NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()],
            stopped: true,
            ..Default::default()
        };
        let _ = app.update(Message::DailyMade(Ok(stopped)));
        assert!(app.past_days.is_empty());
        assert!(
            app.notice.starts_with("지난 대화 정리를 멈췄습니다."),
            "{}",
            app.notice
        );
        assert!(app.notice.contains("11월 10일"), "{}", app.notice);
        // A job that ends whole says what the button says.
        let whole = worker::DailyMade {
            unchanged: 1,
            ..Default::default()
        };
        assert_eq!(
            past_notice(&whole),
            Some("대화가 바뀌지 않아 하루 정리를 새로 만들지 않았습니다.".into())
        );
    }
    #[test]
    fn a_stop_that_arrives_after_its_making_ended_does_not_stop_the_next_check() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert_eq!(app.daily_kind, DailyKind::PastCheck(id.clone()));
        // The press was for a making of the same project that is over.
        let _ = app.update(Message::StopPastDays(id.clone()));
        assert!(
            !app.past_stop.load(std::sync::atomic::Ordering::Acquire),
            "the check goes on"
        );
    }
    #[test]
    fn the_confirmed_days_wait_for_a_chat_scan_to_end_and_the_question_stays() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        let _ = app.update(Message::DailyMade(Ok(due(&["2026-11-10"]))));
        let asked = app.past_days.get(&id).cloned();
        assert!(matches!(asked, Some(Past::Ask(..))));
        // Opening the project screen again reads the chat files.
        let _ = app.update(Message::ScanChats);
        assert!(app.chat_scanning && !app.past_days_can_start());
        let _ = app.update(Message::MakePastDays(id.clone()));
        assert!(!app.daily_making, "no job meets the scan at the lock");
        assert_eq!(app.past_days.get(&id).cloned(), asked);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        let _ = app.update(Message::MakePastDays(id.clone()));
        assert!(app.daily_making && app.daily_kind == DailyKind::Past(id));
    }
    #[test]
    fn a_saved_project_keeps_its_question_and_a_deleted_one_is_forgotten() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        let close = |app: &mut App, delete: bool| {
            let mut form = Form::new(Kind::Project, String::new(), None);
            form.id = Some(id.clone());
            app.editor_generation += 1;
            let generation = app.editor_generation;
            let mut editor = Editor::new(form, generation);
            // The confirmation may be closed by then; the running save says it.
            editor.deleting = delete;
            app.editor = Some(editor);
            let _ = app.update(Message::Saved(generation, Ok(())));
        };
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        let _ = app.update(Message::DailyMade(Ok(due(&["2026-11-10"]))));
        let asked = app.past_days.get(&id).cloned();
        // The question goes with the folder it was counted in.
        assert!(matches!(&asked, Some(Past::Ask(_, root)) if root == "root"));
        // Saving the project, whatever was edited, keeps the question: the job makes the
        // days only while the project's folder is that one.
        close(&mut app, false);
        assert_eq!(app.past_days.get(&id).cloned(), asked);
        // The making goes on as well.
        let _ = app.update(Message::MakePastDays(id.clone()));
        assert_eq!(app.past_days.get(&id), Some(&Past::Making));
        close(&mut app, false);
        assert!(!app.past_stop.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(app.past_days.get(&id), Some(&Past::Making));
        // A deleted project is forgotten and the job running for it ends.
        close(&mut app, true);
        assert!(app.past_stop.load(std::sync::atomic::Ordering::Acquire));
        assert!(app.past_days.is_empty());
    }
    #[test]
    fn a_check_that_does_not_say_its_folder_asks_nothing() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        let found = worker::DailyMade {
            root: None,
            ..due(&["2026-11-10"])
        };
        let _ = app.update(Message::DailyMade(Ok(found)));
        assert!(!app.past_days.contains_key(&id), "{:?}", app.past_days);
    }
    #[test]
    fn a_making_that_could_not_keep_the_originals_counts_the_days_again() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        let _ = app.update(Message::DailyMade(Ok(due(&["2026-11-10"]))));
        let _ = app.update(Message::MakePastDays(id.clone()));
        assert_eq!(app.past_days.get(&id), Some(&Past::Making));
        // A chat scan held the project while the job kept the originals: no day was made.
        let held = worker::DailyMade {
            unkept: vec!["작업".into()],
            ..Default::default()
        };
        let _ = app.update(Message::DailyMade(Ok(held)));
        // The answer is not used up: the days are counted again and asked about.
        assert!(app.daily_making && app.daily_kind == DailyKind::PastCheck(id.clone()));
        assert_eq!(app.past_days.get(&id), Some(&Past::Checking));
    }
    #[test]
    fn a_retry_whose_scan_ended_during_a_check_runs_when_the_check_ends() {
        use timeschedule_desktop::preferences::DailyRecordMode;
        let (dir, mut app) = scanning_app();
        let _ = app.update(Message::SetDailyMode(DailyRecordMode::Auto));
        let id = capturing(&dir, &mut app);
        // The automatic job met a project another run was keeping: it waits for a scan.
        app.daily_auto_due = false;
        app.daily_retry_after_scan = true;
        app.daily_making = true;
        app.daily_kind = DailyKind::PastCheck(id.clone());
        app.past_days.insert(id.clone(), Past::Checking);
        // That scan ends while the check runs.
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert!(app.daily_auto_due && !app.daily_retry_after_scan);
        assert_eq!(app.daily_kind, DailyKind::PastCheck(id.clone()));
        // The check ends: the automatic turn is still owed and starts.
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        assert!(app.daily_making && app.daily_kind == DailyKind::Auto);
    }
    #[test]
    fn a_check_that_found_nothing_says_which_files_it_could_not_place() {
        let (dir, mut app) = scanning_app();
        let _id = capturing(&dir, &mut app);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        let nothing = worker::DailyMade {
            unplaced: 2,
            ..Default::default()
        };
        let _ = app.update(Message::DailyMade(Ok(nothing)));
        assert!(app.notice.starts_with("정리할 지난 대화가 없습니다."));
        assert!(app.notice.contains("2개"), "{}", app.notice);
    }
    #[test]
    fn the_flag_is_cleared_when_the_next_job_starts_and_quitting_sets_it_for_the_past_days() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        app.past_stop
            .store(true, std::sync::atomic::Ordering::Release);
        app.past_days.insert(
            id.clone(),
            Past::Ask(
                vec![NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()],
                "root".into(),
            ),
        );
        let _ = app.update(Message::MakePastDays(id));
        assert!(app.daily_making);
        assert!(!app.past_stop.load(std::sync::atomic::Ordering::Acquire));
        // Quitting does not wait for the days left.
        let _ = app.exit_when_idle();
        assert!(app.quit_when_idle);
        assert!(app.past_stop.load(std::sync::atomic::Ordering::Acquire));
    }
    #[test]
    fn the_automatic_turn_goes_before_a_check_that_is_owed() {
        use timeschedule_desktop::preferences::DailyRecordMode;
        let (dir, mut app) = scanning_app();
        let _ = app.update(Message::SetDailyMode(DailyRecordMode::Auto));
        let id = capturing(&dir, &mut app);
        assert!(app.daily_auto_due);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        // The turn goes first and the check stays owed.
        assert!(app.daily_making && app.daily_kind == DailyKind::Auto);
        assert!(!app.daily_auto_due);
        assert_eq!(app.past_days.get(&id), Some(&Past::Due));
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        assert!(app.daily_making && app.daily_kind == DailyKind::PastCheck(id.clone()));
        assert_eq!(app.past_days.get(&id), Some(&Past::Checking));
        // The retry after a scan goes first as well.
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        app.daily_retry_after_scan = true;
        app.past_days.insert(id.clone(), Past::Due);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert!(app.daily_making && app.daily_kind == DailyKind::Auto);
        assert!(!app.daily_retry_after_scan);
        assert_eq!(app.past_days.get(&id), Some(&Past::Due));
        // A turn that came while another job ran goes before the check when that ends.
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        assert_eq!(app.past_days.get(&id), Some(&Past::Checking));
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        app.past_days.insert(id.clone(), Past::Due);
        let _ = app.update(Message::MakeDaily(
            NaiveDate::from_ymd_opt(2026, 11, 20).unwrap(),
        ));
        app.daily_auto_due = true;
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        assert!(app.daily_making && app.daily_kind == DailyKind::Auto);
        assert_eq!(app.past_days.get(&id), Some(&Past::Due));
    }
    #[test]
    fn a_question_is_counted_again_after_the_automatic_or_the_button_job_saved_days() {
        for button in [true, false] {
            let (dir, mut app) = scanning_app();
            let id = capturing(&dir, &mut app);
            let other = capturing_another(&dir, &mut app);
            let day = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
            app.past_days
                .insert(id.clone(), Past::Ask(vec![day], "root".into()));
            app.past_days
                .insert(other.clone(), Past::Ask(vec![day], "root".into()));
            let _ = app.update(Message::MakeDaily(day));
            // Nothing saved: the counts stand.
            let _ = app.update(Message::DailyMade(Ok(Default::default())));
            assert_eq!(
                app.past_days.get(&id),
                Some(&Past::Ask(vec![day], "root".into()))
            );
            let _ = app.update(Message::MakeDaily(day));
            let saved = worker::DailyMade {
                saved: vec![day],
                ..Default::default()
            };
            if !button {
                app.daily_kind = DailyKind::Auto;
            }
            let _ = app.update(Message::DailyMade(Ok(saved)));
            // Every question goes back to a check; the first starts, the other waits.
            let states: Vec<_> = app.past_days.values().cloned().collect();
            assert!(states.contains(&Past::Checking) && states.contains(&Past::Due));
            assert!(app.daily_making && matches!(app.daily_kind, DailyKind::PastCheck(_)));
        }
    }
    #[test]
    fn several_projects_wait_or_are_asked_at_once() {
        let (dir, mut app) = scanning_app();
        let one = capturing(&dir, &mut app);
        let two = capturing_another(&dir, &mut app);
        assert_eq!(app.past_days.get(&one), Some(&Past::Due));
        assert_eq!(app.past_days.get(&two), Some(&Past::Due));
        // One check at a time, the first project in order.
        let first = app.past_days.keys().next().cloned().unwrap();
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        assert!(app.daily_making && app.daily_kind == DailyKind::PastCheck(first.clone()));
        let second = app
            .past_days
            .keys()
            .find(|k| **k != first)
            .cloned()
            .unwrap();
        assert_eq!(app.past_days.get(&second), Some(&Past::Due));
        // The first is asked and the second is checked next.
        let _ = app.update(Message::DailyMade(Ok(due(&["2026-11-10"]))));
        assert!(matches!(app.past_days.get(&first), Some(Past::Ask(..))));
        assert_eq!(app.past_days.get(&second), Some(&Past::Checking));
        let _ = app.update(Message::DailyMade(Ok(due(&["2026-11-11", "2026-11-12"]))));
        assert!(matches!(app.past_days.get(&first), Some(Past::Ask(d, _)) if d.len() == 1));
        assert!(matches!(app.past_days.get(&second), Some(Past::Ask(d, _)) if d.len() == 2));
        assert!(!app.daily_making);
    }
    #[test]
    fn the_past_days_button_checks_at_once_when_nothing_runs_and_waits_when_something_does() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        app.past_days.clear();
        let _ = app.update(Message::CheckPastDays(id.clone()));
        assert_eq!(app.past_days.get(&id), Some(&Past::Checking));
        assert!(app.daily_making && app.daily_kind == DailyKind::PastCheck(id.clone()));
        // A check in progress is not replaced by another press, and another project's
        // press waits.
        let _ = app.update(Message::CheckPastDays(id.clone()));
        assert_eq!(app.past_days.get(&id), Some(&Past::Checking));
        let _ = app.update(Message::CheckPastDays("other".into()));
        assert_eq!(app.past_days.get("other"), Some(&Past::Due));
        assert!(app.daily_kind == DailyKind::PastCheck(id.clone()));
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        assert_eq!(app.past_days.get("other"), Some(&Past::Checking));
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        assert!(app.past_days.is_empty());
        // With a daily job running (the button's), the check is owed until it ends.
        let _ = app.update(Message::MakeDaily(
            NaiveDate::from_ymd_opt(2026, 11, 20).unwrap(),
        ));
        let _ = app.update(Message::CheckPastDays(id.clone()));
        assert_eq!(app.past_days.get(&id), Some(&Past::Due));
        let _ = app.update(Message::DailyMade(Ok(Default::default())));
        assert_eq!(app.past_days.get(&id), Some(&Past::Checking));
    }
    #[test]
    fn turning_capture_off_ends_what_the_past_days_state_names() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        assert_eq!(app.past_days.get(&id), Some(&Past::Due));
        let _ = app.update(Message::SetChatCapture(id.clone(), false));
        assert!(app.past_days.is_empty());
        // A question is gone as well.
        let _ = app.update(Message::SetChatCapture(id.clone(), true));
        app.past_days
            .insert(id.clone(), Past::Ask(vec![], "root".into()));
        let _ = app.update(Message::SetChatCapture(id, false));
        assert!(app.past_days.is_empty());
    }
    #[test]
    fn turning_capture_off_stops_the_running_job_of_that_project_only() {
        let (dir, mut app) = scanning_app();
        let id = capturing(&dir, &mut app);
        let other = capturing_another(&dir, &mut app);
        let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
        let checked = match &app.daily_kind {
            DailyKind::PastCheck(p) => p.clone(),
            kind => panic!("{kind:?}"),
        };
        let (running, idle) = if checked == id {
            (id, other)
        } else {
            (other, id)
        };
        fn stopped(app: &App) -> bool {
            app.past_stop.load(std::sync::atomic::Ordering::Acquire)
        }
        // Another project's capture is not this job's.
        app.project = Some(idle.clone());
        let _ = app.update(Message::SetChatCapture(idle.clone(), false));
        assert!(!stopped(&app));
        // Its own: the job ends after the day in progress, and turning it on again before
        // it ends leaves a fresh check owed.
        app.project = Some(running.clone());
        let _ = app.update(Message::SetChatCapture(running.clone(), false));
        assert!(stopped(&app));
        assert!(app.past_days.is_empty());
        let _ = app.update(Message::SetChatCapture(running.clone(), true));
        assert_eq!(app.past_days.get(&running), Some(&Past::Due));
        let ended = worker::DailyMade {
            stopped: true,
            ..Default::default()
        };
        let _ = app.update(Message::DailyMade(Ok(ended)));
        assert!(app.daily_making && app.daily_kind == DailyKind::PastCheck(running.clone()));
        assert_eq!(app.past_days.get(&running), Some(&Past::Checking));
        assert!(!stopped(&app), "the new check starts with the flag cleared");
        // The same for the making.
        let _ = app.update(Message::DailyMade(Ok(due(&["2026-11-10"]))));
        let _ = app.update(Message::MakePastDays(running.clone()));
        assert_eq!(app.past_days.get(&running), Some(&Past::Making));
        let _ = app.update(Message::SetChatCapture(running.clone(), false));
        assert!(stopped(&app));
        assert!(!app.past_days.contains_key(&running));
    }
    #[test]
    fn no_job_that_ended_leaves_a_project_checked_or_made() {
        let results = || -> Vec<Result<worker::DailyMade, String>> {
            vec![
                Ok(Default::default()),
                Ok(due(&["2026-11-10"])),
                Ok(worker::DailyMade {
                    stopped: true,
                    ..Default::default()
                }),
                Ok(worker::DailyMade {
                    failed: vec!["작업: 실패".into()],
                    unkept: vec!["작업".into()],
                    ..Default::default()
                }),
                Err("끝".into()),
            ]
        };
        for check in [true, false] {
            for result in results() {
                let (dir, mut app) = scanning_app();
                let id = capturing(&dir, &mut app);
                if check {
                    let _ = app.update(Message::ChatsScanned(Ok(empty_scan())));
                    assert_eq!(app.past_days.get(&id), Some(&Past::Checking));
                } else {
                    app.past_days
                        .insert(id.clone(), Past::Ask(vec![NaiveDate::MIN], "root".into()));
                    let _ = app.update(Message::MakePastDays(id.clone()));
                    assert_eq!(app.past_days.get(&id), Some(&Past::Making));
                }
                // A making that could not keep the originals made no day: its days are
                // counted again at once. Nothing else leaves a job running.
                let recount = !check && matches!(&result, Ok(made) if !made.unkept.is_empty());
                let _ = app.update(Message::DailyMade(result));
                assert_eq!(app.daily_making, recount);
                if recount {
                    assert_eq!(app.daily_kind, DailyKind::PastCheck(id.clone()));
                    assert_eq!(app.past_days.get(&id), Some(&Past::Checking));
                } else {
                    assert!(nothing_runs_for_the_past_days(&app), "{:?}", app.past_days);
                }
            }
        }
    }
    #[test]
    fn only_the_check_of_the_past_days_is_dry() {
        let id = || "p".to_string();
        assert_eq!(Maker::of(&DailyKind::PastCheck(id())), Maker::Dry);
        for kind in [DailyKind::Auto, DailyKind::Button, DailyKind::Past(id())] {
            assert_eq!(Maker::of(&kind), Maker::Real, "{kind:?}");
        }
    }
    #[test]
    fn the_automatic_and_the_button_jobs_keep_their_notices_after_the_past_days_arrived() {
        let (_dir, mut app) = scanning_app();
        let day = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        let _ = app.update(Message::MakeDaily(day));
        let held = worker::DailyMade {
            unkept: vec!["작업".into()],
            ..Default::default()
        };
        let _ = app.update(Message::DailyMade(Ok(held)));
        assert!(
            app.notice.contains("원문을 다 보관하지 못해"),
            "{}",
            app.notice
        );
        assert!(!app.daily_retry_after_scan, "only an automatic job retries");
    }
    #[test]
    fn the_folder_dialog_fills_the_open_project_editor_and_a_cancel_changes_nothing() {
        let (_dir, mut app) = scanning_app();
        let _ = app.open_form(Form::new(Kind::Project, String::new(), None));
        let g = app.editor.as_ref().unwrap().generation;
        assert!(!app.editor.as_ref().unwrap().dirty());
        let _ = app.update(Message::PickRepoFolder);
        assert!(app.editor.as_ref().unwrap().picking_repo);
        // Cancelled: the field and the pending mark are as they were.
        let _ = app.update(Message::RepoFolderPicked(g, None));
        let e = app.editor.as_ref().unwrap();
        assert_eq!(e.form.repo_path, "");
        assert!(!e.picking_repo && !e.dirty());
        // Chosen into a clean form: the path is in the field, and the footer's
        // "미저장 변경" (it shows when `dirty`) is on.
        let _ = app.update(Message::PickRepoFolder);
        let _ = app.update(Message::RepoFolderPicked(
            g,
            Some(PathBuf::from("C:\\work\\새 저장소")),
        ));
        let e = app.editor.as_ref().unwrap();
        assert_eq!(e.form.repo_path, "C:\\work\\새 저장소");
        assert!(e.dirty());
        // An answer nobody asked this editor for is dropped, and so is one that comes
        // after the editor was closed.
        let _ = app.update(Message::RepoFolderPicked(
            g,
            Some(PathBuf::from("C:\\other")),
        ));
        assert_eq!(
            app.editor.as_ref().unwrap().form.repo_path,
            "C:\\work\\새 저장소"
        );
        app.editor = None;
        let _ = app.update(Message::RepoFolderPicked(
            g,
            Some(PathBuf::from("C:\\other")),
        ));
        assert!(app.editor.is_none());
    }
    #[test]
    fn the_answer_of_a_closed_editor_is_not_taken_by_a_newer_one_with_its_dialog_open() {
        let (_dir, mut app) = scanning_app();
        let _ = app.open_form(Form::new(Kind::Project, String::new(), None));
        let old = app.editor.as_ref().unwrap().generation;
        let _ = app.update(Message::PickRepoFolder);
        app.editor = None;
        let _ = app.open_form(Form::new(Kind::Project, String::new(), None));
        let new = app.editor.as_ref().unwrap().generation;
        assert_ne!(old, new);
        let _ = app.update(Message::PickRepoFolder);
        let _ = app.update(Message::RepoFolderPicked(
            old,
            Some(PathBuf::from("C:\\old")),
        ));
        let e = app.editor.as_ref().unwrap();
        assert_eq!(e.form.repo_path, "");
        assert!(e.picking_repo, "its own dialog is still open");
        let _ = app.update(Message::RepoFolderPicked(
            new,
            Some(PathBuf::from("C:\\new")),
        ));
        let e = app.editor.as_ref().unwrap();
        assert_eq!(e.form.repo_path, "C:\\new");
        assert!(!e.picking_repo && e.dirty());
    }
    #[test]
    fn only_the_project_editor_opens_the_folder_dialog() {
        let (_dir, mut app) = scanning_app();
        let _ = app.open_form(Form::new(Kind::Note, String::new(), None));
        let _ = app.update(Message::PickRepoFolder);
        assert!(!app.editor.as_ref().unwrap().picking_repo);
    }
    #[test]
    fn chat_files_are_read_hourly_while_the_record_inbox_is_polled_every_minute() {
        let (_dir, mut app) = scanning_app();
        app.ticks = RECORDS_POLL_TICKS - 1;
        let _ = app.update(Message::Tick);
        // The fixture's fixed date makes this first tick reload, which holds the poll.
        assert!(app.records_due);
        assert!(!app.chat_scanning, "tick 120 must not read chat files");
        app.loading = false;
        let _ = app.update(Message::Tick);
        assert!(
            app.records_processing,
            "the poll runs on the next free tick"
        );
        assert!(!app.records_due);
        let _ = app.update(Message::RecordsDrained(Ok(Default::default())));
        app.loading = false;
        app.ticks = RECORDS_POLL_TICKS * 2 - 1;
        let _ = app.update(Message::Tick);
        assert!(app.records_processing, "and again a minute later");
        assert!(!app.chat_scanning);
        app.ticks = CHAT_SCAN_TICKS - 1;
        let _ = app.update(Message::Tick);
        assert!(app.chat_scanning, "tick 7200 must read chat files");
        assert!(!app.chat_scan_due);
    }
    #[test]
    fn an_hourly_chat_scan_that_cannot_start_stays_owed() {
        let (dir, mut app) = scanning_app();
        let instance = app._instance.take();
        app.ticks = CHAT_SCAN_TICKS - 1;
        let _ = app.update(Message::Tick);
        assert!(!app.chat_scanning);
        assert!(app.chat_scan_due);
        app._instance = instance;
        let _ = app.update(Message::Tick);
        assert!(app.chat_scanning, "it starts on the next tick that can");
        assert!(!app.chat_scan_due);
        // The hour comes round while that scan still runs: it has already listed the
        // files, so the hourly scan is still owed.
        app.ticks = CHAT_SCAN_TICKS * 2 - 1;
        let _ = app.update(Message::Tick);
        assert!(app.chat_scan_due);
        let _ = app.update(Message::ChatsScanned(Err("끝".into())));
        let _ = app.update(Message::Tick);
        assert!(app.chat_scanning && !app.chat_scan_due);
        drop(dir);
    }
    #[test]
    fn opening_the_projects_screen_reads_chat_files() {
        for message in [
            Message::Navigate(Page::Projects),
            Message::Project(Some("p1".into())),
        ] {
            let (_dir, mut app) = scanning_app();
            let _ = app.update(message);
            assert_eq!(app.page, Page::Projects);
            assert!(app.chat_scanning);
        }
        let (_dir, mut app) = scanning_app();
        let _ = app.update(Message::Navigate(Page::Records));
        assert!(!app.chat_scanning, "other screens do not read chat files");
        // Asked for while a scan runs: one more scan follows it.
        let (_dir, mut app) = scanning_app();
        let _ = app.update(Message::ScanChats);
        assert!(app.chat_scanning && !app.chat_scan_due);
        let _ = app.update(Message::Navigate(Page::Projects));
        assert!(app.chat_scan_due);
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
    fn quit_waits_for_mcp_config_work_to_complete() {
        let mut app = fixture();
        app.mcp_setup.busy = true;
        let _ = app.update(Message::Quit);
        assert!(app.quit_when_idle);
        let _ = app.update(Message::Tick);
        assert!(app.quit_when_idle);
        let _ = app.update(Message::McpSetup(crate::mcp_setup_ui::Action::Finished(
            Ok(crate::mcp_setup_ui::Outcome::Status("done".into())),
        )));
        assert!(!app.mcp_setup.busy);
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
    fn switching_editor_pickers_keeps_the_new_field_and_saves_exact_minutes() {
        use crate::{date_choices::Part, time_input::Field as Time};
        let mut app = fixture();
        let mut form = Form::new(Kind::Event, "2026-11-04".into(), None);
        form.title = "시간 입력 검사".into();
        form.all_day = false;
        let _ = app.open_form(form);
        let _ = app.update(Message::DatePicker(Part::Day));
        let _ = app.update(Message::OpenTimePicker(Time::Start));
        let _ = app.update(Message::DismissDatePicker);
        let _ = app.update(Message::Field(Field::Start("09:07".into())));
        assert_eq!(
            app.editor.as_ref().unwrap().time_picker(),
            Some(Time::Start)
        );

        let _ = app.update(Message::OpenTimePicker(Time::End));
        let _ = app.update(Message::DismissTimePicker(Time::Start));
        let _ = app.update(Message::PickEditorTime(Time::End, "10:15".into()));
        let editor = app.editor.as_ref().unwrap();
        assert!(editor.picker.is_none());
        let store = app.store.as_ref().unwrap();
        editor.form.save(store).unwrap();
        let events = store.list_events("2026-11-04", "2026-11-04").unwrap();
        assert_eq!(events[0].start_time.as_deref(), Some("09:07"));
        assert_eq!(events[0].end_time.as_deref(), Some("10:15"));

        let _ = app.update(Message::OpenTimePicker(Time::Start));
        let _ = app.update(Message::Field(Field::AllDay(true)));
        let _ = app.update(Message::PickEditorTime(Time::Start, "23:45".into()));
        let editor = app.editor.as_ref().unwrap();
        assert!(editor.picker.is_none());
        assert_eq!(editor.form.start, "09:07");
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
