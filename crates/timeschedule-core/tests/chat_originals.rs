use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use timeschedule_core::chat_originals::{
    archive, group_name, OriginalsProject, OriginalsReport, ToolHomes,
};
use uuid::Uuid;

struct World {
    base: PathBuf,
    codex: PathBuf,
    claude: PathBuf,
    vault: PathBuf,
    repo: PathBuf,
}
impl World {
    fn new() -> Self {
        let base = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("ts-originals-{}", Uuid::new_v4()));
        let world = Self {
            codex: base.join("codex"),
            claude: base.join("claude"),
            vault: base.join("vault"),
            repo: base.join("timeschedule"),
            base,
        };
        for dir in [&world.codex, &world.claude, &world.vault, &world.repo] {
            fs::create_dir_all(dir).unwrap();
        }
        world
    }
    fn project(&self, repo: &Path, claude: bool) -> OriginalsProject {
        OriginalsProject {
            id: "p1".into(),
            title: "시간표".into(),
            root: key(repo),
            capture: true,
            claude,
        }
    }
    fn run(&self, projects: &[OriginalsProject]) -> OriginalsReport {
        let homes = ToolHomes {
            codex: &self.codex,
            claude: Some(&self.claude),
        };
        archive(projects, &homes, &self.vault).unwrap()
    }
    fn group(&self, root: &Path) -> PathBuf {
        self.vault.join("originals").join(group_name(&key(root)))
    }
    fn rollout(&self, cwd: &Path, name: &str) -> PathBuf {
        let path = self.codex.join("sessions/2026/11/04").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, codex_text(cwd, name)).unwrap();
        path
    }
    fn claude_session(&self, cwd: &Path) -> (String, PathBuf) {
        let id = Uuid::new_v4().to_string();
        let path = self
            .claude
            .join("projects/dir-a")
            .join(format!("{id}.jsonl"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, claude_text(&id, cwd)).unwrap();
        (id, path)
    }
}
impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_owned();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}
fn codex_text(cwd: &Path, id: &str) -> String {
    let meta = json!({"type":"session_meta","payload":{"id":id,"cwd":cwd,
        "timestamp":"2026-11-04T01:02:03Z","originator":"Codex Desktop"}});
    format!(
        "{meta}\n{}\n",
        json!({"type":"event_msg","text":"파란 우산"})
    )
}
fn claude_text(id: &str, cwd: &Path) -> String {
    let first = json!({"type":"file-history-snapshot"});
    let row = json!({"type":"user","sessionId":id,"cwd":cwd,"entrypoint":"claude-desktop",
        "timestamp":"2026-11-17T01:02:03Z","message":{"role":"user","content":"안녕"}});
    format!("{first}\n{row}\n")
}
fn unzstd(path: &Path) -> Vec<u8> {
    zstd::decode_all(fs::File::open(path).unwrap()).unwrap()
}
fn index(group: &Path) -> Vec<Value> {
    fs::read_to_string(group.join("index.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
fn set_modified(path: &Path, seconds: u64) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
        .unwrap();
}

#[test]
fn sessions_of_both_tools_are_kept_and_decompress_to_the_source_bytes() {
    let w = World::new();
    let codex = w.rollout(&w.repo, "rollout-2026-11-04-a.jsonl");
    let (id, claude) = w.claude_session(&w.repo);
    let report = w.run(&[w.project(&w.repo, true)]);
    assert_eq!(
        (report.archived, report.unchanged, report.failed),
        (2, 0, 0)
    );
    let group = w.group(&w.repo);
    let kept = group.join("codex/2026/11/04/rollout-2026-11-04-a.jsonl.zst");
    assert_eq!(unzstd(&kept), fs::read(&codex).unwrap());
    assert_eq!(
        unzstd(&group.join(format!("claude/dir-a/{id}.jsonl.zst"))),
        fs::read(&claude).unwrap()
    );
    let rows = index(&group);
    assert_eq!(rows.len(), 2);
    let row = rows.iter().find(|r| r["tool"] == "claude").unwrap();
    assert_eq!(row["client"], "claude-desktop");
    assert_eq!(row["started"], "2026-11-17T01:02:03Z");
    assert_eq!(row["sourceSize"], fs::metadata(&claude).unwrap().len());
    let project: Value =
        serde_json::from_slice(&fs::read(group.join("project.json")).unwrap()).unwrap();
    assert_eq!(project["repoKey"], key(&w.repo));
    assert_eq!(project["projectId"], "p1");
}

#[test]
fn a_project_another_process_is_keeping_is_reported_as_not_kept_whole() {
    let w = World::new();
    w.rollout(&w.repo, "rollout-2026-11-04-a.jsonl");
    let group = w.group(&w.repo);
    fs::create_dir_all(&group).unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(group.join(".lock"))
        .unwrap();
    held.lock().unwrap();
    let report = w.run(&[w.project(&w.repo, false)]);
    assert_eq!((report.archived, report.failed, report.locked), (0, 0, 1));
    assert_eq!(report.incomplete, ["p1"]);
    drop(held);
    let report = w.run(&[w.project(&w.repo, false)]);
    assert_eq!((report.archived, report.locked), (1, 0));
    assert!(report.incomplete.is_empty());
}

#[test]
fn subfolders_count_but_other_folders_and_prefix_siblings_do_not() {
    let w = World::new();
    let inside = w.repo.join(".cache").join("gone-worktree");
    let existing = w.repo.join("existing");
    fs::create_dir_all(&existing).unwrap();
    let sibling = w.base.join("timeschedule2");
    let elsewhere = w.base.join("elsewhere");
    for (name, cwd) in [
        ("rollout-1.jsonl", &inside),
        ("rollout-2.jsonl", &existing),
        ("rollout-3.jsonl", &sibling),
        ("rollout-4.jsonl", &elsewhere),
    ] {
        w.rollout(cwd, name);
    }
    let report = w.run(&[w.project(&w.repo, false)]);
    assert_eq!((report.archived, report.failed), (2, 0));
    let kept: Vec<String> = index(&w.group(&w.repo))
        .iter()
        .map(|r| r["file"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        kept,
        [
            "codex/2026/11/04/rollout-1.jsonl.zst",
            "codex/2026/11/04/rollout-2.jsonl.zst"
        ]
    );
}

#[test]
fn without_claude_only_codex_files_are_kept() {
    let w = World::new();
    w.rollout(&w.repo, "rollout-1.jsonl");
    w.claude_session(&w.repo);
    let report = w.run(&[w.project(&w.repo, false)]);
    assert_eq!(report.archived, 1);
    assert!(!w.group(&w.repo).join("claude").exists());
}

#[test]
fn claude_side_files_are_kept_under_the_session_folder() {
    let w = World::new();
    let (id, _) = w.claude_session(&w.repo);
    let side = w.claude.join("projects/dir-a").join(&id);
    fs::create_dir_all(side.join("subagents")).unwrap();
    fs::create_dir_all(side.join("tool-results")).unwrap();
    fs::write(side.join("subagents/agent-x.jsonl"), "{\"a\":1}\n").unwrap();
    fs::write(side.join("tool-results/out.txt"), "결과").unwrap();
    let report = w.run(&[w.project(&w.repo, true)]);
    assert_eq!(report.archived, 3);
    let group = w.group(&w.repo);
    assert_eq!(
        unzstd(&group.join(format!("claude/dir-a/{id}/subagents/agent-x.jsonl.zst"))),
        b"{\"a\":1}\n"
    );
    assert_eq!(
        unzstd(&group.join(format!("claude/dir-a/{id}/tool-results/out.txt.zst"))),
        "결과".as_bytes()
    );
}

#[test]
fn a_claude_file_is_left_for_later_when_new_and_reported_when_it_never_names_its_folder() {
    let w = World::new();
    let dir = w.claude.join("projects/dir-a");
    fs::create_dir_all(&dir).unwrap();
    let row = json!({"type":"file-history-snapshot"}).to_string();
    fs::write(
        dir.join(format!("{}.jsonl", Uuid::new_v4())),
        format!("{row}\n"),
    )
    .unwrap();
    let report = w.run(&[w.project(&w.repo, true)]);
    assert_eq!((report.archived, report.failed), (0, 0));
    fs::write(
        dir.join(format!("{}.jsonl", Uuid::new_v4())),
        format!("{row}\n").repeat(300),
    )
    .unwrap();
    let report = w.run(&[w.project(&w.repo, true)]);
    assert_eq!((report.archived, report.failed), (0, 1));
}

#[test]
fn a_compressed_rollout_is_matched_by_its_first_line_and_copied_byte_for_byte() {
    let w = World::new();
    let plain = codex_text(&w.repo, "x");
    let packed = zstd::encode_all(plain.as_bytes(), 3).unwrap();
    let path = w.codex.join("archived_sessions/rollout-old.jsonl.zst");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, &packed).unwrap();
    let report = w.run(&[w.project(&w.repo, false)]);
    assert_eq!(report.archived, 1);
    let kept = w
        .group(&w.repo)
        .join("codex/archived_sessions/rollout-old.jsonl.zst");
    assert_eq!(fs::read(kept).unwrap(), packed);
}

#[test]
fn a_second_run_archives_nothing_and_writes_nothing() {
    let w = World::new();
    w.rollout(&w.repo, "rollout-1.jsonl");
    let projects = [w.project(&w.repo, true)];
    w.run(&projects);
    let group = w.group(&w.repo);
    let files = [
        group.join("index.jsonl"),
        group.join("project.json"),
        group.join("codex/2026/11/04/rollout-1.jsonl.zst"),
    ];
    let state = || -> Vec<_> {
        files
            .iter()
            .map(|f| {
                (
                    fs::read(f).unwrap(),
                    fs::metadata(f).unwrap().modified().unwrap(),
                )
            })
            .collect()
    };
    let before = state();
    let report = w.run(&projects);
    assert_eq!(
        (report.archived, report.unchanged, report.failed),
        (0, 1, 0)
    );
    assert_eq!(state(), before);
    assert!(!group.join(".tmp").exists(), "no unfinished copy is left");
}

#[test]
fn a_grown_source_is_kept_again_and_its_entry_updated() {
    let w = World::new();
    let path = w.rollout(&w.repo, "rollout-1.jsonl");
    let projects = [w.project(&w.repo, false)];
    w.run(&projects);
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str("{\"more\":true}\n");
    fs::write(&path, &text).unwrap();
    let report = w.run(&projects);
    assert_eq!(report.archived, 1);
    let group = w.group(&w.repo);
    assert_eq!(
        unzstd(&group.join("codex/2026/11/04/rollout-1.jsonl.zst")),
        text.as_bytes()
    );
    let rows = index(&group);
    assert_eq!(
        (rows.len(), rows[0]["sourceSize"].as_u64()),
        (1, Some(text.len() as u64))
    );
}

fn previous(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().contains(".zst.prev-"))
        .collect();
    found.sort();
    found
}

#[test]
fn a_source_that_does_not_continue_the_kept_copy_leaves_that_copy_beside_the_new_one() {
    let w = World::new();
    let path = w.rollout(&w.repo, "rollout-1.jsonl");
    let old = fs::read(&path).unwrap();
    let projects = [w.project(&w.repo, false)];
    w.run(&projects);
    let kept = w.group(&w.repo).join("codex/2026/11/04");
    // Smaller.
    let shorter = codex_text(&w.repo, "r");
    assert!(shorter.len() < old.len());
    fs::write(&path, &shorter).unwrap();
    assert_eq!(w.run(&projects).archived, 1);
    let first = previous(&kept);
    assert_eq!(first.len(), 1);
    assert_eq!(unzstd(&first[0]), old);
    assert_eq!(
        unzstd(&kept.join("rollout-1.jsonl.zst")),
        shorter.as_bytes()
    );
    // Same size, other content.
    let other = shorter.replace("파란", "노란");
    assert_eq!(other.len(), shorter.len());
    fs::write(&path, &other).unwrap();
    set_modified(&path, 1_800_000_000);
    assert_eq!(w.run(&projects).archived, 1);
    let both = previous(&kept);
    assert_eq!(
        both.len(),
        2,
        "an earlier previous copy is never overwritten"
    );
    let contents: Vec<Vec<u8>> = both.iter().map(|p| unzstd(p)).collect();
    assert!(contents.contains(&old));
    assert!(contents.contains(&shorter.clone().into_bytes()));
    assert_eq!(unzstd(&kept.join("rollout-1.jsonl.zst")), other.as_bytes());
}

#[test]
fn without_its_index_a_kept_copy_is_still_not_overwritten_by_other_content() {
    let w = World::new();
    let path = w.rollout(&w.repo, "rollout-1.jsonl");
    let old = fs::read(&path).unwrap();
    let projects = [w.project(&w.repo, false)];
    w.run(&projects);
    let group = w.group(&w.repo);
    fs::remove_file(group.join("index.jsonl")).unwrap();
    fs::write(&path, codex_text(&w.repo, "r")).unwrap();
    assert_eq!(w.run(&projects).archived, 1);
    let kept = previous(&group.join("codex/2026/11/04"));
    assert_eq!(kept.len(), 1);
    assert_eq!(unzstd(&kept[0]), old);
    assert_eq!(index(&group).len(), 1);
}

#[test]
fn a_half_written_compressed_rollout_never_replaces_the_kept_copy() {
    let w = World::new();
    let plain = w.rollout(&w.repo, "rollout-1.jsonl");
    let old = fs::read(&plain).unwrap();
    let projects = [w.project(&w.repo, false)];
    w.run(&projects);
    let kept = w
        .group(&w.repo)
        .join("codex/2026/11/04/rollout-1.jsonl.zst");
    // Enough text that the header is in the first block and the file can be cut short.
    let mut long = old.clone();
    for n in 0..20_000 {
        long.extend_from_slice(
            format!("{{\"n\":{n},\"text\":\"{}\"}}\n", Uuid::new_v4()).as_bytes(),
        );
    }
    let packed = zstd::encode_all(long.as_slice(), 3).unwrap();
    let half = &packed[..packed.len() / 2];
    let zst = plain.with_extension("jsonl.zst");
    // Both files exist while Codex compresses: the plain one is the one kept, and the
    // other is not even opened.
    fs::write(&zst, b"not a zstd file yet").unwrap();
    let report = w.run(&projects);
    assert_eq!(
        (report.archived, report.unchanged, report.failed),
        (0, 1, 0)
    );
    fs::write(&zst, half).unwrap();
    let report = w.run(&projects);
    assert_eq!(
        (report.archived, report.unchanged, report.failed),
        (0, 1, 0)
    );
    // The plain file is gone and the compressed one is cut short.
    fs::remove_file(&plain).unwrap();
    let report = w.run(&projects);
    assert_eq!((report.archived, report.failed), (0, 1));
    assert_eq!(unzstd(&kept), old);
    // Once whole, it takes the place of the plain copy it continues.
    fs::write(&zst, &packed).unwrap();
    let report = w.run(&projects);
    assert_eq!((report.archived, report.failed), (1, 0));
    assert_eq!(fs::read(&kept).unwrap(), packed);
    assert!(previous(kept.parent().unwrap()).is_empty());
}

#[test]
fn a_kept_copy_that_no_longer_decodes_is_set_aside_as_it_is() {
    let w = World::new();
    let path = w.rollout(&w.repo, "rollout-1.jsonl");
    let projects = [w.project(&w.repo, false)];
    w.run(&projects);
    let kept = w
        .group(&w.repo)
        .join("codex/2026/11/04/rollout-1.jsonl.zst");
    let damaged = b"damaged bytes, not zstd".to_vec();
    fs::write(&kept, &damaged).unwrap();
    fs::remove_file(w.group(&w.repo).join("index.jsonl")).unwrap();
    let report = w.run(&projects);
    assert_eq!((report.archived, report.failed), (1, 0));
    let aside = previous(kept.parent().unwrap());
    assert_eq!(aside.len(), 1);
    assert_eq!(fs::read(&aside[0]).unwrap(), damaged);
    assert_eq!(unzstd(&kept), fs::read(&path).unwrap());
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(made.status.success());
}

#[cfg(windows)]
#[test]
fn a_claude_project_folder_that_is_a_link_is_read_and_other_links_are_reported() {
    let w = World::new();
    // The project folder was moved elsewhere and a link left in its place.
    let (id, file) = w.claude_session(&w.repo);
    let moved = w.base.join("moved");
    fs::rename(w.claude.join("projects/dir-a"), &moved).unwrap();
    junction(&w.claude.join("projects/dir-a"), &moved);
    assert!(file.is_file());
    let projects = [w.project(&w.repo, true)];
    let report = w.run(&projects);
    assert_eq!((report.archived, report.failed), (1, 0));
    assert!(w
        .group(&w.repo)
        .join(format!("claude/dir-a/{id}.jsonl.zst"))
        .is_file());
    // A link deeper down is not followed, and says so.
    let elsewhere = w.base.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::create_dir_all(w.codex.join("sessions/2026/11")).unwrap();
    junction(&w.codex.join("sessions/2026/11/05"), &elsewhere);
    let report = w.run(&projects);
    assert_eq!(
        (report.archived, report.unchanged, report.failed),
        (0, 1, 1)
    );
    fs::remove_dir(w.codex.join("sessions/2026/11/05")).unwrap();
    fs::remove_dir(w.claude.join("projects/dir-a")).unwrap();
}

#[test]
fn files_whose_names_differ_only_by_zst_or_by_folder_are_all_kept() {
    let w = World::new();
    let (id, first) = w.claude_session(&w.repo);
    let side = w
        .claude
        .join("projects/dir-a")
        .join(&id)
        .join("tool-results");
    fs::create_dir_all(&side).unwrap();
    fs::write(side.join("out"), "하나").unwrap();
    fs::write(side.join("out.zst"), "둘").unwrap();
    // The same session ID under another Claude project folder.
    let second = w.claude.join("projects/dir-b").join(format!("{id}.jsonl"));
    fs::create_dir_all(second.parent().unwrap()).unwrap();
    let text = format!("{}{{\"more\":1}}\n", fs::read_to_string(&first).unwrap());
    fs::write(&second, &text).unwrap();
    let report = w.run(&[w.project(&w.repo, true)]);
    assert_eq!((report.archived, report.failed), (4, 0));
    let group = w.group(&w.repo);
    let tool = group.join(format!("claude/dir-a/{id}/tool-results"));
    assert_eq!(unzstd(&tool.join("out.zst")), "하나".as_bytes());
    assert_eq!(unzstd(&tool.join("out.zst.zst")), "둘".as_bytes());
    assert_eq!(
        unzstd(&group.join(format!("claude/dir-b/{id}.jsonl.zst"))),
        text.as_bytes()
    );
}

#[test]
fn a_project_another_process_is_keeping_is_left_to_it() {
    let w = World::new();
    w.rollout(&w.repo, "rollout-1.jsonl");
    let group = w.group(&w.repo);
    fs::create_dir_all(&group).unwrap();
    let lock = fs::File::create(group.join(".lock")).unwrap();
    lock.try_lock().unwrap();
    let projects = [w.project(&w.repo, false)];
    let report = w.run(&projects);
    assert_eq!(
        (report.archived, report.unchanged, report.failed),
        (0, 0, 0)
    );
    assert!(!group.join("index.jsonl").exists());
    drop(lock);
    assert_eq!(w.run(&projects).archived, 1);
}

#[cfg(windows)]
#[test]
fn nothing_is_written_through_a_link_inside_the_project_folder() {
    let w = World::new();
    w.rollout(&w.repo, "rollout-1.jsonl");
    let group = w.group(&w.repo);
    let outside = w.base.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(&group).unwrap();
    junction(&group.join("codex"), &outside);
    let report = w.run(&[w.project(&w.repo, false)]);
    assert_eq!((report.archived, report.failed), (0, 1));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    fs::remove_dir(group.join("codex")).unwrap();
}

#[test]
fn a_plain_rollout_replaced_by_its_compressed_file_keeps_one_entry() {
    let w = World::new();
    let plain = w.rollout(&w.repo, "rollout-1.jsonl");
    let projects = [w.project(&w.repo, false)];
    w.run(&projects);
    let packed = zstd::encode_all(fs::read(&plain).unwrap().as_slice(), 3).unwrap();
    fs::remove_file(&plain).unwrap();
    fs::write(plain.with_extension("jsonl.zst"), &packed).unwrap();
    let report = w.run(&projects);
    assert_eq!((report.archived, report.failed), (1, 0));
    let group = w.group(&w.repo);
    assert_eq!(
        fs::read(group.join("codex/2026/11/04/rollout-1.jsonl.zst")).unwrap(),
        packed
    );
    let rows = index(&group);
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["source"].as_str().unwrap().ends_with(".jsonl.zst"));
}

