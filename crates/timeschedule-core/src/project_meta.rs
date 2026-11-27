//! A project's GitHub link and completion date, kept beside `projects` the way
//! `project_display` keeps its short name, so the writers of `projects` stay as they are.
//!
//! The link is only a label: collection still finds a project by its local folder.
//! Marking a project completed only records the day; it starts nothing.
use crate::storage::{database_error, require_changed, validate_date, validate_id, Storage};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

/// Schema 17. Tests that build an older database by dropping later tables leave this
/// one in place, so it is made only when missing.
pub(crate) fn migrate(c: &Connection) -> Result<(), String> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_meta(
            project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
            github_url TEXT NOT NULL DEFAULT '',
            completed_on TEXT);
        PRAGMA user_version=17;",
    )
    .map_err(database_error)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectMeta {
    /// `https://github.com/<owner>/<repo>`, or empty.
    pub github_url: String,
    /// The day the project was marked completed. `None` while it is in progress.
    pub completed_on: Option<String>,
}

/// The link as stored: blank for none, otherwise exactly
/// `https://github.com/<owner>/<repo>`. Surrounding spaces and one trailing `/` are
/// dropped; anything else is refused rather than guessed at. An owner may hold `_`,
/// which managed enterprise accounts carry before their enterprise code.
pub fn github_link(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let refused = || {
        "GitHub 저장소 링크는 https://github.com/<소유자>/<저장소> 모양으로 넣어 주세요."
            .to_string()
    };
    let rest = value
        .strip_prefix("https://github.com/")
        .ok_or_else(refused)?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let (owner, repo) = rest.split_once('/').ok_or_else(refused)?;
    let owner_ok = (1..=39).contains(&owner.len())
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        && !owner.starts_with('-')
        && !owner.ends_with('-');
    let repo_ok = (1..=100).contains(&repo.len())
        && repo
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && repo != "."
        && repo != "..";
    if !owner_ok || !repo_ok {
        return Err(refused());
    }
    Ok(format!("https://github.com/{owner}/{repo}"))
}

impl Storage {
    pub fn project_meta(&self, id: &str) -> Result<ProjectMeta, String> {
        validate_id(id)?;
        self.connection()?
            .query_row(
                "SELECT github_url,completed_on FROM project_meta WHERE project_id=?1",
                [id],
                |r| {
                    Ok(ProjectMeta {
                        github_url: r.get(0)?,
                        completed_on: r.get(1)?,
                    })
                },
            )
            .optional()
            .map(Option::unwrap_or_default)
            .map_err(database_error)
    }

