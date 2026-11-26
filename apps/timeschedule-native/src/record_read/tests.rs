//! The reading commands run as a caller runs them, on a data folder and a document
//! store filled by the app's own paths: kept originals, a daily record made from them
//! with a made-up model answer and shown in the diary, a delivered agent record, and a
//! record written by hand.
use crate::record_cli::{execute, run, tests::registered};
use crate::record_inbox::tests::{sample, TestDir};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use timeschedule_core::{
    chat_originals::group_name,
    daily_record::{day_transcript, record::summarize},
    diary::{EntryInput, ProjectInput},
    storage::Storage,
    terra::{Reply, Usage},
};

const FILE: &str = "claude/C--work/s1.jsonl.zst";
const DAY: &str = "2026-11-24";

struct Fixture {
    dir: TestDir,
    data: PathBuf,
    vault: PathBuf,
    work: PathBuf,
    project: String,
    group: String,
    daily_id: String,
    manual_id: String,
}

/// The project folder's key, as the app derives it from the path.
fn key_of(folder: &Path) -> String {
    let path = std::fs::canonicalize(folder).unwrap();
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("//?/")
        .unwrap_or(&text)
        .trim_end_matches('/')
        .to_lowercase()
}

/// A Claude Code session a person opened on 2026-11-24: rows 1 to 4, a minute apart
/// from 10:00 Korean time. Row 3 is an answer longer than the transcript keeps.
fn session_rows() -> Vec<Value> {
    let person = |minute: u32, text: &str| {
        json!({"type":"user","timestamp":format!("2026-11-24T01:{minute:02}:00Z"),"cwd":"C:\\work",
            "sessionId":"s1","uuid":format!("u{minute}"),"origin":{"kind":"human"},"turnOrigin":"human",
            "message":{"role":"user","content":text}})
    };
    let ai = |minute: u32, text: &str| {
        json!({"type":"assistant","timestamp":format!("2026-11-24T01:{minute:02}:00Z"),"cwd":"C:\\work",
            "sessionId":"s1","uuid":format!("a{minute}"),
            "message":{"role":"assistant","id":format!("m{minute}"),"content":[{"type":"text","text":text}]}})
    };
    vec![
        person(0, "하루 기록이 다이어리에 두 번 보여"),
        ai(
            1,
            "UNIQUE constraint failed 오류였습니다. 수령 기록 표를 더해 고쳤습니다.",
        ),
        ai(2, &format!("긴 설명 {}", "가".repeat(5000))),
        person(3, "좋아, 그렇게 가자"),
    ]
}

