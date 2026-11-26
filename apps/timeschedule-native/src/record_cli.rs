use crate::record_inbox;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use timeschedule_core::storage::Storage;
use timeschedule_desktop::paths::{self, InstanceLock};

const USAGE: &str = "record enqueue --input FILE | drain | status --work-id UUID --revision N | configure-local-chats --project UUID --enabled true/false | scan-local-chats | daily --project UUID|FOLDER [--dry-run] [--date YYYY-MM-DD] [--model NAME] [--transcript FILE] | project --project UUID|FOLDER [--dry-run] [--model NAME] (prints only) | projects | list [--project UUID|FOLDER] [--from DATE] [--to DATE] [--origin LIST] | search --query WORDS [list's filters] | show --id UUID [--evidence] | transcript --project UUID|FOLDER (--date DATE [--query WORDS | --file FILE --line N [--context K]] | --from DATE --to DATE --query WORDS) | original --project UUID|FOLDER --file FILE --line N [--lines K]; reading commands take [--offset N] [--max-chars N] and print only; every command takes [--data-dir DIR], and all but the reading ones [--result FILE]";

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
        "daily" => (
            &["--project"],
            &["--date", "--transcript", "--model"],
            &["--dry-run"],
        ),
        "project" => (&["--project"], &["--model"], &["--dry-run"]),
        _ => crate::record_read::arguments(command).ok_or(USAGE)?,
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
        // A reading command writes nothing, not even a result file: its answer is what
        // it prints. Nor does `project`, which also opens the database: a result path
        // can name the database or a file beside it by a way no path check sees.
        let reading = crate::record_read::arguments(command).is_some();
        if key == "--result" && (reading || command == "project") {
            return Err(
                "읽기 명령과 project 명령은 --result를 받지 않습니다. 결과는 표준 출력으로만 나옵니다."
                    .into(),
            );
        }
        if !["--data-dir", "--result"].contains(&key)
            && !extra.contains(&key)
            && !optional.contains(&key)
        {
            return Err(format!("알 수 없는 인자: {key}"));
        }
        // Words to search for may start like an argument: `--query --dry-run`.
        let value = args
            .get(index + 1)
            .filter(|v| (key == "--query" || !v.starts_with("--")) && !v.is_empty())
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
pub(crate) fn execute(args: &[String]) -> Result<serde_json::Value, String> {
    let (command, options) = parse(args)?;
    let store = document_store(command, &options);
    let refused = refused_outputs(command, &options, store.as_ref());
    perform(command, &options, refused, store)
}

/// Runs a command that `parse` read. `store` is `document_store` of it and `refused` is
/// `refused_outputs` of that same store.
fn perform(
    command: &str,
    options: &HashMap<&str, &str>,
    refused: Option<String>,
    store: Option<Store>,
) -> Result<serde_json::Value, String> {
    // A command that does not use the store is stopped before it does anything, too.
    if let (None, Some(why)) = (&store, &refused) {
        return Err(why.clone());
    }
    if let Some(store) = store {
        return match refused {
            Some(why) => Err(why),
            None if command == "daily" => daily(options, store),
            None if command == "project" => {
                let vault = store?;
                let db = crate::record_read::database(options)?;
                crate::record_project::perform(options, &vault, &db)
            }
            None => crate::record_read::perform(command, options, store),
        };
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
        let scan = crate::worker::scan_chats(&store, &dir, true);
        // A part that could not start says why in place of its counts.
        fn part<T: serde::Serialize>(result: &Result<T, String>) -> serde_json::Value {
            match result {
                Ok(report) => serde_json::json!(report),
                Err(message) => serde_json::json!({"error": message}),
            }
        }
        // The excerpt scan that could not start at all is the error it was before this
        // command reported each part: exit code 1, with the other parts' results kept.
        let outcome = match (&scan.excerpts, scan.needs_attention()) {
            (Err(_), _) => "error",
            (_, true) => "attention",
            _ => "scanned",
        };
        return Ok(serde_json::json!({
            "outcome": outcome,
            "message": scan.excerpts.as_ref().err(),
            "report": part(&scan.excerpts),
            "originals": part(&scan.originals),
            "daily": part(&scan.daily),
        }));
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

/// Whether `path` (a `place`) is `folder` or below it.
fn within(path: &str, folder: &str) -> bool {
    // A drive's root, `c:\`, already ends with the separator.
    let folder = folder.trim_end_matches(['\\', '/']);
    path.strip_prefix(folder)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['\\', '/']))
}

