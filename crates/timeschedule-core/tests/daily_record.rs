//! The day transcript, read from a made-up originals folder in the layout
//! `chat_originals` writes.
use chrono::NaiveDate;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use timeschedule_core::daily_record::{day_transcript, DayTranscript, Speaker};

const PROJECT: &str = "11111111-2222-4333-8444-555555555555";

struct Vault(PathBuf);
impl Drop for Vault {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Kept {
    file: &'static str,
    tool: &'static str,
    /// The session ID in the index. Rows that name no session get this one.
    session: &'static str,
    client: &'static str,
    started: &'static str,
    modified: &'static str,
    rows: Vec<Value>,
}

fn nanos(at: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(at)
        .unwrap()
        .timestamp_nanos_opt()
        .unwrap() as u64
}

fn vault(kept: &[Kept]) -> Vault {
    vault_titled(kept, "작업")
}

fn vault_titled(kept: &[Kept], title: &str) -> Vault {
    let root = std::env::temp_dir().join(format!("ts-daily-{}", uuid::Uuid::new_v4()));
    let dir = root.join("originals").join("c--work-0000000000000000");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("project.json"),
        json!({"schemaVersion":1,"projectId":PROJECT,"title":title,"repoKey":"c:/work"})
            .to_string(),
    )
    .unwrap();
    let mut index = String::new();
    for k in kept {
        let path = dir.join(k.file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text: String = k
            .rows
            .iter()
            .map(|r| format!("{}\n", stamped(r.clone(), k.tool, k.session)))
            .collect();
        std::fs::write(&path, zstd::encode_all(text.as_bytes(), 3).unwrap()).unwrap();
        index.push_str(
            &json!({
                "file": k.file, "tool": k.tool, "session": k.session, "cwd": "C:\\work",
                "client": k.client, "started": k.started, "source": "x",
                "sourceSize": text.len(), "sourceModified": nanos(k.modified),
                "archivedAt": "2026-11-25T00:00:00Z"
            })
            .to_string(),
        );
        index.push('\n');
    }
    std::fs::write(dir.join("index.jsonl"), index).unwrap();
    Vault(root)
}

/// The row as the tool would write it in `session`. A test that needs a row of another
/// session, or of none (`null`), sets the field itself.
fn stamped(mut row: Value, tool: &str, session: &str) -> Value {
    if tool != "codex" {
        if row.get("sessionId").is_none() {
            row["sessionId"] = json!(session);
        }
        return row;
    }
    let key = match row["type"].as_str() {
        Some("session_meta") => "id",
        Some("event_msg") if row["payload"]["type"] == "item_completed" => "thread_id",
        _ => return row,
    };
    if row["payload"].get(key).is_none() {
        row["payload"][key] = json!(session);
    }
    row
}

fn day(vault: &Path, date: &str) -> DayTranscript {
    day_transcript(
        vault,
        PROJECT,
        NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
    )
    .unwrap()
}

/// `(speaker, source tag, line, text)` of every block, in order.
fn seen(t: &DayTranscript) -> Vec<(Speaker, String, usize, String)> {
    t.blocks
        .iter()
        .map(|b| {
            (
                b.speaker,
                t.sources[b.source].tag.clone(),
                b.line,
                b.text.clone(),
            )
        })
        .collect()
}

fn codex_meta(originator: &str, source: Value) -> Value {
    json!({"timestamp":"2026-11-24T00:00:00Z","type":"session_meta","payload":{
        "cwd":"C:\\work","originator":originator,"source":source,"thread_source":"user"}})
}
fn codex_item(at: &str, item: Value) -> Value {
    json!({"timestamp":at,"type":"event_msg","payload":{"type":"item_completed","item":item}})
}
fn codex_user(at: &str, id: &str, client: bool, text: &str) -> Value {
    let mut item = json!({"type":"UserMessage","id":id,"content":[{"type":"text","text":text}]});
    if client {
        item["client_id"] = json!("6f1c2d3e-0000-4000-8000-000000000000");
    }
    codex_item(at, item)
}
fn codex_ai(at: &str, id: &str, text: &str) -> Value {
    codex_item(
        at,
        json!({"type":"AgentMessage","id":id,"content":[{"type":"text","text":text}]}),
    )
}

#[test]
fn days_split_at_korean_midnight_and_blocks_point_at_their_rows() {
    let v = vault(&[Kept {
        file: "codex/2026/11/24/rollout-a.jsonl.zst",
        tool: "codex",
        session: "a",
        client: "Codex Desktop",
        started: "2026-11-24T00:00:00Z",
        modified: "2026-11-24T16:00:00Z",
        rows: vec![
            codex_meta("Codex Desktop", json!("vscode")),
            // 23:59 and 00:01 Korean time.
            codex_user("2026-11-24T14:59:00Z", "u1", true, "늦은 밤 요청"),
            codex_ai("2026-11-24T15:01:00Z", "a1", "자정 뒤 답"),
            // The same item written again is one block.
            codex_ai("2026-11-24T15:01:00Z", "a1", "자정 뒤 답"),
        ],
    }]);
    let before = day(&v.0, "2026-11-24");
    assert_eq!(
        seen(&before),
        [(Speaker::Human, "S1".into(), 2, "늦은 밤 요청".into())]
    );
    let after = day(&v.0, "2026-11-25");
    assert_eq!(
        seen(&after),
        [(Speaker::Ai, "S1".into(), 3, "자정 뒤 답".into())]
    );
    assert_eq!(
        after.sources[0].files,
        ["codex/2026/11/24/rollout-a.jsonl.zst"]
    );
    assert!(after.render().contains("[1] 00:01 AI · S1\n자정 뒤 답\n"));
}

#[test]
fn codex_messages_are_a_persons_only_with_a_client_id() {
    let v = vault(&[
        Kept {
            file: "codex/2026/11/24/rollout-app.jsonl.zst",
            tool: "codex",
            session: "app",
            client: "Codex Desktop",
            started: "2026-11-24T01:00:00Z",
            modified: "2026-11-24T03:00:00Z",
            rows: vec![
                codex_meta("Codex Desktop", json!("vscode")),
                codex_user("2026-11-24T01:00:00Z", "u1", true, "사람이 쓴 요청"),
                codex_user("2026-11-24T01:30:00Z", "u2", false, "자동 실행 알림"),
                codex_ai("2026-11-24T02:00:00Z", "a1", "답"),
                codex_item(
                    "2026-11-24T02:10:00Z",
                    json!({"type":"CommandExecution","id":"c1","exit_code":0,"aggregated_output":"ok"}),
                ),
                codex_item(
                    "2026-11-24T02:20:00Z",
                    json!({"type":"CommandExecution","id":"c2","exit_code":1,"aggregated_output":"error: build failed\nnoise"}),
                ),
            ],
        },
        Kept {
            file: "codex/2026/11/24/rollout-exec.jsonl.zst",
            tool: "codex",
            session: "exec",
            client: "codex_exec",
            started: "2026-11-24T04:00:00Z",
            modified: "2026-11-24T05:00:00Z",
            rows: vec![
                codex_meta("codex_exec", json!("exec")),
                codex_user("2026-11-24T04:00:00Z", "u1", false, "검토해 줘"),
                codex_ai("2026-11-24T04:10:00Z", "a1", "중간 보고"),
                codex_ai("2026-11-24T04:20:00Z", "a2", "검토 결과"),
            ],
        },
        Kept {
            file: "codex/2026/11/24/rollout-untyped.jsonl.zst",
            tool: "codex",
            session: "untyped",
            client: "Codex Desktop",
            started: "2026-11-24T06:00:00Z",
            modified: "2026-11-24T07:00:00Z",
            rows: vec![
                codex_meta("Codex Desktop", json!("vscode")),
                codex_user("2026-11-24T06:00:00Z", "u1", false, "표시 없는 요청"),
                codex_ai("2026-11-24T06:10:00Z", "a1", "그 답"),
            ],
        },
    ]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        seen(&t),
        [
            (Speaker::Human, "S1".into(), 2, "사람이 쓴 요청".into()),
            (Speaker::Delegated, "S1".into(), 3, "자동 실행 알림".into()),
            (Speaker::Ai, "S1".into(), 4, "답".into()),
            (
                Speaker::Check,
                "S1".into(),
                6,
                "종료 코드 1\nerror: build failed".into()
            ),
            (Speaker::AgentRun, "S2".into(), 4, "검토 결과".into()),
            (Speaker::AgentRun, "S3".into(), 3, "그 답".into()),
        ]
    );
    assert_eq!(
        t.sources.iter().map(|s| s.agent_run).collect::<Vec<_>>(),
        [false, true, true]
    );
    // A session that looks opened from the app but has no marked message is reported.
    assert_eq!(t.warnings.len(), 1);
    assert!(t.warnings[0].starts_with("S3: "));
    let summary = t.summary();
    assert_eq!((summary.sessions, summary.agent_runs), (1, 2));
}

fn claude_row(kind: &str, at: &str, extra: Value) -> Value {
    let mut row = json!({"type":kind,"timestamp":at,"cwd":"C:\\work"});
    for (k, v) in extra.as_object().unwrap() {
        row[k] = v.clone();
    }
    row
}

#[test]
fn claude_rows_are_attributed_by_their_origin() {
    let long = "가".repeat(5000);
    let v = vault(&[
        Kept {
            file: "claude/C--work/c.jsonl.zst",
            tool: "claude",
            session: "c",
            client: "claude-desktop",
            started: "2026-11-24T00:00:00Z",
            modified: "2026-11-24T10:00:00Z",
            rows: vec![
                claude_row("queue-operation", "2026-11-24T00:00:00Z", json!({})),
                claude_row(
                    "user",
                    "2026-11-24T00:01:00Z",
                    json!({
                    "origin":{"kind":"human"},"turnOrigin":"human",
                    "message":{"role":"user","content":[
                        {"type":"text","text":"<system-reminder>앱이 넣은 글</system-reminder>"},
                        {"type":"text","text":"설계해 줘"}]}}),
                ),
                claude_row(
                    "user",
                    "2026-11-24T00:02:00Z",
                    json!({
                    "turnOrigin":"host_synthetic","message":{"role":"user","content":"지".repeat(600)}}),
                ),
                claude_row(
                    "user",
                    "2026-11-24T00:03:00Z",
                    json!({
                    "origin":{"kind":"task-notification"},"turnOrigin":"task_notification",
                    "message":{"role":"user","content":"<task-notification>끝남</task-notification>"}}),
                ),
                claude_row(
                    "assistant",
                    "2026-11-24T00:04:00Z",
                    json!({"message":{"role":"assistant","content":[
                    {"type":"thinking","thinking":""},
                    {"type":"text","text":long},
                    {"type":"tool_use","id":"t1","name":"AskUserQuestion","input":{}},
                    {"type":"tool_use","id":"t2","name":"Bash","input":{"command":"cargo test"}},
                    {"type":"tool_use","id":"t3","name":"Read","input":{}}]}}),
                ),
                claude_row(
                    "user",
                    "2026-11-24T00:05:00Z",
                    json!({
                    "toolUseResult":{"questions":[],"answers":{"어느 안":"A안"}},"message":{"role":"user","content":[
                        {"type":"tool_result","tool_use_id":"t1","content":"A안을 고름"},
                        {"type":"tool_result","tool_use_id":"t2","is_error":true,"content":"Exit code 101\nerror: test failed"},
                        {"type":"tool_result","tool_use_id":"t3","content":"파일 내용"}]}}),
                ),
                claude_row(
                    "assistant",
                    "2026-11-24T00:06:00Z",
                    json!({"isSidechain":true,
                    "message":{"role":"assistant","content":[{"type":"text","text":"곁가지"}]}}),
                ),
                claude_row(
                    "user",
                    "2026-11-24T00:07:00Z",
                    json!({"isMeta":true,
                    "message":{"role":"user","content":"메타"}}),
                ),
            ],
        },
        Kept {
            file: "claude/C--work/c/subagents/agent-1.jsonl.zst",
            tool: "claude",
            session: "c",
            client: "claude-desktop",
            started: "2026-11-24T00:00:00Z",
            modified: "2026-11-24T10:00:00Z",
            rows: vec![
                claude_row(
                    "user",
                    "2026-11-24T01:00:00Z",
                    json!({"isSidechain":true,
                    "message":{"role":"user","content":"하위 에이전트에게 준 지시"}}),
                ),
                claude_row(
                    "assistant",
                    "2026-11-24T01:01:00Z",
                    json!({"isSidechain":true,
                    "message":{"role":"assistant","content":[{"type":"text","text":"첫 답"}]}}),
                ),
                claude_row(
                    "assistant",
                    "2026-11-24T01:02:00Z",
                    json!({"isSidechain":true,
                    "message":{"role":"assistant","content":[{"type":"text","text":"마지막 답"}]}}),
                ),
            ],
        },
        // A tool-result file beside the session is in the index but is not dialogue.
        // Were it read, its row would be a person's.
        Kept {
            file: "claude/C--work/c/tool-results/x.jsonl.zst",
            tool: "claude",
            session: "c",
            client: "claude-desktop",
            started: "2026-11-24T00:00:00Z",
            modified: "2026-11-24T10:00:00Z",
            rows: vec![claude_row(
                "user",
                "2026-11-24T02:00:00Z",
                json!({"origin":{"kind":"human"},"turnOrigin":"human",
                    "message":{"role":"user","content":"도구 결과 파일의 글"}}),
            )],
        },
    ]);
    let t = day(&v.0, "2026-11-24");
    let blocks = seen(&t);
    let speakers: Vec<_> = blocks
        .iter()
        .map(|(s, tag, line, _)| (*s, tag.as_str(), *line))
        .collect();
    assert_eq!(
        speakers,
        [
            (Speaker::Human, "S1", 2),
            (Speaker::Delegated, "S1", 3),
            (Speaker::Ai, "S1", 5),
            (Speaker::Human, "S1", 6),
            (Speaker::Check, "S1", 6),
            (Speaker::AgentRun, "S2", 3),
        ]
    );
    assert_eq!(blocks[0].3, "설계해 줘");
    assert_eq!(blocks[1].3.chars().count(), 501);
    assert_eq!(blocks[2].3.chars().count(), 4001);
    assert!(t.blocks[2].clipped && !t.blocks[0].clipped);
    assert_eq!(
        blocks[3].3,
        "질문: 어느 안
고른 답: A안"
    );
    assert_eq!(blocks[4].3, "종료 코드 101\nerror: test failed");
    assert_eq!(blocks[5].3, "마지막 답");
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
}

#[test]
fn the_index_rules_out_files_outside_the_day() {
    let v = vault(&[Kept {
        file: "codex/2026/11/20/rollout-old.jsonl.zst",
        tool: "codex",
        session: "old",
        client: "Codex Desktop",
        started: "2026-11-20T00:00:00Z",
        modified: "2026-11-20T02:00:00Z",
        rows: vec![
            codex_meta("Codex Desktop", json!("vscode")),
            // A row dated on the day, in a file the index says last changed before it.
            codex_user("2026-11-24T01:00:00Z", "u1", true, "보이면 안 되는 글"),
        ],
    }]);
    assert!(day(&v.0, "2026-11-24").blocks.is_empty());
    assert_eq!(day(&v.0, "2026-11-20").blocks.len(), 0);
    let missing = day_transcript(
        &v.0,
        "00000000-0000-4000-8000-000000000000",
        NaiveDate::from_ymd_opt(2026, 11, 24).unwrap(),
    );
    assert!(missing.is_err());
}

fn codex_file(file: &'static str, session: &'static str, rows: Vec<Value>) -> Kept {
    Kept {
        file,
        tool: "codex",
        session,
        client: "Codex Desktop",
        started: "2026-11-24T00:00:00Z",
        modified: "2026-11-24T10:00:00Z",
        rows,
    }
}
fn claude_file(file: &'static str, rows: Vec<Value>) -> Kept {
    Kept {
        file,
        tool: "claude",
        session: "c",
        client: "claude-desktop",
        started: "2026-11-24T00:00:00Z",
        modified: "2026-11-24T10:00:00Z",
        rows,
    }
}
fn typed() -> Value {
    json!({"origin":{"kind":"human"},"turnOrigin":"human"})
}
fn claude_user(at: &str, marks: Value, text: &str) -> Value {
    let mut extra = marks;
    extra["message"] = json!({"role":"user","content":text});
    claude_row("user", at, extra)
}
fn claude_ai(at: &str, id: &str, blocks: Value) -> Value {
    claude_row(
        "assistant",
        at,
        json!({"message":{"role":"assistant","id":id,"content":blocks}}),
    )
}
fn text_block(text: &str) -> Value {
    json!({"type":"text","text":text})
}
fn without_time(mut row: Value) -> Value {
    row.as_object_mut().unwrap().remove("timestamp");
    row
}
fn speakers(t: &DayTranscript) -> Vec<(Speaker, String, String)> {
    seen(t)
        .into_iter()
        .map(|(speaker, tag, _, text)| (speaker, tag, text))
        .collect()
}

#[test]
fn a_codex_fork_does_not_repeat_what_its_parent_said() {
    // A thread the person forked in the app and then typed nothing into.
    let mut fork_meta = codex_meta("Codex Desktop", json!("vscode"));
    fork_meta["payload"]["forked_from_id"] = json!("parent");
    // The rows a fork copies from its parent name the parent.
    let mut parent_meta = codex_meta("Codex Desktop", json!("vscode"));
    parent_meta["payload"]["id"] = json!("parent");
    let mut copied_approval = codex_user("2026-11-24T01:00:00Z", "u1", true, "진행해");
    copied_approval["payload"]["thread_id"] = json!("parent");
    let mut copied_answer = codex_ai("2026-11-24T01:01:00Z", "a1", "진행합니다");
    copied_answer["payload"]["thread_id"] = json!("parent");
    let v = vault(&[
        codex_file(
            "codex/2026/11/24/rollout-1-parent.jsonl.zst",
            "parent",
            vec![
                codex_meta("Codex Desktop", json!("vscode")),
                codex_user("2026-11-24T01:00:00Z", "u1", true, "진행해"),
                codex_ai("2026-11-24T01:01:00Z", "a1", "진행합니다"),
            ],
        ),
        codex_file(
            "codex/2026/11/24/rollout-2-fork.jsonl.zst",
            "fork",
            vec![
                fork_meta,
                parent_meta,
                copied_approval,
                copied_answer,
                codex_ai("2026-11-24T02:01:00Z", "a2", "검토 결과"),
            ],
        ),
    ]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        speakers(&t),
        [
            (Speaker::Human, "S1".into(), "진행해".into()),
            (Speaker::Ai, "S1".into(), "진행합니다".into()),
            (Speaker::AgentRun, "S2".into(), "검토 결과".into()),
        ]
    );
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
}

#[test]
fn a_claude_row_is_a_persons_only_when_no_mark_says_otherwise() {
    let mut other_session = claude_user("2026-11-24T00:06:00Z", typed(), "다른 세션의 승인");
    other_session["sessionId"] = json!("another");
    let mut sidechain = typed();
    sidechain["isSidechain"] = json!(true);
    let v = vault(&[claude_file(
        "claude/C--work/c.jsonl.zst",
        vec![
            claude_user("2026-11-24T00:01:00Z", typed(), "사람의 요청"),
            claude_user(
                "2026-11-24T00:02:00Z",
                json!({"origin":{"kind":"human"}}),
                "turnOrigin 없는 사람 글",
            ),
            claude_user(
                "2026-11-24T00:03:00Z",
                json!({"origin":{"kind":"human"},"turnOrigin":"host_synthetic"}),
                "표시가 엇갈린 지시",
            ),
            claude_user(
                "2026-11-24T00:04:00Z",
                json!({"origin":{}}),
                "kind 없는 origin",
            ),
            // The app writes this row itself, without either mark.
            claude_user(
                "2026-11-24T00:05:00Z",
                json!({}),
                "[Request interrupted by user]",
            ),
            other_session,
            claude_user("2026-11-24T00:07:00Z", sidechain, "곁가지의 글"),
            claude_ai("2026-11-24T00:08:00Z", "m1", json!([text_block("답")])),
        ],
    )]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        speakers(&t),
        [
            (Speaker::Human, "S1".into(), "사람의 요청".into()),
            (
                Speaker::Human,
                "S1".into(),
                "turnOrigin 없는 사람 글".into()
            ),
            (Speaker::Delegated, "S1".into(), "표시가 엇갈린 지시".into()),
            (Speaker::Ai, "S1".into(), "답".into()),
        ]
    );
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
}

#[test]
fn a_claude_session_without_any_origin_mark_is_reported() {
    let v = vault(&[claude_file(
        "claude/C--work/c.jsonl.zst",
        vec![
            // A file from before Claude Code wrote who sent a message.
            claude_user("2026-11-24T00:01:00Z", json!({}), "표시 없는 요청"),
            claude_ai("2026-11-24T00:02:00Z", "m1", json!([text_block("답")])),
        ],
    )]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        speakers(&t),
        [(Speaker::AgentRun, "S1".into(), "답".into())]
    );
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
    assert!(t.warnings[0].starts_with("S1: "));
}

#[test]
fn a_failed_question_is_no_answer_and_text_beside_tool_results_is_kept() {
    let ask = |id: &str| json!({"type":"tool_use","id":id,"name":"AskUserQuestion","input":{}});
    let v = vault(&[claude_file(
        "claude/C--work/c.jsonl.zst",
        vec![
            claude_user("2026-11-24T00:01:00Z", typed(), "물어봐 줘"),
            claude_ai("2026-11-24T00:02:00Z", "m1", json!([ask("q1"), ask("q2")])),
            // The question failed: whatever the row holds, nobody answered.
            claude_row(
                "user",
                "2026-11-24T00:03:00Z",
                json!({"toolUseResult":{"questions":[],"answers":{"진행할까요":"승인"}},
                    "message":{"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"q1","is_error":true,"content":"승인함"}]}}),
            ),
            // A result whose row does not hold the picked answers is not a person's either.
            claude_row(
                "user",
                "2026-11-24T00:04:00Z",
                json!({"message":{"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"q2","content":"승인한 것으로 봄"}]}}),
            ),
            // A person's text in the same row as a tool result.
            claude_row(
                "user",
                "2026-11-24T00:05:00Z",
                json!({"origin":{"kind":"human"},"turnOrigin":"human","toolUseResult":{},
                    "message":{"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"x","content":"파일 내용"},
                    {"type":"text","text":"그만하고 B안으로"}]}}),
            ),
        ],
    )]);
    assert_eq!(
        speakers(&day(&v.0, "2026-11-24")),
        [
            (Speaker::Human, "S1".into(), "물어봐 줘".into()),
            (Speaker::Human, "S1".into(), "그만하고 B안으로".into()),
        ]
    );
}

#[test]
fn copies_of_one_codex_item_are_one_block_and_must_agree() {
    let v = vault(&[codex_file(
        "codex/2026/11/24/rollout-a.jsonl.zst",
        "a",
        vec![
            codex_meta("Codex Desktop", json!("vscode")),
            // An empty first copy does not hide the filled one.
            codex_user("2026-11-24T01:00:00Z", "u1", true, ""),
            codex_user("2026-11-24T01:00:01Z", "u1", true, "채워진 요청"),
            codex_ai("2026-11-24T01:01:00Z", "a1", "답"),
            // Copies that disagree about the person's mark are not a person's.
            codex_user("2026-11-24T01:02:00Z", "u2", false, "표시가 엇갈린 글"),
            codex_user("2026-11-24T01:02:01Z", "u2", true, "표시가 엇갈린 글"),
        ],
    )]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        seen(&t),
        [
            (Speaker::Human, "S1".into(), 3, "채워진 요청".into()),
            (Speaker::Ai, "S1".into(), 4, "답".into()),
            (
                Speaker::Delegated,
                "S1".into(),
                5,
                "표시가 엇갈린 글".into()
            ),
        ]
    );
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
    assert!(t.warnings[0].starts_with("S1: "));
}

#[test]
fn a_codex_item_that_names_no_session_is_left_out_and_reported() {
    let mut unnamed = codex_user(
        "2026-11-24T01:02:00Z",
        "u2",
        true,
        "어느 세션의 글인지 모름",
    );
    unnamed["payload"]["thread_id"] = Value::Null;
    let v = vault(&[codex_file(
        "codex/2026/11/24/rollout-a.jsonl.zst",
        "a",
        vec![
            codex_meta("Codex Desktop", json!("vscode")),
            codex_user("2026-11-24T01:00:00Z", "u1", true, "요청"),
            unnamed,
        ],
    )]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(speakers(&t), [(Speaker::Human, "S1".into(), "요청".into())]);
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
}

#[test]
fn a_session_nobody_typed_into_is_an_agent_run_in_either_tool() {
    let v = vault(&[
        // A Claude Code session another session started: an instruction, no person.
        claude_file(
            "claude/C--work/c.jsonl.zst",
            vec![
                claude_user(
                    "2026-11-24T00:01:00Z",
                    json!({"turnOrigin":"host_synthetic"}),
                    "이 작업을 해",
                ),
                claude_ai(
                    "2026-11-24T00:02:00Z",
                    "m1",
                    json!([text_block("중간 보고")]),
                ),
                // One answer written as two rows and three text blocks.
                claude_ai(
                    "2026-11-24T00:03:00Z",
                    "m2",
                    json!([text_block("결과 1"), text_block("결과 2")]),
                ),
                claude_ai("2026-11-24T00:03:01Z", "m2", json!([text_block("결과 3")])),
            ],
        ),
        claude_file(
            "claude/C--work/c/subagents/agent-1.jsonl.zst",
            vec![
                claude_row(
                    "user",
                    "2026-11-24T01:00:00Z",
                    json!({"isSidechain":true,"message":{"role":"user","content":"지시"}}),
                ),
                claude_row(
                    "assistant",
                    "2026-11-24T01:01:00Z",
                    json!({"isSidechain":true,"message":{"role":"assistant","id":"m9","content":[
                        text_block("앞 조각"), text_block("뒤 조각")]}}),
                ),
            ],
        ),
    ]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        seen(&t),
        [
            (
                Speaker::AgentRun,
                "S1".into(),
                3,
                "결과 1\n\n결과 2\n\n결과 3".into()
            ),
            (
                Speaker::AgentRun,
                "S2".into(),
                2,
                "앞 조각\n\n뒤 조각".into()
            ),
        ]
    );
    assert_eq!(
        t.sources.iter().map(|s| s.agent_run).collect::<Vec<_>>(),
        [true, true]
    );
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
}

#[test]
fn one_codex_session_in_several_files_is_one_source() {
    let whole = || {
        vec![
            codex_meta("Codex Desktop", json!("vscode")),
            codex_user("2026-11-24T01:00:00Z", "u1", true, "진행해"),
            codex_ai("2026-11-24T01:01:00Z", "a1", "진행합니다"),
        ]
    };
    let v = vault(&[
        codex_file("codex/2026/11/24/rollout-a.jsonl.zst", "a", whole()),
        // The same rollout after Codex moved it.
        codex_file("codex/archived_sessions/rollout-a.jsonl.zst", "a", whole()),
        // The session went on in a second file, where the person typed nothing. The
        // index alone would rule the first file out of the next day.
        Kept {
            started: "2026-11-25T02:00:00Z",
            modified: "2026-11-25T03:00:00Z",
            ..codex_file(
                "codex/2026/11/25/rollout-a_2.jsonl.zst",
                "a",
                vec![
                    codex_meta("Codex Desktop", json!("vscode")),
                    codex_ai("2026-11-25T02:00:00Z", "a2", "이어서 한 답"),
                ],
            )
        },
    ]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        speakers(&t),
        [
            (Speaker::Human, "S1".into(), "진행해".into()),
            (Speaker::Ai, "S1".into(), "진행합니다".into()),
        ]
    );
    let placed = |t: &DayTranscript| -> Vec<(String, usize)> {
        t.blocks
            .iter()
            .map(|b| (t.sources[b.source].files[b.file].clone(), b.line))
            .collect()
    };
    assert_eq!(
        placed(&t),
        [
            ("codex/2026/11/24/rollout-a.jsonl.zst".to_owned(), 2),
            ("codex/2026/11/24/rollout-a.jsonl.zst".to_owned(), 3),
        ]
    );
    let summary = t.summary();
    assert_eq!((summary.sessions, summary.agent_runs), (1, 0));
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
    // The next day's answer is still an answer in a session a person typed into.
    let next = day(&v.0, "2026-11-25");
    assert_eq!(
        speakers(&next),
        [(Speaker::Ai, "S1".into(), "이어서 한 답".into())]
    );
    assert_eq!(
        placed(&next),
        [("codex/2026/11/25/rollout-a_2.jsonl.zst".to_owned(), 2)]
    );
}

#[test]
fn a_row_without_a_time_takes_the_time_of_the_row_before_it() {
    // Both files last changed after Korean midnight.
    let late = |kept: Kept| Kept {
        modified: "2026-11-24T16:00:00Z",
        ..kept
    };
    let v = vault(&[
        late(codex_file(
            "codex/2026/11/24/rollout-a.jsonl.zst",
            "a",
            vec![
                codex_meta("Codex Desktop", json!("vscode")),
                codex_user("2026-11-24T14:59:00Z", "u1", true, "자정 전 요청"),
                // A row this reader does not keep, after Korean midnight.
                json!({"timestamp":"2026-11-24T15:01:00Z","type":"event_msg","payload":{"type":"token_count"}}),
                without_time(codex_user("", "u2", true, "자정 뒤 승인")),
            ],
        )),
        late(claude_file(
            "claude/C--work/c.jsonl.zst",
            vec![
                claude_user("2026-11-24T14:59:00Z", typed(), "자정 전 글"),
                claude_row("progress", "2026-11-24T15:02:00Z", json!({})),
                without_time(claude_user("", typed(), "자정 뒤 글")),
            ],
        )),
    ]);
    let texts = |date| {
        speakers(&day(&v.0, date))
            .into_iter()
            .map(|(_, _, text)| text)
            .collect::<Vec<_>>()
    };
    assert_eq!(texts("2026-11-24"), ["자정 전 요청", "자정 전 글"]);
    assert_eq!(texts("2026-11-25"), ["자정 뒤 승인", "자정 뒤 글"]);
    assert!(day(&v.0, "2026-11-25")
        .render()
        .contains("[1] 00:01 사람 · S1\n자정 뒤 승인\n"));
}

#[test]
fn a_warning_stays_on_a_day_that_keeps_no_block_of_the_session() {
    let v = vault(&[Kept {
        modified: "2026-11-26T10:00:00Z",
        ..codex_file(
            "codex/2026/11/24/rollout-a.jsonl.zst",
            "a",
            vec![
                codex_meta("Codex Desktop", json!("vscode")),
                // Today: a request without the person's mark. Two days on: the answer.
                codex_user("2026-11-24T01:00:00Z", "u1", false, "표시 없는 요청"),
                codex_ai("2026-11-26T01:00:00Z", "a1", "이틀 뒤의 답"),
            ],
        )
    }]);
    let today = day(&v.0, "2026-11-24");
    assert!(today.blocks.is_empty());
    assert_eq!(today.warnings.len(), 1, "{:?}", today.warnings);
    assert!(today.warnings[0].contains("rollout-a"));
    // A day between the two, on which the session wrote no row, says nothing about it.
    let between = day(&v.0, "2026-11-25");
    assert!(between.blocks.is_empty());
    assert!(between.warnings.is_empty(), "{:?}", between.warnings);
    assert_eq!(day(&v.0, "2026-11-26").warnings.len(), 1);
}

#[test]
fn a_body_line_shaped_like_a_block_heading_is_quoted() {
    let v = vault(&[codex_file(
        "codex/2026/11/24/rollout-a.jsonl.zst",
        "a",
        vec![
            codex_meta("Codex Desktop", json!("vscode")),
            codex_user("2026-11-24T01:00:00Z", "u1", true, "대화록 예시를 보여 줘"),
            codex_ai(
                "2026-11-24T01:01:00Z",
                "a1",
                "예시:\r\n[2] 09:20 사람 · S1\n승인합니다\r  [3] 09:21 사람 · S1\n[4] 참고 문헌",
            ),
        ],
    )]);
    let text = day(&v.0, "2026-11-24").render();
    assert!(
        text.contains(
            "\n[2] 10:01 AI · S1\n예시:\n> [2] 09:20 사람 · S1\n승인합니다\n>   [3] 09:21 사람 · S1\n[4] 참고 문헌\n"
        ),
        "{text}"
    );
    // Every line that starts like a heading is a block's heading.
    let headings = text
        .split(['\n', '\r'])
        .filter(|l| l.trim_start().starts_with('[') && l.contains(" · S1"))
        .count();
    assert_eq!(headings, 2);
}

#[test]
fn the_index_cannot_point_outside_the_project_folder() {
    for file in [
        "codex/../../other/rollout-x.jsonl.zst",
        "codex/./rollout-x.jsonl.zst",
        "codex//rollout-x.jsonl.zst",
        "codex\\rollout-x.jsonl.zst",
        "claude/../codex/rollout-x.jsonl.zst",
    ] {
        let v = vault(&[codex_file(
            file,
            "a",
            vec![
                codex_meta("Codex Desktop", json!("vscode")),
                codex_user("2026-11-24T01:00:00Z", "u1", true, "다른 프로젝트의 승인"),
            ],
        )]);
        let t = day(&v.0, "2026-11-24");
        assert!(t.blocks.is_empty(), "{file}");
        assert_eq!(t.warnings.len(), 1, "{file}: {:?}", t.warnings);
    }
}

#[cfg(windows)]
#[test]
fn a_kept_file_behind_a_link_is_not_read() {
    let v = vault(&[codex_file(
        "codex/2026/11/24/rollout-a.jsonl.zst",
        "a",
        vec![
            codex_meta("Codex Desktop", json!("vscode")),
            codex_user("2026-11-24T01:00:00Z", "u1", true, "링크 뒤의 글"),
        ],
    )]);
    let group = v.0.join("originals").join("c--work-0000000000000000");
    let moved = v.0.join("elsewhere");
    std::fs::rename(group.join("codex"), &moved).unwrap();
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(group.join("codex"))
        .arg(&moved)
        .output()
        .unwrap();
    assert!(made.status.success());
    let t = day(&v.0, "2026-11-24");
    assert!(t.blocks.is_empty());
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
}

#[test]
fn a_failed_test_report_keeps_no_secret_line_and_claude_exit_codes_are_read() {
    let tap = "TAP version 13\nAPI_KEY=sk-live-123\nnot ok 3 - saves the record\n# tests 5\n# pass 4\n# fail 1\n";
    let shell = |id: &str| json!({"type":"tool_use","id":id,"name":"Bash","input":{}});
    let v = vault(&[
        codex_file(
            "codex/2026/11/24/rollout-a.jsonl.zst",
            "a",
            vec![
                codex_meta("Codex Desktop", json!("vscode")),
                codex_user("2026-11-24T01:00:00Z", "u1", true, "테스트해 줘"),
                codex_item(
                    "2026-11-24T01:01:00Z",
                    json!({"type":"CommandExecution","id":"c1","exit_code":1,"aggregated_output":tap}),
                ),
            ],
        ),
        claude_file(
            "claude/C--work/c.jsonl.zst",
            vec![
                claude_user("2026-11-24T02:00:00Z", typed(), "빌드해 줘"),
                claude_ai("2026-11-24T02:01:00Z", "m1", json!([shell("t1")])),
                claude_row(
                    "user",
                    "2026-11-24T02:02:00Z",
                    json!({"toolUseResult":{"exitCode":3,"stdout":""},"message":{"role":"user","content":[
                        {"type":"tool_result","tool_use_id":"t1","is_error":true,"content":"error: build failed"}]}}),
                ),
            ],
        ),
    ]);
    let checks: Vec<String> = seen(&day(&v.0, "2026-11-24"))
        .into_iter()
        .filter(|(speaker, ..)| *speaker == Speaker::Check)
        .map(|(.., text)| text)
        .collect();
    assert_eq!(
        checks,
        [
            "종료 코드 1\nnot ok 3 - saves the record\n# fail 1",
            "종료 코드 3\nerror: build failed"
        ]
    );
}

fn with(mut row: Value, key: &str, value: Value) -> Value {
    row[key] = value;
    row
}

#[test]
fn a_question_answer_is_what_the_person_picked_not_what_the_tool_wrote() {
    let ask = |id: &str| json!({"type":"tool_use","id":id,"name":"AskUserQuestion","input":{}});
    let result =
        |id: &str, text: &str| json!({"type":"tool_result","tool_use_id":id,"content":text});
    let answered = |at: &str, tool_use_result: Value, results: Value| {
        claude_row(
            "user",
            at,
            json!({"toolUseResult":tool_use_result,"message":{"role":"user","content":results}}),
        )
    };
    let v = vault(&[claude_file(
        "claude/C--work/c.jsonl.zst",
        vec![
            claude_user("2026-11-24T00:01:00Z", typed(), "물어봐 줘"),
            claude_ai(
                "2026-11-24T00:02:00Z",
                "m1",
                json!([
                    ask("q1"),
                    ask("q2"),
                    ask("q3"),
                    ask("q4"),
                    ask("q5"),
                    ask("q6")
                ]),
            ),
            // The tool's text quotes an option the person did not pick.
            answered(
                "2026-11-24T00:03:00Z",
                json!({"questions":[{"question":"배포할까요"}],"answers":{"배포할까요":"보류"}}),
                json!([result("q1", "선택지: 승인, 보류. 사용자가 답했습니다.")]),
            ),
            // No answer was picked.
            answered(
                "2026-11-24T00:04:00Z",
                json!({"questions":[{"question":"배포할까요"}],"answers":{}}),
                json!([result("q2", "승인")]),
            ),
            // Two questions answered in one row: whose answers these are is not written.
            answered(
                "2026-11-24T00:05:00Z",
                json!({"answers":{"배포할까요":"승인"}}),
                json!([result("q3", "승인"), result("q4", "승인")]),
            ),
            // The answers are kept when the tool's text is empty, in the order asked.
            answered(
                "2026-11-24T00:06:00Z",
                json!({"questions":[{"question":"둘째\n질문"},{"question":"첫째 질문"}],
                    "answers":{"첫째 질문":"가","둘째\n질문":"나"}}),
                json!([result("q5", "")]),
            ),
            // The answers are not a table of questions and answers.
            answered(
                "2026-11-24T00:07:00Z",
                json!({"answers":"승인"}),
                json!([result("q6", "승인")]),
            ),
        ],
    )]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        seen(&t),
        [
            (Speaker::Human, "S1".into(), 1, "물어봐 줘".into()),
            (
                Speaker::Human,
                "S1".into(),
                3,
                "질문: 배포할까요\n고른 답: 보류".into()
            ),
            (
                Speaker::Human,
                "S1".into(),
                6,
                "질문: 둘째 질문\n고른 답: 나\n\n질문: 첫째 질문\n고른 답: 가".into()
            ),
        ]
    );
}

#[test]
fn a_claude_copy_that_is_not_a_persons_stands_against_one_that_is() {
    let notice = json!({"origin":{"kind":"task-notification"},"turnOrigin":"task_notification"});
    let meta = with(typed(), "isMeta", json!(true));
    let u = |row: Value, uuid: &str| with(row, "uuid", json!(uuid));
    let v = vault(&[claude_file(
        "claude/C--work/c.jsonl.zst",
        vec![
            u(claude_user("2026-11-24T00:01:00Z", typed(), "진행해"), "u1"),
            u(claude_user("2026-11-24T00:01:01Z", notice, "진행해"), "u1"),
            // The copy that says otherwise can come first.
            u(claude_user("2026-11-24T00:02:00Z", meta, "승인"), "u2"),
            u(claude_user("2026-11-24T00:02:01Z", typed(), "승인"), "u2"),
            u(
                claude_user("2026-11-24T00:03:00Z", typed(), "사본이 하나인 글"),
                "u3",
            ),
        ],
    )]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        seen(&t),
        [
            (Speaker::Delegated, "S1".into(), 1, "진행해".into()),
            (Speaker::Delegated, "S1".into(), 4, "승인".into()),
            (Speaker::Human, "S1".into(), 5, "사본이 하나인 글".into()),
        ]
    );
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
    assert!(t.warnings[0].starts_with("S1: "));
}

#[test]
fn a_session_with_a_file_that_cannot_be_read_has_no_persons_message() {
    let v = vault(&[
        codex_file(
            "codex/2026/11/24/rollout-a.jsonl.zst",
            "a",
            vec![
                codex_meta("Codex Desktop", json!("vscode")),
                codex_user("2026-11-24T01:00:00Z", "u1", true, "진행해"),
                codex_ai("2026-11-24T01:01:00Z", "a1", "진행합니다"),
            ],
        ),
        codex_file("codex/archived_sessions/rollout-a.jsonl.zst", "a", vec![]),
        codex_file(
            "codex/2026/11/24/rollout-b.jsonl.zst",
            "b",
            vec![
                codex_meta("Codex Desktop", json!("vscode")),
                codex_user("2026-11-24T02:00:00Z", "u1", true, "다른 세션의 요청"),
            ],
        ),
    ]);
    let broken =
        v.0.join("originals/c--work-0000000000000000/codex/archived_sessions/rollout-a.jsonl.zst");
    std::fs::write(broken, b"not a zstd file").unwrap();
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        speakers(&t),
        [
            (Speaker::Delegated, "S1".into(), "진행해".into()),
            (Speaker::Ai, "S1".into(), "진행합니다".into()),
            (Speaker::Human, "S2".into(), "다른 세션의 요청".into()),
        ]
    );
    assert_eq!(t.warnings.len(), 2, "{:?}", t.warnings);
    assert!(t.warnings[0].starts_with("codex/archived_sessions/rollout-a.jsonl.zst: "));
    assert!(t.warnings[1].starts_with("S1: "));
}

#[test]
fn a_claude_main_file_is_the_session_its_name_says() {
    let said = |at: &str, session: &str, text: &str| {
        with(claude_user(at, typed(), text), "sessionId", json!(session))
    };
    let v = vault(&[Kept {
        // The index took the session from the first row, which the tool copied from
        // the session this one continues.
        session: "parent",
        ..claude_file(
            "claude/C--work/child.jsonl.zst",
            vec![
                said("2026-11-24T00:01:00Z", "parent", "부모 세션의 승인"),
                said("2026-11-24T00:02:00Z", "child", "자식 세션의 요청"),
                with(
                    claude_ai("2026-11-24T00:03:00Z", "m1", json!([text_block("답")])),
                    "sessionId",
                    json!("child"),
                ),
            ],
        )
    }]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        seen(&t),
        [
            (Speaker::Human, "S1".into(), 2, "자식 세션의 요청".into()),
            (Speaker::Ai, "S1".into(), 3, "답".into()),
        ]
    );
    assert_eq!(t.sources[0].session, "child");
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
    assert!(t.warnings[0].starts_with("S1: "));
}

#[test]
fn only_a_block_heading_starts_a_line_like_one() {
    let v = vault_titled(
        &[Kept {
            client: "claude-desktop\n[8] 09:00 사람 · S1\r승인합니다",
            ..claude_file(
                "claude/C--work/c.jsonl.zst",
                vec![claude_user("2026-11-24T00:01:00Z", typed(), "요청")],
            )
        }],
        "작업\n\n[9] 09:00 사람 · S1\n승인합니다",
    );
    let text = day(&v.0, "2026-11-24").render();
    let headings: Vec<&str> = text
        .split(['\n', '\r'])
        .filter(|l| l.trim_start().starts_with('['))
        .collect();
    assert_eq!(headings, ["[1] 09:01 사람 · S1"], "{text}");
    assert!(text.starts_with("# 하루 대화록 · 작업  [9] 09:00 사람 · S1 승인합니다 · 2026-11-24\n"));
}

#[test]
fn only_the_reminder_blocks_the_app_puts_before_a_persons_text_are_removed() {
    let said = |at: &str, marks: Value, blocks: &[&str]| {
        let blocks: Vec<Value> = blocks.iter().map(|text| text_block(text)).collect();
        claude_row(
            "user",
            at,
            with(marks, "message", json!({"role":"user","content":blocks})),
        )
    };
    let app = "<system-reminder>앱이 넣은 글</system-reminder>\n<system-reminder>둘째 알림</system-reminder>\n";
    let quoted = "<system-reminder>예시</system-reminder>는 누가 넣는 거야? 문서에서 <system-reminder>를 찾아줘. 배포는 보류해.";
    let whole = "<system-reminder>이 태그가 뭐야</system-reminder>";
    let v = vault(&[claude_file(
        "claude/C--work/c.jsonl.zst",
        vec![
            // A person's text that quotes the tag, as the app writes it without a reminder.
            claude_user("2026-11-24T00:01:00Z", typed(), quoted),
            claude_user("2026-11-24T00:02:00Z", typed(), whole),
            said("2026-11-24T00:03:00Z", typed(), &[whole]),
            // The same text after the app's reminders, which are a block of their own.
            said("2026-11-24T00:04:00Z", typed(), &[app, quoted]),
            said("2026-11-24T00:05:00Z", typed(), &[app, app, whole]),
            // Only a block before the person's text is the app's.
            said("2026-11-24T00:06:00Z", typed(), &["요청", whole]),
            // An instruction another session sent has them in its text.
            claude_user(
                "2026-11-24T00:07:00Z",
                json!({"turnOrigin":"host_synthetic"}),
                &format!("{app}지시 <system-reminder>가운데</system-reminder> 끝"),
            ),
        ],
    )]);
    let both = format!("요청\n{whole}");
    let expected = [
        (Speaker::Human, quoted),
        (Speaker::Human, whole),
        (Speaker::Human, whole),
        (Speaker::Human, quoted),
        (Speaker::Human, whole),
        (Speaker::Human, both.as_str()),
        (
            Speaker::Delegated,
            "지시 <system-reminder>가운데</system-reminder> 끝",
        ),
    ];
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        speakers(&t),
        expected.map(|(speaker, text)| (speaker, "S1".to_string(), text.to_string()))
    );
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
}

#[test]
fn a_copy_of_a_question_result_in_a_row_left_out_stands_against_the_answer() {
    let ask = |id: &str| json!({"type":"tool_use","id":id,"name":"AskUserQuestion","input":{}});
    let answered = |at: &str, id: &str, failed: bool| {
        claude_row(
            "user",
            at,
            json!({"toolUseResult":{"questions":[{"question":"배포할까요"}],"answers":{"배포할까요":"승인"}},
                "message":{"role":"user","content":[
                {"type":"tool_result","tool_use_id":id,"is_error":failed,"content":"답함"}]}}),
        )
    };
    let v = vault(&[claude_file(
        "claude/C--work/c.jsonl.zst",
        vec![
            claude_user("2026-11-24T00:01:00Z", typed(), "물어봐 줘"),
            claude_ai(
                "2026-11-24T00:02:00Z",
                "m1",
                json!([ask("q1"), ask("q2"), ask("q3"), ask("q4"), ask("q5")]),
            ),
            answered("2026-11-24T00:03:00Z", "q1", false),
            with(
                answered("2026-11-24T00:03:01Z", "q1", false),
                "isMeta",
                json!(true),
            ),
            // The copy that says otherwise can come first.
            with(
                answered("2026-11-24T00:04:00Z", "q2", false),
                "sessionId",
                json!("another"),
            ),
            answered("2026-11-24T00:04:01Z", "q2", false),
            answered("2026-11-24T00:05:00Z", "q3", false),
            with(
                answered("2026-11-24T00:05:01Z", "q3", false),
                "sessionId",
                Value::Null,
            ),
            // A copy that failed makes no block of the answer either.
            answered("2026-11-24T00:06:00Z", "q4", false),
            answered("2026-11-24T00:06:01Z", "q4", true),
            answered("2026-11-24T00:07:00Z", "q5", false),
        ],
    )]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        seen(&t),
        [
            (Speaker::Human, "S1".into(), 1, "물어봐 줘".into()),
            (
                Speaker::Human,
                "S1".into(),
                11,
                "질문: 배포할까요\n고른 답: 승인".into()
            ),
        ]
    );
    // Copies that do not agree, and the row that names no session.
    assert_eq!(t.warnings.len(), 2, "{:?}", t.warnings);
}

#[test]
fn only_blank_lines_are_taken_from_the_ends_of_a_text() {
    let ask = json!({"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{}});
    let code = "    if ready:\n        deploy()  ";
    let v = vault(&[
        codex_file(
            "codex/2026/11/24/rollout-a.jsonl.zst",
            "a",
            vec![
                codex_meta("Codex Desktop", json!("vscode")),
                codex_user(
                    "2026-11-24T01:00:00Z",
                    "u1",
                    true,
                    &format!("\r\n  \n{code}\n \n\n"),
                ),
                codex_ai("2026-11-24T01:01:00Z", "a1", &format!("\n{code}\n")),
            ],
        ),
        claude_file(
            "claude/C--work/c.jsonl.zst",
            vec![
                claude_user("2026-11-24T02:00:00Z", typed(), &format!("{code}\n")),
                claude_ai("2026-11-24T02:01:00Z", "m1", json!([ask])),
                claude_row(
                    "user",
                    "2026-11-24T02:02:00Z",
                    json!({"toolUseResult":{"answers":{"어느 쪽":"\n  B안 \n"}},
                        "message":{"role":"user","content":[
                        {"type":"tool_result","tool_use_id":"q1","content":""}]}}),
                ),
            ],
        ),
    ]);
    assert_eq!(
        speakers(&day(&v.0, "2026-11-24")),
        [
            (Speaker::Human, "S1".into(), code.into()),
            (Speaker::Ai, "S1".into(), code.into()),
            (Speaker::Human, "S2".into(), code.into()),
            (
                Speaker::Human,
                "S2".into(),
                "질문: 어느 쪽\n고른 답:   B안 ".into()
            ),
        ]
    );
}

/// Writes `text` as the kept file `file`, for rows that are not JSON.
fn rewrite(vault: &Vault, file: &str, text: &str) {
    let path = vault
        .0
        .join("originals/c--work-0000000000000000")
        .join(file);
    std::fs::write(path, zstd::encode_all(text.as_bytes(), 3).unwrap()).unwrap();
}

#[test]
fn a_finished_row_that_is_not_json_is_reported_and_an_unfinished_last_row_is_not() {
    let files = [
        "codex/2026/11/24/rollout-a.jsonl.zst",
        "codex/2026/11/24/rollout-b.jsonl.zst",
    ];
    let v = vault(&[
        codex_file(files[0], "a", vec![]),
        codex_file(files[1], "b", vec![]),
    ]);
    let row = |session: &str, row: Value| format!("{}\n", stamped(row, "codex", session));
    let cut =
        r#"{"timestamp":"2026-11-24T02:00:00Z","type":"event_msg","payload":{"type":"item_comp"#;
    let rows = |session: &str, between: &str| {
        [
            row(session, codex_meta("Codex Desktop", json!("vscode"))),
            row(
                session,
                codex_user("2026-11-24T01:00:00Z", "u1", true, "요청"),
            ),
            between.to_owned(),
            row(session, codex_ai("2026-11-24T03:00:00Z", "a1", "답")),
        ]
        .concat()
    };
    // A row cut short and ended, with a blank line after it; and a file that ends in
    // a row the tool is still writing.
    rewrite(&v, files[0], &rows("a", &format!("{cut}\n\n")));
    rewrite(&v, files[1], &format!("{}{cut}", rows("b", "")));
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        seen(&t),
        [
            (Speaker::Human, "S1".into(), 2, "요청".into()),
            (Speaker::Human, "S2".into(), 2, "요청".into()),
            (Speaker::Ai, "S1".into(), 5, "답".into()),
            (Speaker::Ai, "S2".into(), 3, "답".into()),
        ]
    );
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
    assert!(t.warnings[0].starts_with("S1: "));
}

#[test]
fn a_row_over_the_line_limit_that_ends_the_file_is_reported_on_the_day_after_the_last_time() {
    let huge = "가".repeat(3 * 1024 * 1024);
    let v = vault(&[Kept {
        started: "2026-11-23T14:00:00Z",
        ..codex_file(
            "codex/2026/11/23/rollout-a.jsonl.zst",
            "a",
            vec![
                with(
                    codex_meta("Codex Desktop", json!("vscode")),
                    "timestamp",
                    json!("2026-11-23T14:00:00Z"),
                ),
                // 23:59 Korean time on the 23rd; the long row was written after midnight.
                codex_user("2026-11-23T14:59:00Z", "u1", true, "요청"),
                codex_user("2026-11-23T15:01:00Z", "u2", true, &huge),
            ],
        )
    }]);
    let t = day(&v.0, "2026-11-24");
    assert!(t.blocks.is_empty());
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
    assert!(t.warnings[0].starts_with("codex/2026/11/23/rollout-a.jsonl.zst: "));
}

#[test]
fn plain_claude_text_without_an_origin_is_reported_beside_rows_that_have_one() {
    let ask = json!({"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{}});
    let v = vault(&[
        // A session begun before Claude Code wrote who sent a message, and gone on with after.
        Kept {
            modified: "2026-11-25T10:00:00Z",
            ..claude_file(
                "claude/C--work/c.jsonl.zst",
                vec![
                    claude_user("2026-11-24T00:01:00Z", json!({}), "표시 없는 요청"),
                    claude_user("2026-11-24T00:02:00Z", typed(), "표시 있는 요청"),
                    claude_user("2026-11-25T00:01:00Z", typed(), "다음 날의 요청"),
                ],
            )
        },
        // No row has an origin, and a picked answer makes it a person's session.
        Kept {
            session: "d",
            ..claude_file(
                "claude/C--work/d.jsonl.zst",
                vec![
                    claude_user("2026-11-24T01:01:00Z", json!({}), "표시 없는 요청"),
                    claude_ai("2026-11-24T01:02:00Z", "m1", json!([ask])),
                    claude_row(
                        "user",
                        "2026-11-24T01:03:00Z",
                        json!({"toolUseResult":{"answers":{"어느 쪽":"B안"}},
                            "message":{"role":"user","content":[
                            {"type":"tool_result","tool_use_id":"q1","content":""}]}}),
                    ),
                ],
            )
        },
    ]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        speakers(&t),
        [
            (Speaker::Human, "S1".into(), "표시 있는 요청".into()),
            (
                Speaker::Human,
                "S2".into(),
                "질문: 어느 쪽\n고른 답: B안".into()
            ),
        ]
    );
    assert_eq!(t.warnings.len(), 2, "{:?}", t.warnings);
    assert!(t.warnings[0].starts_with("S1: "));
    assert!(t.warnings[1].starts_with("S2: "));
    // The plain row was not on the next day.
    let next = day(&v.0, "2026-11-25");
    assert_eq!(speakers(&next).len(), 1);
    assert!(next.warnings.is_empty(), "{:?}", next.warnings);
}

#[test]
fn the_last_answer_of_an_agent_run_is_the_latest_by_time() {
    let exec = |file: &'static str, at: &str, id: &str, text: &str| Kept {
        client: "codex_exec",
        ..codex_file(
            file,
            "exec",
            vec![
                codex_meta("codex_exec", json!("exec")),
                codex_ai(at, id, text),
            ],
        )
    };
    let v = vault(&[
        // The file that sorts first holds the later answer.
        exec(
            "codex/2026/11/24/rollout-1.jsonl.zst",
            "2026-11-24T06:00:00Z",
            "a2",
            "오후의 답",
        ),
        exec(
            "codex/2026/11/24/rollout-2.jsonl.zst",
            "2026-11-24T01:00:00Z",
            "a1",
            "오전의 답",
        ),
    ]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        seen(&t),
        [(Speaker::AgentRun, "S1".into(), 2, "오후의 답".into())]
    );
    assert_eq!(t.blocks[0].file, 0);
}

#[test]
fn a_claude_answer_written_as_several_rows_keeps_each_rows_day_and_place() {
    let part = |at: &str, uuid: &str, text: &str| {
        with(
            claude_ai(at, "m1", json!([text_block(text)])),
            "uuid",
            json!(uuid),
        )
    };
    let rows = |first: Value| {
        vec![
            first,
            // 23:59 and 00:01 Korean time, and the second row written again.
            part("2026-11-24T14:59:00Z", "r1", "자정 전 조각"),
            part("2026-11-24T15:01:00Z", "r2", "자정 뒤 조각"),
            part("2026-11-24T15:01:00Z", "r2", "자정 뒤 조각"),
            part("2026-11-24T15:02:00Z", "r3", "마지막 조각"),
        ]
    };
    let late = |kept: Kept| Kept {
        modified: "2026-11-24T16:00:00Z",
        ..kept
    };
    let v = vault(&[
        late(claude_file(
            "claude/C--work/c.jsonl.zst",
            rows(claude_user("2026-11-24T14:58:00Z", typed(), "요청")),
        )),
        // The same rows in a session nobody typed into.
        late(Kept {
            session: "d",
            ..claude_file(
                "claude/C--work/d.jsonl.zst",
                rows(claude_user(
                    "2026-11-24T14:58:00Z",
                    json!({"turnOrigin":"host_synthetic"}),
                    "지시",
                )),
            )
        }),
    ]);
    assert_eq!(
        seen(&day(&v.0, "2026-11-24")),
        [
            (Speaker::Human, "S1".into(), 1, "요청".into()),
            (Speaker::Ai, "S1".into(), 2, "자정 전 조각".into()),
            (Speaker::AgentRun, "S2".into(), 2, "자정 전 조각".into()),
        ]
    );
    assert_eq!(
        seen(&day(&v.0, "2026-11-25")),
        [
            (Speaker::Ai, "S1".into(), 3, "자정 뒤 조각".into()),
            (
                Speaker::AgentRun,
                "S2".into(),
                3,
                "자정 뒤 조각\n\n마지막 조각".into()
            ),
            (Speaker::Ai, "S1".into(), 5, "마지막 조각".into()),
        ]
    );
}

#[test]
fn a_day_with_only_rows_that_could_not_be_judged_is_reported() {
    let mut unnamed = codex_user(
        "2026-11-24T01:00:00Z",
        "u1",
        true,
        "어느 세션의 글인지 모름",
    );
    unnamed["payload"]["thread_id"] = Value::Null;
    let v = vault(&[
        codex_file(
            "codex/2026/11/24/rollout-a.jsonl.zst",
            "a",
            vec![codex_meta("Codex Desktop", json!("vscode")), unnamed],
        ),
        // A file from before Claude Code wrote who sent a message, with no answer.
        claude_file(
            "claude/C--work/c.jsonl.zst",
            vec![claude_user(
                "2026-11-24T02:00:00Z",
                json!({}),
                "표시 없는 요청",
            )],
        ),
    ]);
    let t = day(&v.0, "2026-11-24");
    assert!(t.blocks.is_empty());
    assert_eq!(t.warnings.len(), 2, "{:?}", t.warnings);
    assert!(t.warnings[0].starts_with("codex/2026/11/24/rollout-a.jsonl.zst: "));
    assert!(t.warnings[1].starts_with("claude/C--work/c.jsonl.zst: "));
}

#[test]
fn a_row_over_the_line_limit_is_counted_and_reported() {
    let huge = "가".repeat(3 * 1024 * 1024);
    let v = vault(&[Kept {
        modified: "2026-11-26T10:00:00Z",
        ..codex_file(
            "codex/2026/11/24/rollout-a.jsonl.zst",
            "a",
            vec![
                codex_meta("Codex Desktop", json!("vscode")),
                codex_user("2026-11-24T01:00:00Z", "u1", true, "요청"),
                codex_user("2026-11-24T02:00:00Z", "u2", true, &huge),
                codex_ai("2026-11-24T03:00:00Z", "a1", "답"),
                codex_ai("2026-11-25T03:00:00Z", "a2", "다음 날의 답"),
            ],
        )
    }]);
    let t = day(&v.0, "2026-11-24");
    // The rows after the long one keep their line numbers.
    assert_eq!(
        seen(&t),
        [
            (Speaker::Human, "S1".into(), 2, "요청".into()),
            (Speaker::Ai, "S1".into(), 4, "답".into()),
        ]
    );
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
    assert!(t.warnings[0].starts_with("S1: "));
    // The long row was not on the next day.
    assert!(day(&v.0, "2026-11-25").warnings.is_empty());
}

#[test]
fn a_project_id_kept_in_two_folders_is_refused() {
    let v = vault(&[codex_file(
        "codex/2026/11/24/rollout-a.jsonl.zst",
        "a",
        vec![
            codex_meta("Codex Desktop", json!("vscode")),
            codex_user("2026-11-24T01:00:00Z", "u1", true, "요청"),
        ],
    )]);
    // The project's folder was changed in the app: the originals of the new folder go
    // to a second folder with the same project ID.
    let moved = v.0.join("originals").join("d--moved-1111111111111111");
    std::fs::create_dir_all(&moved).unwrap();
    std::fs::write(
        moved.join("project.json"),
        json!({"schemaVersion":1,"projectId":PROJECT,"title":"작업","repoKey":"d:/moved"})
            .to_string(),
    )
    .unwrap();
    std::fs::write(moved.join("index.jsonl"), "").unwrap();
    let date = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
    let refused = day_transcript(&v.0, PROJECT, date).unwrap_err();
    assert!(
        refused.contains("c:/work") && refused.contains("d:/moved"),
        "{refused}"
    );
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(made.status.success());
}

#[cfg(windows)]
#[test]
fn a_linked_project_folder_is_not_read_to_find_the_project() {
    let v = vault(&[codex_file(
        "codex/2026/11/24/rollout-a.jsonl.zst",
        "a",
        vec![
            codex_meta("Codex Desktop", json!("vscode")),
            codex_user("2026-11-24T01:00:00Z", "u1", true, "이 프로젝트의 요청"),
        ],
    )]);
    // A folder elsewhere that claims the same project, linked into the originals under
    // a name that is listed first.
    let elsewhere = v.0.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(
        elsewhere.join("project.json"),
        json!({"schemaVersion":1,"projectId":PROJECT,"title":"다른 곳","repoKey":"c:/other"})
            .to_string(),
    )
    .unwrap();
    std::fs::write(elsewhere.join("index.jsonl"), "").unwrap();
    junction(&v.0.join("originals").join("a-linked"), &elsewhere);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(t.title, "작업");
    assert_eq!(
        speakers(&t),
        [(Speaker::Human, "S1".into(), "이 프로젝트의 요청".into())]
    );
}

/// Windows lets only some accounts make a file link. Where it does not, the two files
/// cannot be linked either, and there is nothing to check.
#[cfg(windows)]
#[test]
fn a_linked_project_file_or_index_is_not_read() {
    let date = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
    for name in ["project.json", "index.jsonl"] {
        let v = vault(&[codex_file(
            "codex/2026/11/24/rollout-a.jsonl.zst",
            "a",
            vec![
                codex_meta("Codex Desktop", json!("vscode")),
                codex_user("2026-11-24T01:00:00Z", "u1", true, "요청"),
            ],
        )]);
        let group = v.0.join("originals").join("c--work-0000000000000000");
        let real = v.0.join(name);
        std::fs::rename(group.join(name), &real).unwrap();
        if let Err(error) = std::os::windows::fs::symlink_file(&real, group.join(name)) {
            eprintln!("skipped: this account cannot make a file link ({error})");
            return;
        }
        assert!(day_transcript(&v.0, PROJECT, date).is_err(), "{name}");
    }
}

#[test]
fn only_the_apps_whole_interruption_text_is_known_to_be_the_apps() {
    let v = vault(&[claude_file(
        "claude/C--work/c.jsonl.zst",
        vec![
            claude_user("2026-11-24T00:01:00Z", typed(), "사람의 요청"),
            claude_user(
                "2026-11-24T00:02:00Z",
                json!({}),
                "[Request interrupted by user]",
            ),
            claude_user(
                "2026-11-24T00:03:00Z",
                json!({}),
                "[Request interrupted by user for tool use]",
            ),
            // A person can start a message with the app's words.
            claude_user(
                "2026-11-24T00:04:00Z",
                json!({}),
                "[Request interrupted by user]가 왜 뜨나요?",
            ),
        ],
    )]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        speakers(&t),
        [(Speaker::Human, "S1".into(), "사람의 요청".into())]
    );
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
    assert!(t.warnings[0].contains("일반 글 1개"), "{:?}", t.warnings);
}

#[test]
fn copies_of_a_question_result_that_hold_different_answers_leave_no_block() {
    let ask = |id: &str| json!({"type":"tool_use","id":id,"name":"AskUserQuestion","input":{}});
    let answered = |at: &str, id: &str, answer: &str| {
        claude_row(
            "user",
            at,
            json!({"toolUseResult":{"questions":[{"question":"배포할까요"}],"answers":{"배포할까요":answer}},
                "message":{"role":"user","content":[
                {"type":"tool_result","tool_use_id":id,"content":"답함"}]}}),
        )
    };
    let v = vault(&[claude_file(
        "claude/C--work/c.jsonl.zst",
        vec![
            claude_user("2026-11-24T00:01:00Z", typed(), "물어봐 줘"),
            claude_ai("2026-11-24T00:02:00Z", "m1", json!([ask("q1"), ask("q2")])),
            answered("2026-11-24T00:03:00Z", "q1", "승인"),
            answered("2026-11-24T00:03:01Z", "q1", "보류"),
            answered("2026-11-24T00:03:02Z", "q1", "승인"),
            // Copies that hold the same answer are one block.
            answered("2026-11-24T00:04:00Z", "q2", "승인"),
            answered("2026-11-24T00:04:01Z", "q2", "승인"),
        ],
    )]);
    let t = day(&v.0, "2026-11-24");
    assert_eq!(
        seen(&t),
        [
            (Speaker::Human, "S1".into(), 1, "물어봐 줘".into()),
            (
                Speaker::Human,
                "S1".into(),
                6,
                "질문: 배포할까요\n고른 답: 승인".into()
            ),
        ]
    );
    assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
}

#[test]
fn a_reminder_before_an_image_is_not_what_the_person_wrote() {
    let image =
        json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":""}});
    let said = |at: &str, blocks: Value| {
        claude_row(
            "user",
            at,
            with(typed(), "message", json!({"role":"user","content":blocks})),
        )
    };
    let app = "<system-reminder>앱이 넣은 글</system-reminder>";
    let v = vault(&[claude_file(
        "claude/C--work/c.jsonl.zst",
        vec![
            // The person sent only an image; the one text block is the app's.
            said("2026-11-24T00:01:00Z", json!([text_block(app), image])),
            said(
                "2026-11-24T00:02:00Z",
                json!([text_block(app), image, text_block("이 화면을 봐줘")]),
            ),
            said(
                "2026-11-24T00:03:00Z",
                json!([image, text_block("둘째 화면")]),
            ),
            // A message that is one block is the person's, whatever it holds.
            said("2026-11-24T00:04:00Z", json!([text_block(app)])),
        ],
    )]);
    let t = day(&v.0, "2026-11-24");
    let expected = ["이 화면을 봐줘", "둘째 화면", app];
    assert_eq!(
        speakers(&t),
        expected.map(|text| (Speaker::Human, "S1".to_string(), text.to_string()))
    );
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
}

#[test]
fn a_person_message_in_the_older_codex_form_is_reported_not_lost_quietly() {
    let older = |at: &str, text: &str| json!({"timestamp":at,"type":"event_msg","payload":{"type":"user_message","message":text}});
    let v = vault(&[
        codex_file(
            "codex/2026/11/24/rollout-old.jsonl.zst",
            "old",
            vec![
                codex_meta("codex_cli_rs", json!("cli")),
                older("2026-11-24T01:00:00Z", "배포는 보류해"),
                codex_ai("2026-11-24T01:05:00Z", "a1", "알겠습니다"),
            ],
        ),
        // The current form writes `user_message` too, beside the item that is read.
        codex_file(
            "codex/2026/11/24/rollout-new.jsonl.zst",
            "new",
            vec![
                codex_meta("Codex Desktop", json!("vscode")),
                older("2026-11-24T02:00:00Z", "진행해"),
                codex_user("2026-11-24T02:00:00Z", "u1", true, "진행해"),
            ],
        ),
    ]);
    let t = day(&v.0, "2026-11-24");
    // The older message is not taken for a person's, and the session says so.
    assert!(!t.blocks.iter().any(|b| b.text == "배포는 보류해"));
    let older_warnings: Vec<_> = t
        .warnings
        .iter()
        .filter(|w| w.contains("user_message"))
        .collect();
    assert_eq!(older_warnings.len(), 1, "{:?}", t.warnings);
    assert!(older_warnings[0].contains("1개"));
    // Not on a day the message is not on.
    assert!(day(&v.0, "2026-11-25")
        .warnings
        .iter()
        .all(|w| !w.contains("user_message")));
}
