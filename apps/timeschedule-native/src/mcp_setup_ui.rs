//! Stable settings layout; all filesystem/network work starts only on button press.
use crate::{
    app::Message,
    mcp_clients::{AiApp, Registration},
    mcp_setup::{self, Client, Plan},
};
use iced::{
    widget::{button, column, pick_list, row, scrollable, text, text_input},
    Element, Length, Task,
};
use std::path::PathBuf;

#[derive(Default)]
pub struct State {
    pub client: Client,
    pub path: String,
    pub busy: bool,
    pub preview: Option<Plan>,
    pub status: String,
    /// Connection board: one registration per `AiApp::ALL` row.
    pub registrations: [Registration; 3],
    pub checking: bool,
    pub generation: u64,
    /// What the token panel shows.
    pub token: TokenPanel,
    /// Set by the app once the server has started; empty while it is off.
    pub token_control: crate::mcp::TokenControl,
}
/// The token panel's state. Every state renders at the same height.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum TokenPanel {
    #[default]
    Idle,
    /// The token file was unreadable when the app started and was replaced.
    Recovered,
    /// The first click: waiting for the user to confirm or cancel.
    Confirming,
    Working,
    /// Done. `running` says whether the server took the new token at once.
    Reissued {
        running: bool,
    },
    Failed(String),
}
#[derive(Clone, Debug)]
pub enum Action {
    Client(Client),
    Path(String),
    Detect,
    Preview,
    Register,
    Check,
    Finished(Result<Outcome, String>),
    /// Re-read every app's registration for the connection board.
    Refresh,
    Loaded(u64, [Registration; 3]),
    /// A board row's button: preview that app's registration.
    Prepare(Client),
    /// A board row's button: check the server and that app's registration.
    Inspect(Client),
    /// The token panel: ask before reissuing, then cancel or go ahead.
    ReissueAsk,
    ReissueCancel,
    ReissueConfirm,
    /// Whether the server was running when it began, and how it ended.
    Reissued(bool, Result<(), String>),
}
#[derive(Clone, Debug)]
pub enum Outcome {
    Detected(PathBuf),
    Preview(Plan),
    Status(String),
}

