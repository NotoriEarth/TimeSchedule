//! Opens an automatic development record's original `record.md`.
//!
//! The work ID comes only from evidence the app stored when it accepted the
//! record. The file is then found through the record state folder the record
//! script registered, that folder's document vault setting and the vault's own
//! catalog — never from a path written in the diary text — and it must be the
//! record file of exactly that ID inside that vault, with no links on the way.
use serde::Deserialize;
use std::path::{Path, PathBuf};

const LIMIT: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultSettings {
    schema_version: u32,
    root: PathBuf,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultMarker {
    schema_version: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogEntry {
    schema_version: u32,
    id: String,
    relative: String,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, what: &str) -> Result<T, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| format!("{what}을 열지 못했습니다: {e}"))?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{what}을 읽지 못했습니다: {e}"))?;
    if bytes.len() as u64 > LIMIT {
        return Err(format!("{what} 크기가 너무 큽니다."));
    }
    let text = String::from_utf8_lossy(&bytes);
    serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("{what} 형식을 확인해 주세요: {e}"))
}

fn canonical_work_id(work_id: &str) -> Result<(), String> {
    match uuid::Uuid::parse_str(work_id) {
        Ok(id) if id.to_string() == work_id => Ok(()),
        _ => Err("원본을 찾을 작업 ID가 올바르지 않습니다.".into()),
    }
}

