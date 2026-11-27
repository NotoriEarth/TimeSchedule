//! `record project`: the project record of one project, made from its daily records
//! and its other kept records by one model call, or with `--dry-run` only what that
//! call would be given.
use crate::record_cli::DailyError;
use serde_json::{json, Value};
use std::{collections::HashMap, path::Path};
use timeschedule_core::{
    daily_record::record::estimated_tokens,
    project_record::{self, Dossier, Outcome},
    storage::Storage,
    terra,
};

/// What a run would send and whether it would call the model; nothing is written.
pub(crate) fn preview(vault: &Path, dossier: &Dossier, model: &str) -> Result<Value, String> {
    let chars = dossier.chars();
    let estimate = estimated_tokens(chars);
    Ok(json!({
        "outcome": "dry_run",
        "project": {"id": dossier.project_id, "title": dossier.title},
        "completedOn": dossier.completed_on,
        "beforeCompletion": dossier.completed_on.is_none(),
        "days": dossier.days.len(),
        "firstDay": dossier.days.first().map(|d| &d.date),
        "lastDay": dossier.days.last().map(|d| &d.date),
        "records": dossier.record_counts(),
        "citable": dossier.citable(),
        "chars": chars,
        "estimatedInputTokens": estimate,
        "limit": project_record::MAX_ESTIMATED_TOKENS,
        "tooLarge": estimate > project_record::MAX_ESTIMATED_TOKENS,
        "missingDays": {"count": dossier.missing_days.len(), "days": dossier.missing_days},
        "warnings": dossier.warnings,
        "model": model,
        "effort": project_record::EFFORT,
        "current": project_record::is_current(vault, dossier, model, project_record::EFFORT)?,
        "path": project_record::record_path(vault, dossier)?,
    }))
}

/// Runs `record project` with the document store at `vault` and the database `store`.
pub(crate) fn perform(
    options: &HashMap<&str, &str>,
    vault: &Path,
    store: &Storage,
) -> Result<Value, String> {
    let dossier = project_record::gather(store, vault, options["--project"])?;
    let model = options
        .get("--model")
        .copied()
        .unwrap_or(project_record::MODEL);
    let mut out = preview(vault, &dossier, model)?;
    if options.contains_key("--dry-run") {
        return Ok(out);
    }
    match summarize_project(store, vault, &dossier, model).map_err(|e| e.message)? {
        Outcome::Saved { path, record } => {
            out["outcome"] = "saved".into();
            out["record"] = json!({
                "path": path,
                "revision": record.revision,
                "model": record.model,
                "usage": record.usage,
                "checks": record.checks,
                "overview": record.overview.is_some(),
                "features": record.features.len(),
                "decisions": record.decisions.len(),
                "troubleshooting": record.troubleshooting.len(),
                "results": record.results.len(),
                "limits": record.limits.len(),
            });
        }
        Outcome::Unchanged { path, revision } => {
            out["outcome"] = "unchanged".into();
            out["record"] = json!({"path": path, "revision": revision});
        }
        Outcome::Busy { path } => {
            out["outcome"] = "busy".into();
            out["record"] = json!({"path": path});
        }
    }
    Ok(out)
}

/// Has `model` make the project record of `dossier` through the logged-in Codex CLI,
/// checked and kept below `vault`. Nothing is kept when, by the time the model has
/// answered, the project is gone from `store` or has another folder: its record would
/// stand in the way of the project's next owner of that folder.
pub(crate) fn summarize_project(
    store: &Storage,
    vault: &Path,
    dossier: &Dossier,
    model: &str,
) -> Result<Outcome, DailyError> {
    let asked = std::cell::Cell::new(false);
    project_record::summarize_while(
        vault,
        dossier,
        model,
        project_record::EFFORT,
        |prompt, schema| {
            let reply = terra::run(
                prompt,
                &terra::Call {
                    model,
                    effort: project_record::EFFORT,
                    schema: Some(schema),
                    // A whole project at high effort can take many minutes.
                    timeout: std::time::Duration::from_secs(1800),
                    answer_limit: 1 << 20,
                    read_usage: true,
                    unfence: false,
                },
            );
            asked.set(reply.as_ref().map_or_else(|failed| failed.sent, |_| true));
            reply
        },
        || project_record::holds(store, dossier),
    )
    .map_err(|message| DailyError {
        message,
        asked: asked.get(),
    })
}

