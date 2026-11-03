//! Stable settings layout; all filesystem/network work starts only on button press.
use crate::{
    app::Message,
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
}
#[derive(Clone, Debug)]
pub enum Outcome {
    Detected(PathBuf),
    Preview(Plan),
    Status(String),
}

impl State {
    pub fn update(&mut self, action: Action, dir: PathBuf) -> Task<Message> {
        if self.busy && !matches!(action, Action::Finished(_)) {
            return Task::none();
        }
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
                return Task::none();
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
                let path = if path.trim().is_empty() { mcp_setup::default_path(client)? } else { PathBuf::from(path.trim()) };
                if matches!(action, Action::Register) {
                    let plan = preview.ok_or("미리보기를 먼저 확인하세요.")?;
                    crate::mcp_connection::probe(&dir)?;
                    let backup = mcp_setup::apply(&plan)?;
                    let backup = backup.map(|p| format!("\n백업: {}", p.display())).unwrap_or_default();
                    return Ok(Outcome::Status(format!("등록 완료. {client}에서 연결을 다시 불러오거나 앱을 재시작하세요.\nAI 앱의 실제 연결 승인은 해당 앱에서 확인하세요.{backup}")));
                }
                let endpoint = crate::mcp_connection::current_endpoint(&dir)?;
                let exe = std::env::current_exe().map_err(|_| "실행 파일 위치 확인 실패")?;
                let plan = mcp_setup::prepare(client, &path, &exe, &dir, &endpoint)?;
                if matches!(action, Action::Preview) { return Ok(Outcome::Preview(plan)); }
                let server = crate::mcp_connection::probe(&dir)?;
                let registration = if plan.unchanged() { "등록 설정 일치" } else { "등록 설정 없음 또는 갱신 필요" };
                Ok(Outcome::Status(format!("{server}\n{registration} · AI 앱의 재연결·권한 승인은 해당 앱에서 확인하세요.")))
            }).await.unwrap_or_else(|_| Err("연결 작업을 완료하지 못했습니다. 다시 시도하세요.".into()))
            },
            |result| Message::McpSetup(Action::Finished(result)),
        )
    }
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
        pick_list([Client::Codex, Client::ClaudeCode, Client::ClaudeDesktop], Some(state.client), move |c| msg(Action::Client(c))).width(220),
        path,
        row![
            button("경로 찾기").padding(8).on_press_maybe(enabled.then_some(msg(Action::Detect))),
            button("등록 미리보기").padding(8).on_press_maybe(enabled.then_some(msg(Action::Preview))),
            button("등록 적용").padding(8).on_press_maybe((enabled && state.preview.is_some()).then_some(msg(Action::Register))),
            button("연결 검사").padding(8).on_press_maybe(enabled.then_some(msg(Action::Check)))
        ].spacing(8).wrap(),
        scrollable(text(if state.status.is_empty() { "등록 전 변경 내용을 미리 확인할 수 있습니다." } else { &state.status }).size(13)).height(52).width(Length::Fill),
        scrollable(text(preview).size(13)).height(112).width(Length::Fill),
        text("Codex·Claude Code는 최신 버전을 사용하세요. Claude Desktop은 연결 중에만 작은 중계 프로세스를 사용합니다.").size(12),
        text("이 기능은 로컬 일정 연결입니다. 다른 PC로 일정·다이어리를 옮기는 기능은 별도입니다.").size(12)
    ].spacing(8).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_and_path_changes_invalidate_preview_and_busy_actions_do_not_mutate() {
        let mut state = State::default();
        state.path = "before".into();
        state.busy = true;
        let _ = state.update(Action::Client(Client::ClaudeDesktop), PathBuf::new());
        let _ = state.update(Action::Path("after".into()), PathBuf::new());
        assert_eq!(state.client, Client::Codex);
        assert_eq!(state.path, "before");
        let _ = state.update(Action::Finished(Err("failure".into())), PathBuf::new());
        assert!(!state.busy);
        assert_eq!(state.status, "failure");
        let _ = state.update(Action::Client(Client::ClaudeDesktop), PathBuf::new());
        assert!(state.path.is_empty());
    }
}