/// Whether an output file named `path` would be written among the kept originals, the
/// daily records or the project records of the document store at `vault`. The program
/// keeps those folders itself, and an output there could replace a record or a kept
/// revision.
fn in_kept_folders(vault: &Path, path: &str) -> bool {
    let path = Path::new(path);
    // The file the output names may itself be a link: `place` follows it to where it
    // points, but writing replaces the name. So the name's own place, its folder
    // resolved and the name put after it, is checked too.
    let entry = match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => {
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            Some(
                place(parent).trim_end_matches(['\\', '/']).to_owned()
                    + "\\"
                    + &name.to_string_lossy().to_lowercase(),
            )
        }
        _ => None,
    };
    let places = [Some(place(path)), entry];
    ["daily", "originals", "project"].iter().any(|folder| {
        let folder = place(&vault.join(folder));
        places.iter().flatten().any(|at| within(at, &folder))
    })
}

/// Where the document store of a `daily`, `project` or reading run is, or why it was
/// not found.
type Store = Result<PathBuf, String>;

/// The document store `daily` and the reading commands read, and `daily` writes; `None`
/// for any other command. A run asks this once. The check of where its outputs go and
/// the run itself are given the same answer, so a setting changed in between cannot
/// have the check look at one store while the run writes over a record of another.
fn document_store(command: &str, options: &HashMap<&str, &str>) -> Option<Store> {
    (matches!(command, "daily" | "project") || crate::record_read::arguments(command).is_some())
        .then(|| {
            #[cfg(test)]
            tests::STORE_LOOKUPS.with(|count| count.set(count.get() + 1));
            let dir = paths::data_dir(options.get("--data-dir").map(PathBuf::from), true)?;
            let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
            crate::record_original::originals_vault(&dir)
        })
}

/// Why `daily` must not write the files its arguments name, if it must not. `store` is
/// `document_store` of the run. Such a run writes neither file, the result included: a
/// refusal must leave a transcript, a record or a kept revision as it was. This is
/// asked once, before anything is written: writing makes folders, and a path can
/// resolve differently after. A reading command names no file to write (`parse`
/// refuses `--result` for it), so this has nothing to refuse there.
fn refused_outputs(
    command: &str,
    options: &HashMap<&str, &str>,
    store: Option<&Store>,
) -> Option<String> {
    let Some(store) = store else {
        return kept_result(command, options);
    };
    if result_is_transcript(options) {
        return Some("--transcript와 --result는 서로 다른 파일이어야 합니다.".into());
    }
    let named: Vec<(&str, &str)> = ["--result", "--transcript"]
        .into_iter()
        .filter_map(|key| Some((key, *options.get(key)?)))
        .collect();
    if named.is_empty() {
        return None;
    }
    // While the store cannot be found, nothing says an output is not one of its
    // records or kept revisions, so none is written.
    let vault = match store {
        Ok(vault) => vault,
        Err(why) => {
            return Some(format!(
                "{why} 문서 저장소의 위치를 확인하지 못해 --result와 --transcript 파일을 쓰지 않았습니다."
            ))
        }
    };
    named.into_iter().find_map(|(key, path)| {
        in_kept_folders(vault, path).then(|| {
            format!("{key}는 문서 저장소의 daily, originals, project 폴더 밖의 파일이어야 합니다.")
        })
    })
}

/// Why a command that does not use the document store must not write its `--result`
/// where it names: among the store's kept originals, daily records or project
/// records, which a result file would replace. `configure-sync` is held against the
/// store it registers as well as the one registered now. A store that cannot be found
/// refuses nothing here: these commands ran before the store existed, and keep running.
fn kept_result(command: &str, options: &HashMap<&str, &str>) -> Option<String> {
    let result = options.get("--result")?;
    let registered = paths::data_dir(options.get("--data-dir").map(PathBuf::from), true)
        .ok()
        .and_then(|dir| {
            let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
            crate::record_original::originals_vault(&dir).ok()
        });
    let registering = (command == "configure-sync")
        .then(|| crate::record_original::state_vault(Path::new(options.get("--state-dir")?)))
        .flatten();
    [registered, registering]
        .iter()
        .flatten()
        .any(|vault| in_kept_folders(vault, result))
        .then(|| {
            "--result는 문서 저장소의 daily, originals, project 폴더 밖의 파일이어야 합니다.".into()
        })
}

