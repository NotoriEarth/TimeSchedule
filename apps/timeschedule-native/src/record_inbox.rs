use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use timeschedule_core::{
    project_records::{ProjectRecord, RecordReceipt},
    storage::Storage,
};

pub const MAX_INPUT_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrainReport {
    pub saved: usize,
    pub unchanged: usize,
    pub stale: usize,
    pub conflicts: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct SavedReceipt {
    record: ProjectRecord,
    receipt: RecordReceipt,
}

fn filename(id: &str, revision: u64) -> Result<String, String> {
    if uuid::Uuid::parse_str(id).map(|x| x.to_string()).as_deref() != Ok(id)
        || revision == 0
        || revision > i64::MAX as u64
    {
        return Err("작업 ID 또는 revision을 확인해 주세요.".into());
    }
    Ok(format!("{id}.{revision}.json"))
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("기록 파일 크기 제한을 초과했습니다.".into());
    }
    Ok(bytes)
}

pub fn read_record(path: &Path) -> Result<ProjectRecord, String> {
    let record: ProjectRecord = serde_json::from_slice(&read_limited(path, MAX_INPUT_BYTES)?)
        .map_err(|e| format!("기록 JSON 오류: {e}"))?;
    record.validate()?;
    Ok(record)
}

fn read_saved(path: &Path) -> Result<SavedReceipt, String> {
    serde_json::from_slice(&read_limited(path, MAX_INPUT_BYTES * 2)?)
        .map_err(|e| format!("기록 수신 결과 오류: {e}"))
}

// Atomically publish an immutable file. An existing destination is never
// replaced, including a concurrent producer's file. Both links stay on the
// same volume; a crash before publication leaves only a harmless .tmp file.
fn write_once(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("파일 위치가 올바르지 않습니다.")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        fs::hard_link(&temp, path).map_err(|e| format!("기록 파일 원자적 저장 실패: {e}"))
    })();
    let _ = fs::remove_file(&temp);
    result
}

fn queued(record: &ProjectRecord) -> RecordReceipt {
    record.receipt(
        "queued",
        None,
        "수신 큐에 저장했습니다. 다이어리 반영을 기다립니다.",
    )
}

pub fn enqueue(dir: &Path, record: &ProjectRecord) -> Result<RecordReceipt, String> {
    record.validate()?;
    let name = filename(&record.work_id, record.revision)?;
    let saved_path = dir.join("record-receipts").join(&name);
    if saved_path.exists() {
        let saved = read_saved(&saved_path)?;
        if saved.record != *record {
            return Err("같은 revision에 다른 내용이 이미 수신되었습니다.".into());
        }
        return Ok(saved.receipt);
    }
    let path = dir.join("record-inbox").join(name);
    let bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err("기록 파일 크기 제한을 초과했습니다.".into());
    }
    if let Err(error) = write_once(&path, &bytes) {
        if path.exists() && read_record(&path)? == *record {
            return Ok(queued(record));
        }
        return Err(error);
    }
    Ok(queued(record))
}

pub fn receipt(dir: &Path, id: &str, revision: u64) -> Result<RecordReceipt, String> {
    let name = filename(id, revision)?;
    let saved_path = dir.join("record-receipts").join(&name);
    if saved_path.exists() {
        let saved = read_saved(&saved_path)?;
        if saved.receipt.work_id != id || saved.receipt.revision != revision {
            return Err("기록 수신 결과의 식별자가 일치하지 않습니다.".into());
        }
        return Ok(saved.receipt);
    }
    let pending = dir.join("record-inbox").join(name);
    if pending.exists() {
        return Ok(queued(&read_record(&pending)?));
    }
    Ok(RecordReceipt {
        work_id: id.into(),
        revision,
        outcome: "missing".into(),
        entry_id: None,
        message: "해당 수신 결과가 없습니다. 실패 보관함과 입력을 확인해 주세요.".into(),
    })
}

