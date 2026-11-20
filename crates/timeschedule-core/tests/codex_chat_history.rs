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
        c.execute_batch("ALTER TABLE codex_chat_projects DROP COLUMN include_claude; DROP TABLE chat_experience_evidence; ALTER TABLE portfolio_editions DROP COLUMN carried_edit; PRAGMA user_version=12").unwrap();
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

// The original Claude CLI probe retained only these visible events at these physical
// lines. Use synthetic IDs, paths and metadata; the fixture contains no account data.
fn silver_ferry(path: &Path, repo: &Path, session: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = fs::File::create(path).unwrap();
    for n in 1..=59 {
        let (role, content) = match n {
            9 => (
                "user",
                json!("상태 이름으로 노란 나침반을 제안하고 이유를 설명해줘."),
            ),
            40 => (
                "assistant",
                json!([{"type":"text","text":"노란 나침반을 제안합니다."}]),
            ),
            51 => (
                "user",
                json!("수정 요청. 은빛 나루로 바꾸고 두 명령을 실행해줘."),
            ),
            54 | 55 => {
                let (key, cmd) = if n == 54 {
                    ("missing", "git rev-parse --verify refs/heads/missing")
                } else {
                    ("probe", "git rev-parse --is-inside-work-tree")
                };
                (
                    "assistant",
                    json!([{"type":"tool_use","id":key,"name":"Bash","input":{"command":format!("cd /example; {cmd}; echo \"exit=$?\"")}}]),
                )
            }
            56 | 57 => {
                let (key, output) = if n == 56 {
                    ("missing", "fatal: Needed a single revision\nexit=128")
                } else {
                    ("probe", "true\nexit=0")
                };
                (
                    "user",
                    json!([{"type":"tool_result","tool_use_id":key,"content":output,"is_error":false}]),
                )
            }
            59 => (
                "assistant",
                json!([{"type":"text","text":"이름을 은빛 나루로 바꿨습니다."}]),
            ),
            _ => {
                writeln!(file, "{}", json!({"type":"mode","sessionId":session})).unwrap();
                continue;
            }
        };
        writeln!(file, "{}", claude_row(session, repo, n, role, content)).unwrap();
    }
}
fn claude_row(session: &str, repo: &Path, n: usize, role: &str, content: Value) -> Value {
    json!({"uuid":format!("event-{n}"),"sessionId":session,"cwd":repo,"timestamp":"2026-11-19T11:00:00Z","entrypoint":"cli","isSidechain":false,"type":role,"message":{"role":role,"content":content}})
}