#[cfg(test)]
mod tests {
    use crate::record_cli::{execute, tests::registered};
    use crate::record_inbox::tests::TestDir;
    use std::path::Path;
    use timeschedule_core::{diary::ProjectInput, storage::Storage};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_dry_run_tells_what_would_be_sent_and_writes_nothing() {
        let dir = TestDir::new();
        let vault = dir.0.join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::write(vault.join("vault.json"), r#"{"schemaVersion":1}"#).unwrap();
        let data = registered(&dir, &vault);
        let repo = dir.0.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let id = {
            let store = Storage::open(data.join("schedule.db")).unwrap();
            let id = store
                .save_project(ProjectInput {
                    id: None,
                    title: "앱".into(),
                    description: String::new(),
                    repo_path: repo.to_string_lossy().into_owned(),
                })
                .unwrap()
                .id;
            store
                .save_entry(timeschedule_core::diary::EntryInput {
                    id: None,
                    project_id: Some(id.clone()),
                    date: "2026-11-20".into(),
                    kind: "development".into(),
                    title: "직접".into(),
                    body: "본문".into(),
                    source_url: String::new(),
                })
                .unwrap();
            id
        };
        let data_dir = data.to_string_lossy().into_owned();
        let out = execute(&args(&[
            "record",
            "project",
            "--project",
            &id,
            "--dry-run",
            "--data-dir",
            &data_dir,
        ]))
        .unwrap();
        assert_eq!(out["outcome"], "dry_run");
        assert_eq!(out["beforeCompletion"], true);
        assert_eq!(out["days"], 0);
        assert_eq!(out["records"]["manual"], 1);
        assert_eq!(out["model"], "gpt-6.2-astra");
        assert_eq!(out["effort"], "high");
        assert_eq!(out["current"], false);
        assert_eq!(out["tooLarge"], false);
        assert!(out["estimatedInputTokens"].as_u64().unwrap() > 0);
        assert!(!vault.join("project").exists(), "a dry run writes nothing");
        // --model names the model the run would use.
        let out = execute(&args(&[
            "record",
            "project",
            "--project",
            &id,
            "--dry-run",
            "--model",
            "other",
            "--data-dir",
            &data_dir,
        ]))
        .unwrap();
        assert_eq!(out["model"], "other");
    }

    #[test]
    fn the_command_takes_only_its_arguments_and_writes_no_result_file() {
        let dir = TestDir::new();
        let vault = dir.0.join("vault");
        std::fs::create_dir_all(vault.join("project")).unwrap();
        std::fs::write(vault.join("vault.json"), r#"{"schemaVersion":1}"#).unwrap();
        let data = registered(&dir, &vault);
        let data_dir = data.to_string_lossy().into_owned();
        assert!(execute(&args(&["record", "project", "--data-dir", &data_dir])).is_err());
        assert!(execute(&args(&[
            "record",
            "project",
            "--project",
            "x",
            "--date",
            "2026-11-20",
            "--data-dir",
            &data_dir,
        ]))
        .is_err());
        // Not even beside the database it reads: the result is only printed.
        let result = data.join("schedule.db");
        let args = args(&[
            "record",
            "project",
            "--project",
            "x",
            "--dry-run",
            "--data-dir",
            &data_dir,
            "--result",
            result.to_str().unwrap(),
        ]);
        let error = execute(&args).unwrap_err();
        assert!(error.contains("--result를 받지 않습니다"), "{error}");
        assert_eq!(crate::record_cli::run(&args), 1);
        assert!(!Path::new(&result).exists());
    }

    #[test]
    fn another_command_does_not_write_its_result_over_a_project_record() {
        let dir = TestDir::new();
        let vault = dir.0.join("vault");
        let kept = vault.join("project").join("p").join("record.json");
        std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
        std::fs::write(vault.join("vault.json"), r#"{"schemaVersion":1}"#).unwrap();
        std::fs::write(&kept, "기록").unwrap();
        let data = registered(&dir, &vault);
        let args = args(&[
            "record",
            "status",
            "--work-id",
            "11111111-1111-4111-8111-111111111111",
            "--revision",
            "1",
            "--data-dir",
            data.to_str().unwrap(),
            "--result",
            kept.to_str().unwrap(),
        ]);
        assert_eq!(crate::record_cli::run(&args), 1);
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "기록");
        // Nor over one in the store a registration is about to name.
        let other = TestDir::new();
        let state = other.0.join("state");
        std::fs::create_dir_all(&state).unwrap();
        let settings = serde_json::json!({"schemaVersion": 1, "root": vault});
        std::fs::write(state.join("vault-settings.json"), settings.to_string()).unwrap();
        let script = other.0.join("project-records.mjs");
        std::fs::write(&script, "").unwrap();
        let node = std::env::current_exe().unwrap();
        let fresh = other.0.join("data");
        let args = self::args(&[
            "record",
            "configure-sync",
            "--node",
            node.to_str().unwrap(),
            "--script",
            script.to_str().unwrap(),
            "--state-dir",
            state.to_str().unwrap(),
            "--data-dir",
            fresh.to_str().unwrap(),
            "--result",
            kept.to_str().unwrap(),
        ]);
        assert_eq!(crate::record_cli::run(&args), 1);
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "기록");
        assert!(
            !fresh.join("project-record-sync.json").exists(),
            "not registered"
        );
    }
}
