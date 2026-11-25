//! The day transcript: what people and AI said in one project's chat sessions on one
//! Korean calendar day, read from the kept originals (see `chat_originals`).
//!
//! Only dialogue is kept. Tool output, reasoning, injected instructions and compaction
//! summaries are left out; a failed command keeps its exit code and diagnostic lines.
//! A session nobody typed into (an agent run) keeps only its last answer of the day.
//! Each block says who said it and where it is in the kept file, so a later step can
//! cite it by number.
//!
//! A later step takes a block marked as a person's for proof that a person said it.
//! So a message is a person's only when every mark the tool wrote agrees: the message
//! names this session, carries the tool's mark of a typed message, and nothing in its
//! row or its session says otherwise. The tool readers (`codex`, `claude`) apply this
//! to one message. What is decided over a whole session is decided here: its files are
//! read together, a message written twice counts once and is a person's only when every
//! copy is, a session with a file that cannot be read has no person's message, and a
//! session with no person's message is an agent run whichever tool wrote it.
use crate::{
    chat_history::transcript::line,
    chat_originals::{group_name, is_link, kept_path},
    project_records::repo_key,
};
use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    fs,
    io::BufReader,
    path::{Path, PathBuf},
};

mod claude;
mod codex;
pub mod diary;
pub mod record;

/// Korea Standard Time. Days are split at its midnight.
pub fn kst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("valid offset")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Speaker {
    /// Text a person typed, or an answer they picked in a question dialog.
    Human,
    /// An answer the AI showed in a session a person opened.
    Ai,
    /// The last answer of the day of a session nobody typed into.
    AgentRun,
    /// A failed command: exit code and diagnostic lines.
    Check,
    /// An instruction another session or an automation sent.
    Delegated,
}

impl Speaker {
    pub fn label(self) -> &'static str {
        match self {
            Self::Human => "사람",
            Self::Ai => "AI",
            Self::AgentRun => "에이전트 실행",
            Self::Check => "검사 출력",
            Self::Delegated => "다른 세션의 지시",
        }
    }
    /// Characters kept of one block. Measured on 2026-11-24 over the TimeSchedule
    /// originals: no human message was longer than 6,093 characters and 99% of AI
    /// answers were at most 3,359.
    fn cap(self) -> usize {
        match self {
            Self::Human => 8000,
            Self::Ai => 4000,
            Self::AgentRun => 2000,
            Self::Check => 1000,
            Self::Delegated => 500,
        }
    }
}

/// What a tool reader says one message is. A reader calls a message a person's only
/// when every mark in the file agrees. Whether the session is an agent run is decided
/// in `day_transcript`, over all files of the session.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Role {
    /// Text a person typed.
    Human,
    /// An answer a person picked in a question dialog. It is shown as a person's or
    /// not at all: copies of it that do not agree leave no block.
    Picked,
    Ai,
    Check,
    Delegated,
}

/// One message of a session file.
pub(crate) struct Item {
    /// The same key in the files of one session is the same message written again.
    pub key: Option<String>,
    pub role: Role,
    /// 1-based line in the decompressed session file.
    pub line: usize,
    /// The time of its row, or of the last row before it that has one.
    pub at: Option<DateTime<FixedOffset>>,
    /// Rows the tool wrote of one answer share a group. An agent run keeps the whole
    /// of its last answer.
    pub group: Option<String>,
    pub text: String,
    /// The answers of a `Picked` message, each as the person picked it. The text shows
    /// them under their questions. No other message has any.
    pub answers: Vec<String>,
}

impl Item {
    /// A row with the key `key` that is no message of a person's. It has no text, so
    /// it makes no block; it stands against a copy of the row that looks like one.
    pub(crate) fn contrary(key: String, row: &Row) -> Self {
        Self {
            key: Some(key),
            role: Role::Delegated,
            line: row.line,
            at: row.at,
            group: None,
            text: String::new(),
            answers: Vec::new(),
        }
    }
}