impl State {
    pub fn update(
        &mut self,
        action: Action,
        dir: PathBuf,
        endpoint: Option<String>,
    ) -> Task<Message> {
        // Board reads are read-only, so they never wait for a registration.
        match action {
            Action::Refresh => return self.refresh(dir, endpoint),
            Action::Loaded(generation, registrations) => {
                if generation == self.generation {
                    self.registrations = registrations;
                    self.checking = false;
                }
                return Task::none();
            }
            Action::ReissueAsk => {
                if !self.busy {
                    self.token = TokenPanel::Confirming;
                }
                return Task::none();
            }
            Action::ReissueCancel => {
                if self.token == TokenPanel::Confirming {
                    self.token = TokenPanel::Idle;
                }
                return Task::none();
            }
            Action::ReissueConfirm => {
                // Only as the answer to the question, and never beside a
                // registration or check that is using the current token.
                if self.busy || self.token != TokenPanel::Confirming {
                    return Task::none();
                }
                self.busy = true;
                self.token = TokenPanel::Working;
                let control = self.token_control.clone();
                let running = endpoint.is_some();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || control.reissue(&dir))
                            .await
                            .unwrap_or_else(|_| {
                                Err("재발급 작업을 완료하지 못했습니다. 연결 검사로 현재 상태를 확인하세요.".into())
                            })
                    },
                    move |result| Message::McpSetup(Action::Reissued(running, result)),
                );
            }
            Action::Reissued(running, result) => {
                self.busy = false;
                self.token = match result {
                    Ok(()) => TokenPanel::Reissued { running },
                    Err(error) => TokenPanel::Failed(error),
                };
                return Task::none();
            }
            _ => {}
        }
        if self.busy && !matches!(action, Action::Finished(_)) {
            return Task::none();
        }
        let action = match action {
            Action::Prepare(client) => {
                self.select(client);
                Action::Preview
            }
            Action::Inspect(client) => {
                self.select(client);
                Action::Check
            }
            other => other,
        };
        match action {
            Action::Client(client) => {
                self.client = client;
                self.path.clear();
                self.preview = None;
                self.status.clear();
                return Task::none();
            }
            Action::Path(path) => {
                if path.len() <= 4096 {
                    self.path = path;
                    self.preview = None;
                    self.status.clear();
                }
                return Task::none();
            }
            Action::Finished(result) => {
                self.busy = false;
                // A registration or check may have changed what the board shows.
                let reread = !matches!(result, Ok(Outcome::Detected(_) | Outcome::Preview(_)));
                let refresh = if reread {
                    self.refresh(dir, endpoint)
                } else {
                    Task::none()
                };
                match result {
                    Ok(Outcome::Detected(path)) => {
                        self.path = path.to_string_lossy().into();
                        self.preview = None;
                        self.status =
                            "설정 경로를 준비했습니다. 파일이 없으면 등록할 때 새로 만듭니다."
                                .into();
                    }
                    Ok(Outcome::Preview(plan)) => {
                        self.path = plan.path.to_string_lossy().into();
                        self.status = "변경 내용을 확인한 뒤 ‘등록 적용’을 누르세요.".into();
                        self.preview = Some(plan);
                    }
                    Ok(Outcome::Status(status)) => {
                        self.status = status;
                        self.preview = None;
                    }
                    Err(error) => {
                        self.status = error;
                        self.preview = None;
                    }
                }
                return refresh;
            }
            Action::Register if self.preview.is_none() => return Task::none(),
            _ => {}
        }
        self.busy = true;
        self.status = "확인 중…".into();
        let client = self.client;
        let path = self.path.clone();
        let preview = self.preview.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || -> Result<Outcome, String> {
                if matches!(action, Action::Detect) { return mcp_setup::default_path(client).map(Outcome::Detected); }
                if matches!(action, Action::Check) {
                    let endpoint = crate::mcp_connection::current_endpoint(&dir)?;
                    let exe = std::env::current_exe().map_err(|_| "실행 파일 위치 확인 실패")?;
                    let registration = check_registration(client, &path, mcp_setup::config_candidates(client), &exe, &dir, &endpoint)?;
                    let server = crate::mcp_connection::probe(&dir)?;
                    return Ok(Outcome::Status(format!("{server}\n{registration} · AI 앱의 재연결·권한 승인은 해당 앱에서 확인하세요.")));
                }
                let path = if path.trim().is_empty() { mcp_setup::default_path(client)? } else { PathBuf::from(path.trim()) };
                if matches!(action, Action::Register) {
                    let plan = preview.ok_or("미리보기를 먼저 확인하세요.")?;
                    crate::mcp_connection::probe(&dir)?;
                    let applied = mcp_setup::register(&plan)?;
                    let backup = applied.backup.map(|p| format!("\n백업: {}", p.display())).unwrap_or_default();
                    // Each is a full copy of the configuration that should be gone.
                    let untidy = applied.kept_too_many.first().map(|p| format!("\n지우지 못한 이전 백업(설정 사본) {}개가 남았습니다. 직접 지우세요: {}", applied.kept_too_many.len(), p.display())).unwrap_or_default();
                    return Ok(Outcome::Status(format!("등록 완료. {client}에서 연결을 다시 불러오거나 앱을 재시작하세요.\nAI 앱의 실제 연결 승인은 해당 앱에서 확인하세요.{backup}{untidy}")));
                }
                let endpoint = crate::mcp_connection::current_endpoint(&dir)?;
                let exe = std::env::current_exe().map_err(|_| "실행 파일 위치 확인 실패")?;
                mcp_setup::prepare(client, &path, &exe, &dir, &endpoint).map(Outcome::Preview)
            }).await.unwrap_or_else(|_| Err("연결 작업을 완료하지 못했습니다. 다시 시도하세요.".into()))
            },
            |result| Message::McpSetup(Action::Finished(result)),
        )
    }
    fn select(&mut self, client: Client) {
        self.client = client;
        self.path.clear();
        self.preview = None;
        self.status.clear();
    }
    /// Re-reads every row's registration off the UI thread. A newer refresh
    /// supersedes an older one still in flight.
    fn refresh(&mut self, dir: PathBuf, endpoint: Option<String>) -> Task<Message> {
        let Some(endpoint) = endpoint else {
            self.checking = false;
            return Task::none();
        };
        self.generation += 1;
        self.checking = true;
        let generation = self.generation;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let exe = std::env::current_exe().unwrap_or_default();
                    AiApp::ALL
                        .map(|app| crate::mcp_clients::registration(app, &exe, &dir, &endpoint))
                })
                .await
                .unwrap_or_else(|_| {
                    std::array::from_fn(|_| {
                        Registration::Error("설정을 확인하지 못했습니다.".into())
                    })
                })
            },
            move |registrations| Message::McpSetup(Action::Loaded(generation, registrations)),
        )
    }
}

