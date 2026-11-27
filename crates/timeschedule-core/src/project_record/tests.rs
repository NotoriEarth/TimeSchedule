use super::*;
use crate::diary::ProjectInput;
use std::cell::Cell;

struct Fixture {
    root: PathBuf,
    store: Storage,
    vault: PathBuf,
    project: String,
    repo: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("project-record-{}", uuid::Uuid::new_v4()));
    let vault = root.join("vault");
    let repo = root.join("repo");
    fs::create_dir_all(&vault).unwrap();
    fs::create_dir_all(&repo).unwrap();
    let store = Storage::open(":memory:").unwrap();
    let repo = repo.to_string_lossy().into_owned();
    let project = store
        .save_project(ProjectInput {
            id: None,
            title: "앱".into(),
            description: String::new(),
            repo_path: repo.clone(),
        })
        .unwrap()
        .id;
    Fixture {
        root,
        store,
        vault,
        project,
        repo,
    }
}

impl Fixture {
    fn daily_dir(&self) -> PathBuf {
        self.vault
            .join("daily")
            .join(group_name(&repo_key(&self.repo)))
    }

    /// A daily record of `date` with one item in each list and the decision `values`.
    fn day(&self, date: &str, project: &str, values: (&str, &str)) {
        let record = json!({
            "schemaVersion": 2,
            "project": {"id": project, "title": "앱", "repoKey": repo_key(&self.repo)},
            "date": date, "revision": 1, "generatedAt": "2026-11-20T00:00:00Z",
            "model": "m", "effort": "medium", "promptVersion": 4,
            "input": {"sessions": 1, "agentRuns": 0, "blocks": 3, "chars": 10,
                      "fingerprint": "f", "latestArchived": null, "warnings": []},
            "usage": {"seconds": 1.0},
            "checks": {"withoutEvidence": 0, "quotesNotFound": 0,
                       "proposedByUnsupported": 0, "decidedByUnsupported": 0},
            "overview": {"text": format!("{date}에 한 일"), "evidence": []},
            "development": [{"title": "검색", "detail": "검색 색인을 더했다", "evidence": []}],
            "troubleshooting": [{"problem": "느림", "cause": "색인 없음", "fix": null,
                                 "verification": "검색 0.3초", "evidence": []}],
            "decisions": [{"decision": "SQLite FTS를 쓴다", "reason": null, "rejected": ["외부 검색 서버"],
                           "proposedBy": values.0, "proposalEvidence": [],
                           "decidedBy": values.1, "decisionEvidence": [], "evidence": []}],
            "nextSteps": [{"text": "정렬", "evidence": []}],
            "sources": []
        });
        fs::create_dir_all(self.daily_dir()).unwrap();
        fs::write(
            self.daily_dir().join(format!("{date}.json")),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    }

    fn entry(&self, date: &str, title: &str, body: &str, source_ref: Option<&str>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.store
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_ref,source_body,updated_at)
                 VALUES(?1,?2,?3,'development',?4,?5,?6,?5,'2026-11-20T00:00:00Z')",
                rusqlite::params![id, self.project, date, title, body, source_ref],
            )
            .unwrap();
        id
    }

    fn dossier(&self) -> Dossier {
        gather(&self.store, &self.vault, &self.project).unwrap()
    }
}

fn reply(answer: Value) -> Result<Reply, Failed> {
    Ok(Reply {
        text: answer.to_string(),
        usage: Usage::default(),
        seconds: 2.0,
    })
}

fn answer(decision_keys: &[&str], result: &str, result_keys: &[&str]) -> Value {
    json!({
        "overview": {"text": "검색이 되는 앱", "evidence": ["D:2026-11-20:개요:1"]},
        "features": [
            {"title": "검색", "detail": "색인", "evidence": ["D:2026-11-20:개발:1"]},
            {"title": "근거 없음", "detail": "빠진다", "evidence": []}
        ],
        "decisions": [{"decision": "FTS", "reason": null, "rejected": [], "evidence": decision_keys}],
        "troubleshooting": [],
        "results": [{"text": result, "evidence": result_keys}],
        "limits": []
    })
}