/// One session file as a tool reader read it.
#[derive(Default)]
pub(crate) struct Parsed {
    /// The file says a program started the session: `codex exec`, a subagent.
    pub by_program: bool,
    /// The file looks like a person's session, but it has messages without the mark
    /// that would make them a person's.
    pub unmarked: bool,
    /// Rows left out because they do not say which session they belong to.
    pub unnamed: usize,
    /// Rows of the day left out that hold plain text with no mark of who sent it.
    pub plain: usize,
    /// Codex messages of the day written only in an older form (`event_msg` of type
    /// `user_message`), in a file that has no `UserMessage` item to read instead.
    pub older: usize,
    pub items: Vec<Item>,
    /// What `Rows` saw of the day. `read_file` fills it in.
    pub seen: Seen,
}

/// What the rows of one file say about the day, whether or not they are dialogue.
#[derive(Default, Clone, Copy)]
pub(crate) struct Seen {
    /// A row is on the day, or has no time to tell.
    pub touched: bool,
    /// Rows that are or may be on the day and were not read: over the line limit, or
    /// not JSON.
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// `S1`, `S2`, … in the transcript.
    pub tag: String,
    /// Kept paths below the project's originals folder. A Codex session can be kept
    /// in more than one file.
    pub files: Vec<String>,
    pub tool: String,
    pub client: String,
    pub session: String,
    pub agent_run: bool,
}

#[derive(Debug, Clone)]
pub struct Block {
    /// 1-based number the transcript shows.
    pub number: usize,
    pub speaker: Speaker,
    /// The answers a person picked, when the block is made of a question dialog. The
    /// text shows each under its question; the questions are the AI's words and only
    /// these answers are the person's. They are kept apart from the text, whole, so
    /// that where an answer ends is never read back from text that may be cut.
    pub picked: Option<Vec<String>>,
    /// Index into `DayTranscript::sources`.
    pub source: usize,
    /// Index into the source's `files`.
    pub file: usize,
    pub line: usize,
    pub at: DateTime<FixedOffset>,
    pub text: String,
    pub clipped: bool,
}