#[test]
fn a_deleted_source_keeps_its_copy_and_entry() {
    let w = World::new();
    let path = w.rollout(&w.repo, "rollout-1.jsonl");
    let projects = [w.project(&w.repo, false)];
    w.run(&projects);
    fs::remove_file(&path).unwrap();
    let report = w.run(&projects);
    assert_eq!((report.archived, report.failed), (0, 0));
    let group = w.group(&w.repo);
    assert!(group.join("codex/2026/11/04/rollout-1.jsonl.zst").is_file());
    assert_eq!(index(&group).len(), 1);
}

#[test]
fn a_folder_that_belongs_to_another_repository_is_not_touched() {
    let w = World::new();
    w.rollout(&w.repo, "rollout-1.jsonl");
    let group = w.group(&w.repo);
    fs::create_dir_all(&group).unwrap();
    let other =
        json!({"schemaVersion":1,"projectId":"p2","title":"다른","repoKey":"d:/other"}).to_string();
    fs::write(group.join("project.json"), &other).unwrap();
    fs::create_dir_all(group.join(".tmp")).unwrap();
    fs::write(group.join(".tmp/theirs"), "남의 것").unwrap();
    let report = w.run(&[w.project(&w.repo, false)]);
    assert_eq!((report.archived, report.failed), (0, 1));
    let names: Vec<_> = fs::read_dir(&group)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(names
        .iter()
        .all(|n| n == "project.json" || n == ".lock" || n == ".tmp"));
    assert_eq!(
        fs::read_to_string(group.join(".tmp/theirs")).unwrap(),
        "남의 것"
    );
    assert_eq!(
        fs::read_to_string(group.join("project.json")).unwrap(),
        other
    );
}