/// Writes `bytes` whole to a temporary file beside `path` and renames it to `path`. The
/// name then holds what it held or all of `bytes`, and a file the name was a hard link
/// of is left as it was.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(".record-output-{}.tmp", uuid::Uuid::new_v4()));
    let written =
        std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written.map_err(|e| e.to_string())
}

/// The day transcript of one project and, without `--dry-run`, the daily record a
/// model makes of it. It reads the kept originals in the document store and writes
/// only below the store's `daily` folder, so it runs while the app is open and makes
/// no data folder. `store` is `document_store` of the run.
fn daily(options: &HashMap<&str, &str>, store: Store) -> Result<serde_json::Value, String> {
    use timeschedule_core::{
        daily_record::{
            day_transcript, kst,
            record::{estimated_tokens, Outcome},
        },
        terra,
    };
    let date = match options.get("--date") {
        Some(text) => date_arg(text).map_err(|e| format!("--date는 {e}"))?,
        None => chrono::Utc::now().with_timezone(&kst()).date_naive(),
    };
    let vault = store?;
    let transcript = day_transcript(&vault, options["--project"], date)?;
    let written = match options.get("--transcript") {
        Some(path) => {
            let path = Path::new(path);
            write_whole(path, transcript.render().as_bytes())?;
            Some(path.to_string_lossy().into_owned())
        }
        None => None,
    };
    let summary = transcript.summary();
    let mut out = serde_json::json!({
        "outcome": if transcript.warnings.is_empty() { "dry_run" } else { "attention" },
        "project": {"id": transcript.project_id, "title": transcript.title},
        "date": date.to_string(),
        "summary": summary,
        "estimatedInputTokens": estimated_tokens(summary.chars),
        "sources": transcript.sources,
        "warnings": transcript.warnings,
        "latestArchived": transcript.latest_archived,
        "transcript": written,
    });
    // A day with warnings and no dialogue has nothing to send, and its warnings are
    // what the caller needs to see: the dialogue may be in a file that was not read.
    let unread_day = transcript.blocks.is_empty() && !transcript.warnings.is_empty();
    if options.contains_key("--dry-run") || unread_day {
        return Ok(out);
    }
    let model = options.get("--model").copied().unwrap_or(terra::MODEL);
    let outcome = summarize_day(&vault, &transcript, model).map_err(|e| e.message)?;
    let warned = !transcript.warnings.is_empty();
    match outcome {
        Outcome::Saved { path, record } => {
            out["outcome"] = (if warned { "attention" } else { "saved" }).into();
            out["record"] = serde_json::json!({
                "path": path,
                "revision": record.revision,
                "model": record.model,
                "usage": record.usage,
                "checks": record.checks,
                "overview": record.overview.is_some(),
                "development": record.development.len(),
                "troubleshooting": record.troubleshooting.len(),
                "decisions": record.decisions.len(),
                "nextSteps": record.next_steps.len(),
            });
        }
        Outcome::Unchanged { path, revision } => {
            out["outcome"] = (if warned { "attention" } else { "unchanged" }).into();
            out["record"] = serde_json::json!({"path": path, "revision": revision});
        }
        Outcome::Busy { path } => {
            out["outcome"] = "busy".into();
            out["record"] = serde_json::json!({"path": path});
        }
    }
    Ok(out)
}

/// A date argument: `YYYY-MM-DD`, a real date. The parser also takes `2026-1-2` and
/// `+10000-01-01`, so the shape is checked first: four digits, two and two.
pub(crate) fn date_arg(text: &str) -> Result<chrono::NaiveDate, String> {
    let shaped = |(at, byte): (usize, u8)| match at {
        4 | 7 => byte == b'-',
        _ => byte.is_ascii_digit(),
    };
    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .filter(|_| text.len() == 10 && text.bytes().enumerate().all(shaped))
        .ok_or_else(|| "YYYY-MM-DD 형식이어야 합니다.".to_string())
}