#[derive(Debug, Clone)]
pub struct DayTranscript {
    pub project_id: String,
    pub title: String,
    /// The project's `repoKey` and its folder name in `originals`. The daily record
    /// is kept under the same folder name in `daily`.
    pub repo_key: String,
    pub folder: String,
    pub date: NaiveDate,
    pub sources: Vec<Source>,
    pub blocks: Vec<Block>,
    /// Files that could not be read, and sessions whose speaker could not be decided.
    pub warnings: Vec<String>,
    /// When the newest file in the project's originals was kept.
    pub latest_archived: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectFile {
    project_id: String,
    title: String,
    repo_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    file: String,
    tool: String,
    session: String,
    client: String,
    started: String,
    source_modified: u64,
    archived_at: String,
}

const LINKED: &str = "링크를 거치는 보관본은 읽지 않습니다.";

fn linked(path: &Path) -> Result<bool, String> {
    is_link(path).map_err(|e| e.to_string())
}

/// The project's folder in `<vault>/originals`. `project` is a project ID or an absolute
/// folder path of the project. Like the code that keeps the files, it reads nothing
/// through a link: a linked folder or `project.json` is not opened to see whose it is.
fn project_dir(vault: &Path, project: &str) -> Result<(PathBuf, ProjectFile), String> {
    let originals = vault.join("originals");
    if linked(&originals)? {
        return Err(LINKED.into());
    }
    let key = Path::new(project).is_absolute().then(|| repo_key(project));
    let folders: Vec<PathBuf> = match &key {
        Some(key) => vec![originals.join(group_name(key))],
        None => fs::read_dir(&originals)
            .map_err(|_| "보관된 채팅 원문이 없습니다.".to_string())?
            .flatten()
            .map(|entry| entry.path())
            .collect(),
    };
    let mut found = Vec::new();
    let mut links = 0;
    for dir in folders {
        let file = dir.join("project.json");
        if linked(&dir)? || linked(&file)? {
            links += 1;
            continue;
        }
        let Ok(bytes) = fs::read(&file) else {
            continue;
        };
        let info: ProjectFile = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let wanted = match &key {
            Some(key) => &info.repo_key == key,
            None => info.project_id == project,
        };
        if wanted {
            found.push((dir, info));
        }
    }
    if found.len() > 1 {
        // The project's folder was changed in the app, and each folder has originals.
        let folders: Vec<&str> = found.iter().map(|(_, f)| f.repo_key.as_str()).collect();
        return Err(format!(
            "이 프로젝트 ID의 보관 폴더가 {}개입니다. --project에 프로젝트 폴더의 경로를 지정하세요: {}",
            found.len(),
            folders.join(", ")
        ));
    }
    found.pop().ok_or_else(|| match (&key, links) {
        (Some(_), 0) => format!("이 폴더의 보관된 원문이 없습니다: {project}"),
        (None, 0) => format!("이 프로젝트의 보관된 원문이 없습니다: {project}"),
        _ => format!("{LINKED} 링크가 아닌 보관 폴더에는 이 프로젝트가 없습니다: {project}"),
    })
}

/// Whether a kept file is a dialogue: a session file, or a Claude subagent transcript.
/// Claude tool-result files beside a session are not.
fn is_dialogue(entry: &Entry) -> bool {
    entry.file.ends_with(".jsonl.zst")
        && match entry.tool.as_str() {
            "codex" => true,
            "claude" => entry.file.split('/').count() == 3 || entry.file.contains("/subagents/"),
            _ => false,
        }
}

/// What makes kept files one session. A Codex session can be kept in more than one
/// file: Codex moves a rollout to `archived_sessions`, and goes on in a second rollout.
/// A Claude Code file is a session of its own, because a subagent transcript carries
/// the session ID of the session that started it.
fn session_key(entry: &Entry) -> (&str, &str) {
    match entry.tool.as_str() {
        "codex" => ("codex", &entry.session),
        _ => ("claude", &entry.file),
    }
}

/// The session the rows of a kept file must name. For Codex it is the index's, which
/// the reader checks against the file's first row. A Claude Code file is kept as
/// `claude/<folder>/<session>.jsonl.zst`, and what belongs to it below
/// `claude/<folder>/<session>/`, so the path names the session. The index's session
/// is that of the main file's first row, which the tool can have copied from the
/// session this one continues.
fn session_id(entry: &Entry) -> &str {
    let named = entry
        .file
        .split('/')
        .nth(2)
        .filter(|_| entry.tool != "codex");
    match named {
        Some(name) => name.strip_suffix(".jsonl.zst").unwrap_or(name),
        None => &entry.session,
    }
}

/// The text with every kind of line end as `\n`, so a line of it is a line to any reader.
fn plain_lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace(
        ['\r', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}'],
        "\n",
    )
}

/// The text on one line.
fn one_line(text: &str) -> String {
    plain_lines(text).replace('\n', " ")
}

/// What ends the text of a block that was cut. The program writes it, not the speaker.
const CUT_MARK: char = '…';

fn clip(text: &str, chars: usize) -> (String, bool) {
    match text.char_indices().nth(chars) {
        Some((end, _)) => (format!("{}{CUT_MARK}", &text[..end]), true),
        None => (text.to_owned(), false),
    }
}

impl Block {
    /// The text as far as it was written: without the mark that ends a cut block.
    fn as_written(&self) -> &str {
        match self.text.strip_suffix(CUT_MARK) {
            Some(written) if self.clipped => written,
            _ => &self.text,
        }
    }
}

/// One kept file, read for the day from `day.0` to before `day.1`.
fn read_file(
    dir: &Path,
    entry: &Entry,
    day: (DateTime<FixedOffset>, DateTime<FixedOffset>),
) -> Result<Parsed, String> {
    let path = kept_path(dir, &entry.file).map_err(|e| e.to_string())?;
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let decoder = zstd::stream::read::Decoder::new(file).map_err(|e| e.to_string())?;
    let mut rows = Rows {
        input: BufReader::new(decoder),
        buf: Vec::new(),
        line: 0,
        clock: DateTime::parse_from_rfc3339(&entry.started)
            .ok()
            .map(|t| t.with_timezone(&kst())),
        day,
        seen: Seen::default(),
        undated_skipped: 0,
    };
    let session = session_id(entry);
    let mut parsed = match entry.tool.as_str() {
        "codex" => codex::read(&mut rows, session),
        _ => claude::read(&mut rows, session, entry.file.contains("/subagents/")),
    }?;
    parsed.seen = rows.seen;
    Ok(parsed)
}

