use std::{
    fs,
    path::{Path, PathBuf},
};
use timeschedule_core::{
    diary::{EntryInput, ProjectInput},
    project_records::ProjectRecord,
    storage::Storage,
};
use uuid::Uuid;

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("timeschedule-portfolio-{}", Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn record(repo: &Path, kind: &str, body: &str) -> ProjectRecord {
    ProjectRecord {
        schema_version: 2,
        work_id: Uuid::new_v4().to_string(),
        revision: 1,
        project_title: "TimeSchedule".into(),
        repo_path: repo.to_str().unwrap().into(),
        date: "2026-11-19".into(),
        kind: kind.into(),
        title: format!("{kind} 시험 기록"),
        summary: "프로젝트 기록에서 확인한 변경.".into(),
        space_url: String::new(),
        status: "verification_pending".into(),
        delivery_state: "local".into(),
        detail: body.into(),
        started_at: None,
        completed_at: None,
        commits: vec![],
    }
}
fn manual(project: &str, kind: &str, body: &str) -> EntryInput {
    EntryInput {
        id: None,
        project_id: Some(project.into()),
        kind: kind.into(),
        date: "2026-11-19".into(),
        title: "직접 기록".into(),
        body: body.into(),
        source_url: String::new(),
    }
}

#[test]
fn delivered_experiences_answers_and_edited_markdown_survive_retry_restart_and_revision() {
    let dir = Dir::new();
    let db = dir.0.join("schedule.db");
    let store = Storage::open(&db).unwrap();
    let design = record(
        &dir.0,
        "design",
        "기술 선택·설계: 기록 원문을 먼저 보관했다.",
    );
    let mut fix = record(
        &dir.0,
        "troubleshooting",
        "## 검증\n두 건에서 복구했다.\n같은 Windows 장치에서만 확인했다.",
    );
    let a = store
        .apply_project_record(&design)
        .unwrap()
        .entry_id
        .unwrap();
    let b = store.apply_project_record(&fix).unwrap().entry_id.unwrap();
    assert_eq!(
        store.apply_project_record(&design).unwrap().outcome,
        "unchanged"
    );
    let id = store.get_entry(&a).unwrap().project_id.unwrap();
    let panel = store.portfolio_panel(&id).unwrap();
    assert_eq!(panel.experiences, 2);
    assert!(panel.needs_answer);
    assert!(store.export_portfolio(&id, &dir.0).is_err());
    store.save_portfolio_answer(&id, None, "").unwrap();
    let first = store.export_portfolio(&id, &dir.0).unwrap();
    let content = fs::read_to_string(&first).unwrap();
    let evidence = fs::read_to_string(first.with_file_name("evidence.md")).unwrap();
    for entry in [&a, &b] {
        assert!(content.contains(&format!("[기록 근거](evidence.md#entry-{entry})")));
        assert!(evidence.contains(&format!("id=\"entry-{entry}\"")));
    }
    assert!(content.contains("같은 Windows 장치에서만 확인했다."));
    assert!(evidence.contains(&fix.work_id));
    fs::write(&first, "사용자가 고친 포트폴리오").unwrap();
    drop(store);
    let store = Storage::open(&db).unwrap();
    assert_eq!(
        store.portfolio_panel(&id).unwrap().answer.as_deref(),
        Some("")
    );
    assert!(!store.portfolio_panel(&id).unwrap().needs_answer);
    assert_eq!(store.export_portfolio(&id, &dir.0).unwrap(), first);
    assert_eq!(
        fs::read_to_string(&first).unwrap(),
        "사용자가 고친 포트폴리오"
    );
    fix.revision = 2;
    fix.detail.push_str("\n다른 운영체제는 미확인.");
    assert_eq!(
        store
            .apply_project_record(&fix)
            .unwrap()
            .entry_id
            .as_deref(),
        Some(b.as_str())
    );
    let second = store.export_portfolio(&id, &dir.0).unwrap();
    assert_ne!(first, second);
    assert!(fs::read_to_string(&second)
        .unwrap()
        .contains("다른 운영체제는 미확인."));
    assert_eq!(
        fs::read_to_string(first).unwrap(),
        "사용자가 고친 포트폴리오"
    );
    assert_eq!(store.portfolio_panel(&id).unwrap().experiences, 2);
    let mut edit = manual(&id, "design", "사용자가 정정한 설계 근거.");
    edit.id = Some(a.clone());
    store.save_entry(edit).unwrap();
    let mut next = design;
    next.revision = 2;
    assert_eq!(
        store.apply_project_record(&next).unwrap().outcome,
        "conflict"
    );
    let corrected = store.export_portfolio(&id, &dir.0).unwrap();
    assert!(fs::read_to_string(corrected)
        .unwrap()
        .contains("사용자가 정정한 설계 근거."));
}

#[test]
fn known_roles_are_scoped_and_user_answers_can_be_corrected() {
    let dir = Dir::new();
    let s = Storage::open(":memory:").unwrap();
    let create = |title: &str| {
        s.save_project(ProjectInput {
            id: None,
            title: title.into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap()
        .id
    };
    let a = create("원본 프로젝트");
    let b = create("다른 프로젝트");
    for kind in ["note", "summary", "portfolio", "commit"] {
        s.save_entry(manual(&a, kind, "수집 제외 표식")).unwrap();
    }
    s.save_entry(manual(&b, "development", "별도 프로젝트 근거"))
        .unwrap();
    s.save_entry(manual(
        &a,
        "design",
        "## 개인 역할\n스키마를 검토했다.\n\n## 설계\n원문과 표시본 분리",
    ))
    .unwrap();
    assert!(!s.portfolio_panel(&a).unwrap().needs_answer);
    let file = s.export_portfolio(&a, &dir.0).unwrap();
    let md = fs::read_to_string(file).unwrap();
    assert!(md.contains("스키마를 검토했다."));
    assert!(!md.contains("수집 제외 표식"));
    assert!(!md.contains("별도 프로젝트 근거"));
    assert!(s.portfolio_panel(&b).unwrap().needs_answer);
    s.save_portfolio_answer(&b, None, "데이터 설계").unwrap();
    s.save_portfolio_answer(&b, Some("데이터 설계"), "복구 검증")
        .unwrap();
    assert!(s
        .save_portfolio_answer(&b, Some("데이터 설계"), "오래된 답변")
        .is_err());
    let file = s.export_portfolio(&b, &dir.0).unwrap();
    assert!(fs::read_to_string(file)
        .unwrap()
        .contains("복구 검증 [사용자 답변](evidence.md#role)"));
    s.save_portfolio_answer(&a, None, "").unwrap();
    assert!(!fs::read_to_string(s.export_portfolio(&a, &dir.0).unwrap())
        .unwrap()
        .contains("스키마를 검토했다."));
}

#[test]
fn incomplete_exports_retry_and_a_deleted_user_file_is_preserved() {
    let dir = Dir::new();
    let s = Storage::open(":memory:").unwrap();
    let received = s
        .apply_project_record(&record(&dir.0, "development", "개인 역할: 보류"))
        .unwrap();
    let id = s
        .get_entry(&received.entry_id.unwrap())
        .unwrap()
        .project_id
        .unwrap();
    let root = dir.0.join("exports");
    fs::write(&root, "장애").unwrap();
    assert!(s.export_portfolio(&id, &root).is_err());
    assert!(s.portfolio_panel(&id).unwrap().last_file.is_none());
    fs::remove_file(&root).unwrap();
    let file = s.export_portfolio(&id, &root).unwrap();
    assert_eq!(
        file,
        root.join(s.portfolio_panel(&id).unwrap().last_file.unwrap())
    );
    assert!(file
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("v1-"));
    fs::remove_file(&file).unwrap();
    assert!(s.export_portfolio(&id, &root).is_err());
    assert!(!file.exists());
}

#[test]
fn selected_editions_reopen_edits_after_switching_subsets_and_restart() {
    use std::collections::BTreeSet;
    let dir = Dir::new();
    let path = dir.0.join("schedule.db");
    let db = Storage::open(&path).unwrap();
    let project = db
        .save_project(ProjectInput {
            id: None,
            title: "TimeSchedule".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap()
        .id;
    let design = db
        .save_entry(manual(&project, "design", "설계 본문"))
        .unwrap()
        .id;
    let fix = db
        .save_entry(manual(&project, "troubleshooting", "검사 본문"))
        .unwrap()
        .id;
    let skip = db
        .save_entry(manual(&project, "development", "제외 본문"))
        .unwrap()
        .id;
    let selected = BTreeSet::from([design, fix]);
    db.save_portfolio_answer(&project, None, "").unwrap();
    let first = db
        .export_portfolio_selection(&project, &dir.0, Some(&selected))
        .unwrap();
    for f in [&first, &first.with_file_name("evidence.md")] {
        let data = fs::read_to_string(f).unwrap();
        assert!(
            data.contains("설계 본문") && data.contains("검사 본문") && !data.contains("제외 본문")
        );
    }
    fs::write(&first, "직접 수정한 문장").unwrap();
    let other = db
        .export_portfolio_selection(&project, &dir.0, Some(&BTreeSet::from([skip])))
        .unwrap();
    assert_ne!(first, other);
    drop(db);
    let db = Storage::open(path).unwrap();
    assert_eq!(
        first,
        db.export_portfolio_selection(&project, &dir.0, Some(&selected))
            .unwrap()
    );
    let panel = db.portfolio_panel(&project).unwrap();
    assert_eq!(panel.last_selection, Some(selected));
    assert_eq!(panel.answer.as_deref(), Some(""));
    assert_eq!(fs::read_to_string(first).unwrap(), "직접 수정한 문장");
    for invalid in [
        BTreeSet::new(),
        BTreeSet::from([Uuid::new_v4().to_string()]),
    ] {
        assert!(db
            .export_portfolio_selection(&project, &dir.0, Some(&invalid))
            .is_err());
    }
}

#[test]
fn existing_v9_edition_and_role_survive_upgrade() {
    let dir = Dir::new();
    let path = dir.0.join("schedule.db");
    let s = Storage::open(&path).unwrap();
    let r = s
        .apply_project_record(&record(&dir.0, "design", "설계 내용"))
        .unwrap();
    let project = s
        .get_entry(&r.entry_id.unwrap())
        .unwrap()
        .project_id
        .unwrap();
    s.save_portfolio_answer(&project, None, "").unwrap();
    let file = s.export_portfolio(&project, &dir.0).unwrap();
    fs::write(&file, "이전 버전에서 편집한 문장").unwrap();
    drop(s);
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("ALTER TABLE portfolio_editions RENAME TO current_editions;
      CREATE TABLE portfolio_editions(project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
      revision INTEGER NOT NULL,folder TEXT NOT NULL,markdown TEXT NOT NULL,evidence TEXT NOT NULL,published INTEGER NOT NULL DEFAULT 0);
      INSERT INTO portfolio_editions SELECT project_id,revision,folder,markdown,evidence,published FROM current_editions;
      DROP TABLE current_editions; DROP TABLE chat_experience_receipts; PRAGMA user_version=9;").unwrap();
    drop(c);
    let upgraded = Storage::open(&path).unwrap();
    assert_eq!(upgraded.export_portfolio(&project, &dir.0).unwrap(), file);
    assert_eq!(
        fs::read_to_string(file).unwrap(),
        "이전 버전에서 편집한 문장"
    );
    assert_eq!(
        upgraded
            .portfolio_panel(&project)
            .unwrap()
            .answer
            .as_deref(),
        Some("")
    );
}

#[test]
fn terra_selection_preserves_legacy_edits_and_rejects_unreviewed_or_stale_results() {
    use std::collections::BTreeSet;
    let dir = Dir::new();
    let store = Storage::open(dir.0.join("schedule.db")).unwrap();
    let project = store
        .save_project(ProjectInput {
            id: None,
            title: "다른 프로젝트".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap()
        .id;
    let chosen = store
        .save_entry(manual(&project, "design", "선택한 자료"))
        .unwrap()
        .id;
    store
        .save_entry(manual(&project, "troubleshooting", "제외된 자료"))
        .unwrap();
    let selection = BTreeSet::from([chosen.clone()]);
    store.save_portfolio_answer(&project, None, "").unwrap();
    let old = store
        .export_portfolio_selection(&project, &dir.0, Some(&selection))
        .unwrap();
    fs::write(&old, "사용자가 확정한 문장").unwrap();
    let export = |writer: &dyn Fn(&str) -> Result<String, String>| {
        store.export_portfolio_with_writer(&project, &dir.0, Some(&selection), writer)
    };
    assert!(export(&|_| Ok(r#"[{"id":"wrong","paragraphs":["다른 근거"]}]"#.into())).is_err());
    assert!(export(&|_| Err("호출 실패".into())).is_err());
    let reply = serde_json::json!([{"id":chosen,"paragraphs":["모델이 작성한 요약"]}]).to_string();
    assert!(export(&|_| {
        store.save_portfolio_answer(&project, Some(""), "수정한 역할")?;
        Ok(reply.clone())
    })
    .is_err());
    store
        .save_portfolio_answer(&project, Some("수정한 역할"), "")
        .unwrap();
    let new = export(&|prompt| {
        assert!(prompt.contains("선택한 자료") && !prompt.contains("제외된 자료"));
        Ok(reply.clone())
    })
    .unwrap();
    assert_ne!(new, old);
    assert_eq!(fs::read_to_string(old).unwrap(), "사용자가 확정한 문장");
    let body = fs::read_to_string(&new).unwrap();
    for text in [
        "사용자가 확정한 문장",
        "모델이 작성한 요약",
        "개인 기여: 보류",
        &format!("evidence.md#entry-{chosen}"),
    ] {
        assert!(body.contains(text));
    }
    fs::write(&new, "다시 편집한 문장").unwrap();
    assert_eq!(
        export(&|_| panic!("같은 입력으로 모델을 재호출했습니다")).unwrap(),
        new
    );
    assert_eq!(fs::read_to_string(new).unwrap(), "다시 편집한 문장");
    assert_eq!(
        store.portfolio_panel(&project).unwrap().answer.as_deref(),
        Some("")
    );
}
