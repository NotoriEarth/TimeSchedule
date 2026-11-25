use crate::record_inbox;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use timeschedule_core::storage::Storage;
use timeschedule_desktop::paths::{self, InstanceLock};

const USAGE: &str = "record enqueue --input FILE | drain | status --work-id UUID --revision N | configure-local-chats --project UUID --enabled true/false | scan-local-chats | daily --project UUID|FOLDER --dry-run [--date YYYY-MM-DD] [--transcript FILE] [--data-dir DIR] [--result FILE]";

/// Characters of transcript per input token, and the input Codex CLI adds to every
/// call. Both measured on 2026-11-24 (see docs/daily-record.md); the first is rounded
/// down for Korean text, so the estimate is a rough upper figure.
const CHARS_PER_TOKEN: f64 = 3.3;
const CALL_OVERHEAD_TOKENS: usize = 11_449;

fn parse(args: &[String]) -> Result<(&str, HashMap<&str, &str>), String> {
    if args.len() < 2 || args[0] != "record" {
        return Err(USAGE.into());
    }
    let command = args[1].as_str();
    let (extra, optional, flags): (&[&str], &[&str], &[&str]) = match command {
        "enqueue" => (&["--input"], &[], &[]),
        "drain" => (&[], &[], &[]),
        "status" => (&["--work-id", "--revision"], &[], &[]),
        "configure-sync" => (&["--node", "--script", "--state-dir"], &[], &[]),
        "configure-codex-chats" | "configure-local-chats" => {
            (&["--project", "--enabled"], &[], &[])
        }
        "scan-codex-chats" | "scan-local-chats" => (&[], &[], &[]),
        "daily" => (&["--project"], &["--date", "--transcript"], &["--dry-run"]),
        _ => return Err(USAGE.into()),
    };
    let mut options = HashMap::new();
    let mut index = 2;
    while index < args.len() {
        let key = args[index].as_str();
        if flags.contains(&key) {
            if options.insert(key, "true").is_some() {
                return Err(format!("중복 인자: {key}"));
            }
            index += 1;
            continue;
        }
        if !["--data-dir", "--result"].contains(&key)
            && !extra.contains(&key)
            && !optional.contains(&key)
        {
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

/// What `run` does with `args`, without the printing and the result file.
#[cfg(test)]
fn execute(args: &[String]) -> Result<serde_json::Value, String> {
    let (command, options) = parse(args)?;
    let one_file = result_is_transcript(&options);
    perform(command, &options, one_file)
}

/// Runs a command that `parse` read. `one_file` is `result_is_transcript` of its options.
fn perform(
    command: &str,
    options: &HashMap<&str, &str>,
    one_file: bool,
) -> Result<serde_json::Value, String> {
    if command == "daily" {
        return daily(options, one_file);
    }
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
            if command.ends_with("-chats") {
                return Err(message);
            }
            return Ok(match queued {
                Some(receipt) => serde_json::to_value(receipt).map_err(|e| e.to_string())?,
                None => serde_json::json!({ "outcome": "queued", "message": message }),
            });
        }
    };
    let store = Storage::open(dir.join("schedule.db"))?;
    if matches!(command, "configure-codex-chats" | "configure-local-chats") {
        let enabled = options["--enabled"]
            .parse::<bool>()
            .map_err(|_| "--enabled에 true 또는 false를 지정하세요.")?;
        store.set_capture_sources(
            options["--project"],
            enabled,
            command == "configure-local-chats",
        )?;
        return Ok(serde_json::json!({"outcome":"configured","enabled":enabled}));
    }
    if matches!(command, "scan-codex-chats" | "scan-local-chats") {
        if command == "scan-codex-chats" {
            let report = store.scan_codex_chats(&crate::worker::codex_home()?, true)?;
            return Ok(
                serde_json::json!({"outcome":if report.failed>0 {"attention"}else{"scanned"},"report":report}),
            );
        }
        let scan = crate::worker::scan_chats(&store, &dir, true)?;
        let originals = match &scan.originals {
            Ok(report) => serde_json::json!(report),
            Err(message) => serde_json::json!({"error": message}),
        };
        return Ok(
            serde_json::json!({"outcome":if scan.needs_attention() {"attention"}else{"scanned"},"report":scan.excerpts,"originals":originals}),
        );
    }
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

/// Where `path` is or will be, to tell whether two arguments name one file. The
/// nearest folder above it that exists is resolved through its links, and the parts
/// not made yet are put after it.
fn place(path: &Path) -> String {
    // On NTFS `day.md::$DATA` and `day.md:note` are streams of the file `day.md`: in a
    // file name, a `:` starts the stream name, so the file is what comes before it.
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let file = match name.as_deref().and_then(|n| n.split_once(':')) {
        Some((stem, _)) => path.with_file_name(stem),
        None => path.to_path_buf(),
    };
    let path = file.as_path();
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let resolved = absolute.ancestors().find_map(|above| {
        let made = std::fs::canonicalize(above).ok()?;
        let rest = absolute.strip_prefix(above).ok()?;
        // Joining an empty rest would end the path of a file that exists with `\`.
        Some(if rest.as_os_str().is_empty() {
            made
        } else {
            made.join(rest)
        })
    });
    // Windows file names do not differ by case.
    resolved
        .unwrap_or(absolute)
        .to_string_lossy()
        .to_lowercase()
}

/// Whether `--transcript` and `--result` name one file. The result is written last and
/// would replace the transcript. This is asked once, before anything is written:
/// writing the transcript makes folders, and a path can resolve differently after.
fn result_is_transcript(options: &HashMap<&str, &str>) -> bool {
    match (options.get("--transcript"), options.get("--result")) {
        (Some(transcript), Some(result)) => {
            place(Path::new(transcript)) == place(Path::new(result))
        }
        _ => false,
    }
}

/// The day transcript of one project. It reads only the kept originals in the document
/// store, so it runs while the app is open and makes no data folder. The model call
/// comes in a later version.
fn daily(options: &HashMap<&str, &str>, one_file: bool) -> Result<serde_json::Value, String> {
    use timeschedule_core::daily_record::{day_transcript, kst};
    if !options.contains_key("--dry-run") {
        return Err("record daily는 아직 --dry-run으로만 실행합니다. 하루 대화록만 만들고 모델은 부르지 않습니다.".into());
    }
    if one_file {
        return Err("--transcript와 --result는 서로 다른 파일이어야 합니다.".into());
    }
    let date = match options.get("--date") {
        // The parser also takes `2026-1-2` and `+10000-01-01`, so the shape is checked
        // first: four digits, two and two.
        Some(text) => chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            .filter(|_| {
                let shaped = |(at, byte): (usize, u8)| match at {
                    4 | 7 => byte == b'-',
                    _ => byte.is_ascii_digit(),
                };
                text.len() == 10 && text.bytes().enumerate().all(shaped)
            })
            .ok_or("--date는 YYYY-MM-DD 형식이어야 합니다.")?,
        None => chrono::Utc::now().with_timezone(&kst()).date_naive(),
    };
    let dir = paths::data_dir(options.get("--data-dir").map(PathBuf::from), true)?;
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    let vault = crate::record_original::originals_vault(&dir)?;
    let transcript = day_transcript(&vault, options["--project"], date)?;
    let written = match options.get("--transcript") {
        Some(path) => {
            let path = Path::new(path);
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(path, transcript.render()).map_err(|e| e.to_string())?;
            Some(path.to_string_lossy().into_owned())
        }
        None => None,
    };
    let summary = transcript.summary();
    let estimate = (summary.chars as f64 / CHARS_PER_TOKEN).ceil() as usize + CALL_OVERHEAD_TOKENS;
    Ok(serde_json::json!({
        "outcome": if transcript.warnings.is_empty() { "dry_run" } else { "attention" },
        "project": {"id": transcript.project_id, "title": transcript.title},
        "date": date.to_string(),
        "summary": summary,
        "estimatedInputTokens": estimate,
        "sources": transcript.sources,
        "warnings": transcript.warnings,
        "latestArchived": transcript.latest_archived,
        "transcript": written,
    }))
}

/// Runs the record command line in `args` (`record ...`) and returns the exit code.
///
/// The outcome is printed, and written to `--result` when the arguments say beyond
/// doubt which file that is: not when they could not be read, and not when the file
/// is also the `--transcript`, which a refusal must leave as it was. Any other error
/// is written there, so a caller that reads the file does not take an older result
/// for this run's.
pub fn run(args: &[String]) -> i32 {
    let parsed = parse(args);
    let one_file = parsed
        .as_ref()
        .is_ok_and(|(_, options)| result_is_transcript(options));
    let output = parsed
        .as_ref()
        .map_err(String::clone)
        .and_then(|(command, options)| perform(command, options, one_file))
        .unwrap_or_else(|message| serde_json::json!({"outcome": "error", "message": message}));
    let bytes = serde_json::to_vec_pretty(&output).expect("JSON value serialization");
    if let Ok((_, options)) = &parsed {
        if let Some(path) = options.get("--result").filter(|_| !one_file) {
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
            vec!["record", "daily", "--dry-run"],
            vec![
                "record",
                "daily",
                "--project",
                "x",
                "--dry-run",
                "--dry-run",
            ],
            vec!["record", "enqueue", "--input", "x", "--dry-run"],
        ] {
            assert!(execute(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
    }

    #[test]
    fn record_daily_runs_only_as_a_dry_run_for_now() {
        let dir = TestDir::new();
        let args = |extra: &[&str]| {
            let mut args = vec!["record", "daily", "--project", "x"];
            args.extend_from_slice(extra);
            args.extend(["--data-dir", dir.0.to_str().unwrap()]);
            args.into_iter().map(String::from).collect::<Vec<_>>()
        };
        assert!(execute(&args(&[])).unwrap_err().contains("--dry-run"));
        for date in [
            "2026/11/24",
            "2026-1-2",
            "2026-11-24 ",
            "+2026-11-24",
            "-0001-01-01",
            "+10000-01-01",
            "02026-11-24",
            "2026-02-30",
        ] {
            assert!(execute(&args(&["--dry-run", "--date", date]))
                .unwrap_err()
                .contains("YYYY-MM-DD"));
        }
    }

    /// Through `run`, which is what writes `--result`. A dry run that succeeds is run
    /// by `tests/daily-record.test.mjs`, which can give the program a home of its own.
    #[test]
    fn record_daily_refuses_bad_arguments_before_it_writes_anything() {
        let dir = TestDir::new();
        let data = dir.0.join("no-data-folder-yet");
        let out = dir.0.join("out.md");
        // The same file, spelled differently.
        let same = dir.0.join("OUT.MD");
        let result = dir.0.join("result.json");
        let daily = |extra: &[&str]| {
            let mut args = vec!["record", "daily", "--project", "x"];
            args.extend_from_slice(extra);
            args.extend(["--data-dir", data.to_str().unwrap()]);
            run(&args.into_iter().map(String::from).collect::<Vec<_>>())
        };
        // A transcript of an earlier run is not replaced by the refusal.
        std::fs::write(&out, "앞서 만든 대화록").unwrap();
        let named_twice = [
            "--dry-run",
            "--transcript",
            out.to_str().unwrap(),
            "--result",
            same.to_str().unwrap(),
        ];
        assert_eq!(daily(&named_twice), 1);
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "앞서 만든 대화록");
        // Arguments that cannot be read do not say where the result goes.
        let unread = ["--dry-run", "--result", result.to_str().unwrap(), "--bogus"];
        assert_eq!(daily(&unread), 1);
        assert!(!result.exists());
        // Any other refusal is written to the result file the caller named.
        let bad_date = [
            "--dry-run",
            "--date",
            "2026/11/24",
            "--result",
            result.to_str().unwrap(),
        ];
        assert_eq!(daily(&bad_date), 1);
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&result).unwrap()).unwrap();
        assert_eq!(written["outcome"], "error");
        assert!(written["message"].as_str().unwrap().contains("YYYY-MM-DD"));
        assert_eq!(daily(&["--result", result.to_str().unwrap()]), 1);
        assert!(std::fs::read_to_string(&result)
            .unwrap()
            .contains("--dry-run"));
        assert!(!data.exists());
        let left: Vec<_> = std::fs::read_dir(&dir.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left.len(), 2, "{left:?}");
    }

    #[test]
    fn record_daily_sees_a_stream_name_as_its_file() {
        let dir = TestDir::new();
        let transcript = dir.0.join("day.md");
        for result in ["day.md::$DATA", "DAY.md:note"] {
            let result = dir.0.join(result);
            let options = HashMap::from([
                ("--transcript", transcript.to_str().unwrap()),
                ("--result", result.to_str().unwrap()),
            ]);
            assert!(result_is_transcript(&options), "{}", result.display());
        }
        let other = dir.0.join("result.json");
        let options = HashMap::from([
            ("--transcript", transcript.to_str().unwrap()),
            ("--result", other.to_str().unwrap()),
        ]);
        assert!(!result_is_transcript(&options));
    }

    /// The two paths are one file that is not there yet, in a folder that is not there
    /// yet, and one of them goes through a junction.
    #[cfg(windows)]
    #[test]
    fn record_daily_sees_one_file_through_a_junction_to_a_folder_not_made_yet() {
        let dir = TestDir::new();
        let real = dir.0.join("real");
        let link = dir.0.join("link");
        std::fs::create_dir_all(&real).unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&real)
            .output()
            .unwrap();
        assert!(made.status.success());
        let transcript = real.join("new").join("deeper").join("day.md");
        let result = link.join("new").join("deeper").join("DAY.md");
        let args = [
            "record",
            "daily",
            "--project",
            "x",
            "--dry-run",
            "--transcript",
            transcript.to_str().unwrap(),
            "--result",
            result.to_str().unwrap(),
            "--data-dir",
            dir.0.join("no-data-folder-yet").to_str().unwrap(),
        ]
        .map(String::from);
        assert!(execute(&args).unwrap_err().contains("서로 다른 파일"));
        assert_eq!(run(&args), 1);
        assert!(!real.join("new").exists());
        std::fs::remove_dir(&link).unwrap();
    }
}