#[test]
fn the_bundle_holds_the_project_daily_records_in_order_and_its_records_without_chat_excerpts() {
    let f = fixture();
    f.day("2026-11-21", &f.project, ("ai", "human_approved"));
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    f.day("2026-11-22", "someone-else", ("ai", "human_approved"));
    let agent = f.entry("2026-11-21", "자동", "개발 본문", Some("space-work:1"));
    let manual = f.entry("2026-11-19", "손", "직접 쓴 본문", None);
    f.entry("2026-11-21", "채팅", "채팅 발췌 본문", Some("codex-chat:1"));
    f.entry(
        "2026-11-21",
        "하루",
        "하루 정리 본문",
        Some("daily-record:1"),
    );
    f.store
        .set_project_completed(&f.project, Some("2026-12-01"))
        .unwrap();
    let d = f.dossier();
    let days: Vec<&str> = d.days.iter().map(|r| r.date.as_str()).collect();
    assert_eq!(days, ["2026-11-20", "2026-11-21"]);
    assert!(
        d.warnings.iter().any(|w| w.starts_with("2026-11-22.json")),
        "{:?}",
        d.warnings
    );
    let ids: Vec<&str> = d.records.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, [manual.as_str(), agent.as_str()]);
    let text = d.render();
    assert!(text.contains("상태: 완료 · 2026-12-01"));
    assert!(text.contains("[D:2026-11-20:결정:1] SQLite FTS를 쓴다 / 이유: 이유 기록 없음 / 버린 대안: 외부 검색 서버 / 제안: AI / 확정: 사람 승인"));
    assert!(text.contains(&format!(
        "### [R:{agent}] 2026-11-21 · 자동 개발 기록 · 자동"
    )));
    assert!(!text.contains("채팅 발췌 본문") && !text.contains("하루 정리 본문"));
    // No originals are kept, so the days with dialogue could not be counted: said.
    assert!(d.missing_days.is_empty());
    assert!(d
        .warnings
        .iter()
        .any(|w| w.starts_with("대화가 있는 날을 세지 못했습니다")));
}

#[test]
fn a_project_without_a_folder_has_no_bundle() {
    let f = fixture();
    let id = f
        .store
        .save_project(ProjectInput {
            id: None,
            title: "폴더 없음".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap()
        .id;
    assert!(gather(&f.store, &f.vault, &id).is_err());
}

#[test]
fn an_answer_citing_a_key_the_bundle_lacks_is_refused_whole_and_logged() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let d = f.dossier();
    let error = summarize(&f.vault, &d, "m", "high", |_, _| {
        reply(answer(
            &["D:2026-11-20:결정:9"],
            "x",
            &["D:2026-11-20:개요:1"],
        ))
    })
    .unwrap_err();
    assert!(error.contains("D:2026-11-20:결정:9"), "{error}");
    let dir = f.vault.join("project").join(&d.folder).join(&d.project_id);
    assert!(!dir.join("record.json").exists());
    let usage = fs::read_to_string(dir.join("usage.jsonl")).unwrap();
    assert!(usage.contains("\"outcome\":\"refused\""), "{usage}");
}

#[test]
fn a_decision_carries_the_cited_daily_decisions_word_for_word_and_no_confirmation_of_its_own() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    f.day("2026-11-21", &f.project, ("human", "human_decided"));
    let (_, index) = f.dossier().render_indexed();
    // The model's sentence says the opposite of what was approved: it is kept as the
    // model's words, and the approval stays on the daily decision copied beside it.
    let mut a = answer(
        &["D:2026-11-20:결정:1", "D:2026-11-21:결정:1"],
        "x",
        &["D:2026-11-20:개요:1"],
    );
    a["decisions"][0]["decision"] = "외부 검색 서버를 쓴다".into();
    let body = check(&index, &a.to_string()).unwrap();
    let d = &body.decisions[0];
    assert_eq!(d.decision, "외부 검색 서버를 쓴다");
    let copies: Vec<(&str, &str, Proposer, Decider)> = d
        .daily
        .iter()
        .map(|c| {
            (
                c.date.as_str(),
                c.decision.as_str(),
                c.proposed_by,
                c.decided_by,
            )
        })
        .collect();
    assert_eq!(
        copies,
        [
            (
                "2026-11-20",
                "SQLite FTS를 쓴다",
                Proposer::Ai,
                Decider::HumanApproved
            ),
            (
                "2026-11-21",
                "SQLite FTS를 쓴다",
                Proposer::Human,
                Decider::HumanDecided
            ),
        ]
    );
    assert_eq!(d.daily[0].rejected, ["외부 검색 서버"]);
    // A decision that cites no daily decision has no confirmation on record.
    let body = check(
        &index,
        &answer(&["D:2026-11-20:개발:1"], "x", &["D:2026-11-20:개요:1"]).to_string(),
    )
    .unwrap();
    assert!(body.decisions[0].daily.is_empty());
    assert_eq!(body.checks.decisions_without_daily, 1);
}