pub(crate) struct Row {
    /// 1-based line in the decompressed session file.
    pub line: usize,
    /// The row's time, or that of the last row before it that has one.
    pub at: Option<DateTime<FixedOffset>>,
    /// That time is on the day asked for, or there is none to tell.
    pub on_day: bool,
    pub value: serde_json::Value,
}

/// The rows of a decompressed session file. A row longer than the line limit and a
/// finished row that is not JSON are counted and skipped. An unfinished last row is
/// not returned and not counted: the tool is still writing it.
pub(crate) struct Rows<R> {
    input: R,
    buf: Vec<u8>,
    line: usize,
    /// The time of the last row read that has one. It starts at the session's start.
    clock: Option<DateTime<FixedOffset>>,
    /// The day asked for: from the first time to before the second.
    day: (DateTime<FixedOffset>, DateTime<FixedOffset>),
    seen: Seen,
    /// Rows not read since the last row before the day. Their own time is not read, so
    /// the next row with a time says whether they may be on the day.
    undated_skipped: usize,
}

impl<R: std::io::BufRead> Rows<R> {
    pub(crate) fn next(&mut self) -> Result<Option<Row>, String> {
        loop {
            if !line(&mut self.input, &mut self.buf)? {
                // No later row says when these were written, so they may be on the day.
                self.seen.skipped += std::mem::take(&mut self.undated_skipped);
                return Ok(None);
            }
            self.line += 1;
            let (start, end) = self.day;
            let value = match self.buf.as_slice() {
                // `line` hands back nothing of a row over its limit.
                [] => None,
                blank if blank.trim_ascii().is_empty() => continue,
                row => serde_json::from_slice::<serde_json::Value>(row).ok(),
            };
            let Some(value) = value else {
                match self.clock {
                    Some(before) if before >= end => {}
                    Some(before) if before >= start => self.seen.skipped += 1,
                    _ => self.undated_skipped += 1,
                }
                continue;
            };
            let at = value["timestamp"]
                .as_str()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok());
            if let Some(at) = at {
                let at = at.with_timezone(&kst());
                self.clock = Some(at);
                // Rows not read between a row before the day and this one.
                let waiting = std::mem::take(&mut self.undated_skipped);
                if at >= start {
                    self.seen.skipped += waiting;
                }
            }
            let on_day = self.clock.is_none_or(|t| t >= start && t < end);
            self.seen.touched |= on_day;
            return Ok(Some(Row {
                line: self.line,
                at: self.clock,
                on_day,
                value,
            }));
        }
    }
}

/// The messages of one session, each once, with the index of the file it is in.
/// Copies of a message must agree that it is a person's, and copies of a picked answer
/// on the answer; the first copy with text gives the text and the place. `disputed`
/// says some copies did not agree. Typed text they do not agree on is kept as an
/// instruction; a picked answer is left out.
fn merge(files: Vec<Vec<Item>>) -> (Vec<(usize, Item)>, bool) {
    let mut messages: Vec<(usize, Item)> = Vec::new();
    let mut known: HashMap<String, usize> = HashMap::new();
    let mut disputed = false;
    for (file, items) in files.into_iter().enumerate() {
        for item in items {
            let first = item.key.as_ref().and_then(|key| known.get(key).copied());
            let Some(first) = first else {
                if let Some(key) = &item.key {
                    known.insert(key.clone(), messages.len());
                }
                messages.push((file, item));
                continue;
            };
            let (kept_file, kept) = &mut messages[first];
            let roles = [kept.role, item.role];
            let picked = roles.contains(&Role::Picked);
            // Two copies of a picked answer that hold different answers do not agree
            // either, though both say a person picked.
            let disagree = if roles == [Role::Picked; 2] {
                kept.text != item.text || kept.answers != item.answers
            } else {
                kept.role != item.role && (picked || roles.contains(&Role::Human))
            };
            disputed |= disagree;
            let fills = kept.text.trim().is_empty() && !item.text.trim().is_empty();
            let role = match (disagree, fills) {
                (true, _) => Role::Delegated,
                (false, true) => item.role,
                (false, false) => kept.role,
            };
            if fills {
                *kept_file = file;
                *kept = item;
            }
            kept.role = role;
            if disagree && picked {
                kept.text.clear();
            }
        }
    }
    messages.retain(|(_, item)| !item.text.trim().is_empty());
    (messages, disputed)
}