fn safe_group(group: &str) -> bool {
    let bytes = group.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 80
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// Every step from the vault root to the file must be a real directory or file.
fn no_links(root: &Path, parts: &[&str]) -> Result<PathBuf, String> {
    let mut current = root.to_path_buf();
    for part in parts {
        current.push(part);
        let meta = std::fs::symlink_metadata(&current).map_err(|_| {
            "원본 파일이 없거나 이동되었습니다. 문서 저장소를 확인해 주세요.".to_string()
        })?;
        if meta.file_type().is_symlink() {
            return Err("문서 저장소 안의 링크 경로는 열지 않습니다.".into());
        }
    }
    Ok(current)
}

fn vault_root(state_dir: &Path, home: Option<&Path>) -> Result<PathBuf, String> {
    let settings = state_dir.join("vault-settings.json");
    // Only a file that is really absent means "use the default store".
    if settings
        .try_exists()
        .map_err(|e| format!("문서 저장소 설정을 확인하지 못했습니다: {e}"))?
    {
        let value: VaultSettings = read_json(&settings, "문서 저장소 설정")?;
        if value.schema_version != 1 || !value.root.is_absolute() {
            return Err("문서 저장소 설정을 확인해 주세요.".into());
        }
        return Ok(value.root);
    }
    default_vault(home)
}

/// The record script's default when nothing was configured.
fn default_vault(home: Option<&Path>) -> Result<PathBuf, String> {
    home.map(|h| h.join("Documents").join("TimeScheduleData"))
        .ok_or_else(|| "문서 저장소 위치를 확인하지 못했습니다.".into())
}

/// The original file of one automatic record, or why it cannot be opened.
pub fn locate(state_dir: &Path, work_id: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    canonical_work_id(work_id)?;
    let root = std::fs::canonicalize(vault_root(state_dir, home)?)
        .map_err(|_| "문서 저장소 폴더를 찾지 못했습니다.".to_string())?;
    let marker: VaultMarker = read_json(&no_links(&root, &["vault.json"])?, "문서 저장소 표식")?;
    if marker.schema_version != 1 {
        return Err("지원하지 않는 문서 저장소 형식입니다.".into());
    }
    let catalog = no_links(&root, &[".sync", "catalog", &format!("{work_id}.json")])
        .map_err(|_| "이 작업의 원본이 문서 저장소에 아직 없습니다.".to_string())?;
    let entry: CatalogEntry = read_json(&catalog, "문서 위치 목록")?;
    let parts: Vec<&str> = entry.relative.split(['/', '\\']).collect();
    let valid = entry.schema_version == 1
        && entry.id == work_id
        && parts.len() == 3
        && parts[0] == "projects"
        && safe_group(parts[1])
        && parts[2] == work_id;
    if !valid {
        return Err("문서 위치 목록이 이 작업과 맞지 않습니다. 원본을 열지 않았습니다.".into());
    }
    let file = no_links(&root, &[parts[0], parts[1], parts[2], "record.md"])?;
    let canonical =
        std::fs::canonicalize(&file).map_err(|_| "원본 파일을 찾지 못했습니다.".to_string())?;
    if !canonical.starts_with(&root) || !canonical.is_file() {
        return Err("문서 저장소 밖의 파일은 열지 않습니다.".into());
    }
    Ok(canonical)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// The document store root where chat originals are kept: the registered record state
/// folder's store, or the default one when no record delivery is registered.
pub fn originals_vault(data_dir: &Path) -> Result<PathBuf, String> {
    let missing = || "문서 저장소를 찾지 못해 채팅 원문을 보관하지 못했습니다.".to_string();
    let registered = data_dir
        .join("project-record-sync.json")
        .try_exists()
        .map_err(|e| format!("자동 기록 전달 설정을 확인하지 못했습니다: {e}"))?;
    let root = if registered {
        vault_root(
            &crate::record_sync::registered_state_dir(data_dir)?,
            home_dir().as_deref(),
        )
    } else {
        default_vault(home_dir().as_deref())
    }?;
    let root = std::fs::canonicalize(root).map_err(|_| missing())?;
    let marker = no_links(&root, &["vault.json"]).map_err(|_| missing())?;
    let marker: VaultMarker = read_json(&marker, "문서 저장소 표식")?;
    if marker.schema_version != 1 {
        return Err("지원하지 않는 문서 저장소 형식입니다.".into());
    }
    Ok(root)
}

pub fn open(data_dir: &Path, work_id: &str) -> Result<(), String> {
    let state_dir = crate::record_sync::registered_state_dir(data_dir)?;
    let file = locate(&state_dir, work_id, home_dir().as_deref())?;
    timeschedule_desktop::open_local_document(&file)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: &str = "d53e0d97-9ff4-4e78-baf0-9f2b9f7a2e4c";

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn vault() -> (Dir, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("ts-original-{}", uuid::Uuid::new_v4()));
        let state = base.join("state");
        let root = base.join("vault");
        let record = root.join("projects").join("repo-0123abcd").join(WORK);
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&record).unwrap();
        std::fs::create_dir_all(root.join(".sync").join("catalog")).unwrap();
        std::fs::write(
            root.join("vault.json"),
            r#"{"schemaVersion":1,"vaultId":"x"}"#,
        )
        .unwrap();
        std::fs::write(record.join("record.md"), "# 원문\n").unwrap();
        std::fs::write(
            root.join(".sync").join("catalog").join(format!("{WORK}.json")),
            format!(r#"{{"schemaVersion":1,"id":"{WORK}","relative":"projects\\repo-0123abcd\\{WORK}","established":true}}"#),
        )
        .unwrap();
        std::fs::write(
            state.join("vault-settings.json"),
            serde_json::json!({"schemaVersion":1,"root":root,"spaceEnabled":false}).to_string(),
        )
        .unwrap();
        (Dir(base), state, root)
    }

    #[test]
    fn opens_only_the_catalogued_record_of_that_work_inside_the_vault() {
        let (_dir, state, root) = vault();
        let file = locate(&state, WORK, None).unwrap();
        assert!(file.ends_with(Path::new(WORK).join("record.md")));
        assert!(file.starts_with(std::fs::canonicalize(&root).unwrap()));
        // Not a canonical ID, or another work's ID: nothing is opened.
        assert!(locate(&state, &WORK.to_uppercase(), None).is_err());
        assert!(locate(&state, "../../etc/passwd", None).is_err());
        assert!(locate(&state, "00000000-0000-4000-8000-000000000000", None).is_err());
    }

    #[test]
    fn a_catalog_pointing_elsewhere_is_refused() {
        let (_dir, state, root) = vault();
        let catalog = root
            .join(".sync")
            .join("catalog")
            .join(format!("{WORK}.json"));
        for relative in [
            format!("projects/../../outside/{WORK}"),
            format!("studies/repo-0123abcd/{WORK}"),
            "projects/repo-0123abcd/00000000-0000-4000-8000-000000000000".to_string(),
            format!("projects/Repo/{WORK}"),
        ] {
            std::fs::write(
                &catalog,
                serde_json::json!({"schemaVersion":1,"id":WORK,"relative":relative}).to_string(),
            )
            .unwrap();
            assert!(locate(&state, WORK, None).is_err(), "{relative}");
        }
    }

    #[test]
    fn default_vault_follows_the_script_and_missing_files_are_explained() {
        let (_dir, state, root) = vault();
        std::fs::remove_file(state.join("vault-settings.json")).unwrap();
        // Without settings the script's default under the home folder is used.
        let home = root.parent().unwrap().join("home");
        std::fs::create_dir_all(home.join("Documents")).unwrap();
        assert!(locate(&state, WORK, Some(&home)).is_err());
        std::fs::rename(&root, home.join("Documents").join("TimeScheduleData")).unwrap();
        assert!(locate(&state, WORK, Some(&home)).is_ok());
        let record = home
            .join("Documents")
            .join("TimeScheduleData")
            .join("projects")
            .join("repo-0123abcd")
            .join(WORK)
            .join("record.md");
        std::fs::remove_file(record).unwrap();
        assert!(locate(&state, WORK, Some(&home))
            .unwrap_err()
            .contains("원본"));
    }
}