#[test]
fn a_reason_or_rejected_option_stays_only_when_a_cited_text_has_it_word_for_word() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let rec = f.entry(
        "2026-11-21",
        "결정 메모",
        "FTS로 한 이유는  설치할 서버가\n없어서다.",
        None,
    );
    let (_, index) = f.dossier().render_indexed();
    let decide = |reason: Value, rejected: &[&str]| {
        let mut a = answer(
            &["D:2026-11-20:결정:1", &format!("R:{rec}")],
            "x",
            &["D:2026-11-20:개요:1"],
        );
        a["decisions"][0]["reason"] = reason;
        a["decisions"][0]["rejected"] = json!(rejected);
        let body = check(&index, &a.to_string()).unwrap();
        (body.decisions[0].clone(), body.checks)
    };
    // Quoted from the record, whatever the blanks between its words.
    let (d, checks) = decide(json!("설치할 서버가 없어서다"), &["외부 검색 서버"]);
    assert_eq!(d.reason.as_deref(), Some("설치할 서버가 없어서다"));
    assert_eq!(d.rejected, ["외부 검색 서버"]);
    assert_eq!(
        (checks.reasons_unsupported, checks.rejected_unsupported),
        (0, 0)
    );
    // Words of the daily decision's own sentence are not a rejected option of it, and a
    // missing reason written with other blanks is still no reason.
    let (d, checks) = decide(json!("이유  기록 없음"), &["SQLite FTS"]);
    assert_eq!(d.reason, None);
    assert!(d.rejected.is_empty());
    assert_eq!(
        (checks.reasons_unsupported, checks.rejected_unsupported),
        (1, 1)
    );
    // Written by the model: not kept.
    let (d, checks) = decide(json!("유지비를 줄이려고"), &["Elasticsearch"]);
    assert_eq!(d.reason, None);
    assert!(d.rejected.is_empty());
    assert_eq!(
        (checks.reasons_unsupported, checks.rejected_unsupported),
        (1, 1)
    );
    // The words the bundle shows for a missing reason are no reason.
    let (d, _) = decide(json!("이유 기록 없음"), &[]);
    assert_eq!(d.reason, None);
    let (d, checks) = decide(Value::Null, &[]);
    assert_eq!((d.reason, checks.reasons_unsupported), (None, 0));
}

#[test]
fn a_result_keeps_its_numbers_only_when_a_cited_text_has_them_and_uncited_items_go() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let (_, index) = f.dossier().render_indexed();
    let trouble = ["D:2026-11-20:트러블:1"];
    let body = check(
        &index,
        &answer(&["D:2026-11-20:결정:1"], "0.3초", &trouble).to_string(),
    )
    .unwrap();
    assert_eq!(body.decisions[0].reason, None);
    // The feature that cites nothing is left out.
    assert_eq!(body.features.len(), 1);
    assert_eq!(body.checks.without_evidence, 1);
    // So is a decision that cites nothing.
    let body = check(&index, &answer(&[], "0.3초", &trouble).to_string()).unwrap();
    assert!(body.decisions.is_empty());
    assert_eq!(body.checks.without_evidence, 2);
    for (text, kept) in [
        ("검색이 0.3초 걸린다", 1),
        ("검색이 0.30초", 0),
        ("3배 빠르다", 0),
        ("빨라졌다", 1),
    ] {
        let body = check(
            &index,
            &answer(&["D:2026-11-20:결정:1"], text, &trouble).to_string(),
        )
        .unwrap();
        assert_eq!(body.results.len(), kept, "{text}");
        assert_eq!(body.checks.results_unsupported, 1 - kept, "{text}");
    }
    assert_eq!(
        numbers("1,200건과 3.5배, 2026-11-20"),
        ["1,200", "3.5", "2026", "11", "20"]
    );
    assert_eq!(
        numbers("-3에서 +3, \u{2212}2, 버전-2, v1-2"),
        ["-3", "+3", "-2", "-2", "1", "2"]
    );
    assert_eq!(numbers("편차\u{2212}3%와 편차+3%"), ["-3", "+3"]);
    // Digits beside a full-width one are part of a number that is not read.
    assert_eq!(numbers("\u{FF11}2초, 7초"), ["7"]);
    assert_eq!(
        numbers("\u{FF11}.25초, 1,\u{FF12}\u{FF10}\u{FF10}건, 9건"),
        ["9"]
    );
    // A sign is part of the number, and a digit that cannot be read keeps the result out.
    for text in ["+0.3초", "\u{FF19}\u{FF19}% 단축"] {
        let body = check(
            &index,
            &answer(&["D:2026-11-20:결정:1"], text, &trouble).to_string(),
        )
        .unwrap();
        assert!(body.results.is_empty(), "{text}");
    }
}

