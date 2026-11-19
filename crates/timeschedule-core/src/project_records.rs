use crate::storage::{
    database_error, timestamp, validate_date, validate_text, validated_title, Storage,
};
use crate::timeline::{self, RecordCommit};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectRecord {
    pub schema_version: u32,
    pub work_id: String,
    pub revision: u64,
    pub project_title: String,
    pub repo_path: String,
    pub date: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub space_url: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub delivery_state: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// v3: the work's begin receipt, when one exists. Never "now".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// v3: the confirmed completion moment of a completed status only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// v3: real commits this work owns, verified by the recorder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<RecordCommit>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordReceipt {
    pub work_id: String,
    pub revision: u64,
    pub outcome: String,
    pub entry_id: Option<String>,
    pub message: String,
}

impl ProjectRecord {
    pub fn validate(&self) -> Result<(), String> {
        if ![1, 2, 3].contains(&self.schema_version) {
            return Err("지원하지 않는 프로젝트 기록 형식입니다.".into());
        }
        if self.schema_version == 1 && (!self.delivery_state.is_empty() || !self.detail.is_empty())
        {
            return Err("오프라인 기록은 v2 형식이 필요합니다.".into());
        }
        if self.schema_version < 3
            && (self.started_at.is_some()
                || self.completed_at.is_some()
                || !self.commits.is_empty())
        {
            return Err("작업 시각과 커밋 근거는 v3 형식이 필요합니다.".into());
        }
        if self.schema_version == 3 {
            self.validate_timeline()?;
        }
        if self.schema_version >= 2 {
            if !["local", "pending", "synced"].contains(&self.delivery_state.as_str())
                || self.detail.trim().is_empty()
                || self.detail.len() > 48000
                || self.detail.contains('\0')
            {
                return Err("연결 상태와 전체 기록 원문을 확인해 주세요.".into());
            }
        }
        if Uuid::parse_str(&self.work_id)
            .map(|id| id.to_string())
            .as_deref()
            != Ok(self.work_id.as_str())
        {
            return Err("작업 ID는 표준 UUID여야 합니다.".into());
        }
        if self.revision == 0 || self.revision > i64::MAX as u64 {
            return Err("기록 revision은 양의 정수여야 합니다.".into());
        }
        validated_title(&self.title)?;
        validated_title(&self.project_title)?;
        validate_text(&self.repo_path, "저장소 경로", 2000)?;
        if !Path::new(&self.repo_path).is_absolute() || self.repo_path.contains('\0') {
            return Err("저장소 경로는 절대 경로여야 합니다.".into());
        }
        validate_date(&self.date)?;
        validate_text(&self.summary, "프로젝트 기록 요약", 10000)?;
        if self.summary.trim().is_empty() {
            return Err("프로젝트 기록 요약을 입력해 주세요.".into());
        }
        if ![
            "design",
            "development",
            "troubleshooting",
            "portfolio",
            "summary",
        ]
        .contains(&self.kind.as_str())
        {
            return Err("외부 프로젝트 기록 종류를 확인해 주세요.".into());
        }
        self.status_label()?;
        validate_text(&self.space_url, "Space 링크", 2000)?;
        if ["local", "pending"].contains(&self.delivery_state.as_str()) {
            return if self.space_url.is_empty() {
                Ok(())
            } else {
                Err("로컬·대기 기록에는 Space 링크를 넣을 수 없습니다.".into())
            };
        }
        let url = url::Url::parse(&self.space_url).map_err(|_| "Space 링크를 확인해 주세요.")?;
        let valid_page = url
            .path()
            .strip_prefix("/space/page_")
            .is_some_and(|id| id.len() == 32 && id.bytes().all(|ch| ch.is_ascii_hexdigit()));
        if url.scheme() != "https"
            || url.host_str() != Some("chatgpt.com")
            || !valid_page
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("검증된 https://chatgpt.com/space/page_… 링크가 필요합니다.".into());
        }
        Ok(())
    }

    fn validate_timeline(&self) -> Result<(), String> {
        let started = self
            .started_at
            .as_deref()
            .map(timeline::parse_instant)
            .transpose()?;
        let completed = self
            .completed_at
            .as_deref()
            .map(timeline::parse_instant)
            .transpose()?;
        if completed.is_some() && !timeline::completes(&self.status) {
            return Err("완료 시각은 확인된 완료 상태에만 둘 수 있습니다.".into());
        }
        if let (Some(started), Some(completed)) = (started, completed) {
            if completed < started {
                return Err("완료 시각이 작업 시작보다 앞설 수 없습니다.".into());
            }
        }
        timeline::validate_commits(&self.commits)
    }

    fn status_label(&self) -> Result<&'static str, String> {
        match self.status.as_str() {
            "in_progress" => Ok("진행 중"),
            "verification_pending" => Ok("검증 대기"),
            "verified" => Ok("확인한 범위 검증 완료"),
            "completed" => Ok("작업 정리 완료"),
            "merged" => Ok("병합 완료"),
            _ => Err("프로젝트 작업 상태를 확인해 주세요.".into()),
        }
    }

    pub fn receipt(&self, outcome: &str, entry_id: Option<String>, message: &str) -> RecordReceipt {
        RecordReceipt {
            work_id: self.work_id.clone(),
            revision: self.revision,
            outcome: outcome.into(),
            entry_id,
            message: message.into(),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct ManagedSnapshot {
    project_id: Option<String>,
    date: String,
    kind: String,
    title: String,
    body: String,
    source_url: String,
}

pub(crate) fn repo_key(value: &str) -> String {
    let path = std::fs::canonicalize(value).unwrap_or_else(|_| value.into());
    let normalized = path.to_string_lossy().replace('\\', "/");
    let normalized = normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .trim_end_matches('/');
    if cfg!(windows) {
        normalized.to_lowercase()
    } else {
        normalized.to_string()
    }
}

impl Storage {
    pub fn apply_project_record(&self, record: &ProjectRecord) -> Result<RecordReceipt, String> {
        record.validate()?;
        let payload = serde_json::to_string(record).map_err(|e| e.to_string())?;
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let previous: Option<(u64, String, String, String)> = tx.query_row(
            "SELECT revision,payload,entry_id,managed_snapshot FROM project_record_receipts WHERE work_id=?1",
            [&record.work_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional().map_err(database_error)?;

        if let Some((revision, previous_payload, entry_id, snapshot)) = &previous {
            if record.revision < *revision {
                return Ok(record.receipt(
                    "stale",
                    Some(entry_id.clone()),
                    "최신 기록보다 오래된 입력을 무시했습니다.",
                ));
            }
            let current: Option<ManagedSnapshot> = tx.query_row(
                "SELECT project_id,date,kind,title,body,source_url FROM diary_entries WHERE id=?1",
                [entry_id], |row| Ok(ManagedSnapshot { project_id: row.get(0)?, date: row.get(1)?, kind: row.get(2)?, title: row.get(3)?, body: row.get(4)?, source_url: row.get(5)? }),
            ).optional().map_err(database_error)?;
            let managed: ManagedSnapshot = serde_json::from_str(snapshot)
                .map_err(|e| format!("기록 원본 상태를 읽지 못했습니다: {e}"))?;
            if current.as_ref() != Some(&managed) {
                return Ok(record.receipt(
                    "conflict",
                    Some(entry_id.clone()),
                    "사용자가 수정하거나 삭제한 다이어리 기록을 보존했습니다.",
                ));
            }
            if record.revision == *revision {
                return Ok(if payload == *previous_payload {
                    record.receipt(
                        "unchanged",
                        Some(entry_id.clone()),
                        "이미 반영된 기록입니다.",
                    )
                } else {
                    record.receipt(
                        "conflict",
                        Some(entry_id.clone()),
                        "같은 revision에 서로 다른 내용이 있습니다.",
                    )
                });
            }
        }

        let key = repo_key(&record.repo_path);
        let projects: Vec<(String, String)> = {
            let mut query = tx
                .prepare("SELECT id,repo_path FROM projects")
                .map_err(database_error)?;
            let rows = query
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(database_error)?;
            rows.collect::<Result<_, _>>().map_err(database_error)?
        };
        let matches: Vec<_> = projects
            .into_iter()
            .filter(|(_, path)| !path.is_empty() && repo_key(path) == key)
            .collect();
        if matches.len() > 1 {
            return Err("같은 저장소 경로가 여러 프로젝트에 등록되어 있습니다. 프로젝트 연결을 먼저 정리해 주세요.".into());
        }
        let now = timestamp();
        let project_id = if let Some((id, _)) = matches.first() {
            id.clone()
        } else {
            let id = Uuid::new_v4().to_string();
            tx.execute("INSERT INTO projects(id,title,description,repo_path,updated_at) VALUES(?1,?2,'',?3,?4)", params![id, record.project_title.trim(), record.repo_path, now]).map_err(database_error)?;
            id
        };
        let entry_id = previous
            .as_ref()
            .map(|row| row.2.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let body = if record.schema_version >= 2 {
            format!(
                "{}\n\n상태: {}\n\n{}",
                record.detail.trim(),
                record.status_label()?,
                if record.delivery_state == "local" {
                    "로컬 저장 완료 — Space 연동은 선택 사항입니다."
                } else if record.delivery_state == "pending" {
                    "Space 연결 대기 — 원문은 다이어리에 보관되어 있습니다."
                } else {
                    "Space 연결 완료"
                }
            )
        } else {
            format!(
                "{}\n\n상태: {}",
                record.summary.trim(),
                record.status_label()?
            )
        };
        let snapshot = ManagedSnapshot {
            project_id: Some(project_id),
            date: record.date.clone(),
            kind: record.kind.clone(),
            title: record.title.trim().into(),
            body,
            source_url: record.space_url.clone(),
        };
        let source_ref = format!("space-work:{}", record.work_id);
        tx.execute(
            "INSERT INTO diary_entries(id,project_id,date,kind,title,body,source_url,source_ref,source_body,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id,date=excluded.date,kind=excluded.kind,title=excluded.title,
             body=excluded.body,source_url=excluded.source_url,source_body=excluded.source_body,updated_at=excluded.updated_at",
            params![entry_id, snapshot.project_id, snapshot.date, snapshot.kind, snapshot.title, snapshot.body, snapshot.source_url, source_ref, record.summary, now],
        ).map_err(database_error)?;
        // Evidence sits beside the editable copy: a later edit never changes it.
        let known: Vec<RecordCommit> = tx
            .query_row(
                "SELECT commits FROM entry_evidence WHERE entry_id=?1",
                [&entry_id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(database_error)?
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        if record.schema_version == 3 {
            // The begin receipt and the first confirmed completion are fixed
            // once known: a later edit, retry or merge never moves them. Commit
            // times already confirmed stay with their hashes.
            let commits = if record.commits.is_empty() {
                known
            } else {
                record
                    .commits
                    .iter()
                    .map(|commit| {
                        let mut commit = commit.clone();
                        if let Some(old) = known.iter().find(|k| k.hash == commit.hash) {
                            if commit.committed_at.is_none() {
                                commit.committed_at = old.committed_at.clone();
                            }
                            if commit.subject.is_empty() {
                                commit.subject = old.subject.clone();
                            }
                        }
                        commit
                    })
                    .collect()
            };
            tx.execute(
                "INSERT INTO entry_evidence(entry_id,work_id,started_at,completed_at,status,commits) VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(entry_id) DO UPDATE SET work_id=excluded.work_id,
                 started_at=COALESCE(entry_evidence.started_at,excluded.started_at),
                 completed_at=COALESCE(entry_evidence.completed_at,excluded.completed_at),
                 status=excluded.status,commits=excluded.commits",
                params![entry_id, record.work_id, record.started_at, record.completed_at, record.status, timeline::commits_json(&commits)?],
            ).map_err(database_error)?;
        } else {
            // Earlier formats carry no times: keep whatever exact evidence
            // exists. A first v2 delivery keeps the exact owned-commit hashes of
            // its detail, so the app can confirm their times when it is opened.
            let named = if known.is_empty() {
                timeline::owned_commits_in(&record.detail)
            } else {
                known
            };
            tx.execute(
                "INSERT INTO entry_evidence(entry_id,work_id,status,commits) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(entry_id) DO UPDATE SET work_id=excluded.work_id,status=excluded.status,commits=excluded.commits",
                params![entry_id, record.work_id, record.status, timeline::commits_json(&named)?],
            ).map_err(database_error)?;
        }
        tx.execute(
            "INSERT INTO project_record_receipts(work_id,revision,payload,entry_id,managed_snapshot) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(work_id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload,entry_id=excluded.entry_id,managed_snapshot=excluded.managed_snapshot",
            params![record.work_id, record.revision, payload, entry_id, serde_json::to_string(&snapshot).map_err(|e| e.to_string())?],
        ).map_err(database_error)?;
        tx.commit().map_err(database_error)?;
        Ok(record.receipt(
            "saved",
            Some(entry_id),
            "기록 원문과 연결 상태를 다이어리에 저장했습니다.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diary::{EntryInput, ProjectInput};
    use crate::timeline::EntryTimeline;

    fn record() -> ProjectRecord {
        ProjectRecord {
            schema_version: 1,
            work_id: "129c8037-5ff2-48b9-bb32-b9d6978ac628".into(),
            revision: 1,
            project_title: "기록 연결 테스트".into(),
            repo_path: std::env::current_dir().unwrap().to_string_lossy().into(),
            date: "2026-10-31".into(),
            kind: "troubleshooting".into(),
            title: "입력 문제 원인과 해결".into(),
            summary: "오류를 수정했다. 실제 IME 동작은 아직 확인하지 않았다.".into(),
            space_url: "https://chatgpt.com/space/page_6ae1b18a44f08191ad2a86276972476c".into(),
            status: "verification_pending".into(),
            delivery_state: String::new(),
            detail: String::new(),
            started_at: None,
            completed_at: None,
            commits: vec![],
        }
    }

    fn timed() -> ProjectRecord {
        ProjectRecord {
            schema_version: 3,
            delivery_state: "local".into(),
            space_url: String::new(),
            detail: format!("배경과 해결\n\n작업 커밋 · {}", "c".repeat(40)),
            status: "verified".into(),
            started_at: Some("2026-10-31T00:10:00.000Z".into()),
            completed_at: Some("2026-10-31T09:30:00+09:00".into()),
            commits: vec![RecordCommit {
                hash: "c".repeat(40),
                committed_at: Some("2026-10-31T09:20:00+09:00".into()),
                subject: "기록 시각 저장".into(),
                url: String::new(),
            }],
            ..record()
        }
    }

    #[test]
    fn v3_keeps_begin_completion_and_owned_commits_beside_the_display_copy() {
        let store = Storage::open(":memory:").unwrap();
        let input = timed();
        let receipt = store.apply_project_record(&input).unwrap();
        let id = receipt.entry_id.unwrap();
        let t = store.entry_timeline(&id).unwrap();
        assert_eq!(t.work_id.as_deref(), Some(input.work_id.as_str()));
        assert_eq!(
            timeline::kst_minute(t.started_at.as_deref()),
            "2026-10-31 09:10"
        );
        assert_eq!(
            timeline::kst_minute(t.completed_at.as_deref()),
            "2026-10-31 09:30"
        );
        assert_eq!(t.status, "verified");
        assert_eq!(t.commits, input.commits);
        assert!(!t.edited);
        // The same payload again is unchanged and does not move any time.
        assert_eq!(
            store.apply_project_record(&input).unwrap().outcome,
            "unchanged"
        );
        assert_eq!(store.entry_timeline(&id).unwrap(), t);
        // A person's edit is flagged, preserved, and never rewrites the evidence.
        let entry = store.get_entry(&id).unwrap();
        store
            .save_entry(EntryInput {
                id: Some(id.clone()),
                project_id: entry.project_id,
                date: entry.date,
                kind: entry.kind,
                title: "직접 고친 제목".into(),
                body: entry.body,
                source_url: entry.source_url,
            })
            .unwrap();
        let edited = store.entry_timeline(&id).unwrap();
        assert!(edited.edited);
        assert_eq!(edited.started_at, t.started_at);
        let mut later = input.clone();
        later.revision = 2;
        later.completed_at = Some("2026-11-01T10:00:00+09:00".into());
        assert_eq!(
            store.apply_project_record(&later).unwrap().outcome,
            "conflict"
        );
        assert_eq!(
            store.entry_timeline(&id).unwrap().completed_at,
            t.completed_at
        );
        // Deleting the record removes its evidence with it.
        store.delete_entry(&id).unwrap();
        assert_eq!(store.entry_timeline(&id).unwrap(), EntryTimeline::default());
    }

    #[test]
    fn v3_rejects_invented_completion_and_foreign_formats() {
        let mut pending = timed();
        pending.status = "verification_pending".into();
        assert!(pending.validate().unwrap_err().contains("완료"));
        pending.completed_at = None;
        assert!(
            pending.validate().is_ok(),
            "pending work has no completion time"
        );
        let mut backwards = timed();
        backwards.completed_at = Some("2026-10-30T09:00:00+09:00".into());
        assert!(backwards.validate().is_err());
        let mut naive = timed();
        naive.started_at = Some("2026-10-31 09:10".into());
        assert!(naive.validate().is_err());
        let mut old = timed();
        old.schema_version = 2;
        assert!(old.validate().unwrap_err().contains("v3"));
        let mut v2 = old.clone();
        v2.started_at = None;
        v2.completed_at = None;
        v2.commits.clear();
        assert!(v2.validate().is_ok());
        // The serialized v2 payload is byte-identical to the format before v3,
        // so receipts accepted earlier still compare as unchanged.
        let json = serde_json::to_string(&v2).unwrap();
        for absent in ["startedAt", "completedAt", "commits"] {
            assert!(!json.contains(absent), "{absent}");
        }
        assert!(serde_json::from_str::<ProjectRecord>(
            &json.replace("\"title\"", "\"unknownField\":1,\"title\"")
        )
        .is_err());
    }

    #[test]
    fn v2_redelivery_keeps_exact_evidence_already_known() {
        let store = Storage::open(":memory:").unwrap();
        let id = store
            .apply_project_record(&timed())
            .unwrap()
            .entry_id
            .unwrap();
        let mut v2 = timed();
        v2.schema_version = 2;
        v2.revision = 2;
        v2.started_at = None;
        v2.completed_at = None;
        v2.commits.clear();
        assert_eq!(store.apply_project_record(&v2).unwrap().outcome, "saved");
        let t = store.entry_timeline(&id).unwrap();
        assert!(t.started_at.is_some());
        assert_eq!(t.commits.len(), 1);
    }

    #[test]
    fn a_folded_list_row_carries_the_chosen_summary_and_begin_time_but_no_body() {
        let store = Storage::open(":memory:").unwrap();
        let input = timed();
        let id = store
            .apply_project_record(&input)
            .unwrap()
            .entry_id
            .unwrap();
        let row = store
            .list_entries("2026-10-31", "2026-10-31", None, 0)
            .unwrap()
            .remove(0);
        assert_eq!(row.id, id);
        assert_eq!(row.summary, input.summary);
        assert!(row.timeline.is_automatic() && row.timeline.sourced);
        assert_eq!(
            timeline::reference_line(&row.date, &row.timeline),
            "작업 시작 · 2026-10-31 09:10"
        );
        let json = serde_json::to_value(&row).unwrap();
        assert!(json.get("body").is_none() && json.get("detail").is_none());
    }

    #[test]
    fn first_completion_begin_and_commit_times_stay_through_later_deliveries() {
        let store = Storage::open(":memory:").unwrap();
        let first = timed();
        let id = store
            .apply_project_record(&first)
            .unwrap()
            .entry_id
            .unwrap();
        // A merge reflected later, with another completion moment and a commit
        // whose time it did not repeat, moves nothing already confirmed.
        let mut merged = timed();
        merged.revision = 2;
        merged.status = "merged".into();
        merged.completed_at = Some("2026-11-02T10:00:00+09:00".into());
        merged.started_at = Some("2026-11-01T00:00:00Z".into());
        merged.commits[0].committed_at = None;
        assert_eq!(
            store.apply_project_record(&merged).unwrap().outcome,
            "saved"
        );
        let t = store.entry_timeline(&id).unwrap();
        assert_eq!(t.status, "merged");
        assert_eq!(t.completed_at, first.completed_at);
        assert_eq!(t.started_at, first.started_at);
        assert_eq!(t.commits[0].committed_at, first.commits[0].committed_at);
        // Waiting for verification again shows that status, not a completion.
        let mut pending = merged.clone();
        pending.revision = 3;
        pending.status = "verification_pending".into();
        pending.completed_at = None;
        assert_eq!(
            store.apply_project_record(&pending).unwrap().outcome,
            "saved"
        );
        let t = store.entry_timeline(&id).unwrap();
        assert_eq!(t.shown_completion(), None);
        assert_eq!(
            timeline::completion_line(&t).as_deref(),
            Some("현재 상태 · 검증 대기")
        );
        assert_eq!(
            t.completed_at, first.completed_at,
            "the first completion is kept"
        );
    }

    #[test]
    fn offline_record_keeps_full_text_and_later_links_the_same_entry() {
        let store = Storage::open(":memory:").unwrap();
        let mut pending = record();
        pending.schema_version = 2;
        pending.delivery_state = "pending".into();
        pending.detail = "작업 배경과 시도한 방법\n\n원인은 미확인. 후속 검증 필요.".into();
        pending.space_url.clear();
        let saved = store.apply_project_record(&pending).unwrap();
        let id = saved.entry_id.unwrap();
        let offline = store.get_entry(&id).unwrap();
        assert!(offline.body.contains(&pending.detail));
        assert!(offline.body.contains("Space 연결 대기"));
        assert!(offline.source_url.is_empty());
        pending.revision += 1;
        pending.delivery_state = "synced".into();
        pending.space_url = record().space_url;
        assert_eq!(
            store
                .apply_project_record(&pending)
                .unwrap()
                .entry_id
                .as_deref(),
            Some(id.as_str())
        );
        let online = store.get_entry(&id).unwrap();
        assert!(online.body.contains(&pending.detail));
        assert!(!online.body.contains("Space 연결 대기"));
        assert_eq!(online.source_url, pending.space_url);
        assert_eq!(
            store.apply_project_record(&pending).unwrap().outcome,
            "unchanged"
        );
        pending.revision -= 1;
        pending.delivery_state = "pending".into();
        pending.space_url.clear();
        assert_eq!(
            store.apply_project_record(&pending).unwrap().outcome,
            "stale"
        );
        assert_eq!(
            store
                .list_entries("2026-10-01", "2026-10-31", None, 0)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn pending_record_requires_v2_full_text_and_no_unverified_link() {
        let mut pending = record();
        pending.delivery_state = "pending".into();
        pending.space_url.clear();
        assert!(pending.validate().is_err());
        pending.schema_version = 2;
        assert!(pending.validate().is_err());
        pending.detail = "완전한 작업 원문".into();
        assert!(pending.validate().is_ok());
        pending.space_url = record().space_url;
        assert!(pending.validate().is_err());
    }

    #[test]
    fn local_record_is_complete_without_space_and_preserves_user_edits() {
        let store = Storage::open(":memory:").unwrap();
        let mut local = record();
        local.schema_version = 2;
        local.delivery_state = "local".into();
        local.detail = "로컬 문서 저장소의 원문".into();
        local.space_url.clear();
        assert!(local.validate().is_ok());
        let receipt = store.apply_project_record(&local).unwrap();
        let entry = store.get_entry(receipt.entry_id.as_ref().unwrap()).unwrap();
        assert!(entry.body.contains("로컬 저장 완료"));
        assert!(!entry.body.contains("Space 연결 대기"));
        assert!(entry.source_url.is_empty());
        local.space_url = record().space_url;
        assert!(local.validate().is_err());
    }

    #[test]
    fn record_without_pr_round_trips() {
        let store = Storage::open(":memory:").unwrap();
        let saved = store.apply_project_record(&record()).unwrap();
        assert_eq!(saved.outcome, "saved");
        let entry = store.get_entry(saved.entry_id.as_ref().unwrap()).unwrap();
        assert_eq!(entry.title, "입력 문제 원인과 해결");
        assert_eq!(
            entry.body,
            "오류를 수정했다. 실제 IME 동작은 아직 확인하지 않았다.\n\n상태: 검증 대기"
        );
        assert_eq!(
            entry.source_url,
            "https://chatgpt.com/space/page_6ae1b18a44f08191ad2a86276972476c"
        );
        assert_eq!(store.list_projects().unwrap().len(), 1);
        assert!(store
            .search_diary("원인", 20)
            .unwrap()
            .iter()
            .any(|hit| hit.id == entry.id));
    }

    #[test]
    fn revisions_are_idempotent_and_stale_is_ignored() {
        let store = Storage::open(":memory:").unwrap();
        let first = record();
        let initial = store.apply_project_record(&first).unwrap();
        assert_eq!(
            store.apply_project_record(&first).unwrap().outcome,
            "unchanged"
        );
        let mut next = first.clone();
        next.revision = 2;
        next.summary = "저장과 재열기를 확인했다. 다중 모니터는 미확인.".into();
        next.status = "verified".into();
        assert_eq!(
            store.apply_project_record(&next).unwrap().entry_id,
            initial.entry_id
        );
        assert_eq!(store.apply_project_record(&first).unwrap().outcome, "stale");
        let entry = store.get_entry(initial.entry_id.as_ref().unwrap()).unwrap();
        assert!(entry.body.contains("다중 모니터는 미확인."));
        next.summary = "같은 revision의 다른 입력".into();
        assert_eq!(
            store.apply_project_record(&next).unwrap().outcome,
            "conflict"
        );
        assert_eq!(
            store
                .list_entries("2026-10-01", "2026-10-31", None, 0)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn user_edit_and_delete_are_preserved() {
        let store = Storage::open(":memory:").unwrap();
        let mut input = record();
        let saved = store.apply_project_record(&input).unwrap();
        let entry = store.get_entry(saved.entry_id.as_ref().unwrap()).unwrap();
        store
            .save_entry(EntryInput {
                id: Some(entry.id.clone()),
                project_id: entry.project_id,
                date: entry.date,
                kind: entry.kind,
                title: "내가 고친 제목".into(),
                body: entry.body,
                source_url: entry.source_url,
            })
            .unwrap();
        input.revision = 2;
        input.summary = "후속 결과".into();
        assert_eq!(
            store.apply_project_record(&input).unwrap().outcome,
            "conflict"
        );
        assert_eq!(store.get_entry(&entry.id).unwrap().title, "내가 고친 제목");
        store.delete_entry(&entry.id).unwrap();
        assert_eq!(
            store.apply_project_record(&input).unwrap().outcome,
            "conflict"
        );
        assert!(store
            .list_entries("2026-10-01", "2026-10-31", None, 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn ambiguous_project_path_is_rejected() {
        let store = Storage::open(":memory:").unwrap();
        for title in ["첫 프로젝트", "중복 프로젝트"] {
            store
                .save_project(ProjectInput {
                    id: None,
                    title: title.into(),
                    description: String::new(),
                    repo_path: record().repo_path,
                })
                .unwrap();
        }
        assert!(store
            .apply_project_record(&record())
            .unwrap_err()
            .contains("여러 프로젝트"));
        assert!(store
            .list_entries("2026-10-01", "2026-10-31", None, 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn invalid_record_is_atomic() {
        let store = Storage::open(":memory:").unwrap();
        for input in [
            ProjectRecord {
                schema_version: 99,
                ..record()
            },
            ProjectRecord {
                work_id: "../escape".into(),
                ..record()
            },
            ProjectRecord {
                revision: 0,
                ..record()
            },
            ProjectRecord {
                space_url:
                    "https://chatgpt.com.evil.test/space/page_6ae1b18a44f08191ad2a86276972476c"
                        .into(),
                ..record()
            },
            ProjectRecord {
                space_url: "http://chatgpt.com/space/page_6ae1b18a44f08191ad2a86276972476c".into(),
                ..record()
            },
            ProjectRecord {
                title: " ".into(),
                ..record()
            },
            ProjectRecord {
                date: "2026-02-30".into(),
                ..record()
            },
            ProjectRecord {
                repo_path: "relative/path".into(),
                ..record()
            },
            ProjectRecord {
                status: "invented-success".into(),
                ..record()
            },
        ] {
            assert!(store.apply_project_record(&input).is_err());
        }
        assert!(store.list_projects().unwrap().is_empty());
    }

    #[test]
    fn v2_migration_preserves_records() {
        let dir = std::env::temp_dir().join(format!(
            "timeschedule-record-migration-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("schedule.db");
        {
            let store = Storage::open(&path).unwrap();
            store.connection().unwrap().execute_batch("DROP TABLE chat_experience_receipts; DROP TABLE codex_chat_sessions; DROP TABLE codex_chat_projects; DROP VIEW project_experiences; DROP TABLE portfolio_answers; DROP TABLE portfolio_editions; DROP TABLE entry_time; DROP TABLE entry_evidence; DROP TABLE IF EXISTS project_record_receipts; DROP TABLE entry_display; DROP TABLE project_display; DROP TABLE task_details; DROP TABLE calendar_categories; DROP INDEX tasks_date; PRAGMA user_version = 2; INSERT INTO notes VALUES ('128b13cb-0cf2-41da-84c4-db20dca65d73','기존 노트','보존할 본문','2026-10-30'); DROP TABLE search_order;").unwrap();
        }
        {
            let store = Storage::open(&path).unwrap();
            assert_eq!(store.list_notes().unwrap()[0].body, "보존할 본문");
            assert_eq!(
                store.apply_project_record(&record()).unwrap().outcome,
                "saved"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