const ROW_HEIGHT: f32 = 34.0;
type Tone = fn(crate::theme::Colors) -> iced::Color;
fn toned(tone: Tone) -> impl Fn(&iced::Theme) -> iced::widget::text::Style {
    move |theme| iced::widget::text::Style {
        color: Some(tone(crate::theme::colors(theme))),
    }
}
/// One line that never wraps: the board keeps a fixed height whatever it says.
fn line<'a>(value: String, size: u32, tone: Option<Tone>) -> Element<'a, Message> {
    let label = text(value)
        .size(size)
        .wrapping(iced::widget::text::Wrapping::None);
    let label = match tone {
        Some(tone) => label.style(toned(tone)),
        None => label.style(crate::theme::muted),
    };
    iced::widget::container(label)
        .width(Length::Fill)
        .clip(true)
        .into()
}

/// The server's own state, above the board.
pub fn server_line(status: &str, running: bool) -> Element<'_, Message> {
    let tone: Tone = if running { |c| c.green } else { |c| c.danger };
    row![
        text("●").size(14).style(toned(tone)),
        text(if status.is_empty() { "꺼짐" } else { status }).size(13)
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}

/// The `연결 검사` wording for one app's registration.
fn check_registration(
    client: Client,
    explicit: &str,
    candidates: Result<Vec<PathBuf>, String>,
    exe: &std::path::Path,
    dir: &std::path::Path,
    endpoint: &str,
) -> Result<&'static str, String> {
    // A typed path is checked on its own; otherwise every location the app may
    // read counts, as on the board (Claude Desktop can have two).
    let paths = if explicit.trim().is_empty() {
        candidates?
    } else {
        vec![PathBuf::from(explicit.trim())]
    };
    match crate::mcp_clients::registration_at(client, &paths, exe, dir, endpoint) {
        Registration::Current => Ok("등록 설정 일치"),
        Registration::Outdated => Ok("등록이 예전 형식이거나 바뀌었습니다 · 다시 등록하세요"),
        Registration::Missing | Registration::Unchecked => Ok("등록되어 있지 않습니다"),
        Registration::Error(message) => Err(message),
    }
}