#[test]
fn a_kept_record_is_not_asked_again_and_a_completion_makes_the_next_revision() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let good = || {
        reply(answer(
            &["D:2026-11-20:결정:1"],
            "0.3초",
            &["D:2026-11-20:트러블:1"],
        ))
    };
    let project = || f.store.project_of(&f.project).unwrap();
    assert_eq!(kept_completion(&f.vault, &project()).unwrap(), None);
    let d = f.dossier();
    let Outcome::Saved { path, record } = summarize(&f.vault, &d, "m", "high", |prompt, schema| {
        assert!(prompt.contains("상태: 진행 중"));
        assert_eq!(schema, &super::schema());
        good()
    })
    .unwrap() else {
        panic!("saved")
    };
    assert_eq!(record.revision, 1);
    assert_eq!(record.completed_on, None);
    assert_eq!(kept_completion(&f.vault, &project()).unwrap(), Some(None));
    assert_eq!(record.decisions[0].daily[0].proposed_by, Proposer::Ai);
    assert!(is_current(&f.vault, &d, "m", "high").unwrap());
    let asked = Cell::new(false);
    let again = summarize(&f.vault, &d, "m", "high", |_, _| {
        asked.set(true);
        good()
    })
    .unwrap();
    assert!(matches!(again, Outcome::Unchanged { revision: 1, .. }));
    assert!(!asked.get());
    // Completing the project changes the question: a second revision, the first set aside.
    f.store
        .set_project_completed(&f.project, Some("2026-12-01"))
        .unwrap();
    let d = f.dossier();
    assert!(!is_current(&f.vault, &d, "m", "high").unwrap());
    let Outcome::Saved { record, .. } =
        summarize(&f.vault, &d, "m", "high", |_, _| good()).unwrap()
    else {
        panic!("saved")
    };
    assert_eq!(record.revision, 2);
    assert_eq!(record.completed_on.as_deref(), Some("2026-12-01"));
    assert_eq!(
        kept_completion(&f.vault, &project()).unwrap(),
        Some(Some("2026-12-01".into()))
    );
    let dir = path.parent().unwrap();
    let first: ProjectRecord =
        serde_json::from_slice(&fs::read(dir.join("record.r1.json")).unwrap()).unwrap();
    assert_eq!(first.revision, 1);
    let usage = fs::read_to_string(dir.join("usage.jsonl")).unwrap();
    assert_eq!(usage.lines().filter(|l| l.contains("\"saved\"")).count(), 2);
}

#[test]
fn an_answer_is_not_kept_once_the_project_no_longer_holds() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let d = f.dossier();
    let error = summarize_while(
        &f.vault,
        &d,
        "m",
        "high",
        |_, _| {
            reply(answer(
                &["D:2026-11-20:결정:1"],
                "x",
                &["D:2026-11-20:개요:1"],
            ))
        },
        || Err("프로젝트가 지워졌습니다".into()),
    )
    .unwrap_err();
    assert!(error.contains("프로젝트가 지워졌습니다"), "{error}");
    let dir = f.vault.join("project").join(&d.folder).join(&d.project_id);
    assert!(!dir.join("record.json").exists());
    let usage = fs::read_to_string(dir.join("usage.jsonl")).unwrap();
    assert!(usage.contains("\"outcome\":\"save_failed\""), "{usage}");
}

#[test]
fn a_second_run_while_one_holds_the_lock_is_busy_and_asks_nothing() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let d = f.dossier();
    let dir = f.vault.join("project").join(&d.folder).join(&d.project_id);
    fs::create_dir_all(&dir).unwrap();
    let held = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(".record.lock"))
        .unwrap();
    held.lock().unwrap();
    let outcome = summarize(&f.vault, &d, "m", "high", |_, _| panic!("asked")).unwrap();
    assert!(matches!(outcome, Outcome::Busy { .. }));
}