    /// Every project that has a link or a completion date, by project ID.
    pub fn all_project_meta(&self) -> Result<HashMap<String, ProjectMeta>, String> {
        let c = self.connection()?;
        let mut q = c
            .prepare("SELECT project_id,github_url,completed_on FROM project_meta")
            .map_err(database_error)?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    ProjectMeta {
                        github_url: r.get(1)?,
                        completed_on: r.get(2)?,
                    },
                ))
            })
            .map_err(database_error)?
            .collect::<Result<_, _>>()
            .map_err(database_error)?;
        Ok(rows)
    }

    /// Saves a project as `save_calendar_project` does, together with its link, in one
    /// transaction: a refused link or a failed write keeps nothing, so saving again
    /// never makes a second project.
    pub fn save_project_with_link(
        &self,
        input: crate::diary::ProjectInput,
        short_name: &str,
        link: &str,
    ) -> Result<crate::diary::Project, String> {
        let link = github_link(link)?;
        self.save_calendar_project_with(input, short_name, |tx, id| {
            tx.execute(
                "INSERT INTO project_meta(project_id,github_url) VALUES(?1,?2)
                 ON CONFLICT(project_id) DO UPDATE SET github_url=excluded.github_url",
                params![id, link],
            )
            .map(|_| ())
            .map_err(database_error)
        })
    }

    /// Sets the link of an existing project; blank removes it.
    pub fn save_project_link(&self, id: &str, link: &str) -> Result<(), String> {
        validate_id(id)?;
        let link = github_link(link)?;
        self.upsert(
            id,
            "INSERT INTO project_meta(project_id,github_url) SELECT id,?2 FROM projects WHERE id=?1
             ON CONFLICT(project_id) DO UPDATE SET github_url=excluded.github_url",
            &link,
        )
    }

    /// Marks an existing project completed on `date`, or in progress again with `None`.
    pub fn set_project_completed(&self, id: &str, date: Option<&str>) -> Result<(), String> {
        validate_id(id)?;
        if let Some(date) = date {
            validate_date(date)?;
        }
        self.upsert(
            id,
            "INSERT INTO project_meta(project_id,completed_on) SELECT id,?2 FROM projects WHERE id=?1
             ON CONFLICT(project_id) DO UPDATE SET completed_on=excluded.completed_on",
            &date,
        )
    }

    fn upsert(&self, id: &str, sql: &str, value: &dyn rusqlite::ToSql) -> Result<(), String> {
        let changed = self
            .connection()?
            .execute(sql, params![id, value])
            .map_err(database_error)?;
        require_changed(changed, "프로젝트")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diary::ProjectInput;

    fn project(store: &Storage) -> String {
        store
            .save_calendar_project(
                ProjectInput {
                    id: None,
                    title: "앱".into(),
                    description: String::new(),
                    repo_path: String::new(),
                },
                "",
            )
            .unwrap()
            .id
    }

    #[test]
    fn link_takes_only_a_repository_address() {
        for (input, stored) in [
            ("", ""),
            ("  ", ""),
            (
                " https://github.com/NotoriEarth/TimeSchedule/ ",
                "https://github.com/NotoriEarth/TimeSchedule",
            ),
            (
                "https://github.com/a-b/c.d_e",
                "https://github.com/a-b/c.d_e",
            ),
            (
                "https://github.com/mona-cat_octo/demo",
                "https://github.com/mona-cat_octo/demo",
            ),
        ] {
            assert_eq!(github_link(input).unwrap(), stored, "{input}");
        }
        for refused in [
            "http://github.com/a/b",
            "https://gitlab.com/a/b",
            "https://github.com/a",
            "https://github.com/a/b/tree/main",
            "https://github.com/a/b?tab=readme",
            "https://github.com/-a/b",
            "https://github.com/a/..",
            "https://github.com/a b/c",
            "C:\\work\\repo",
        ] {
            assert!(github_link(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn link_and_completion_are_kept_apart_and_go_with_the_project() {
        let store = Storage::open(":memory:").unwrap();
        let id = project(&store);
        assert_eq!(store.project_meta(&id).unwrap(), ProjectMeta::default());

        store
            .save_project_link(&id, "https://github.com/o/r/")
            .unwrap();
        store
            .set_project_completed(&id, Some("2026-12-01"))
            .unwrap();
        store
            .save_project_link(&id, "https://github.com/o/s")
            .unwrap();
        let meta = ProjectMeta {
            github_url: "https://github.com/o/s".into(),
            completed_on: Some("2026-12-01".into()),
        };
        assert_eq!(store.project_meta(&id).unwrap(), meta);
        assert_eq!(store.all_project_meta().unwrap()[&id], meta);

        store.set_project_completed(&id, None).unwrap();
        assert_eq!(store.project_meta(&id).unwrap().completed_on, None);
        assert!(store
            .save_project_link(&id, "https://example.com/o/r")
            .is_err());
        assert_eq!(
            store.project_meta(&id).unwrap().github_url,
            "https://github.com/o/s"
        );

        store.delete_project(&id).unwrap();
        assert!(store.all_project_meta().unwrap().is_empty());
    }

    #[test]
    fn a_failed_link_write_keeps_nothing_and_saving_again_makes_one_project() {
        let store = Storage::open(":memory:").unwrap();
        let input = || ProjectInput {
            id: None,
            title: "앱".into(),
            description: String::new(),
            repo_path: String::new(),
        };
        store
            .connection()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER refuse BEFORE INSERT ON project_meta BEGIN SELECT RAISE(ABORT,'disk'); END;",
            )
            .unwrap();
        assert!(store
            .save_project_with_link(input(), "짧", "https://github.com/o/r")
            .is_err());
        assert!(store.list_projects().unwrap().is_empty());
        store
            .connection()
            .unwrap()
            .execute_batch("DROP TRIGGER refuse;")
            .unwrap();
        let saved = store
            .save_project_with_link(input(), "짧", "https://github.com/o/r")
            .unwrap();
        assert_eq!(store.list_projects().unwrap().len(), 1);
        assert_eq!(store.project_short_name(&saved.id).unwrap(), "짧");
        assert_eq!(
            store.project_meta(&saved.id).unwrap().github_url,
            "https://github.com/o/r"
        );
        // An edit with a refused link changes neither the project nor its link.
        let mut edit = input();
        edit.id = Some(saved.id.clone());
        edit.title = "새 이름".into();
        assert!(store
            .save_project_with_link(edit, "짧", "https://example.com/o/r")
            .is_err());
        assert_eq!(store.list_projects().unwrap()[0].title, "앱");
    }

    #[test]
    fn an_unknown_project_gets_nothing() {
        let store = Storage::open(":memory:").unwrap();
        let missing = uuid::Uuid::new_v4().to_string();
        assert!(store.save_project_link(&missing, "").is_err());
        assert!(store
            .set_project_completed(&missing, Some("2026-12-01"))
            .is_err());
        assert!(store.all_project_meta().unwrap().is_empty());
        let id = project(&store);
        assert!(store
            .set_project_completed(&id, Some("2026-13-01"))
            .is_err());
    }

    #[test]
    fn an_older_database_gains_the_table_and_keeps_its_projects() {
        let old = std::env::temp_dir().join(format!("meta16-{}.db", uuid::Uuid::new_v4()));
        let id = uuid::Uuid::new_v4().to_string();
        {
            let mut c = Connection::open(&old).unwrap();
            crate::storage::migrate_to(&mut c, 16).unwrap();
            c.execute(
                "INSERT INTO projects VALUES(?1,'옛','','','2026-11-01T00:00:00Z')",
                [&id],
            )
            .unwrap();
        }
        let store = Storage::open(&old).unwrap();
        assert_eq!(store.list_projects().unwrap()[0].title, "옛");
        store
            .set_project_completed(&id, Some("2026-12-01"))
            .unwrap();
        let version: i64 = store
            .connection()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 18);
        let stem = old.file_stem().unwrap().to_string_lossy().into_owned();
        let backups: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                let name = p.file_name().unwrap().to_string_lossy();
                name.starts_with(&stem) && name.contains("pre-v18")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        drop(store);
        for path in backups.iter().chain([&old]) {
            let _ = std::fs::remove_file(path);
        }
    }
}