fn fixture() -> Fixture {
    let dir = TestDir::new();
    let vault = dir.0.join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    std::fs::write(vault.join("vault.json"), r#"{"schemaVersion":1}"#).unwrap();
    let data = registered(&dir, &vault);
    let work = dir.0.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let key = key_of(&work);
    let group = group_name(&key);

    let store = Storage::open(data.join("schedule.db")).unwrap();
    let project = store
        .save_project(ProjectInput {
            id: None,
            title: "작업".into(),
            description: String::new(),
            repo_path: work.to_string_lossy().into(),
        })
        .unwrap()
        .id;
    store.set_capture_sources(&project, true, true).unwrap();

    let originals = vault.join("originals").join(&group);
    std::fs::create_dir_all(originals.join("claude/C--work")).unwrap();
    std::fs::write(
        originals.join("project.json"),
        json!({"schemaVersion":1,"projectId":project,"title":"작업","repoKey":key}).to_string(),
    )
    .unwrap();
    let text: String = session_rows().iter().map(|r| format!("{r}\n")).collect();
    std::fs::write(
        originals.join(FILE),
        zstd::encode_all(text.as_bytes(), 3).unwrap(),
    )
    .unwrap();
    let modified = chrono::DateTime::parse_from_rfc3339("2026-11-24T10:00:00Z")
        .unwrap()
        .timestamp_nanos_opt()
        .unwrap();
    std::fs::write(
        originals.join("index.jsonl"),
        json!({"file":FILE,"tool":"claude","session":"s1","cwd":"C:\\work","client":"claude-desktop",
            "started":"2026-11-24T00:00:00Z","source":"x","sourceSize":1,
            "sourceModified":modified,"archivedAt":"2026-11-25T00:00:00Z"})
        .to_string()
            + "\n",
    )
    .unwrap();

    let transcript = day_transcript(
        &vault,
        &project,
        chrono::NaiveDate::parse_from_str(DAY, "%Y-%m-%d").unwrap(),
    )
    .unwrap();
    assert_eq!(transcript.blocks.len(), 4);
    let answer = json!({
        "overview": {"text": "다이어리의 중복 표시를 고쳤다.", "evidence": [1, 2]},
        "development": [{"title": "수령 기록 표", "detail": "표를 더했다.", "evidence": [2]}],
        "troubleshooting": [{"problem": "하루 기록이 두 번 보임", "cause": "UNIQUE 제약 오류",
            "fix": "수령 기록 표", "verification": null, "evidence": [1, 2]}],
        "decisions": [{"decision": "수령 기록 표를 둔다", "reason": null, "rejected": [],
            "proposedBy": "ai", "proposalEvidence": [{"block": 2, "quote": "수령 기록 표를 더해 고쳤습니다."}],
            "decidedBy": "human_approved", "decisionEvidence": [{"block": 4, "quote": "좋아, 그렇게 가자"}],
            "evidence": []}],
        "nextSteps": [{"text": "실제 앱에서 확인", "evidence": [3]}]
    });
    summarize(&vault, &transcript, "m", "medium", |_, _| {
        Ok(Reply {
            text: answer.to_string(),
            usage: Usage::default(),
            seconds: 1.0,
        })
    })
    .unwrap();
    assert_eq!(store.import_daily_records(&vault).unwrap().added, 1);

    let mut agent = sample();
    agent.repo_path = work.to_string_lossy().into();
    crate::record_inbox::enqueue(&data, &agent).unwrap();
    crate::record_inbox::drain(&data, &store).unwrap();

    let manual_id = store
        .save_entry(EntryInput {
            id: None,
            project_id: Some(project.clone()),
            date: "2026-11-25".into(),
            kind: "note".into(),
            title: "직접 쓴 메모".into(),
            body: format!("UNIQUE를 다시 볼 것.\n{}", "나".repeat(2500)),
            source_url: String::new(),
        })
        .unwrap()
        .id;
    let daily_id = store
        .list_entries("2026-11-24", "2026-11-24", Some(&project), 0)
        .unwrap()[0]
        .id
        .clone();
    drop(store);
    Fixture {
        dir,
        data,
        vault,
        work,
        project,
        group,
        daily_id,
        manual_id,
    }
}

impl Fixture {
    fn read(&self, args: &[&str]) -> Result<Value, String> {
        let mut all: Vec<String> = vec!["record".into()];
        all.extend(args.iter().map(|a| a.to_string()));
        all.extend(["--data-dir".into(), self.data.to_string_lossy().into()]);
        execute(&all)
    }
    fn ok(&self, args: &[&str]) -> Value {
        let value = self.read(args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
        assert_eq!(value["outcome"], "ok", "{value}");
        value
    }
}

/// The walk of the design: find a fix in the records, read its evidence, then what was
/// said, then the line of the original the transcript cut.
#[test]
fn a_fix_is_followed_from_the_records_to_the_original_line() {
    let f = fixture();
    let projects = f.ok(&["projects"]);
    let row = &projects["projects"][0];
    assert_eq!(row["id"], f.project.as_str());
    assert_eq!(row["capture"], true);
    assert_eq!(row["records"], json!({"agent": 1, "daily": 1, "manual": 1}));
    assert_eq!(
        (row["originals"].clone(), row["dailyDays"].clone()),
        (json!(1), json!(1))
    );

    let work = f.work.to_string_lossy().into_owned();
    let found = f.ok(&["search", "--project", &work, "--query", "두 번"]);
    assert_eq!(found["total"], 1);
    let hit = &found["records"][0];
    assert_eq!(
        (hit["id"].as_str(), hit["origin"].as_str()),
        (Some(f.daily_id.as_str()), Some("daily"))
    );
    assert!(
        hit["excerpts"][0].as_str().unwrap().contains("두 번"),
        "{hit}"
    );

    let shown = f.ok(&["show", "--id", &f.daily_id]);
    assert_eq!(shown["edited"], false);
    assert_eq!(
        shown["daily"]["file"],
        format!("daily/{}/{DAY}.json", f.group)
    );
    assert!(shown["body"].as_str().unwrap().contains("## 트러블슈팅"));
    assert_eq!(shown["next"], Value::Null);

    let evidence = f.ok(&["show", "--id", &f.daily_id, "--evidence"]);
    let sections: Vec<&str> = evidence["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["section"].as_str().unwrap())
        .collect();
    assert_eq!(
        sections,
        [
            "overview",
            "development",
            "troubleshooting",
            "decision",
            "nextStep"
        ]
    );
    assert_eq!(evidence["items"][2]["blocks"], json!([1, 2]));
    assert_eq!(evidence["items"][3]["decidedBy"], "사람 승인");
    let block2 = evidence["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["block"] == 2)
        .unwrap();
    assert_eq!(
        (block2["file"].as_str(), block2["line"].as_u64()),
        (Some(FILE), Some(2))
    );
    assert_eq!(block2["speaker"], "AI");

    let around = f.ok(&[
        "transcript",
        "--project",
        &f.project,
        "--date",
        DAY,
        "--file",
        FILE,
        "--line",
        "2",
        "--context",
        "1",
    ]);
    assert_eq!(around["focus"], 2);
    let numbers: Vec<u64> = around["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["number"].as_u64().unwrap())
        .collect();
    assert_eq!(numbers, [1, 2, 3]);
    assert_eq!(around["blocks"][0]["speaker"], "사람");
    assert_eq!(around["blocks"][2]["clipped"], true);

    let line = around["blocks"][2]["line"].as_u64().unwrap().to_string();
    let original = f.ok(&[
        "original",
        "--project",
        &work,
        "--file",
        FILE,
        "--line",
        &line,
    ]);
    let text = original["lines"][0]["text"].as_str().unwrap();
    assert!(text.contains(&"가".repeat(5000)), "the whole row is given");
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap(),
        session_rows()[2]
    );
}

/// Each page says where the next begins, and reading on gives every item once.
#[test]
fn pages_go_on_where_the_last_one_stopped() {
    let f = fixture();
    let mut ids = Vec::new();
    let mut offset = "0".to_string();
    loop {
        let page = f.ok(&[
            "list",
            "--project",
            &f.project,
            "--max-chars",
            "1000",
            "--offset",
            &offset,
        ]);
        assert_eq!(page["total"], 3);
        ids.extend(
            page["records"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["id"].clone()),
        );
        match page["next"]["offset"].as_u64() {
            Some(next) => offset = next.to_string(),
            None => break,
        }
    }
    assert_eq!(ids.len(), 3);
    assert!(ids.windows(2).all(|w| w[0] != w[1]));
    // Newest first.
    assert_eq!(ids[0], f.manual_id.as_str());

    let mut body = String::new();
    let mut offset = "0".to_string();
    loop {
        let page = f.ok(&[
            "show",
            "--id",
            &f.manual_id,
            "--max-chars",
            "1000",
            "--offset",
            &offset,
        ]);
        body += page["body"].as_str().unwrap();
        match page["next"]["offset"].as_u64() {
            Some(next) => offset = next.to_string(),
            None => break,
        }
    }
    assert_eq!(body, format!("UNIQUE를 다시 볼 것.\n{}", "나".repeat(2500)));

    let first = f.ok(&[
        "original",
        "--project",
        &f.project,
        "--file",
        FILE,
        "--line",
        "3",
        "--max-chars",
        "1000",
    ]);
    assert_eq!(first["lines"][0]["clipped"], true);
    assert_eq!(
        first["next"],
        json!({"line": 3, "offset": 1000, "lines": 1})
    );
    let rest = f.ok(&[
        "original",
        "--project",
        &f.project,
        "--file",
        FILE,
        "--line",
        "3",
        "--offset",
        "1000",
        "--max-chars",
        "1000000",
    ]);
    let whole = first["lines"][0]["text"].as_str().unwrap().to_owned()
        + rest["lines"][0]["text"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&whole).unwrap(),
        session_rows()[2]
    );
    assert_eq!(rest["next"], Value::Null);
}

#[test]
fn records_are_chosen_by_origin_date_and_words() {
    let f = fixture();
    let manual = f.ok(&["list", "--origin", "manual"]);
    assert_eq!(manual["total"], 1);
    let both = f.ok(&[
        "list",
        "--origin",
        "daily,agent",
        "--from",
        "2026-11-01",
        "--to",
        "2026-11-30",
    ]);
    assert_eq!(both["total"], 1, "the agent record is of 2026-10-31");
    assert!(f
        .read(&["list", "--origin", "bogus"])
        .unwrap_err()
        .contains("daily"));
    assert!(f
        .read(&["list", "--from", "2026-11-30", "--to", "2026-11-01"])
        .unwrap_err()
        .contains("늦습니다"));
    // Two letters: no trigram, the text is compared, ignoring the case of ASCII letters.
    let short = f.ok(&["search", "--query", "un"]);
    let mut origins: Vec<&str> = short["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["origin"].as_str().unwrap())
        .collect();
    origins.sort();
    assert_eq!(origins, ["daily", "manual"]);
    // Words that start like an argument are words.
    assert_eq!(f.ok(&["search", "--query", "--없는말"])["total"], 0);
}

#[test]
fn the_dialogue_of_several_days_is_searched_in_order_and_in_pages() {
    let f = fixture();
    let found = f.ok(&[
        "transcript",
        "--project",
        &f.project,
        "--from",
        "2026-11-23",
        "--to",
        "2026-11-25",
        "--query",
        "unique",
    ]);
    assert_eq!(found["daysRead"], 3);
    assert_eq!(found["blocks"].as_array().unwrap().len(), 1);
    assert_eq!(found["blocks"][0]["date"], DAY);
    assert_eq!(found["next"], Value::Null);

    let paged = f.ok(&[
        "transcript",
        "--project",
        &f.project,
        "--from",
        "2026-11-23",
        "--to",
        "2026-11-25",
        "--query",
        "가",
        "--max-chars",
        "1000",
    ]);
    // Block 3 is longer than the page and is given alone; block 4 waits for the next.
    assert_eq!(paged["blocks"].as_array().unwrap().len(), 1);
    assert_eq!(paged["blocks"][0]["number"], 3);
    assert_eq!(paged["next"], json!({"from": DAY, "offset": 1}));
    let rest = f.ok(&[
        "transcript",
        "--project",
        &f.project,
        "--from",
        DAY,
        "--to",
        "2026-11-25",
        "--query",
        "가",
        "--offset",
        "1",
    ]);
    assert_eq!(rest["blocks"][0]["number"], 4);
    assert_eq!(rest["next"], Value::Null);

    for (args, why) in [
        (
            vec!["--from", "2026-10-01", "--to", "2026-11-25", "--query", "x"],
            "31일",
        ),
        (
            vec!["--from", "2026-11-23", "--to", "2026-11-25"],
            "--query",
        ),
        (vec!["--from", "2026-11-23"], "함께"),
        (vec!["--date", DAY, "--file", FILE], "함께"),
        (vec!["--date", DAY, "--context", "2"], "--context"),
        (
            vec![
                "--from",
                "2026-11-23",
                "--to",
                "2026-11-25",
                "--query",
                "x",
                "--context",
                "abc",
            ],
            "--context",
        ),
    ] {
        let mut all = vec!["transcript", "--project", f.project.as_str()];
        all.extend(args);
        let error = f.read(&all).unwrap_err();
        assert!(error.contains(why), "{error}");
    }
}

/// While the app owns the data folder the commands still read, and they change nothing
/// in it or in the store.
#[test]
fn reading_takes_no_lock_and_leaves_the_data_and_the_store_as_they_were() {
    let f = fixture();
    let owner = timeschedule_desktop::paths::InstanceLock::acquire(&f.data).unwrap();
    let db = f.data.join("schedule.db");
    let before = std::fs::read(&db).unwrap();
    let listing = |root: &Path| -> Vec<(PathBuf, u64)> {
        fn walk(dir: &Path, out: &mut Vec<(PathBuf, u64)>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push((path.clone(), std::fs::metadata(&path).unwrap().len()));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, &mut out);
        out.sort();
        out
    };
    let store_before = listing(&f.vault);
    f.ok(&["projects"]);
    f.ok(&["list"]);
    f.ok(&["search", "--query", "두 번"]);
    f.ok(&["show", "--id", &f.daily_id, "--evidence"]);
    f.ok(&["transcript", "--project", &f.project, "--date", DAY]);
    f.ok(&[
        "original",
        "--project",
        &f.project,
        "--file",
        FILE,
        "--line",
        "1",
    ]);
    assert_eq!(std::fs::read(&db).unwrap(), before);
    assert_eq!(listing(&f.vault), store_before);
    drop(owner);

    // A result file is written only outside the store and the data folder.
    for inside in [
        f.vault.join("daily").join("out.json"),
        f.vault.join("out.json"),
        f.data.join("schedule.db"),
    ] {
        let code = run(&[
            "record",
            "list",
            "--data-dir",
            f.data.to_str().unwrap(),
            "--result",
            inside.to_str().unwrap(),
        ]
        .map(String::from));
        assert_eq!(code, 1);
    }
    assert_eq!(std::fs::read(&db).unwrap(), before);
    assert!(!f.vault.join("out.json").exists());
    let outside = f.dir.0.join("out.json");
    let code = run(&[
        "record",
        "list",
        "--data-dir",
        f.data.to_str().unwrap(),
        "--result",
        outside.to_str().unwrap(),
    ]
    .map(String::from));
    assert_eq!(code, 0);
    let written: Value = serde_json::from_slice(&std::fs::read(&outside).unwrap()).unwrap();
    assert_eq!(written["total"], 3);

    // No data folder is made to read from.
    let missing = f.dir.0.join("no-data-folder");
    let error =
        execute(&["record", "list", "--data-dir", missing.to_str().unwrap()].map(String::from))
            .unwrap_err();
    assert!(error.contains("DB 파일이 없습니다"), "{error}");
    assert!(!missing.exists());
}

#[test]
fn a_record_that_is_not_a_daily_record_has_no_evidence_and_an_agent_record_names_its_work() {
    let f = fixture();
    let error = f
        .read(&["show", "--id", &f.manual_id, "--evidence"])
        .unwrap_err();
    assert!(error.contains("하루 정리 기록이 아니어서"), "{error}");
    let agent = &f.ok(&["list", "--origin", "agent"])["records"][0];
    let shown = f.ok(&["show", "--id", agent["id"].as_str().unwrap()]);
    assert_eq!(shown["agent"]["workId"], sample().work_id.as_str());
    // The store has no catalog of it: the record says so instead of a path.
    assert!(shown["agent"]["documentError"].is_string(), "{shown}");
}

/// A kept original of the fixture's project made of `lines`, in place of the session.
fn replace_original(f: &Fixture, lines: &[String]) {
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let path = f.vault.join("originals").join(&f.group).join(FILE);
    std::fs::write(path, zstd::encode_all(text.as_bytes(), 3).unwrap()).unwrap();
}

#[test]
fn an_original_names_only_lines_that_are_there_and_keeps_the_count_asked_for() {
    let f = fixture();
    replace_original(&f, &["a".repeat(1000), "b".into(), "c".into(), "d".into()]);
    let read = |args: &[&str]| {
        let mut all = vec!["original", "--project", f.project.as_str(), "--file", FILE];
        all.extend(args);
        f.ok(&all)
    };
    // The page is full after line 1; line 2 is there, so the answer goes on to it.
    let first = read(&["--line", "1", "--lines", "3", "--max-chars", "1000"]);
    assert_eq!(first["next"], json!({"line": 2, "lines": 2}));
    let rest = read(&["--line", "2", "--lines", "2"]);
    let texts: Vec<&str> = rest["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        texts,
        ["b", "c"],
        "two lines were left of the three asked for"
    );
    // The last line fills the page: nothing is said to come after it.
    let last = read(&["--line", "4", "--lines", "2", "--max-chars", "1000"]);
    assert_eq!(last["next"], Value::Null);
}

#[test]
fn a_line_of_exactly_eight_mib_is_one_line() {
    let f = fixture();
    let limit = 8 * 1024 * 1024;
    replace_original(&f, &["y".repeat(limit), "b".into(), "c".into()]);
    let tail = f.ok(&[
        "original",
        "--project",
        &f.project,
        "--file",
        FILE,
        "--line",
        "1",
        "--offset",
        &(limit - 1).to_string(),
        "--lines",
        "2",
    ]);
    assert_eq!(tail["lines"][0]["text"], "y");
    assert_eq!(tail["lines"][0]["long"], false);
    assert_eq!(
        (
            tail["lines"][1]["line"].clone(),
            tail["lines"][1]["text"].clone()
        ),
        (json!(2), json!("b"))
    );
}

#[test]
fn evidence_is_given_only_for_the_project_and_day_it_was_made_for() {
    let f = fixture();
    let store = Storage::open(f.data.join("schedule.db")).unwrap();
    let other_folder = f.dir.0.join("other");
    std::fs::create_dir_all(&other_folder).unwrap();
    let other = store
        .save_project(ProjectInput {
            id: None,
            title: "다른 작업".into(),
            description: String::new(),
            repo_path: other_folder.to_string_lossy().into(),
        })
        .unwrap()
        .id;
    let entry = store.get_entry(&f.daily_id).unwrap();
    store
        .save_entry(EntryInput {
            id: Some(entry.id.clone()),
            project_id: Some(other.clone()),
            date: entry.date.clone(),
            kind: entry.kind.clone(),
            title: entry.title.clone(),
            body: entry.body.clone(),
            source_url: String::new(),
        })
        .unwrap();
    drop(store);
    let error = f
        .read(&["show", "--id", &f.daily_id, "--evidence"])
        .unwrap_err();
    assert!(error.contains("다른 프로젝트로 옮겨져"), "{error}");
}

#[test]
fn a_folder_of_two_projects_is_not_guessed_and_capture_follows_the_folder() {
    let f = fixture();
    let store = Storage::open(f.data.join("schedule.db")).unwrap();
    let project = store
        .list_projects()
        .unwrap()
        .into_iter()
        .find(|p| p.id == f.project)
        .unwrap();
    // The project is given another folder: the app no longer collects its chats.
    let moved = f.dir.0.join("moved");
    std::fs::create_dir_all(&moved).unwrap();
    store
        .save_project(ProjectInput {
            id: Some(project.id.clone()),
            title: project.title.clone(),
            description: String::new(),
            repo_path: moved.to_string_lossy().into(),
        })
        .unwrap();
    assert!(!store.chat_capture_enabled(&f.project).unwrap());
    for _ in 0..2 {
        store
            .save_project(ProjectInput {
                id: None,
                title: "같은 폴더".into(),
                description: String::new(),
                repo_path: f.work.to_string_lossy().into(),
            })
            .unwrap();
    }
    drop(store);
    let rows = f.ok(&["projects"]);
    let row = rows["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == f.project.as_str())
        .unwrap()
        .clone();
    assert_eq!(row["capture"], false);
    let error = f
        .read(&["list", "--project", f.work.to_str().unwrap()])
        .unwrap_err();
    assert!(error.contains("프로젝트가 2개입니다"), "{error}");
}

#[test]
fn a_crlf_line_of_exactly_eight_mib_is_not_long_and_an_unfinished_last_line_is_no_line() {
    let f = fixture();
    let limit = 8 * 1024 * 1024;
    let text = format!("{}\r\nb\r\n", "y".repeat(limit));
    let path = f.vault.join("originals").join(&f.group).join(FILE);
    std::fs::write(&path, zstd::encode_all(text.as_bytes(), 3).unwrap()).unwrap();
    let tail = f.ok(&[
        "original",
        "--project",
        &f.project,
        "--file",
        FILE,
        "--line",
        "1",
        "--offset",
        &(limit - 1).to_string(),
        "--lines",
        "2",
    ]);
    assert_eq!(tail["lines"][0]["long"], false);
    assert_eq!(tail["lines"][0]["text"], "y");
    assert_eq!(tail["lines"][1]["text"], "b");

    // A long last line the tool has not finished, after one finished line.
    let text = format!("a\n{}", "가".repeat(limit / 2));
    std::fs::write(&path, zstd::encode_all(text.as_bytes(), 3).unwrap()).unwrap();
    for line in ["2", "3"] {
        let error = f
            .read(&[
                "original",
                "--project",
                &f.project,
                "--file",
                FILE,
                "--line",
                line,
            ])
            .unwrap_err();
        assert!(error.contains("1줄입니다"), "{error}");
    }
}

#[test]
fn only_record_files_count_as_days_of_daily_records() {
    let f = fixture();
    std::fs::create_dir_all(f.vault.join("daily").join(&f.group).join("2026-11-30.json")).unwrap();
    assert_eq!(f.ok(&["projects"])["projects"][0]["dailyDays"], 1);
}