/// The daily record asks for a summary of one day, not hard reasoning.
const DAILY_EFFORT: &str = "medium";

/// What says that the daily record of `transcript` would be asked the same question
/// again: the fingerprint `summarize` compares, for the model the app uses.
pub(crate) fn daily_input_key(
    transcript: &timeschedule_core::daily_record::DayTranscript,
) -> String {
    timeschedule_core::daily_record::record::fingerprint(
        transcript,
        timeschedule_core::terra::MODEL,
        DAILY_EFFORT,
    )
}

/// Why a daily record was not made, and whether the model had been asked by then: a
/// refused answer or a failed call has used the model, a lock or a file that could not
/// be read has not.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DailyError {
    pub message: String,
    pub asked: bool,
}

/// Has `model` make the daily record of `transcript` through the logged-in Codex CLI,
/// checked and kept below `vault`. The command line and the app's 하루 정리 both make
/// records here.
pub(crate) fn summarize_day(
    vault: &Path,
    transcript: &timeschedule_core::daily_record::DayTranscript,
    model: &str,
) -> Result<timeschedule_core::daily_record::record::Outcome, DailyError> {
    use timeschedule_core::terra;
    let asked = std::cell::Cell::new(false);
    timeschedule_core::daily_record::record::summarize(
        vault,
        transcript,
        model,
        DAILY_EFFORT,
        |prompt, schema| {
            let reply = terra::run(
                prompt,
                &terra::Call {
                    model,
                    effort: DAILY_EFFORT,
                    schema: Some(schema),
                    timeout: std::time::Duration::from_secs(600),
                    answer_limit: 1 << 20,
                    read_usage: true,
                    // The answer is checked as the model wrote it.
                    unfence: false,
                },
            );
            asked.set(reply.as_ref().map_or_else(|failed| failed.sent, |_| true));
            reply
        },
    )
    .map_err(|message| DailyError {
        message,
        asked: asked.get(),
    })
}

/// What `summarize_day` would do with the transcript, without doing it: no model is
/// asked and nothing is written. `Unchanged` when the latest record was made from this
/// input, an error when the day is too large to send, `Due` when the model would be asked.
pub(crate) fn would_summarize_day(
    vault: &Path,
    transcript: &timeschedule_core::daily_record::DayTranscript,
    model: &str,
) -> Result<crate::worker::Made, DailyError> {
    use timeschedule_core::daily_record::record::{check_size, is_current};
    let unasked = |message| DailyError {
        message,
        asked: false,
    };
    if is_current(vault, transcript, model, DAILY_EFFORT).map_err(unasked)? {
        return Ok(crate::worker::Made::Unchanged);
    }
    check_size(transcript.summary().chars).map_err(unasked)?;
    Ok(crate::worker::Made::Due)
}