/// The connection board: one row per AI app with its state, detail and action.
/// Every row has a fixed height and always renders its button, so a changing
/// state never moves the controls below it.
pub fn board<'a>(
    state: &'a State,
    running: bool,
    seen: &[crate::mcp_clients::Seen],
    now: chrono::DateTime<chrono::Local>,
) -> Element<'a, Message> {
    use crate::mcp_clients::{classify, row_state, when, Origin, RowState};
    let enabled = !state.busy;
    let mut rows = column![row![
        text("연결 상태").size(18),
        iced::widget::space().width(Length::Fill),
        button(text("새로 고침").size(12))
            .padding([5, 10])
            .style(crate::theme::secondary)
            .on_press_maybe(
                (running && !state.checking).then_some(Message::McpSetup(Action::Refresh))
            )
    ]
    .align_y(iced::Alignment::Center)]
    .spacing(4);
    for (app, registration) in AiApp::ALL.into_iter().zip(&state.registrations) {
        let client = app.client();
        let last = seen
            .iter()
            .find(|s| classify(&s.name, s.title.as_deref()) == Origin::App(app));
        let (label, tone, detail, action): (&str, Tone, String, (&str, Option<Action>)) =
            match row_state(running, registration, last) {
                RowState::ServerOff => (
                    "서버 꺼짐",
                    |c| c.danger,
                    "TimeSchedule의 MCP 서버가 꺼져 있어 확인할 수 없습니다".into(),
                    ("검사", None),
                ),
                RowState::Checking => (
                    "확인 중",
                    |c| c.muted,
                    "설정 파일을 확인하고 있습니다".into(),
                    ("검사", None),
                ),
                RowState::Missing => (
                    "미등록",
                    |c| c.muted,
                    "이 PC의 설정에 등록되어 있지 않습니다".into(),
                    ("등록 준비", Some(Action::Prepare(client))),
                ),
                RowState::Outdated => (
                    "재등록 필요",
                    |c| c.danger,
                    "예전 형식이거나 앱 위치·포트가 바뀌었습니다".into(),
                    ("다시 등록", Some(Action::Prepare(client))),
                ),
                RowState::Registered => (
                    "등록됨",
                    |c| c.amber,
                    "아직 접속 기록이 없습니다 · AI 앱을 다시 시작하세요".into(),
                    ("검사", Some(Action::Inspect(client))),
                ),
                RowState::Connected(at) => (
                    "연결됨",
                    |c| c.green,
                    match last {
                        Some(s) => {
                            format!("마지막 접속 {} · {} {}", when(now, at), s.name, s.version)
                        }
                        None => format!("마지막 접속 {}", when(now, at)),
                    },
                    ("검사", Some(Action::Inspect(client))),
                ),
                RowState::Error(message) => (
                    "확인 실패",
                    |c| c.danger,
                    message
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .chars()
                        .take(80)
                        .collect(),
                    ("다시 확인", Some(Action::Inspect(client))),
                ),
            };
        let (action_label, message) = action;
        rows = rows.push(
            row![
                text("●").size(14).style(toned(tone)),
                iced::widget::container(line(app.label().into(), 14, Some(|c| c.text))).width(190),
                iced::widget::container(line(label.into(), 13, Some(tone))).width(100),
                line(detail, 12, None),
                button(text(action_label).size(12).width(Length::Fill).center())
                    .padding([5, 10])
                    .width(96)
                    .style(crate::theme::secondary)
                    .on_press_maybe(message.filter(|_| enabled).map(Message::McpSetup))
            ]
            .spacing(10)
            .height(ROW_HEIGHT)
            .align_y(iced::Alignment::Center),
        );
    }
    let others: Vec<String> = seen
        .iter()
        .filter(|s| classify(&s.name, s.title.as_deref()) == Origin::Other)
        .take(3)
        .map(|s| format!("{} ({})", s.name, when(now, s.last_seen)))
        .collect();
    let footer = if others.is_empty() {
        "AI 앱이 접속하면 앱 이름과 마지막 접속 시간이 표시됩니다. 접속 기록에는 일정 내용을 남기지 않습니다.".to_string()
    } else {
        format!("그 밖의 접속: {}", others.join(", "))
    };
    rows.push(iced::widget::container(line(footer, 12, None)).height(18))
        .into()
}