#[test]
fn codex_and_claude_preserve_separate_sources_across_resume_reread_restart_and_portfolio() {
    use timeschedule_core::chat_experiences::Kind;
    let home = Temp::new();
    let repo = Temp::new();
    let db = home.0.join("schedule.db");
    let store = Storage::open(&db).unwrap();
    let project = store
        .save_project(ProjectInput {
            id: None,
            title: "테스트".into(),
            description: String::new(),
            repo_path: repo.0.to_str().unwrap().into(),
        })
        .unwrap()
        .id;
    // Deliberately use the same provider-issued session UUID for both sources.
    let (id, codex) = rollout(&home.0, &repo.0, "Codex Desktop");
    append(&codex, message(&id, "UserMessage", "초록 등대로 수정"));
    append(
        &codex,
        message(&id, "AgentMessage", "초록 등대로 바꿨습니다."),
    );
    append(
        &codex,
        json!({"type":"event_msg","payload":{"type":"item_completed","thread_id":id,"item":{"type":"CommandExecution","id":"tap","exit_code":0,"aggregated_output":"TAP version 13\n# tests 1\n# pass 1"}}}),
    );
    let claude = home.0.join("projects/example").join(format!("{id}.jsonl"));
    silver_ferry(&claude, &repo.0, &id);
    let other = Temp::new();
    let unrelated = Uuid::new_v4().to_string();
    silver_ferry(
        &home
            .0
            .join("projects/elsewhere")
            .join(format!("{unrelated}.jsonl")),
        &other.0,
        &unrelated,
    );
    let scan = |s: &Storage, force| s.scan_local_chats(&home.0, Some(&home.0), force).unwrap();
    store.set_capture_sources(&project, true, false).unwrap();
    assert_eq!(scan(&store, false).saved, 1);
    let old = store.reviewable_chats(&project).unwrap()[0].id.clone();
    store.set_chat_capture(&project, true).unwrap();
    assert_eq!(scan(&store, false).saved, 1);
    let choices = store.reviewable_chats(&project).unwrap();
    assert_eq!(choices.len(), 2);
    let chat = choices
        .iter()
        .find(|c| c.title.starts_with("Claude Code"))
        .unwrap()
        .id
        .clone();
    let original = store.get_entry(&old).unwrap().source_body;
    for block in [
        "사용자 · 원문 2행\n파란 우산",
        "사용자 · 원문 3행\n초록 등대로 수정",
        "AI · 원문 4행\n초록 등대로 바꿨습니다.",
        "검사 출력 · 원문 5행\n종료 코드 0\nTAP version 13",
    ] {
        assert_eq!(original.matches(block).count(), 1);
    }
    let before = store.get_entry(&chat).unwrap().source_body;
    for block in [
        "사용자 · 원문 9행\n상태 이름으로 노란 나침반",
        "AI · 원문 40행\n노란 나침반",
        "사용자 · 원문 51행\n수정 요청. 은빛 나루",
        "검사 출력 · 원문 56행\n종료 코드 128\nfatal: Needed a single revision\nexit=128",
        "검사 출력 · 원문 57행\n종료 코드 0\ntrue\nexit=0",
        "AI · 원문 59행\n이름을 은빛 나루",
    ] {
        assert_eq!(before.matches(block).count(), 1, "{block}");
    }
    // A replayed event, a non-shell result and synthetic context are not new turns.
    append(
        &claude,
        claude_row(
            &id,
            &repo.0,
            40,
            "assistant",
            json!([{"type":"text","text":"노란 나침반을 제안합니다."}]),
        ),
    );
    append(
        &claude,
        claude_row(
            &id,
            &repo.0,
            61,
            "user",
            json!([{"type":"tool_result","tool_use_id":"unrelated","content":"PRIVATE"}]),
        ),
    );
    let mut meta = claude_row(&id, &repo.0, 62, "user", json!("PRIVATE"));
    meta["isMeta"] = json!(true);
    append(&claude, meta);
    for _ in 63..67 {
        append(&claude, json!({"type":"mode"}));
    }
    append(
        &claude,
        claude_row(
            &id,
            &repo.0,
            67,
            "user",
            json!("재개 확인. 이름과 실제 출력을 알려줘."),
        ),
    );
    for _ in 68..72 {
        append(&claude, json!({"type":"mode"}));
    }
    append(
        &claude,
        claude_row(
            &id,
            &repo.0,
            72,
            "assistant",
            json!([{"type":"text","text":"은빛 나루, true입니다."}]),
        ),
    );
    drop(store);
    let store = Storage::open(&db).unwrap();
    assert_eq!(scan(&store, false).saved, 1);
    let resumed = store.get_entry(&chat).unwrap().source_body;
    assert!(resumed.starts_with(&before));
    assert!(!resumed.contains("PRIVATE"));
    assert_eq!(resumed.matches("재개 확인.").count(), 1);
    assert!(resumed.contains("AI · 원문 72행\n은빛 나루, true입니다."));
    assert_eq!(resumed.matches("\n## ").count(), 8);
    for force in [true, false] {
        let r = scan(&store, force);
        assert_eq!((r.saved, r.unchanged, r.failed), (0, 2, 0));
    }
    assert_eq!(store.get_entry(&old).unwrap().source_body, original);
    assert_eq!(store.get_entry(&chat).unwrap().source_body, resumed);
    assert_eq!(store.reviewable_chats(&project).unwrap().len(), 2);

    // Exercise the same B/C entry points as the app, with deterministic model replies.
    let review=store.review_chat_with_inference(&project,&chat,|request|{
        let prompt=request["messages"][0]["content"].as_str().unwrap();
        assert!(prompt.contains("4 (검사 출력) 종료 코드 128"));
        assert!(prompt.contains("5 (검사 출력) 종료 코드 0\ntrue"));
        Ok(json!({"design": null,"troubleshooting":{"title":"Git 상태 검사","problem":4,"reason":0,"solution":6,"test":5,"limit":0}}).to_string())
    }).unwrap();
    assert_eq!(store.portfolio_panel(&project).unwrap().experiences, 0);
    assert_eq!(
        store
            .confirm_chat_candidates(&review, &[Kind::Troubleshooting])
            .unwrap()
            .added,
        1
    );
    assert_eq!(
        store
            .confirm_chat_candidates(&review, &[Kind::Troubleshooting])
            .unwrap()
            .kept,
        1
    );
    let entry = store
        .portfolio_panel(&project)
        .unwrap()
        .choices
        .remove(0)
        .id;
    let selection = std::collections::BTreeSet::from([entry.clone()]);
    store.save_portfolio_answer(&project, None, "").unwrap();
    let file = store
        .export_portfolio_with_writer(&project, &home.0, Some(&selection), |prompt| {
            assert!(prompt.contains("fatal: Needed a single revision"));
            Ok(json!([{"id":entry,"paragraphs":["상태 확인 결과는 true였다."]}]).to_string())
        })
        .unwrap();
    let evidence = fs::read_to_string(file.with_file_name("evidence.md")).unwrap();
    assert!(evidence.contains("Claude Code 채팅 「") && evidence.contains("원문 57행"));
    fs::write(&file, "사용자가 고친 문장").unwrap();
    drop(store);
    let store = Storage::open(&db).unwrap();
    assert_eq!(scan(&store, true).unchanged, 2);
    let reused = store
        .export_portfolio_with_writer(&project, &home.0, Some(&selection), |_| {
            panic!("same inputs")
        })
        .unwrap();
    assert_eq!(file, reused);
    assert_eq!(fs::read_to_string(file).unwrap(), "사용자가 고친 문장");
    assert_eq!(
        store.portfolio_panel(&project).unwrap().answer.as_deref(),
        Some("")
    );
}
