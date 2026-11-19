//! Which AI apps have connected to the local calendar MCP, and how each app's
//! registration stands. Only a client's name, version and time are recorded.
use crate::mcp_setup::Client;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One row of the connection board. Codex and ChatGPT Desktop share one
/// configuration file, so they share one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiApp {
    CodexChatGpt,
    ClaudeCode,
    ClaudeDesktop,
}
impl AiApp {
    pub const ALL: [Self; 3] = [Self::CodexChatGpt, Self::ClaudeCode, Self::ClaudeDesktop];
    pub fn label(self) -> &'static str {
        match self {
            Self::CodexChatGpt => "Codex · ChatGPT Desktop",
            Self::ClaudeCode => "Claude Code",
            Self::ClaudeDesktop => "Claude Desktop",
        }
    }
    /// The setup client whose configuration this row reflects.
    pub fn client(self) -> Client {
        match self {
            Self::CodexChatGpt => Client::Codex,
            Self::ClaudeCode => Client::ClaudeCode,
            Self::ClaudeDesktop => Client::ClaudeDesktop,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Origin {
    App(AiApp),
    /// TimeSchedule's own connection check, which is not an AI app.
    SelfCheck,
    Other,
}
pub const SELF_CHECK_NAME: &str = "TimeSchedule connection check";

/// Maps the name an MCP client announces in `initialize` to a board row.
/// Unknown clients are kept as "other" rather than guessed into a row.
/// The client's own name decides; a free-form title is consulted only when the
/// name says nothing, so "codex-mcp-client" titled "… with Claude" stays Codex.
pub fn classify(name: &str, title: Option<&str>) -> Origin {
    if name == SELF_CHECK_NAME {
        return Origin::SelfCheck;
    }
    match product(name).or_else(|| title.and_then(product)) {
        Some(app) => Origin::App(app),
        None => Origin::Other,
    }
}
fn product(value: &str) -> Option<AiApp> {
    let text = value.to_lowercase();
    if text.contains("claude-code") || text.contains("claude code") {
        Some(AiApp::ClaudeCode)
    } else if text.contains("claude") {
        Some(AiApp::ClaudeDesktop)
    } else if ["codex", "chatgpt", "openai"]
        .iter()
        .any(|word| text.contains(word))
    {
        Some(AiApp::CodexChatGpt)
    } else {
        None
    }
}

/// At most `limit` characters with control characters removed: names come from
/// whatever connects and end up on screen.
fn clean(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect::<String>()
        .trim()
        .to_string()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seen {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub version: String,
    pub last_seen: DateTime<Local>,
}

const MAX_CLIENTS: usize = 16;
const FILE_NAME: &str = "mcp-clients.json";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    schema_version: u32,
    clients: Vec<Seen>,
}

/// The last `initialize` from each client, newest first. Kept in the data folder
/// so the board still shows a connection after TimeSchedule restarts.
pub struct ClientLog {
    /// Held only for in-memory updates and copies; the screen reads it on every
    /// render, so it is never held across disk I/O.
    seen: Mutex<Vec<Seen>>,
    /// Serializes file writes so the last writer always saves the newest log.
    persistence: Mutex<()>,
    file: Option<PathBuf>,
    /// Test hook: the next file write waits here, standing in for a slow disk.
    #[cfg(test)]
    pause_before_write: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}
impl ClientLog {
    fn new(seen: Vec<Seen>, file: Option<PathBuf>) -> Self {
        Self {
            seen: Mutex::new(seen),
            persistence: Mutex::new(()),
            file,
            #[cfg(test)]
            pause_before_write: Mutex::new(None),
        }
    }
    #[cfg(test)]
    pub fn memory() -> Self {
        Self::new(Vec::new(), None)
    }
    /// A missing or damaged file starts an empty log; it is only a display aid.
    pub fn load(dir: &Path) -> Self {
        let file = dir.join(FILE_NAME);
        let mut seen = std::fs::read(&file)
            .ok()
            .filter(|bytes| bytes.len() <= 64 * 1024)
            .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
            .filter(|stored| stored.schema_version == 1)
            .map(|stored| stored.clients)
            .unwrap_or_default();
        seen.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        seen.truncate(MAX_CLIENTS);
        Self::new(seen, Some(file))
    }
    pub fn record(&self, name: &str, title: Option<&str>, version: &str, at: DateTime<Local>) {
        if classify(name, title) == Origin::SelfCheck {
            return;
        }
        let entry = Seen {
            name: clean(name, 100),
            title: title.map(|t| clean(t, 100)).filter(|t| !t.is_empty()),
            version: clean(version, 40),
            last_seen: at,
        };
        // Writers queue here first, then copy the newest state while holding the
        // memory lock only briefly; a slow disk delays other writers, never reads.
        let persistence = self.persistence.lock();
        let clients = {
            let Ok(mut seen) = self.seen.lock() else {
                return;
            };
            seen.retain(|s| s.name != entry.name);
            seen.push(entry);
            seen.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
            seen.truncate(MAX_CLIENTS);
            seen.clone()
        };
        if let (Some(file), Ok(_queue)) = (&self.file, persistence) {
            #[cfg(test)]
            if let Some(gate) = self.pause_before_write.lock().unwrap().take() {
                let _ = gate.recv();
            }
            // Best effort: the in-memory log stays correct if the disk refuses.
            let _ = crate::mcp::write_json_atomic(
                file,
                &Stored {
                    schema_version: 1,
                    clients,
                },
            );
        }
    }
    /// Newest first.
    pub fn snapshot(&self) -> Vec<Seen> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }
    #[cfg(test)]
    pub fn last_for(&self, app: AiApp) -> Option<Seen> {
        self.snapshot()
            .into_iter()
            .find(|s| classify(&s.name, s.title.as_deref()) == Origin::App(app))
    }
    #[cfg(test)]
    pub fn others(&self) -> Vec<Seen> {
        self.snapshot()
            .into_iter()
            .filter(|s| classify(&s.name, s.title.as_deref()) == Origin::Other)
            .collect()
    }
    #[cfg(test)]
    fn pause_next_write(&self, gate: std::sync::mpsc::Receiver<()>) {
        *self.pause_before_write.lock().unwrap() = Some(gate);
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Registration {
    #[default]
    Unchecked,
    Missing,
    Current,
    /// An entry exists but differs from what this app would write now, for
    /// example a registration made before 0.13.1 or after the EXE moved.
    Outdated,
    Error(String),
}
/// Reads, never writes, each candidate configuration file. One current entry
/// is enough (Claude Desktop may have two install locations); otherwise an
/// entry that needs redoing outranks an unreadable file, which outranks none.
pub fn registration_at(
    client: Client,
    paths: &[PathBuf],
    exe: &Path,
    dir: &Path,
    endpoint: &str,
) -> Registration {
    let mut outdated = false;
    let mut error = None;
    for path in paths {
        match path.try_exists() {
            Ok(true) => {}
            Ok(false) => continue,
            // Unknown is not "not registered": report that the file is unreadable.
            Err(_) => {
                error.get_or_insert_with(|| {
                    "설정 파일에 접근할 수 없습니다. 경로와 접근 권한을 확인하세요.".to_string()
                });
                continue;
            }
        }
        match crate::mcp_setup::prepare(client, path, exe, dir, endpoint) {
            Ok(plan) if plan.unchanged() => return Registration::Current,
            Ok(plan) if plan.replacing => outdated = true,
            Ok(_) => {}
            Err(message) => {
                error.get_or_insert(message);
            }
        }
    }
    if outdated {
        Registration::Outdated
    } else if let Some(message) = error {
        Registration::Error(message)
    } else {
        Registration::Missing
    }
}
/// Registration of `app` in the configuration files this PC actually has.
pub fn registration(app: AiApp, exe: &Path, dir: &Path, endpoint: &str) -> Registration {
    match crate::mcp_setup::config_candidates(app.client()) {
        Ok(paths) => registration_at(app.client(), &paths, exe, dir, endpoint),
        Err(message) => Registration::Error(message),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RowState {
    ServerOff,
    Checking,
    Missing,
    Outdated,
    Registered,
    Connected(DateTime<Local>),
    Error(String),
}
/// The registration file decides whether a row is set up at all; a recorded
/// `initialize` upgrades a current registration to "connected".
pub fn row_state(
    server_running: bool,
    registration: &Registration,
    last: Option<&Seen>,
) -> RowState {
    if !server_running {
        return RowState::ServerOff;
    }
    match (registration, last) {
        (Registration::Missing, _) => RowState::Missing,
        (Registration::Outdated, _) => RowState::Outdated,
        (Registration::Error(message), _) => RowState::Error(message.clone()),
        (Registration::Current | Registration::Unchecked, Some(seen)) => {
            RowState::Connected(seen.last_seen)
        }
        (Registration::Current, None) => RowState::Registered,
        (Registration::Unchecked, None) => RowState::Checking,
    }
}
/// How long ago `at` was, for a status line that is redrawn every tick.
pub fn when(now: DateTime<Local>, at: DateTime<Local>) -> String {
    let age = now.signed_duration_since(at);
    if age.num_seconds() < 60 {
        "방금".into()
    } else if age.num_minutes() < 60 {
        format!("{}분 전", age.num_minutes())
    } else if at.date_naive() == now.date_naive() {
        at.format("오늘 %H:%M").to_string()
    } else {
        at.format("%m-%d %H:%M").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn at(h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 16, h, m, 0).unwrap()
    }
    fn temp() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("timeschedule-clients-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn clients_are_mapped_to_their_rows_and_the_self_check_is_recognised() {
        for (name, title, expected) in [
            ("claude-code", None, Origin::App(AiApp::ClaudeCode)),
            ("Claude Code", None, Origin::App(AiApp::ClaudeCode)),
            ("claude-ai", None, Origin::App(AiApp::ClaudeDesktop)),
            (
                "app",
                Some("Claude Desktop"),
                Origin::App(AiApp::ClaudeDesktop),
            ),
            ("codex-mcp-client", None, Origin::App(AiApp::CodexChatGpt)),
            ("ChatGPT", None, Origin::App(AiApp::CodexChatGpt)),
            (
                "client",
                Some("OpenAI Codex"),
                Origin::App(AiApp::CodexChatGpt),
            ),
            (SELF_CHECK_NAME, None, Origin::SelfCheck),
            // A recognised name wins over a title that mentions another product.
            (
                "codex-mcp-client",
                Some("Codex with Claude"),
                Origin::App(AiApp::CodexChatGpt),
            ),
            (
                "claude-code",
                Some("works with ChatGPT"),
                Origin::App(AiApp::ClaudeCode),
            ),
            (
                "generic",
                Some("Claude Code"),
                Origin::App(AiApp::ClaudeCode),
            ),
            ("cursor-vscode", None, Origin::Other),
            ("", None, Origin::Other),
        ] {
            assert_eq!(classify(name, title), expected, "{name} / {title:?}");
        }
    }

    #[test]
    fn a_connection_is_kept_across_restarts_without_the_self_check() {
        let dir = temp();
        let log = ClientLog::load(&dir);
        log.record("claude-code", None, "2.1.0", at(10, 0));
        log.record(SELF_CHECK_NAME, None, "0.14.0", at(10, 1));
        log.record("claude-code", None, "2.1.1", at(10, 5));
        log.record("codex-mcp-client", Some("Codex"), "0.163.0", at(10, 3));
        let reloaded = ClientLog::load(&dir);
        let seen = reloaded.snapshot();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(seen[0].name, "claude-code", "newest first");
        assert_eq!(seen[0].version, "2.1.1");
        assert_eq!(seen[0].last_seen, at(10, 5));
        assert_eq!(
            reloaded
                .last_for(AiApp::CodexChatGpt)
                .unwrap()
                .title
                .as_deref(),
            Some("Codex")
        );
        assert!(reloaded.last_for(AiApp::ClaudeDesktop).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_log_stays_small_bounded_and_survives_a_damaged_file() {
        let dir = temp();
        std::fs::write(dir.join("mcp-clients.json"), b"{not json").unwrap();
        let log = ClientLog::load(&dir);
        assert!(log.snapshot().is_empty());
        let long = format!("evil\u{0}\u{1b}[31m{}", "x".repeat(500));
        log.record(&long, Some(&"t".repeat(500)), &"9".repeat(500), at(9, 0));
        let seen = &log.snapshot()[0];
        assert!(
            seen.name.chars().all(|c| !c.is_control()),
            "{:?}",
            seen.name
        );
        assert!(seen.name.chars().count() <= 100);
        assert!(seen.title.as_ref().unwrap().chars().count() <= 100);
        assert!(seen.version.chars().count() <= 40);
        for i in 0..40 {
            log.record(
                &format!("client-{i}"),
                None,
                "1",
                at(9, 0) + Duration::minutes(i),
            );
        }
        let seen = log.snapshot();
        assert_eq!(seen.len(), 16, "the oldest records are dropped");
        assert_eq!(seen[0].name, "client-39");
        assert_eq!(ClientLog::load(&dir).snapshot().len(), 16);
        assert_eq!(log.others().len(), 16);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reading_the_log_never_waits_for_a_disk_write() {
        let dir = temp();
        let log = std::sync::Arc::new(ClientLog::load(&dir));
        log.record("claude-code", None, "1", at(9, 0));
        // Stand in for a slow disk: the next record stops inside its file write.
        let (release, gate) = std::sync::mpsc::channel();
        log.pause_next_write(gate);
        let writer = {
            let log = log.clone();
            std::thread::spawn(move || log.record("codex-mcp-client", None, "1", at(9, 5)))
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        let (send, receive) = std::sync::mpsc::channel();
        {
            let log = log.clone();
            std::thread::spawn(move || send.send(log.snapshot()[0].name.clone()).unwrap());
        }
        let read = receive.recv_timeout(std::time::Duration::from_secs(2));
        release.send(()).unwrap();
        writer.join().unwrap();
        assert_eq!(
            read.as_deref(),
            Ok("codex-mcp-client"),
            "the screen's read waited for the disk, or missed the in-memory update"
        );
        // Once the disk is free the newer record is both in memory and on disk.
        assert_eq!(log.snapshot()[0].name, "codex-mcp-client");
        assert_eq!(ClientLog::load(&dir).snapshot()[0].name, "codex-mcp-client");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn row_states_follow_server_registration_and_connection() {
        let seen = Seen {
            name: "claude-code".into(),
            title: None,
            version: "2".into(),
            last_seen: at(10, 0),
        };
        use Registration as R;
        assert_eq!(
            row_state(false, &R::Current, Some(&seen)),
            RowState::ServerOff
        );
        assert_eq!(row_state(true, &R::Unchecked, None), RowState::Checking);
        assert_eq!(
            row_state(true, &R::Unchecked, Some(&seen)),
            RowState::Connected(at(10, 0))
        );
        assert_eq!(row_state(true, &R::Missing, Some(&seen)), RowState::Missing);
        assert_eq!(
            row_state(true, &R::Outdated, Some(&seen)),
            RowState::Outdated
        );
        assert_eq!(row_state(true, &R::Current, None), RowState::Registered);
        assert_eq!(
            row_state(true, &R::Current, Some(&seen)),
            RowState::Connected(at(10, 0))
        );
        assert_eq!(
            row_state(true, &R::Error("bad".into()), None),
            RowState::Error("bad".into())
        );
    }

    #[test]
    fn times_read_naturally() {
        let now = at(15, 30);
        assert_eq!(when(now, now - Duration::seconds(20)), "방금");
        assert_eq!(when(now, now - Duration::minutes(7)), "7분 전");
        assert_eq!(when(now, at(9, 5)), "오늘 09:05");
        assert_eq!(when(now, at(9, 5) - Duration::days(2)), "11-14 09:05");
        // A clock that moved backwards must not show a negative age.
        assert_eq!(when(now, now + Duration::minutes(3)), "방금");
    }

    #[test]
    fn registration_reads_current_outdated_and_missing_entries() {
        let dir = temp();
        let exe = Path::new("C:\\app\\TimeSchedule.exe");
        let endpoint = "http://127.0.0.1:39281/mcp";
        let config = dir.join("config.toml");
        // No file and an unrelated file both mean "not registered".
        assert_eq!(
            registration_at(Client::Codex, &[config.clone()], exe, &dir, endpoint),
            Registration::Missing
        );
        std::fs::write(
            &config,
            "[mcp_servers.other]\nurl='https://example.test/mcp'\n",
        )
        .unwrap();
        assert_eq!(
            registration_at(Client::Codex, &[config.clone()], exe, &dir, endpoint),
            Registration::Missing
        );
        let plan = crate::mcp_setup::prepare(Client::Codex, &config, exe, &dir, endpoint).unwrap();
        crate::mcp_setup::apply(&plan).unwrap();
        assert_eq!(
            registration_at(Client::Codex, &[config.clone()], exe, &dir, endpoint),
            Registration::Current
        );
        // The same entry checked against another port is a registration to redo.
        assert_eq!(
            registration_at(
                Client::Codex,
                &[config.clone()],
                exe,
                &dir,
                "http://127.0.0.1:40123/mcp"
            ),
            Registration::Outdated
        );
        // Of two Claude Desktop locations, one current registration is enough.
        let desktop_a = dir.join("a.json");
        let desktop_b = dir.join("b.json");
        std::fs::write(&desktop_b, "{}").unwrap();
        let plan =
            crate::mcp_setup::prepare(Client::ClaudeDesktop, &desktop_a, exe, &dir, endpoint)
                .unwrap();
        crate::mcp_setup::apply(&plan).unwrap();
        assert_eq!(
            registration_at(
                Client::ClaudeDesktop,
                &[desktop_a, desktop_b],
                exe,
                &dir,
                endpoint
            ),
            Registration::Current
        );
        let broken = dir.join("broken.json");
        std::fs::write(&broken, "{broken").unwrap();
        assert!(matches!(
            registration_at(Client::ClaudeCode, &[broken], exe, &dir, endpoint),
            Registration::Error(_)
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