/// Runs the record command line in `args` (`record ...`) and returns the exit code.
///
/// The outcome is printed, and written to `--result` when the arguments say beyond
/// doubt which file that is and the file may be written: not when they could not be
/// read, and not when `refused_outputs` gives a reason, which it also does when the
/// document store cannot be found. Any other error is written there, so a caller that
/// reads the file does not take an older result for this run's.
pub fn run(args: &[String]) -> i32 {
    let parsed = parse(args);
    let store = parsed
        .as_ref()
        .ok()
        .and_then(|(command, options)| document_store(command, options));
    let refused = parsed
        .as_ref()
        .ok()
        .and_then(|(command, options)| refused_outputs(command, options, store.as_ref()));
    let writes_result = refused.is_none();
    let output = parsed
        .as_ref()
        .map_err(String::clone)
        .and_then(|(command, options)| perform(command, options, refused, store))
        .unwrap_or_else(|message| serde_json::json!({"outcome": "error", "message": message}));
    let bytes = serde_json::to_vec_pretty(&output).expect("JSON value serialization");
    if let Ok((_, options)) = &parsed {
        if let Some(path) = options.get("--result").filter(|_| writes_result) {
            let result = (|| -> Result<(), String> {
                let path = Path::new(path);
                if let Some(input) = options.get("--input") {
                    if std::fs::canonicalize(input).ok().is_some_and(|source| {
                        std::fs::canonicalize(path).ok().as_ref() == Some(&source)
                    }) {
                        return Err("결과 파일은 입력 파일과 다른 위치여야 합니다.".into());
                    }
                }
                write_whole(path, &bytes)
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
        Some("conflict" | "missing" | "attention" | "busy") => 2,
        _ => 0,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::record_inbox::tests::{sample, TestDir};
    use timeschedule_core::storage::Storage;
    use timeschedule_desktop::paths::InstanceLock;

    thread_local! {
        /// How many times this thread has looked up the document store of a `daily` run.
        pub(super) static STORE_LOOKUPS: std::cell::Cell<usize> =
            const { std::cell::Cell::new(0) };
    }

    #[test]
    fn a_dry_look_at_a_day_too_large_to_send_is_an_error_that_asked_nothing() {
        use timeschedule_core::daily_record::{Block, DayTranscript, Source, Speaker};
        let at = chrono::DateTime::parse_from_rfc3339("2026-11-20T01:00:00+09:00").unwrap();
        let blocks = (1..=60)
            .map(|number| Block {
                number,
                speaker: Speaker::Human,
                picked: None,
                source: 0,
                file: 0,
                line: number,
                at,
                text: "가".repeat(8000),
                clipped: false,
            })
            .collect();
        let transcript = DayTranscript {
            project_id: "p".into(),
            title: "작업".into(),
            repo_key: "c:/work".into(),
            folder: "work".into(),
            date: chrono::NaiveDate::from_ymd_opt(2026, 11, 20).unwrap(),
            sources: vec![Source {
                tag: "S1".into(),
                files: vec!["f".into()],
                tool: "claude".into(),
                client: "x".into(),
                session: "s".into(),
                agent_run: false,
            }],
            blocks,
            warnings: Vec::new(),
            latest_archived: None,
        };
        let dir = TestDir::new();
        let error = would_summarize_day(&dir.0, &transcript, "m").unwrap_err();
        assert!(error.message.contains("너무 큽니다"), "{}", error.message);
        assert!(!error.asked);
        assert!(!dir.0.join("daily").exists(), "nothing was written");
    }

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
    fn record_daily_takes_only_a_well_formed_date() {
        let dir = TestDir::new();
        let args = |extra: &[&str]| {
            let mut args = vec!["record", "daily", "--project", "x"];
            args.extend_from_slice(extra);
            args.extend(["--data-dir", dir.0.to_str().unwrap()]);
            args.into_iter().map(String::from).collect::<Vec<_>>()
        };
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
        assert!(!data.exists());
        let left: Vec<_> = std::fs::read_dir(&dir.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");
    }

    /// A data folder registered with a record state folder whose document store is
    /// `vault`, so that where the store is does not depend on the user's home.
    pub(crate) fn registered(dir: &TestDir, vault: &Path) -> PathBuf {
        let (data, state) = (dir.0.join("data"), dir.0.join("state"));
        let script = dir.0.join("project-records.mjs");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(&script, "").unwrap();
        let node = std::env::current_exe().unwrap();
        crate::record_sync::register(&data, &node, &script, &state).unwrap();
        let settings = serde_json::json!({"schemaVersion": 1, "root": vault});
        std::fs::write(state.join("vault-settings.json"), settings.to_string()).unwrap();
        data
    }

    #[test]
    fn record_daily_writes_no_output_while_the_store_cannot_be_found() {
        let dir = TestDir::new();
        let vault = dir.0.join("Vault");
        let record = vault.join("daily").join("p").join("2026-11-24.json");
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(&record, "앞서 만든 기록").unwrap();
        let data = registered(&dir, &vault);
        let out = dir.0.join("out.json");
        let daily = |date: &str, key: &str, path: &Path| {
            let args = [
                "record",
                "daily",
                "--project",
                "x",
                "--dry-run",
                "--date",
                date,
            ];
            let named = [key, path.to_str().unwrap()];
            let data = ["--data-dir", data.to_str().unwrap()];
            let args = args.iter().chain(&named).chain(&data);
            run(&args.map(|arg| arg.to_string()).collect::<Vec<_>>())
        };
        // The store's marker cannot be read, so nothing says where the records are.
        std::fs::write(vault.join("vault.json"), "{").unwrap();
        for (key, path) in [
            ("--result", &record),
            ("--transcript", &record),
            ("--result", &out),
        ] {
            assert_eq!(daily("2026-11-24", key, path), 1);
            assert_eq!(std::fs::read_to_string(&record).unwrap(), "앞서 만든 기록");
        }
        assert!(!out.exists());
        // With the store found, a run's own error is written to the result it named.
        std::fs::write(vault.join("vault.json"), r#"{"schemaVersion":1}"#).unwrap();
        assert_eq!(daily("2026/11/24", "--result", &out), 1);
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(written["outcome"], "error");
        assert!(written["message"].as_str().unwrap().contains("YYYY-MM-DD"));
        assert_eq!(daily("2026/11/24", "--result", &record), 1);
        assert_eq!(std::fs::read_to_string(&record).unwrap(), "앞서 만든 기록");
    }

    /// Two lookups could be told of two stores when the setting changes in between:
    /// the outputs would be checked against one and the run would write in the other.
    #[test]
    fn record_daily_looks_the_store_up_once_for_the_check_and_the_run() {
        let dir = TestDir::new();
        let vault = dir.0.join("Vault");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::write(vault.join("vault.json"), r#"{"schemaVersion":1}"#).unwrap();
        let data = registered(&dir, &vault);
        let out = dir.0.join("out.json");
        let args = [
            "record",
            "daily",
            "--project",
            "x",
            "--dry-run",
            "--result",
            out.to_str().unwrap(),
            "--data-dir",
            data.to_str().unwrap(),
        ]
        .map(String::from);
        let before = STORE_LOOKUPS.get();
        // The run gets as far as reading the store: it has no originals of the project.
        assert_eq!(run(&args), 1);
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert!(written["message"]
            .as_str()
            .unwrap()
            .contains("원문이 없습니다"));
        assert_eq!(STORE_LOOKUPS.get() - before, 1);
    }

    #[test]
    fn record_daily_writes_no_output_among_the_kept_records_or_originals() {
        let dir = TestDir::new();
        let vault = dir.0.join("Vault");
        std::fs::create_dir_all(vault.join("daily").join("p")).unwrap();
        let inside = |path: PathBuf| in_kept_folders(&vault, path.to_str().unwrap());
        assert!(inside(
            vault.join("daily").join("p").join("2026-11-24.json")
        ));
        assert!(inside(
            vault.join("DAILY").join("p").join("2026-11-24.r1.json")
        ));
        assert!(inside(vault.join("daily").join("p").join("x.json:stream")));
        // A folder not made yet, reached by going up and down again.
        let roundabout = vault.join("other").join("..").join("originals");
        assert!(inside(roundabout.join("new").join("day.md")));
        assert!(inside(vault.join("daily")));
        assert!(!inside(vault.join("daily-notes").join("day.md")));
        assert!(!inside(vault.join("result.json")));
        assert!(!inside(dir.0.join("result.json")));
    }

    /// A record name that is a link to a file outside the store is still a name among
    /// the records: writing the result would replace the link.
    #[cfg(windows)]
    #[test]
    fn record_daily_sees_a_linked_record_name_as_inside_the_store() {
        let dir = TestDir::new();
        let vault = dir.0.join("vault");
        let day = vault.join("daily").join("p");
        std::fs::create_dir_all(&day).unwrap();
        let outside = dir.0.join("outside.json");
        std::fs::write(&outside, "{}").unwrap();
        let name = day.join("2026-11-24.json");
        if let Err(error) = std::os::windows::fs::symlink_file(&outside, &name) {
            eprintln!("skipped: this account cannot make a file link ({error})");
            return;
        }
        assert!(in_kept_folders(&vault, name.to_str().unwrap()));
    }

    #[test]
    fn a_drive_root_holds_what_is_below_it() {
        assert!(within(r"c:\data\out.json", r"c:\"));
        assert!(within(r"c:\data", r"c:\data"));
        assert!(!within(r"c:\data-2\out.json", r"c:\data"));
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