/// A message of the day: the index of its file, the message and its time.
type Dated = (usize, Item, DateTime<FixedOffset>);

/// What an agent run keeps of a day: its last answer, the latest by time. Where the
/// tool wrote that answer as several rows, the rows of the day are joined in order at
/// the place of the first.
fn last_answer(day: Vec<Dated>) -> Option<Dated> {
    let mut answers: Vec<Dated> = day
        .into_iter()
        .filter(|(_, m, _)| m.role == Role::Ai)
        .collect();
    // Of answers at one time, the later in the files is the last.
    let last = (0..answers.len()).max_by_key(|i| answers[*i].2)?;
    let group = answers[last].1.group.clone();
    let mut index = 0;
    answers.retain(|(_, m, _)| {
        index += 1;
        index - 1 == last || (group.is_some() && m.group == group)
    });
    answers.sort_by_key(|(_, _, at)| *at);
    let mut parts = answers.into_iter();
    let (file, mut first, at) = parts.next()?;
    for (_, part, _) in parts {
        first.text.push_str("\n\n");
        first.text.push_str(&part.text);
    }
    Some((file, first, at))
}

/// The dialogue of `project` on `date` (Korean time) from the originals in `vault`.
pub fn day_transcript(
    vault: &Path,
    project: &str,
    date: NaiveDate,
) -> Result<DayTranscript, String> {
    let (dir, info) = project_dir(vault, project)?;
    let start = kst()
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight"))
        .single()
        .expect("fixed offset");
    let end = start + chrono::Duration::days(1);
    let index = dir.join("index.jsonl");
    if linked(&index)? {
        return Err(LINKED.into());
    }
    let index =
        fs::read_to_string(index).map_err(|e| format!("보관 목록을 읽지 못했습니다: {e}"))?;
    let mut entries = Vec::new();
    for row in index.lines().filter(|l| !l.trim().is_empty()) {
        entries.push(
            serde_json::from_str::<Entry>(row).map_err(|e| format!("보관 목록 형식 오류: {e}"))?,
        );
    }
    let latest_archived = entries.iter().map(|e| e.archived_at.clone()).max();
    let mut sessions: Vec<Vec<&Entry>> = Vec::new();
    let mut known = HashMap::new();
    for entry in entries.iter().filter(|e| is_dialogue(e)) {
        let at = *known.entry(session_key(entry)).or_insert(sessions.len());
        if at == sessions.len() {
            sessions.push(Vec::new());
        }
        sessions[at].push(entry);
    }
    let mut sources = Vec::new();
    let mut blocks = Vec::new();
    let mut warnings = Vec::new();
    for mut files in sessions {
        // The index alone rules out files that started after the day or last changed
        // before it. A missing start time rules nothing out. Once one file of a session
        // is read, all are: who typed into the session is decided over the whole of it.
        let on_day = |entry: &&Entry| {
            let started = DateTime::parse_from_rfc3339(&entry.started).ok();
            let modified = DateTime::from_timestamp_nanos(entry.source_modified as i64);
            !started.is_some_and(|s| s >= end) && modified >= start
        };
        if !files.iter().any(on_day) {
            continue;
        }
        files.sort_by_key(|e| (&e.started, &e.file));
        let (mut by_program, mut unmarked, mut unnamed, mut plain, mut older) =
            (false, false, 0, 0, 0);
        let (mut touched, mut skipped, mut unread) = (false, 0, false);
        let mut items = Vec::new();
        for entry in &files {
            let parsed = read_file(&dir, entry, (start, end)).unwrap_or_else(|e| {
                warnings.push(format!("{}: 읽지 못했습니다 ({e})", entry.file));
                unread = true;
                Parsed::default()
            });
            by_program |= parsed.by_program;
            unmarked |= parsed.unmarked;
            unnamed += parsed.unnamed;
            plain += parsed.plain;
            older += parsed.older;
            touched |= parsed.seen.touched || parsed.seen.skipped > 0;
            skipped += parsed.seen.skipped;
            items.push(parsed.items);
        }
        let (messages, disputed) = merge(items);
        let typed = messages
            .iter()
            .any(|(_, m)| matches!(m.role, Role::Human | Role::Picked));
        let agent_run = by_program || !typed;
        let undated = messages.iter().filter(|(_, m)| m.at.is_none()).count();
        let mut kept: Vec<Dated> = messages
            .into_iter()
            .filter_map(|(file, m)| {
                let at = m.at.filter(|t| *t >= start && *t < end)?;
                Some((file, m, at))
            })
            .collect();
        if agent_run {
            kept = last_answer(kept).into_iter().collect();
        }
        let mut doubts: Vec<String> = Vec::new();
        // What the file that could not be read says about the day is not known, so
        // this is said on every day the index lets the session be on.
        if unread && typed {
            doubts.push(
                "이 세션의 파일 일부를 읽지 못했습니다. 사본이 모두 사람 글이라고 하는지 확인할 수 없어, 사람 표시가 있는 글도 사람 글로 세지 않았습니다.".into(),
            );
        }
        // A day the session wrote no row on says nothing about the session. A day it
        // did keeps the doubts, even when no block of the session is left.
        if touched {
            if disputed {
                doubts.push(
                    "같은 글의 사본끼리 사람 표시나 고른 답이 달라 그 글을 사람 글로 세지 않았습니다.".into(),
                );
            }
            if unnamed > 0 {
                doubts.push(format!(
                    "어느 세션의 것인지 적히지 않은 행 {unnamed}개를 뺐습니다."
                ));
            }
            if undated > 0 {
                doubts.push(format!("시각을 알 수 없는 글 {undated}개를 뺐습니다."));
            }
            if skipped > 0 {
                doubts.push(format!(
                    "8 MiB를 넘거나 JSON이 아니어서 읽지 않은 행이 {skipped}개 있습니다. 그 행의 글이 빠졌을 수 있습니다."
                ));
            }
            if files.iter().any(|e| e.session != session_id(e)) {
                doubts.push(
                    "보관 목록의 세션 ID가 파일 이름의 세션 ID와 다릅니다. 파일 이름의 ID가 적힌 행만 읽었습니다."
                        .into(),
                );
            }
            if by_program && typed {
                doubts.push(
                    "프로그램이 시작한 세션에 사람 표시가 있는 글이 있습니다. 에이전트 실행으로 셌습니다."
                        .into(),
                );
            }
            if !by_program && !typed && unmarked {
                doubts.push(
                    "사람이 연 것으로 보이는 세션인데 사람이 쓴 글의 표시(Codex는 client_id, Claude Code는 origin)가 없어 에이전트 실행으로 셌습니다."
                        .into(),
                );
            } else if plain > 0 {
                doubts.push(format!(
                    "보낸 쪽 표시(origin)가 없는 일반 글 {plain}개를 뺐습니다. 사람이 쓴 글일 수 있습니다."
                ));
            }
            if older > 0 {
                doubts.push(format!(
                    "예전 Codex 형식(user_message)으로만 적힌 사용자 글 {older}개를 읽지 못했습니다. 사람이 쓴 글일 수 있습니다."
                ));
            }
        }
        let first = files[0];
        // A session with no block left has no number, so its file names it.
        let name = if kept.is_empty() {
            first.file.clone()
        } else {
            format!("S{}", sources.len() + 1)
        };
        warnings.extend(doubts.into_iter().map(|doubt| format!("{name}: {doubt}")));
        if kept.is_empty() {
            continue;
        }
        let source = sources.len();
        sources.push(Source {
            tag: name,
            files: files.iter().map(|e| e.file.clone()).collect(),
            tool: first.tool.clone(),
            client: first.client.clone(),
            session: session_id(first).to_owned(),
            agent_run,
        });
        for (file, message, at) in kept {
            let speaker = match message.role {
                _ if agent_run => Speaker::AgentRun,
                // No copy in the file that could not be read was seen to agree.
                Role::Human | Role::Picked if unread => Speaker::Delegated,
                Role::Human | Role::Picked => Speaker::Human,
                Role::Ai => Speaker::Ai,
                Role::Check => Speaker::Check,
                Role::Delegated => Speaker::Delegated,
            };
            let text = plain_lines(&message.text);
            let (text, clipped) = clip(without_blank_edges(&text), speaker.cap());
            blocks.push(Block {
                number: 0,
                speaker,
                picked: (speaker == Speaker::Human && message.role == Role::Picked)
                    .then_some(message.answers),
                source,
                file,
                line: message.line,
                at,
                text,
                clipped,
            });
        }
    }
    blocks.sort_by_key(|b| (b.at, b.source, b.file, b.line));
    for (i, block) in blocks.iter_mut().enumerate() {
        block.number = i + 1;
    }
    Ok(DayTranscript {
        project_id: info.project_id,
        title: info.title,
        repo_key: info.repo_key,
        folder: dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        date,
        sources,
        blocks,
        warnings,
        latest_archived,
    })
}

