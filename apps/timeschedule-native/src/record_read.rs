//! The `record` commands that only read what the app keeps: projects, diary records and
//! their evidence, the dialogue of a day, and lines of a kept original. They ask no
//! model, take no lock, make no folder and write no file, not even a `--result` file:
//! the answer is what they print. So they run whether the app is open or not. See
//! `docs/record-reading.md`.
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    ops::RangeInclusive,
    path::{Path, PathBuf},
};
use timeschedule_core::{
    reading::{self, Filter, Origin, Pick},
    storage::Storage,
};
use timeschedule_desktop::paths;

type Arguments = (
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
);

/// The required, optional and flag arguments of a reading command; `None` for a command
/// that is not one.
pub(crate) fn arguments(command: &str) -> Option<Arguments> {
    const PAGE: [&str; 2] = ["--offset", "--max-chars"];
    Some(match command {
        "projects" => (&[], &PAGE, &[]),
        "list" => (
            &[],
            &["--project", "--from", "--to", "--origin", PAGE[0], PAGE[1]],
            &[],
        ),
        "search" => (
            &["--query"],
            &["--project", "--from", "--to", "--origin", PAGE[0], PAGE[1]],
            &[],
        ),
        "show" => (&["--id"], &PAGE, &["--evidence"]),
        "transcript" => (
            &["--project"],
            &[
                "--date",
                "--from",
                "--to",
                "--query",
                "--file",
                "--line",
                "--context",
                PAGE[0],
                PAGE[1],
            ],
            &[],
        ),
        "original" => (
            &["--project", "--file", "--line"],
            &["--lines", PAGE[0], PAGE[1]],
            &[],
        ),
        _ => return None,
    })
}

fn number(
    options: &HashMap<&str, &str>,
    key: &str,
    default: usize,
    allowed: RangeInclusive<usize>,
) -> Result<usize, String> {
    let Some(text) = options.get(key) else {
        return Ok(default);
    };
    text.parse::<usize>()
        .ok()
        .filter(|n| allowed.contains(n))
        .ok_or_else(|| {
            format!(
                "{key}는 {}부터 {}까지의 정수여야 합니다.",
                allowed.start(),
                allowed.end()
            )
        })
}

fn date(options: &HashMap<&str, &str>, key: &str) -> Result<Option<chrono::NaiveDate>, String> {
    options
        .get(key)
        .map(|text| crate::record_cli::date_arg(text).map_err(|e| format!("{key}: {e}")))
        .transpose()
}

fn origins(options: &HashMap<&str, &str>) -> Result<Vec<Origin>, String> {
    match options.get("--origin") {
        None => Ok(Origin::DEFAULT.to_vec()),
        Some(text) => text
            .split(',')
            .map(|key| Origin::parse(key.trim()))
            .collect(),
    }
}

/// The app's database, opened for reading only. The data folder is not made.
pub(crate) fn database(options: &HashMap<&str, &str>) -> Result<Storage, String> {
    let dir = paths::data_dir(options.get("--data-dir").map(PathBuf::from), true)?;
    Storage::open_read_only(dir.join("schedule.db"))
}

fn answer(view: impl Serialize) -> Result<Value, String> {
    let mut value = serde_json::to_value(view).map_err(|e| e.to_string())?;
    value["outcome"] = "ok".into();
    Ok(value)
}

