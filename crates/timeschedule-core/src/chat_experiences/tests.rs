use super::*;
use crate::diary::{EntryInput, ProjectInput};

#[test]
fn export_links_initial_and_inline_citations_and_preserves_an_edited_edition() {
    let (db, project, chat) = fixture();
    let session = uuid::Uuid::new_v4().to_string();
    let root = std::env::temp_dir().join(format!("ts-links-{session}"));
    let review = db.review_chat(&project, &chat).unwrap();
    db.confirm_chat_candidates(&review, &[Kind::Design, Kind::Troubleshooting])
        .unwrap();
    db.save_portfolio_answer(&project, None, "").unwrap();
    let choices = db.portfolio_panel(&project).unwrap().choices;
    let selected = choices.iter().map(|c| c.id.clone()).collect();
    let answer = serde_json::to_string(
        &choices
            .iter()
            .map(|c| serde_json::json!({"id":c.id,"paragraphs":["작성한 문장"]}))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    {
        let c = db.connection().unwrap();
        c.execute(
            "UPDATE codex_chat_sessions SET session_id=?1 WHERE entry_id=?2",
            params![format!("manual:claude-desktop:{session}"), chat],
        )
        .unwrap();
        c.execute("UPDATE diary_entries SET body='검토 (AI · 원문 21행), (검사 출력 · 원문 26행), (AI · 원문 26행), (AI · 원문 999행)' WHERE id IN (SELECT entry_id FROM chat_experience_receipts)",[]).unwrap();
        // Reproduce an old edition with no generated line anchors.
        c.execute(
            "UPDATE diary_entries SET source_body='' WHERE id=?1",
            [&chat],
        )
        .unwrap();
    }
    let export = |writer: &dyn Fn(&str) -> Result<String, String>| {
        db.export_portfolio_with_writer(&project, &root, Some(&selected), writer)
            .unwrap()
    };
    let old = export(&|_| Ok(answer.clone()));
    let anchor = format!("source-{session}-L21");
    let edited = format!("직접 수정한 문장 [21행](evidence.md#{anchor})\n\n개인 기여: 보류");
    std::fs::write(&old, &edited).unwrap();
    db.connection()
        .unwrap()
        .execute(
            "UPDATE diary_entries SET source_body=?1 WHERE id=?2",
            params![BODY, chat],
        )
        .unwrap();
    let template = db
        .export_portfolio_selection(&project, &root, Some(&selected))
        .unwrap();
    let fresh = std::fs::read_to_string(template).unwrap();
    assert!(fresh.contains(&format!("](evidence.md#{anchor})")));
    assert!(fresh.contains(&format!("](evidence.md#source-{session}-L26)")));
    assert!(!fresh.contains(&format!(
        "[AI · 원문 26행](evidence.md#source-{session}-L26)"
    )));
    let new = export(&|_| Ok(answer.clone()));
    assert_ne!(new, old);
    let doc = std::fs::read_to_string(&new).unwrap();
    let evidence = std::fs::read_to_string(new.with_file_name("evidence.md")).unwrap();
    assert!(doc.starts_with(&edited));
    assert_eq!(doc.matches(&format!("](evidence.md#{anchor})")).count(), 1);
    assert_eq!(evidence.matches(&format!("id=\"{anchor}\"")).count(), 1);
    assert_eq!(
        evidence
            .matches(&format!("id=\"source-{session}-L26\""))
            .count(),
        1
    );
    assert!(doc.contains(&format!("](evidence.md#source-{session}-L26)")));
    assert!(!evidence.contains(&format!("id=\"source-{session}-L999\"")));
    assert!(evidence.contains("환경이 다르기 때문에 저장값을 읽습니다."));
    assert_eq!(export(&|_| panic!("unchanged inputs called Terra")), new);
    assert_eq!(std::fs::read_to_string(old).unwrap(), edited);
    std::fs::remove_dir_all(root).unwrap();
}

const BODY:&str="Windows Codex · session\n원문: example.jsonl\n\n## 사용자 · 원문 14행\n두 프로세스의 설정 설계를 설명해줘.\n\n## AI · 원문 16행\n이전 설계는 환경변수를 사용합니다.\n\n## 사용자 · 원문 18행\n방금 답변을 수정해줘. 개인 기여는 보류.\n\n## AI · 원문 21행\n앱과 중계는 같은 설정을 공유합니다.\n환경이 다르기 때문에 저장값을 읽습니다.\n\n## 사용자 · 원문 25행\n종료 오류를 확인해줘.\n\n## 검사 출력 · 원문 26행\n종료 코드 1\nTAP version 13\n# tests 1\n# pass 0\n# fail 1\n\n## AI · 원문 28행\n테스트에서 재현했다. 수정 결과는 미확인이다.";
fn fixture() -> (Storage, String, String) {
    let db = Storage::open(":memory:").unwrap();
    let project = db
        .save_project(ProjectInput {
            id: None,
            title: "프로젝트".into(),
            description: String::new(),
            repo_path: String::new(),
        })
        .unwrap()
        .id;
    let chat = db
        .save_entry(EntryInput {
            id: None,
            project_id: Some(project.clone()),
            date: "2026-11-19".into(),
            kind: "note".into(),
            title: "테스트".into(),
            body: BODY.into(),
            source_url: String::new(),
        })
        .unwrap()
        .id;
    {
        let c = db.connection().unwrap();
        c.execute(
            "UPDATE diary_entries SET source_body=body WHERE id=?1",
            [&chat],
        )
        .unwrap();
        c.execute(
            "INSERT INTO codex_chat_sessions VALUES(?1,'S',?2,0,0,'','')",
            params![project, chat],
        )
        .unwrap();
    }
    (db, project, chat)
}

#[test]
fn review_uses_corrected_evidence_and_only_inserts_checked_items() {
    let (db, p, chat) = fixture();
    let review = db.review_chat(&p, &chat).unwrap();
    assert_eq!(review.candidates.len(), 2);
    let design = &review.candidates[0].body;
    assert!(
        design.contains("원문 14행")
            && design.contains("원문 21행")
            && !design.contains("원문 16행")
    );
    assert!(!design.contains("# fail") && review.candidates[1].body.contains("# fail 1"));
    assert!(db.confirm_chat_candidates(&review, &[]).is_err());
    assert_eq!(db.portfolio_panel(&p).unwrap().experiences, 0);
    assert_eq!(
        db.confirm_chat_candidates(&review, &[Kind::Design])
            .unwrap()
            .added,
        1
    );
    let panel = db.portfolio_panel(&p).unwrap();
    assert_eq!(panel.experiences, 1);
    let entry = &panel.choices[0].id;
    db.connection()
        .unwrap()
        .execute(
            "UPDATE diary_entries SET body='수동 편집' WHERE id=?1",
            [entry],
        )
        .unwrap();
    assert_eq!(
        db.confirm_chat_candidates(&review, &[Kind::Design])
            .unwrap()
            .kept,
        1
    );
    assert_eq!(db.get_entry(entry).unwrap().body, "수동 편집");
    db.delete_entry(entry).unwrap();
    assert_eq!(
        db.confirm_chat_candidates(&review, &[Kind::Design])
            .unwrap()
            .added,
        0
    );
    assert_eq!(db.portfolio_panel(&p).unwrap().experiences, 0);
}
#[test]
fn legacy_citation_reuses_same_entry_and_ignores_unchosen_role() {
    let (db, p, chat) = fixture();
    db.save_portfolio_answer(&p, None, "").unwrap();
    let body = format!("수정한 경험 문장\n\nWindows Codex 채팅 「테스트」 · 기록 {chat}");
    let e = db
        .save_entry(EntryInput {
            id: None,
            project_id: Some(p.clone()),
            date: "2026-11-19".into(),
            kind: "design".into(),
            title: "기존 경험".into(),
            body: body.clone(),
            source_url: String::new(),
        })
        .unwrap();
    for pass in 0..2 {
        let review = db.review_chat(&p, &chat).unwrap();
        let result = db
            .confirm_chat_candidates(&review, &[Kind::Design])
            .unwrap();
        assert_eq!((result.added, result.kept), (0, 1));
        assert_eq!(result.evidence_added > 0, pass == 0);
        assert!(db.get_entry(&e.id).unwrap().body.starts_with(&body));
    }
    assert_eq!(db.portfolio_panel(&p).unwrap().answer.as_deref(), Some(""));
}
#[test]
fn stale_or_wrong_project_review_cannot_save() {
    let (db, p, chat) = fixture();
    let review = db.review_chat(&p, &chat).unwrap();
    assert!(db.review_chat("different", &chat).is_err());
    db.connection()
        .unwrap()
        .execute(
            "UPDATE diary_entries SET source_body='## 사용자 · 원문 3행\n안녕' WHERE id=?1",
            [&chat],
        )
        .unwrap();
    assert!(db
        .confirm_chat_candidates(&review, &[Kind::Design])
        .is_err());
    assert!(db.review_chat(&p, &chat).unwrap().candidates.is_empty());
    assert_eq!(db.portfolio_panel(&p).unwrap().experiences, 0);
}

#[test]
fn model_review_renders_only_resolved_evidence_and_saves_only_the_checked_kind() {
    let (db, p, chat_id) = fixture();
    let (chat, source, _) = read_source(&*db.connection().unwrap(), &p, &chat_id).unwrap();
    let candidates=model::review(&chat,&source,|req|{
        assert_eq!(req["model"], "gpt-6.1-terra");
        assert!(req["messages"][0]["content"].as_str().unwrap().contains("환경이 다르기 때문에"));
        Ok(serde_json::json!({
            "design":{"title":"공유 설정","problem":1,"solution":4,"reason":5,"test":0,"limit":0},
            "troubleshooting":{"title":"종료 회귀 검증","problem":6,"solution":8,"reason":0,"test":7,"limit":8}
        }).to_string())
    }).unwrap();
    assert!(candidates[0].body.contains("공유합니다. (AI · 원문 21행)"));
    assert!(candidates[1].body.contains("의사결정: 근거 부족\n"));
    assert!(candidates[1].body.contains("모델: gpt-6.1-terra\n"));
    assert!(candidates[1]
        .body
        .contains("# fail 1 (검사 출력 · 원문 26행)"));
    let review = Review {
        project: p.clone(),
        chat,
        candidates,
        source,
    };
    db.save_portfolio_answer(&p, None, "").unwrap();
    assert!(db.confirm_chat_candidates(&review, &[]).is_err());
    let result = db
        .confirm_chat_candidates(&review, &[Kind::Troubleshooting])
        .unwrap();
    assert_eq!(result.added, 1);
    let mut different = review.clone();
    different.candidates[1].title = "다른 모델 표현".into();
    assert_eq!(
        db.confirm_chat_candidates(&different, &[Kind::Troubleshooting])
            .unwrap()
            .kept,
        1
    );
    let panel = db.portfolio_panel(&p).unwrap();
    assert_eq!(panel.experiences, 1);
    assert_eq!(panel.choices[0].title, "종료 회귀 검증");
    assert_eq!(panel.answer.as_deref(), Some(""));
    db.connection()
        .unwrap()
        .execute(
            "UPDATE diary_entries SET source_body=source_body||' 추가 발화' WHERE id=?1",
            [&chat_id],
        )
        .unwrap();
    assert!(db
        .confirm_chat_candidates(&review, &[Kind::Design])
        .is_err());
}
#[test]
fn invalid_model_output_or_provider_error_never_creates_an_experience() {
    let (db, p, id) = fixture();
    let (chat, source, _) = read_source(&*db.connection().unwrap(), &p, &id).unwrap();
    for (solution, test) in [(0, 0), (999, 0), (4, 4)] {
        let answer = serde_json::json!({"design":{"title":"후보","problem":1,"solution":solution,"reason":0,"test":test,"limit":0},"troubleshooting":null});
        assert!(model::review(&chat, &source, |_| Ok(answer.to_string())).is_err());
    }
    assert!(model::review(&chat, &source, |_| Err("모델 중지".into())).is_err());
    assert!(model::review(&chat, &source, |_| Ok("{}".into())).is_err());
    assert!(model::review(&chat, &source, |_| Ok(
        r#"{"design":null,"troubleshooting":null}"#.into()
    ))
    .unwrap()
    .is_empty());
    assert_eq!(db.portfolio_panel(&p).unwrap().experiences, 0);
}

#[test]
fn confirmed_new_observations_refresh_selected_portfolio_once() {
    let (db, project, chat_id) = fixture();
    let review = |test: usize| {
        db.review_chat_with_inference(&project,&chat_id,|_|Ok(serde_json::json!({
        "design":null,"troubleshooting":{"title":"종료 검증","problem":6,"solution":8,"reason":0,"test":test,"limit":0}
    }).to_string())).unwrap()
    };
    let save = |r: &Review| {
        db.confirm_chat_candidates(r, &[Kind::Troubleshooting])
            .unwrap()
    };
    assert_eq!(save(&review(0)).added, 1);
    let entry = db.portfolio_panel(&project).unwrap().choices[0].id.clone();
    let selected = std::collections::BTreeSet::from([entry.clone()]);
    db.save_portfolio_answer(&project, None, "").unwrap();
    let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
    let export = |writer: &dyn Fn(&str) -> Result<String, String>| {
        db.export_portfolio_with_writer(&project, &root, Some(&selected), writer)
            .unwrap()
    };
    let reply = serde_json::json!([{"id":entry,"paragraphs":["검토 결과"]}]).to_string();
    let old = export(&|_| Ok(reply.clone()));
    std::fs::write(&old, "직접 편집한 문장").unwrap();
    let r = review(7);
    assert_eq!(save(&r).evidence_added, 1);
    assert_eq!(save(&r).evidence_added, 0);
    assert_eq!(
        db.get_entry(&entry)
            .unwrap()
            .body
            .matches("검사 출력 · 원문 26행")
            .count(),
        1
    );
    let updated = export(&|prompt| {
        assert!(prompt.contains("# fail 1"));
        Ok(reply.clone())
    });
    assert_ne!(old, updated);
    let body = std::fs::read_to_string(&updated).unwrap();
    assert!(
        body.contains("직접 편집한 문장")
            && body.contains("개인 기여: 보류")
            && body.contains("검사 출력 · 원문 26행](evidence.md#quote-")
    );
    assert_eq!(
        export(&|_| panic!("unchanged inputs called writer")),
        updated
    );
    assert_eq!(std::fs::read_to_string(old).unwrap(), "직접 편집한 문장");
    // A user's subsequent removal of the quote must survive a repeat confirmation.
    db.connection()
        .unwrap()
        .execute("UPDATE diary_entries SET body='편집' WHERE id=?1", [&entry])
        .unwrap();
    assert_eq!(save(&r).evidence_added, 0);
    let next = export(&|prompt| {
        assert!(!prompt.contains("# fail 1"));
        Ok(reply.clone())
    });
    assert!(std::fs::read_to_string(next)
        .unwrap()
        .contains("직접 편집한 문장"));
    std::fs::remove_dir_all(root).unwrap();
}