const TOKEN_DETAIL_HEIGHT: f32 = 34.0;
/// The token panel: what reissuing does, a two-step button, and the outcome.
/// The header and the two-line detail have fixed heights, so no state moves
/// the controls below.
pub fn token_panel(state: &State) -> Element<'_, Message> {
    let (detail, tone): (String, Option<Tone>) = match &state.token {
        TokenPanel::Idle => (
            "토큰이 유출됐다고 의심되면 새 토큰으로 바꿉니다. 도우미로 등록한 AI 앱은 다시 연결할 때 새 토큰을 받습니다.".into(),
            None,
        ),
        TokenPanel::Recovered => (
            "시작할 때 인증 파일을 읽을 수 없어 새 토큰으로 바꿨습니다. 도우미로 등록한 AI 앱은 그대로 쓰고, 환경변수 수동 연결은 다시 설정하세요.".into(),
            Some(|c| c.amber),
        ),
        TokenPanel::Confirming => (
            "재발급하면 이전 토큰은 즉시 거절됩니다. 토큰을 환경변수에 넣은 수동 연결(mcp-session.ps1)은 끊기므로 다시 설정해야 합니다.".into(),
            Some(|c| c.danger),
        ),
        TokenPanel::Working => ("새 토큰을 저장하고 있습니다…".into(), None),
        TokenPanel::Reissued { running: true } => (
            "재발급했습니다. 이전 토큰은 거절됩니다. AI 앱은 다시 연결하면 새 토큰을 받고, 환경변수 수동 연결은 다시 설정해야 합니다.".into(),
            Some(|c| c.green),
        ),
        TokenPanel::Reissued { running: false } => (
            "새 토큰을 저장했습니다. MCP 서버가 꺼져 있으므로 앱을 다시 시작하면 새 토큰으로 시작합니다.".into(),
            Some(|c| c.amber),
        ),
        TokenPanel::Failed(error) => (
            format!("재발급하지 못했습니다. {error}"),
            Some(|c| c.danger),
        ),
    };
    let small = |label: &'static str, message: Option<Action>| {
        button(text(label).size(12))
            .padding([5, 10])
            .style(crate::theme::secondary)
            .on_press_maybe(message.map(Message::McpSetup))
    };
    let enabled = !state.busy;
    let mut header = row![
        text("인증 토큰").size(18),
        iced::widget::space().width(Length::Fill)
    ]
    .spacing(8)
    .height(ROW_HEIGHT)
    .align_y(iced::Alignment::Center);
    if state.token == TokenPanel::Confirming {
        header = header
            .push(small("취소", Some(Action::ReissueCancel)))
            .push(small(
                "재발급 확인",
                enabled.then_some(Action::ReissueConfirm),
            ));
    } else {
        header = header.push(small("토큰 재발급", enabled.then_some(Action::ReissueAsk)));
    }
    let detail = text(detail).size(12);
    let detail = match tone {
        Some(tone) => detail.style(toned(tone)),
        None => detail.style(crate::theme::muted),
    };
    column![
        header,
        iced::widget::container(detail)
            .width(Length::Fill)
            .height(TOKEN_DETAIL_HEIGHT)
            .clip(true)
    ]
    .spacing(4)
    .into()
}

