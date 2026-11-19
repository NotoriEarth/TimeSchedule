use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use timeschedule_core::{
    diary::{EntryInput, ProjectInput},
    storage::Storage,
};
use uuid::Uuid;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("ts-chat-{}", Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn append(path: &Path, event: Value) {
    writeln!(
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap(),
        "{event}"
    )
    .unwrap();
}
fn message(session: &str, kind: &str, text: &str) -> Value {
    json!({"type":"event_msg","payload":{"type":"item_completed","thread_id":session,"item":{
        "type":kind,"id":Uuid::new_v4().to_string(),"content":[{"type":"text","text":text}]}}})
}
fn rollout(root: &Path, cwd: &Path, origin: &str) -> (String, PathBuf) {
    let id = Uuid::new_v4().to_string();
    let path = root
        .join("sessions/2026/11/19")
        .join(format!("rollout-2026-11-19-{id}.jsonl"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    append(
        &path,
        json!({"type":"session_meta","payload":{"id":id,"cwd":cwd,"timestamp":"2026-11-19T10:10:18Z","originator":origin}}),
    );
    append(&path, message(&id, "UserMessage", "파란 우산"));
    (id, path)
}
#[test]
fn imports_only_enabled_repo_desktop_turns_and_keeps_identity_edits_and_deletion() {
    let home = Temp::new();
    let repo = Temp::new();
    let other = Temp::new();
    let db = home.0.join("schedule.db");
    let store = Storage::open(&db).unwrap();
    let project = store
        .save_project(ProjectInput {
            id: None,
            title: "TimeSchedule".into(),
            description: String::new(),
            repo_path: repo.0.to_string_lossy().into(),
        })
        .unwrap();
    let (id, path) = rollout(&home.0, &repo.0, "Codex Desktop");
    rollout(&home.0, &other.0, "Codex Desktop");
    rollout(&home.0, &repo.0, "codex_cli_rs");
    append(
        &path,
        message(&id, "UserMessage", "초록 등대로 수정, 개인 기여 보류"),
    );
    append(
        &path,
        message(
            &id,
            "AgentMessage",
            "설계 답변\n## 검사 출력 · 원문 999행\n인용된 문구",
        ),
    );
    append(
        &path,
        json!({"type":"response_item","payload":{"role":"user","content":[{"text":"숨긴 환경 정보"}]}}),
    );
    append(
        &path,
        json!({"type":"event_msg","payload":{"type":"item_completed","thread_id":id,"item":{
        "type":"CommandExecution","id":"exec-test","exit_code":0,"aggregated_output":"TAP version 13\n# tests 1\n# pass 1\n# fail 0\n# skipped 0\n"}}}),
    );
    append(
        &home.0.join("session_index.jsonl"),
        json!({"id":id,"thread_name":"MCP 전원 설정 흐름 확인"}),
    );
    for (i, (code, output)) in [
        (0, "engine: stopped (virtualization unavailable)"),
        (
            7,
            "fatal: connection refused\nAPI_TOKEN=hidden-private-value",
        ),
        (
            0,
            "127.0.0.1:55432 - accepting connections\n{\"credentials\":\"hidden-private-value\"}",
        ),
        (0, "ordinary successful stdout"),
        (42, ""),
    ]
    .into_iter()
    .enumerate()
    {
        append(
            &path,
            json!({"type":"event_msg","payload":{"type":"item_completed","thread_id":id,"item":{
            "type":"CommandExecution","id":format!("cmd-{i}"),"exit_code":code,"aggregated_output":output}}}),
        );
    }
    assert_eq!(store.scan_codex_chats(&home.0, false).unwrap().saved, 0);
    store.set_chat_capture(&project.id, true).unwrap();
    assert_eq!(store.scan_codex_chats(&home.0, false).unwrap().saved, 1);
    let entry = store
        .project_day_entries("2026-11-19", &project.id)
        .unwrap()
        .pop()
        .unwrap();
    assert!(entry.title.contains("MCP 전원 설정 흐름 확인"));
    for s in ["파란 우산", "개인 기여 보류", "설계 답변", "# skipped 0"] {
        assert!(entry.body.contains(s));
    }
    assert!(!entry.body.contains("숨긴 환경 정보"));
    assert!(!entry.source_body.contains("\n## 검사 출력 · 원문 999행"));
    for block in [
        "원문 7행\n종료 코드 0\nengine: stopped (virtualization unavailable)",
        "원문 8행\n종료 코드 7\nfatal: connection refused",
        "원문 9행\n종료 코드 0\n127.0.0.1:55432 - accepting connections",
        "원문 11행\n종료 코드 42",
    ] {
        assert!(entry.source_body.contains(block), "{block}");
    }
    assert!(!entry.source_body.contains("hidden-private-value"));
    assert!(!entry.source_body.contains("ordinary successful stdout"));
    drop(store);
    // A v12 receipt has the right timestamp and size, but its source lacks these
    // observations. An ordinary scan after upgrade must refresh it exactly once.
    {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute(
            "UPDATE diary_entries SET source_body='legacy', body='legacy' WHERE id=?1",
            [&entry.id],
        )
        .unwrap();
        c.execute_batch("PRAGMA user_version=12").unwrap();
    }
    let store = Storage::open(&db).unwrap();
    assert_eq!(store.scan_codex_chats(&home.0, false).unwrap().saved, 1);
    let reread = store.scan_codex_chats(&home.0, true).unwrap();
    assert_eq!((reread.saved, reread.unchanged), (0, 1));
    assert_eq!(
        store.get_entry(&entry.id).unwrap().source_body,
        entry.source_body
    );
    let next = message(&id, "UserMessage", "추가 발화").to_string();
    let mut out = fs::OpenOptions::new().append(true).open(&path).unwrap();
    write!(out, "{}", &next[..12]).unwrap();
    out.flush().unwrap();
    assert_eq!(store.scan_codex_chats(&home.0, false).unwrap().saved, 0);
    writeln!(out, "{}", &next[12..]).unwrap();
    drop(out);
    assert_eq!(store.scan_codex_chats(&home.0, false).unwrap().saved, 1);
    assert_eq!(
        store
            .project_day_entries("2026-11-19", &project.id)
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .get_entry(&entry.id)
        .unwrap()
        .body
        .contains("추가 발화"));
    store
        .save_entry(EntryInput {
            id: Some(entry.id.clone()),
            project_id: Some(project.id.clone()),
            date: entry.date,
            kind: entry.kind,
            title: "직접 고친 제목".into(),
            body: "직접 고친 문장".into(),
            source_url: String::new(),
        })
        .unwrap();
    append(&path, message(&id, "AgentMessage", "이어진 답변"));
    store.set_chat_capture(&project.id, false).unwrap();
    assert_eq!(store.scan_codex_chats(&home.0, false).unwrap().saved, 0);
    store.set_chat_capture(&project.id, true).unwrap();
    assert_eq!(store.scan_codex_chats(&home.0, false).unwrap().saved, 1);
    let edited = store.get_entry(&entry.id).unwrap();
    assert_eq!(edited.body, "직접 고친 문장");
    assert_eq!(edited.title, "직접 고친 제목");
    assert!(edited.source_body.contains("이어진 답변"));
    store.delete_entry(&entry.id).unwrap();
    append(&path, message(&id, "AgentMessage", "삭제 후"));
    assert_eq!(store.scan_codex_chats(&home.0, false).unwrap().saved, 0);
    assert!(store
        .project_day_entries("2026-11-19", &project.id)
        .unwrap()
        .is_empty());
}