/// Runs a reading command that `parse` read. `vault` is the document store the run
/// looked up once, or why it was not found; a command that does not need the store
/// still runs without it.
pub(crate) fn perform(
    command: &str,
    options: &HashMap<&str, &str>,
    vault: Result<PathBuf, String>,
) -> Result<Value, String> {
    let budget = number(
        options,
        "--max-chars",
        reading::DEFAULT_BUDGET,
        1000..=1_000_000,
    )?;
    // The database takes an offset as a signed 64-bit number.
    let offset = number(options, "--offset", 0, 0..=i64::MAX as usize)?;
    match command {
        "projects" => {
            let rows = database(options)?.read_projects()?;
            let total = rows.len();
            if offset > 0 && offset >= total {
                return Err(format!("--offset이 프로젝트 수({total}개)를 넘습니다."));
            }
            // The store is counted for the projects of this page only.
            let counted = rows.into_iter().skip(offset).map(|mut row| {
                if let Ok(root) = &vault {
                    (row.originals, row.daily_days) = reading::store_counts(root, &row.folder);
                }
                row
            });
            let (rows, _) = reading::fit(counted, budget);
            let store_error = vault.as_ref().err().cloned();
            let read = offset + rows.len();
            let next = (read < total).then(|| serde_json::json!({ "offset": read }));
            let mut value = answer(serde_json::json!({
                "projects": rows, "total": total, "offset": offset, "next": next,
            }))?;
            if let Some(why) = store_error {
                value["storeError"] = why.into();
            }
            Ok(value)
        }
        "list" | "search" => {
            let store = database(options)?;
            let project = options
                .get("--project")
                .map(|p| store.project_of(p))
                .transpose()?;
            let origins = origins(options)?;
            let filter = Filter {
                project: project.as_ref().map(|p| p.id.as_str()),
                from: date(options, "--from")?,
                to: date(options, "--to")?,
                origins: &origins,
            };
            answer(store.read_records(&filter, options.get("--query").copied(), offset, budget)?)
        }
        "show" => {
            let store = database(options)?;
            let id = options["--id"];
            if options.contains_key("--evidence") {
                return answer(store.read_evidence(&vault?, id, offset, budget)?);
            }
            let document = |work: &str| match &vault {
                Ok(root) => crate::record_original::document_in(root, work)
                    .map(|path| path.display().to_string()),
                Err(why) => Err(why.clone()),
            };
            answer(store.read_record(id, offset, budget, &document)?)
        }
        "transcript" => transcript(options, &vault?, offset, budget),
        "original" => {
            let line = number(options, "--line", 1, 1..=usize::MAX)?;
            let lines = number(options, "--lines", 1, 1..=200)?;
            answer(reading::read_original(
                &vault?,
                options["--project"],
                options["--file"],
                line,
                lines,
                offset,
                budget,
            )?)
        }
        _ => Err(format!("읽기 명령이 아닙니다: {command}")),
    }
}

fn transcript(
    options: &HashMap<&str, &str>,
    vault: &Path,
    offset: usize,
    budget: usize,
) -> Result<Value, String> {
    let project = options["--project"];
    let (day, from, to) = (
        date(options, "--date")?,
        date(options, "--from")?,
        date(options, "--to")?,
    );
    let query = options.get("--query").copied();
    let at = (options.get("--file"), options.contains_key("--line"));
    if let (Some(from), Some(to)) = (from, to) {
        if day.is_some() || at.0.is_some() || at.1 || options.contains_key("--context") {
            return Err("--from과 --to로 여러 날을 읽을 때는 --date, --file, --line, --context를 함께 쓰지 않습니다.".into());
        }
        let query = query.ok_or("여러 날은 --query로 찾을 때만 읽습니다.")?;
        return answer(reading::search_days(
            vault, project, from, to, query, offset, budget,
        )?);
    }
    if from.is_some() || to.is_some() {
        return Err("--from과 --to는 함께 지정해야 합니다.".into());
    }
    let day = day.ok_or("--date 또는 --from과 --to를 지정하세요.")?;
    let pick = match (at, query) {
        ((None, false), None) => Pick::All,
        ((None, false), Some(query)) => Pick::Matching(query),
        ((Some(file), true), None) => Pick::Around {
            file,
            line: number(options, "--line", 1, 1..=usize::MAX)?,
            context: number(options, "--context", 3, 0..=50)?,
        },
        ((Some(_), true), Some(_)) => {
            return Err("--file·--line과 --query는 함께 쓰지 않습니다.".into())
        }
        _ => return Err("--file과 --line은 함께 지정해야 합니다.".into()),
    };
    if options.contains_key("--context") && !matches!(pick, Pick::Around { .. }) {
        return Err("--context는 --file과 --line을 줄 때만 씁니다.".into());
    }
    answer(reading::read_day(
        vault, project, day, pick, offset, budget,
    )?)
}

#[cfg(test)]
mod tests;