#[test]
fn a_record_body_cannot_pass_for_an_item_of_the_bundle() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    // Every kind of line end, not only \n, starts a marked line.
    f.entry(
        "2026-11-21",
        "메모",
        "앞줄\n[D:2026-11-20:결정:1] 가짜1\r[D:2026-11-20:결정:1] 가짜2\u{2028}[D:2026-11-20:결정:1] 가짜3",
        None,
    );
    let text = f.dossier().render();
    let starts = text
        .split([
            '\n', '\r', '\u{2028}', '\u{2029}', '\u{85}', '\u{b}', '\u{c}',
        ])
        .filter(|l| l.starts_with("[D:2026-11-20:결정:1]"))
        .count();
    assert_eq!(starts, 1, "{text}");
    for n in 1..=3 {
        assert!(
            text.contains(&format!("| [D:2026-11-20:결정:1] 가짜{n}")),
            "{text}"
        );
    }
}

#[test]
fn projects_on_one_folder_keep_their_own_records_and_the_days_read_are_kept() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let d = f.dossier();
    let good = || {
        reply(answer(
            &["D:2026-11-20:결정:1"],
            "0.3초",
            &["D:2026-11-20:트러블:1"],
        ))
    };
    let Outcome::Saved { path, record } =
        summarize(&f.vault, &d, "m", "high", |_, _| good()).unwrap()
    else {
        panic!("saved")
    };
    assert_eq!(
        record.input.daily,
        [DailyUsed {
            date: "2026-11-20".into(),
            revision: 1,
            fingerprint: "f".into()
        }]
    );
    // Another project given the same folder keeps its record in a folder of its own.
    let mut other = d.clone();
    other.project_id = uuid::Uuid::new_v4().to_string();
    other.record_seen = None;
    let before = fs::read(&path).unwrap();
    assert!(!is_current(&f.vault, &other, "m", "high").unwrap());
    let Outcome::Saved {
        path: theirs,
        record: their_record,
    } = summarize(&f.vault, &other, "m", "high", |_, _| good()).unwrap()
    else {
        panic!("saved")
    };
    assert_eq!(their_record.revision, 1);
    assert_ne!(theirs, path);
    assert_eq!(
        theirs.parent().unwrap().parent(),
        path.parent().unwrap().parent()
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    // A file copied in from another project's folder is not replaced.
    fs::copy(&path, &theirs).unwrap();
    let error = summarize(&f.vault, &other, "m", "high", |_, _| panic!("asked")).unwrap_err();
    assert!(error.contains("다른 프로젝트의 기록"), "{error}");
    assert!(is_current(&f.vault, &other, "m", "high").is_err());
}

#[test]
fn a_run_that_read_its_input_before_a_newer_record_was_made_does_not_replace_it() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let good = || {
        reply(answer(
            &["D:2026-11-20:결정:1"],
            "0.3초",
            &["D:2026-11-20:트러블:1"],
        ))
    };
    // One run reads its input; then a record is added and another run saves first.
    let slow = f.dossier();
    f.entry("2026-11-21", "최종 검증", "모두 통과", None);
    let fast = f.dossier();
    let Outcome::Saved { path, .. } =
        summarize(&f.vault, &fast, "m", "high", |_, _| good()).unwrap()
    else {
        panic!("saved")
    };
    let newer = fs::read(&path).unwrap();
    let error = summarize(&f.vault, &slow, "m", "high", |_, _| panic!("asked")).unwrap_err();
    assert!(error.contains("새로 만들어졌습니다"), "{error}");
    assert_eq!(fs::read(&path).unwrap(), newer);
    assert!(!path.with_file_name("record.r1.json").exists());
}

#[test]
fn a_bundle_with_nothing_to_cite_is_not_sent() {
    let f = fixture();
    // A daily record kept with every item left out.
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let path = f.daily_dir().join("2026-11-20.json");
    let mut empty: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for list in ["development", "troubleshooting", "decisions", "nextSteps"] {
        empty[list] = json!([]);
    }
    empty.as_object_mut().unwrap().remove("overview");
    fs::write(&path, serde_json::to_vec(&empty).unwrap()).unwrap();
    let d = f.dossier();
    assert_eq!((d.days.len(), d.citable()), (1, 0));
    let error = summarize(&f.vault, &d, "m", "high", |_, _| panic!("asked")).unwrap_err();
    assert!(error.contains("근거로 들"), "{error}");
}