/// Whether a line of a block's text starts the way a block heading does: `[12] 09:20 `.
fn heading_like(line: &str) -> bool {
    let Some((number, rest)) = line
        .trim_start()
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("] "))
    else {
        return false;
    };
    let time = rest.as_bytes();
    !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
        && time.len() > 5
        && time[2] == b':'
        && time[5] == b' '
        && [0, 1, 3, 4].iter().all(|i| time[*i].is_ascii_digit())
}

impl DayTranscript {
    /// The text a model reads. Each block starts with `[number] time speaker · source`,
    /// and no other line starts with `[number] time `: the title and what the index says
    /// of a source are put on one line, and a line of text shaped like one is quoted.
    pub fn render(&self) -> String {
        let mut out = format!(
            "# 하루 대화록 · {} · {}\n\n## 출처\n\n",
            one_line(&self.title),
            self.date
        );
        for s in &self.sources {
            let kind = if s.agent_run {
                "에이전트 실행"
            } else {
                "사람이 연 세션"
            };
            let about = one_line(&format!("{} · {} · {}", s.tag, s.tool, s.client));
            writeln!(out, "{about} · {kind}").unwrap();
        }
        out.push_str("\n## 대화\n");
        for b in &self.blocks {
            let tag = &self.sources[b.source].tag;
            let last = if b.speaker == Speaker::AgentRun {
                " (그날 마지막 답)"
            } else {
                ""
            };
            writeln!(
                out,
                "\n[{}] {} {} · {tag}{last}",
                b.number,
                b.at.format("%H:%M"),
                b.speaker.label(),
            )
            .unwrap();
            // Only a block starts a line the way a heading does. A line of text that
            // looks like one, such as a quoted transcript, is marked as a quote.
            for line in b.text.split('\n') {
                if heading_like(line) {
                    out.push_str("> ");
                }
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    pub fn summary(&self) -> TranscriptSummary {
        let mut blocks = BTreeMap::new();
        let mut clipped = BTreeMap::new();
        for b in &self.blocks {
            *blocks.entry(b.speaker.label()).or_insert(0) += 1;
            if b.clipped {
                *clipped.entry(b.speaker.label()).or_insert(0) += 1;
            }
        }
        let rendered = self.render();
        TranscriptSummary {
            sessions: self.sources.iter().filter(|s| !s.agent_run).count(),
            agent_runs: self.sources.iter().filter(|s| s.agent_run).count(),
            blocks,
            chars: rendered.chars().count(),
            clipped,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSummary {
    /// Sessions a person opened.
    pub sessions: usize,
    pub agent_runs: usize,
    pub blocks: BTreeMap<&'static str, usize>,
    /// Characters of the rendered transcript.
    pub chars: usize,
    /// Blocks cut at their speaker's limit, by speaker.
    pub clipped: BTreeMap<&'static str, usize>,
}

/// The text without the blank lines before and after it. A person's text is shown as
/// written, so the indentation of its first line and the spaces that end its last stay.
/// Lines end in `\n` here (see `plain_lines`).
fn without_blank_edges(text: &str) -> &str {
    let Some(first) = text.find(|c: char| !c.is_whitespace()) else {
        return "";
    };
    let start = text[..first].rfind('\n').map_or(0, |at| at + 1);
    let text = &text[start..];
    let last = text.trim_end().len();
    let end = text[last..].find('\n').map_or(text.len(), |at| last + at);
    &text[..end]
}
