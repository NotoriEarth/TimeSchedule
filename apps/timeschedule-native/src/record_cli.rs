use crate::record_inbox;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use timeschedule_core::storage::Storage;
use timeschedule_desktop::paths::{self, InstanceLock};

const USAGE: &str = "record enqueue --input FILE [--data-dir DIR] [--result FILE]; record drain [--data-dir DIR]; record status --work-id UUID --revision N [--data-dir DIR]";

fn parse(args: &[String]) -> Result<(&str, HashMap<&str, &str>), String> {
    if args.len() < 2 || args[0] != "record" {
        return Err(USAGE.into());
    }
    let command = args[1].as_str();
    let extra: &[&str] = match command {
        "enqueue" => &["--input"],
        "drain" => &[],
        "status" => &["--work-id", "--revision"],
        "configure-sync" => &["--node", "--script", "--state-dir"],
        _ => return Err(USAGE.into()),
    };
    let mut options = HashMap::new();
    let mut index = 2;
    while index < args.len() {
        let key = args[index].as_str();
        if !["--data-dir", "--result"].contains(&key) && !extra.contains(&key) {
            return Err(format!("알 수 없는 인자: {key}"));
        }
        let value = args
            .get(index + 1)
            .filter(|v| !v.starts_with("--") && !v.is_empty())
            .ok_or_else(|| format!("{key} 값이 필요합니다."))?;
        if options.insert(key, value.as_str()).is_some() {
            return Err(format!("중복 인자: {key}"));
        }
        index += 2;
    }
    for key in extra {
        if !options.contains_key(key) {
            return Err(format!("{key} 값이 필요합니다."));
        }
    }
    Ok((command, options))
}

pub fn execute(args: &[String]) -> Result<serde_json::Value, String> {
    let (command, options) = parse(args)?;
    let input = if command == "enqueue" {
        Some(record_inbox::read_record(Path::new(options["--input"]))?)
    } else {
        None
    };
    let dir = paths::resolve_data_dir(options.get("--data-dir").map(PathBuf::from), true)?;
    if command == "configure-sync" {
        crate::record_sync::register(
            &dir,
            Path::new(options["--node"]),
            Path::new(options["--script"]),
            Path::new(options["--state-dir"]),
        )?;
        return Ok(serde_json::json!({"outcome":"configured"}));
    }
    if command == "status" {
        let revision: u64 = options["--revision"]
            .parse()
            .map_err(|_| "revision은 양의 정수여야 합니다.")?;
        return serde_json::to_value(record_inbox::receipt(&dir, options["--work-id"], revision)?)
            .map_err(|e| e.to_string());
    }
    let queued = input
        .as_ref()
        .map(|record| record_inbox::enqueue(&dir, record))
        .transpose()?;
    let _owner = match InstanceLock::acquire(&dir) {
        Ok(owner) => owner,
        Err(message) => {
            return Ok(match queued {
                Some(receipt) => serde_json::to_value(receipt).map_err(|e| e.to_string())?,
                None => serde_json::json!({ "outcome": "queued", "message": message }),
            })
        }
    };
    let store = Storage::open(dir.join("schedule.db"))?;
    let report = record_inbox::drain(&dir, &store)?;
    if let Some(input) = input {
        let receipt = record_inbox::receipt(&dir, &input.work_id, input.revision)?;
        if receipt.outcome == "queued" && report.failed > 0 {
            return Err(report.errors.join("\n"));
        }
        serde_json::to_value(receipt).map_err(|e| e.to_string())
    } else {
        Ok(
            serde_json::json!({"outcome": if report.failed + report.conflicts > 0 { "attention" } else { "drained" }, "report": report}),
        )
    }
}

/// Runs the record command line in `args` (`record ...`) and returns the exit code.
pub fn run(args: &[String]) -> i32 {
    let output = execute(args)
        .unwrap_or_else(|message| serde_json::json!({"outcome": "error", "message": message}));
    let bytes = serde_json::to_vec_pretty(&output).expect("JSON value serialization");
    if let Ok((_, options)) = parse(args) {
        if let Some(path) = options.get("--result") {
            let result = (|| -> Result<(), String> {
                let path = Path::new(path);
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                if let Some(input) = options.get("--input") {
                    if std::fs::canonicalize(input).ok().is_some_and(|source| {
                        std::fs::canonicalize(path).ok().as_ref() == Some(&source)
                    }) {
                        return Err("결과 파일은 입력 파일과 다른 위치여야 합니다.".into());
                    }
                }
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                let temporary = parent.join(format!(".record-result-{}.tmp", uuid::Uuid::new_v4()));
                std::fs::write(&temporary, &bytes).map_err(|e| e.to_string())?;
                if let Err(error) = std::fs::rename(&temporary, path) {
                    let _ = std::fs::remove_file(&temporary);
                    return Err(error.to_string());
                }
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("결과 파일 저장 실패: {error}");
                return 1;
            }
        }
    }
    println!("{}", String::from_utf8_lossy(&bytes));
    match output["outcome"].as_str() {
        Some("error") => 1,
        Some("conflict" | "missing" | "attention") => 2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_inbox::tests::{sample, TestDir};
    use timeschedule_core::storage::Storage;
    use timeschedule_desktop::paths::InstanceLock;

    fn enqueue_args(dir: &TestDir) -> Vec<String> {
        let path = dir.0.join("한글 입력 record.json");
        std::fs::write(&path, serde_json::to_vec(&sample()).unwrap()).unwrap();
        vec![
            "record".into(),
            "enqueue".into(),
            "--input".into(),
            path.to_string_lossy().into(),
            "--data-dir".into(),
            dir.0.to_string_lossy().into(),
        ]
    }

    #[test]
    fn record_cli_saves_without_launching_a_gui() {
        let dir = TestDir::new();
        let value = execute(&enqueue_args(&dir)).unwrap();
        assert_eq!(value["outcome"], "saved");
        let store = Storage::open(dir.0.join("schedule.db")).unwrap();
        let entries = store
            .list_entries("2026-10-01", "2026-10-31", None, 0)
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert!(store
            .get_entry(&entries[0].id)
            .unwrap()
            .body
            .contains("실제 입력은 미확인."));
    }

    #[test]
    fn record_cli_leaves_queue_when_running_app_owns_database() {
        let dir = TestDir::new();
        let owner = InstanceLock::acquire(&dir.0).unwrap();
        assert_eq!(execute(&enqueue_args(&dir)).unwrap()["outcome"], "queued");
        assert!(!dir.0.join("schedule.db").exists());
        drop(owner);
        execute(&[
            "record".into(),
            "drain".into(),
            "--data-dir".into(),
            dir.0.to_string_lossy().into(),
        ])
        .unwrap();
        let status = execute(&[
            "record".into(),
            "status".into(),
            "--work-id".into(),
            sample().work_id,
            "--revision".into(),
            "1".into(),
            "--data-dir".into(),
            dir.0.to_string_lossy().into(),
        ])
        .unwrap();
        assert_eq!(status["outcome"], "saved");
    }

    #[test]
    fn record_cli_rejects_ambiguous_or_unknown_arguments() {
        for args in [
            vec!["record"],
            vec!["record", "unknown"],
            vec!["record", "status", "--revision", "0"],
            vec!["record", "enqueue", "--input"],
        ] {
            assert!(execute(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
    }
}