#[test]
fn a_bundle_too_large_is_not_sent() {
    assert!(check_size(10).is_ok());
    assert!(check_size(1_000_000).is_err());
}

#[test]
fn a_project_deleted_or_moved_after_its_bundle_was_read_no_longer_holds() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let d = f.dossier();
    assert!(holds(&f.store, &d).is_ok());
    let other = f.root.join("other");
    fs::create_dir_all(&other).unwrap();
    f.store
        .save_project(ProjectInput {
            id: Some(f.project.clone()),
            title: "앱".into(),
            description: String::new(),
            repo_path: other.to_string_lossy().into_owned(),
        })
        .unwrap();
    assert!(holds(&f.store, &d).is_err());
    f.store.delete_project(&f.project).unwrap();
    assert!(holds(&f.store, &d).is_err());
}

#[test]
fn a_project_made_again_for_a_deleted_ones_folder_makes_its_own_record() {
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let Outcome::Saved { path: old, .. } =
        summarize(&f.vault, &f.dossier(), "m", "high", |_, _| {
            reply(answer(
                &["D:2026-11-20:결정:1"],
                "x",
                &["D:2026-11-20:개요:1"],
            ))
        })
        .unwrap()
    else {
        panic!("saved")
    };
    let kept = fs::read(&old).unwrap();
    f.store.delete_project(&f.project).unwrap();
    let next = f
        .store
        .save_project(ProjectInput {
            id: None,
            title: "새 앱".into(),
            description: String::new(),
            repo_path: f.repo.clone(),
        })
        .unwrap()
        .id;
    let note = uuid::Uuid::new_v4().to_string();
    f.store
        .connection()
        .unwrap()
        .execute(
            "INSERT INTO diary_entries(id,project_id,date,kind,title,body,updated_at)
             VALUES(?1,?2,'2026-11-25','note','메모','새 앱을 시작했다','2026-11-25T00:00:00Z')",
            rusqlite::params![note, next],
        )
        .unwrap();
    let d = gather(&f.store, &f.vault, &next).unwrap();
    let Outcome::Saved { path, record } = summarize(&f.vault, &d, "m", "high", |_, _| {
        reply(json!({
            "overview": {"text": "새 앱", "evidence": [format!("R:{note}")]},
            "features": [], "decisions": [], "troubleshooting": [], "results": [], "limits": []
        }))
    })
    .unwrap() else {
        panic!("saved")
    };
    assert_eq!(
        (record.revision, record.project.id.as_str()),
        (1, next.as_str())
    );
    assert_ne!(path, old);
    // The deleted project's record stays where it was, untouched.
    assert_eq!(fs::read(&old).unwrap(), kept);
}

/// The store is reached through a junction, and while the model answers the junction is
/// pointed at another folder that has the same `project/<folder>/<project ID>` in it.
/// The record and the usage line go to the store the run started in, and nothing is made
/// in the other folder.
#[cfg(windows)]
#[test]
fn a_junction_above_the_store_pointed_elsewhere_while_the_model_answers_is_not_followed() {
    let point = |link: &Path, target: &Path| {
        if link.exists() {
            fs::remove_dir(link).unwrap();
        }
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
    };
    let f = fixture();
    f.day("2026-11-20", &f.project, ("ai", "human_approved"));
    let link = f.root.join("link");
    point(&link, &f.vault);
    let d = gather(&f.store, &link, &f.project).unwrap();
    let other = f.root.join("other");
    let other_dir = other.join("project").join(&d.folder).join(&d.project_id);
    fs::create_dir_all(&other_dir).unwrap();
    let outcome = summarize(&link, &d, "m", "high", |_, _| {
        point(&link, &other);
        reply(answer(
            &["D:2026-11-20:결정:1"],
            "x",
            &["D:2026-11-20:개요:1"],
        ))
    })
    .unwrap();
    let made_there: Vec<_> = fs::read_dir(&other_dir).unwrap().collect();
    fs::remove_dir(&link).unwrap();
    assert!(made_there.is_empty(), "{made_there:?}");
    assert!(matches!(outcome, Outcome::Saved { .. }));
    let dir = f.vault.join("project").join(&d.folder).join(&d.project_id);
    assert!(dir.join("record.json").is_file());
    assert!(dir.join("usage.jsonl").is_file());
}