pub fn view(state: &State) -> Element<'_, Message> {
    let msg = Message::McpSetup;
    let enabled = !state.busy;
    let mut path = text_input("설정 파일 경로 · 비워두면 자동으로 찾습니다", &state.path)
        .padding(8)
        .size(13);
    if enabled {
        path = path.on_input(move |v| msg(Action::Path(v)));
    }
    let preview = state.preview.as_ref().map(|p| p.preview.as_str()).unwrap_or("AI 앱을 선택하고 ‘등록 미리보기’를 누르세요.\n현재 Windows 사용자에게 등록하여 여러 프로젝트에서 사용할 수 있습니다.\n앱 또는 데이터 폴더를 옮겼다면 새 위치에서 다시 등록하세요.");
    column![
        text("AI 연결 도우미").size(18),
        pick_list(Client::ALL, Some(state.client), move |c| msg(Action::Client(c))).width(220),
        path,
        row![
            button("경로 찾기").padding(8).on_press_maybe(enabled.then_some(msg(Action::Detect))),
            button("등록 미리보기").padding(8).on_press_maybe(enabled.then_some(msg(Action::Preview))),
            button("등록 적용").padding(8).on_press_maybe((enabled && state.preview.is_some()).then_some(msg(Action::Register))),
            button("연결 검사").padding(8).on_press_maybe(enabled.then_some(msg(Action::Check)))
        ].spacing(8).wrap(),
        scrollable(text(if state.status.is_empty() { "등록 전 변경 내용을 미리 확인할 수 있습니다." } else { &state.status }).size(13)).spacing(crate::calendar_ui::SCROLL_GAP).height(52).width(Length::Fill),
        scrollable(text(preview).size(13)).spacing(crate::calendar_ui::SCROLL_GAP).height(112).width(Length::Fill),
        text("ChatGPT Desktop·Codex·Claude Code는 최신 버전을 사용하세요. Claude Desktop은 연결 중에만 작은 중계 프로세스를 사용합니다.").size(12),
        text("ChatGPT Desktop은 Codex와 기본 설정을 공유합니다. 등록 후 ChatGPT의 설정 → MCP servers에서 Restart하세요. 웹·클라우드 연결은 별도입니다.").size(12),
        text("이 기능은 로컬 일정 연결입니다. 다른 PC로 일정·다이어리를 옮기는 기능은 별도입니다.").size(12)
    ].spacing(8).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp_clients::Registration;
    const ENDPOINT: &str = "http://127.0.0.1:39281/mcp";
    fn running() -> Option<String> {
        Some(ENDPOINT.into())
    }
    #[test]
    fn client_and_path_changes_invalidate_preview_and_busy_actions_do_not_mutate() {
        let mut state = State::default();
        state.path = "before".into();
        state.busy = true;
        let _ = state.update(Action::Client(Client::ClaudeDesktop), PathBuf::new(), None);
        let _ = state.update(Action::Path("after".into()), PathBuf::new(), None);
        assert_eq!(state.client, Client::Codex);
        assert_eq!(state.path, "before");
        let _ = state.update(
            Action::Finished(Err("failure".into())),
            PathBuf::new(),
            None,
        );
        assert!(!state.busy);
        assert_eq!(state.status, "failure");
        let _ = state.update(Action::Client(Client::ClaudeDesktop), PathBuf::new(), None);
        assert!(state.path.is_empty());
    }
    #[test]
    fn reissue_needs_a_second_click_and_reports_how_it_ended() {
        let mut state = State::default();
        let dir = PathBuf::new;
        // Confirming without having been asked does nothing: no one-click reissue.
        let _ = state.update(Action::ReissueConfirm, dir(), running());
        assert_eq!(state.token, TokenPanel::Idle);
        assert!(!state.busy);
        let _ = state.update(Action::ReissueAsk, dir(), running());
        assert_eq!(state.token, TokenPanel::Confirming);
        assert!(!state.busy, "asking starts nothing");
        let _ = state.update(Action::ReissueCancel, dir(), running());
        assert_eq!(state.token, TokenPanel::Idle);
        let _ = state.update(Action::ReissueAsk, dir(), running());
        let _ = state.update(Action::ReissueConfirm, dir(), running());
        assert_eq!(state.token, TokenPanel::Working);
        assert!(
            state.busy,
            "registrations and quitting wait for the reissue"
        );
        // Nothing else starts, and a second confirm does not queue another one.
        let _ = state.update(Action::Inspect(Client::ClaudeCode), dir(), running());
        let _ = state.update(Action::ReissueConfirm, dir(), running());
        assert_eq!(state.client, Client::Codex);
        assert_eq!(state.token, TokenPanel::Working);
        let _ = state.update(Action::Reissued(true, Ok(())), dir(), running());
        assert_eq!(state.token, TokenPanel::Reissued { running: true });
        assert!(!state.busy);
        // With the server off the result says a restart is needed.
        let _ = state.update(Action::ReissueAsk, dir(), None);
        let _ = state.update(Action::ReissueConfirm, dir(), None);
        let _ = state.update(Action::Reissued(false, Ok(())), dir(), None);
        assert_eq!(state.token, TokenPanel::Reissued { running: false });
        let _ = state.update(Action::ReissueAsk, dir(), running());
        let _ = state.update(Action::ReissueConfirm, dir(), running());
        let _ = state.update(
            Action::Reissued(true, Err("파일 사용 중".into())),
            dir(),
            running(),
        );
        assert_eq!(state.token, TokenPanel::Failed("파일 사용 중".into()));
        assert!(!state.busy);
        // While a registration or check runs, the question is not even asked.
        state.busy = true;
        let _ = state.update(Action::ReissueAsk, dir(), running());
        assert_eq!(state.token, TokenPanel::Failed("파일 사용 중".into()));
        // A question left open is withdrawn by cancelling, even while busy.
        state.token = TokenPanel::Confirming;
        let _ = state.update(Action::ReissueConfirm, dir(), running());
        assert_eq!(state.token, TokenPanel::Confirming, "ignored while busy");
        let _ = state.update(Action::ReissueCancel, dir(), running());
        assert_eq!(state.token, TokenPanel::Idle);
    }
    #[test]
    fn a_row_check_reads_every_install_location_instead_of_demanding_one() {
        let dir = std::env::temp_dir().join(format!("timeschedule-check-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = std::path::Path::new("C:\\app\\TimeSchedule.exe");
        let regular = dir.join("regular.json");
        let store = dir.join("store.json");
        std::fs::write(&store, "{}").unwrap();
        let plan =
            mcp_setup::prepare(Client::ClaudeDesktop, &regular, exe, &dir, ENDPOINT).unwrap();
        mcp_setup::apply(&plan).unwrap();
        let both = Ok(vec![regular.clone(), store.clone()]);
        assert_eq!(
            check_registration(Client::ClaudeDesktop, "", both, exe, &dir, ENDPOINT),
            Ok("등록 설정 일치")
        );
        // A path typed into the assistant is checked on its own.
        assert_eq!(
            check_registration(
                Client::ClaudeDesktop,
                store.to_str().unwrap(),
                Ok(vec![]),
                exe,
                &dir,
                ENDPOINT
            ),
            Ok("등록되어 있지 않습니다")
        );
        // A Codex entry carries the port, so a server on another port means redo.
        let codex = dir.join("config.toml");
        let plan = mcp_setup::prepare(Client::Codex, &codex, exe, &dir, ENDPOINT).unwrap();
        mcp_setup::apply(&plan).unwrap();
        assert_eq!(
            check_registration(
                Client::Codex,
                "",
                Ok(vec![codex]),
                exe,
                &dir,
                "http://127.0.0.1:40123/mcp"
            ),
            Ok("등록이 예전 형식이거나 바뀌었습니다 · 다시 등록하세요")
        );
        assert!(check_registration(
            Client::Codex,
            "",
            Err("no profile".into()),
            exe,
            &dir,
            ENDPOINT
        )
        .is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn board_refresh_tracks_generations_and_ignores_stale_results() {
        let mut state = State::default();
        let _ = state.update(Action::Refresh, PathBuf::new(), None);
        assert!(
            !state.checking,
            "with the server off there is nothing to compare against"
        );
        let _ = state.update(Action::Refresh, PathBuf::new(), running());
        assert!(state.checking);
        let first = state.generation;
        let _ = state.update(Action::Refresh, PathBuf::new(), running());
        let late = [
            Registration::Current,
            Registration::Current,
            Registration::Current,
        ];
        let _ = state.update(Action::Loaded(first, late), PathBuf::new(), running());
        assert!(state.checking, "an older check must not end the newer one");
        assert_eq!(state.registrations[0], Registration::Unchecked);
        let fresh = [
            Registration::Current,
            Registration::Missing,
            Registration::Outdated,
        ];
        let _ = state.update(
            Action::Loaded(state.generation, fresh),
            PathBuf::new(),
            running(),
        );
        assert!(!state.checking);
        assert_eq!(state.registrations[2], Registration::Outdated);
        // Refreshing is read-only, so it runs even while a registration is busy.
        state.busy = true;
        let _ = state.update(Action::Refresh, PathBuf::new(), running());
        assert!(state.checking);
    }
    #[test]
    fn row_buttons_prepare_or_inspect_that_app_and_results_refresh_the_board() {
        let mut state = State::default();
        state.path = "C:\\custom\\path.json".into();
        let _ = state.update(
            Action::Prepare(Client::ClaudeCode),
            PathBuf::new(),
            running(),
        );
        assert_eq!(state.client, Client::ClaudeCode);
        assert!(
            state.path.is_empty(),
            "the row uses that app's own configuration"
        );
        assert!(state.busy, "the preview started");
        let _ = state.update(Action::Inspect(Client::Codex), PathBuf::new(), running());
        assert_eq!(state.client, Client::ClaudeCode, "ignored while busy");
        let _ = state.update(
            Action::Finished(Ok(Outcome::Status("등록 완료".into()))),
            PathBuf::new(),
            running(),
        );
        assert!(!state.busy);
        assert!(
            state.checking,
            "a finished registration or check re-reads the board"
        );
        let _ = state.update(
            Action::Inspect(Client::ClaudeDesktop),
            PathBuf::new(),
            running(),
        );
        assert_eq!(state.client, Client::ClaudeDesktop);
        assert!(state.busy);
    }
}