#[test]
fn a_registered_folder_that_is_not_captured_keeps_its_sessions_from_the_project_above() {
    let w = World::new();
    let private = w.repo.join("private");
    fs::create_dir_all(&private).unwrap();
    w.rollout(&private, "rollout-1.jsonl");
    w.rollout(&w.repo, "rollout-2.jsonl");
    let mut inner = w.project(&private, true);
    inner.id = "p-private".into();
    inner.capture = false;
    let report = w.run(&[w.project(&w.repo, true), inner]);
    assert_eq!((report.archived, report.failed), (1, 0));
    let rows = index(&w.group(&w.repo));
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["file"].as_str().unwrap().contains("rollout-2"));
    assert!(!w.group(&private).exists());
}

#[test]
fn the_project_with_the_longer_matching_folder_wins() {
    let w = World::new();
    let child = w.repo.join("apps").join("tool");
    fs::create_dir_all(&child).unwrap();
    w.rollout(&child, "rollout-1.jsonl");
    w.rollout(&w.repo, "rollout-2.jsonl");
    let mut inner = w.project(&child, false);
    inner.id = "p-child".into();
    let report = w.run(&[w.project(&w.repo, false), inner]);
    assert_eq!(report.archived, 2);
    let parent = index(&w.group(&w.repo));
    let nested = index(&w.group(&child));
    assert_eq!(parent.len(), 1);
    assert!(parent[0]["file"].as_str().unwrap().contains("rollout-2"));
    assert_eq!(nested.len(), 1);
    assert!(nested[0]["file"].as_str().unwrap().contains("rollout-1"));
}