pub fn drain(dir: &Path, store: &Storage) -> Result<DrainReport, String> {
    let inbox = dir.join("record-inbox");
    if !inbox.exists() {
        return Ok(DrainReport::default());
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let _lock = options
        .open(dir.join("record-drain.lock"))
        .map_err(|e| format!("수신 큐 처리 잠금을 열지 못했습니다: {e}"))?;
    let mut paths: Vec<PathBuf> = fs::read_dir(&inbox)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    let mut report = DrainReport::default();
    for path in paths.into_iter().take(32) {
        let record = match read_record(&path).and_then(|record| {
            if path.file_name().and_then(|s| s.to_str())
                != Some(filename(&record.work_id, record.revision)?.as_str())
            {
                return Err("기록 파일 이름과 작업 식별자가 다릅니다.".into());
            }
            Ok(record)
        }) {
            Ok(record) => record,
            Err(error) => {
                let failed = dir.join("record-failed");
                fs::create_dir_all(&failed).map_err(|e| e.to_string())?;
                fs::rename(&path, failed.join(format!("{}.json", uuid::Uuid::new_v4())))
                    .map_err(|e| e.to_string())?;
                report.failed += 1;
                report.errors.push(error);
                continue;
            }
        };
        let outcome = match store.apply_project_record(&record) {
            Ok(result) => result,
            Err(error) => {
                report.failed += 1;
                report.errors.push(format!("{}: {error}", record.work_id));
                continue; // Retain the queue entry for a later retry.
            }
        };
        let saved_path = dir
            .join("record-receipts")
            .join(filename(&record.work_id, record.revision)?);
        let bytes = serde_json::to_vec(&SavedReceipt {
            record: record.clone(),
            receipt: outcome.clone(),
        })
        .map_err(|e| e.to_string())?;
        if let Err(error) = write_once(&saved_path, &bytes) {
            if !saved_path.exists() || read_saved(&saved_path)?.record != record {
                return Err(error);
            }
        }
        // The DB transaction and durable acknowledgement both completed.
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        match outcome.outcome.as_str() {
            "saved" => report.saved += 1,
            "unchanged" => report.unchanged += 1,
            "stale" => report.stale += 1,
            "conflict" => report.conflicts += 1,
            _ => return Err("알 수 없는 기록 저장 결과입니다.".into()),
        }
    }
    Ok(report)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    pub(crate) fn sample() -> ProjectRecord {
        ProjectRecord {
            schema_version: 1,
            work_id: "b7550794-792e-4dcb-ad5e-502ff72258c6".into(),
            revision: 1,
            project_title: "연동 테스트".into(),
            repo_path: std::env::current_dir().unwrap().to_string_lossy().into(),
            date: "2026-10-31".into(),
            kind: "troubleshooting".into(),
            title: "원인과 검증 범위".into(),
            summary: "자동 검증 통과. 실제 입력은 미확인.".into(),
            space_url: "https://chatgpt.com/space/page_6ae1b18a44f08191ad2a86276972476c".into(),
            status: "verification_pending".into(),
        }
    }

    pub(crate) struct TestDir(pub PathBuf);
    impl TestDir {
        pub(crate) fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "timeschedule-record-inbox-{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            assert!(self.0.starts_with(std::env::temp_dir()));
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn record_queue_distinguishes_queued_from_saved_and_deduplicates() {
        let dir = TestDir::new();
        let store = Storage::open(":memory:").unwrap();
        let input = sample();
        assert_eq!(enqueue(&dir.0, &input).unwrap().outcome, "queued");
        assert_eq!(enqueue(&dir.0, &input).unwrap().outcome, "queued");
        assert_eq!(
            receipt(&dir.0, &input.work_id, 1).unwrap().outcome,
            "queued"
        );
        assert_eq!(drain(&dir.0, &store).unwrap().saved, 1);
        assert_eq!(receipt(&dir.0, &input.work_id, 1).unwrap().outcome, "saved");
        assert_eq!(enqueue(&dir.0, &input).unwrap().outcome, "saved");
        assert_eq!(drain(&dir.0, &store).unwrap().saved, 0);
        let mut conflicting = input;
        conflicting.summary = "같은 revision의 다른 내용".into();
        assert!(enqueue(&dir.0, &conflicting).is_err());
    }

    #[test]
    fn record_queue_restart_after_database_commit_does_not_duplicate() {
        let dir = TestDir::new();
        let store = Storage::open(":memory:").unwrap();
        let input = sample();
        enqueue(&dir.0, &input).unwrap();
        store.apply_project_record(&input).unwrap();
        assert_eq!(drain(&dir.0, &store).unwrap().unchanged, 1);
        assert_eq!(
            receipt(&dir.0, &input.work_id, 1).unwrap().outcome,
            "unchanged"
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
    fn record_queue_isolates_malformed_and_oversized_files() {
        let dir = TestDir::new();
        let store = Storage::open(":memory:").unwrap();
        enqueue(&dir.0, &sample()).unwrap();
        let inbox = dir.0.join("record-inbox");
        std::fs::write(inbox.join("bad.json"), b"{broken").unwrap();
        std::fs::write(
            inbox.join("large.json"),
            vec![b' '; MAX_INPUT_BYTES as usize + 1],
        )
        .unwrap();
        let report = drain(&dir.0, &store).unwrap();
        assert_eq!(report.failed, 2);
        assert_eq!(report.saved, 1);
        assert_eq!(
            std::fs::read_dir(dir.0.join("record-failed"))
                .unwrap()
                .count(),
            2
        );
        assert_eq!(drain(&dir.0, &store).unwrap().failed, 0);
    }

    #[test]
    fn record_queue_newer_revision_wins_and_conflict_is_not_success() {
        let dir = TestDir::new();
        let store = Storage::open(":memory:").unwrap();
        let old = sample();
        let mut new = old.clone();
        new.revision = 2;
        new.summary = "재검증 완료. 다중 모니터 미확인.".into();
        enqueue(&dir.0, &new).unwrap();
        enqueue(&dir.0, &old).unwrap();
        drain(&dir.0, &store).unwrap();
        let saved = receipt(&dir.0, &new.work_id, 2).unwrap();
        let id = saved.entry_id.unwrap();
        assert!(store
            .get_entry(&id)
            .unwrap()
            .body
            .contains("다중 모니터 미확인."));
        store.delete_entry(&id).unwrap();
        new.revision = 3;
        enqueue(&dir.0, &new).unwrap();
        assert_eq!(drain(&dir.0, &store).unwrap().conflicts, 1);
        assert_eq!(
            receipt(&dir.0, &new.work_id, 3).unwrap().outcome,
            "conflict"
        );
    }
}
