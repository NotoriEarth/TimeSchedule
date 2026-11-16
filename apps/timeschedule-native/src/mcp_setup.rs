//! On-demand client registration. Never includes a credential in configuration.
use base64::Engine;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Client {
    #[default]
    Codex,
    ChatGptDesktop,
    ClaudeCode,
    ClaudeDesktop,
}

impl Client {
    pub const ALL: [Self; 4] = [
        Self::Codex,
        Self::ChatGptDesktop,
        Self::ClaudeCode,
        Self::ClaudeDesktop,
    ];
}

impl std::fmt::Display for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Codex => "Codex",
            Self::ChatGptDesktop => "ChatGPT Desktop",
            Self::ClaudeCode => "Claude Code",
            Self::ClaudeDesktop => "Claude Desktop",
        })
    }
}

#[derive(Clone)]
pub struct Plan {
    pub path: PathBuf,
    pub preview: String,
    pub replacing: bool,
    original: Option<Vec<u8>>,
    updated: Vec<u8>,
}
impl Plan {
    pub fn unchanged(&self) -> bool {
        self.original.as_deref() == Some(self.updated.as_slice())
    }
}

pub fn default_path(client: Client) -> Result<PathBuf, String> {
    default_path_from(config_candidates(client)?)
}
fn default_path_from(mut candidates: Vec<PathBuf>) -> Result<PathBuf, String> {
    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        _ => Err("Claude Desktop 설정 위치가 여러 개입니다. Claude의 설정 → Developer → Edit Config에서 경로를 확인해 입력하세요.".into()),
    }
}
/// Every configuration file this client may read on this PC. Only Claude
/// Desktop can have more than one (a regular and a Microsoft Store install).
pub fn config_candidates(client: Client) -> Result<Vec<PathBuf>, String> {
    let profile = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .ok_or("Windows 사용자 폴더를 찾지 못했습니다. 설정 경로를 직접 입력하세요.")?;
    match client {
        Client::Codex | Client::ChatGptDesktop => Ok(vec![std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| profile.join(".codex"))
            .join("config.toml")]),
        Client::ClaudeCode => Ok(vec![std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or(profile)
            .join(".claude.json")]),
        Client::ClaudeDesktop => {
            let appdata = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .ok_or("APPDATA 폴더를 찾지 못했습니다.")?;
            let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
            Ok(claude_desktop_candidates(&appdata, local.as_deref()))
        }
    }
}
/// Claude Desktop's configuration files on this PC.
///
/// Claude Desktop reads `claude_desktop_config.json` from its data folder. A
/// regular install uses `%APPDATA%\Claude`; the Microsoft Store build may use
/// its package folder or `%LOCALAPPDATA%\Claude-Data\<package>`, and links
/// `%APPDATA%\Claude` to the package folder. Paths that resolve to the same
/// folder count once. Electron leaves `lockfile` and `config.json` in a data
/// folder it has run from, and holds `lockfile` exclusively while running:
/// - the one folder in use right now wins;
/// - otherwise a single folder that has ever been run from wins;
/// - several such folders are returned together, so a write target is never
///   guessed (`default_path` then asks for an explicit path).
///
/// An entry in a folder the app no longer reads would show "registered" while
/// Claude Desktop never starts the server.
fn claude_desktop_candidates(appdata: &Path, local: Option<&Path>) -> Vec<PathBuf> {
    const FILE: &str = "claude_desktop_config.json";
    let ran_here =
        |folder: &Path| folder.join("lockfile").is_file() || folder.join("config.json").is_file();
    let roaming = appdata.join("Claude");
    let mut folders = vec![roaming.clone()];
    if let Some(local) = local {
        for (parent, child) in [
            (local.join("Packages"), Some("LocalCache/Roaming/Claude")),
            (local.join("Claude-Data"), None),
        ] {
            let Ok(entries) = std::fs::read_dir(&parent) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if name.starts_with("claude_") {
                    folders.push(match child {
                        Some(child) => entry.path().join(child),
                        None => entry.path(),
                    });
                }
            }
        }
    }
    // One folder reached through a link is one location. `%APPDATA%\Claude`
    // counts only when it holds a config or has been run from.
    let mut seen = std::collections::HashSet::new();
    let folders: Vec<PathBuf> = folders
        .into_iter()
        .filter(|folder| {
            folder.is_dir()
                && (folder != &roaming || folder.join(FILE).is_file() || ran_here(folder))
        })
        .filter(|folder| {
            seen.insert(std::fs::canonicalize(folder).unwrap_or_else(|_| folder.clone()))
        })
        .collect();
    let files = |chosen: Vec<&PathBuf>| chosen.into_iter().map(|f| f.join(FILE)).collect();
    let running: Vec<&PathBuf> = folders
        .iter()
        .filter(|f| in_use(&f.join("lockfile")))
        .collect();
    if running.len() == 1 {
        return files(running);
    }
    let marked: Vec<&PathBuf> = folders.iter().filter(|f| ran_here(f)).collect();
    if !marked.is_empty() {
        return files(marked);
    }
    if folders.is_empty() {
        return vec![roaming.join(FILE)];
    }
    files(folders.iter().collect())
}
/// True while another process holds `path` open without sharing it, as a
/// running Electron app does with its `lockfile`.
fn in_use(path: &Path) -> bool {
    const ERROR_SHARING_VIOLATION: i32 = 32;
    path.is_file()
        && std::fs::File::open(path)
            .err()
            .and_then(|error| error.raw_os_error())
            == Some(ERROR_SHARING_VIOLATION)
}
impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan")
            .field("path", &self.path)
            .field("replacing", &self.replacing)
            .finish_non_exhaustive()
    }
}

/// Encoded PowerShell avoids cmd.exe expansion of %, &, quotes and Unicode paths.
/// Only paths/arguments are encoded; this is not credential encryption.
pub fn helper_command(exe: &Path, dir: &Path, endpoint: &str) -> Result<String, String> {
    // The helper releases the token only to this exact endpoint (digits-only port, no quoting).
    crate::mcp_connection::endpoint(endpoint)?;
    let literal = |p: &Path| -> Result<String, String> {
        let s = p.to_str().ok_or("경로를 문자열로 읽을 수 없습니다.")?;
        Ok(format!("'{}'", s.replace('\'', "''")))
    };
    let script = format!("$ErrorActionPreference='Stop'; & {} --mcp-auth-headers --data-dir {} --endpoint '{endpoint}' | Out-String; exit $LASTEXITCODE", literal(exe)?, literal(dir)?);
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    Ok(format!(
        "powershell.exe -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

const MAX_CONFIG: u64 = 2 * 1024 * 1024;
fn read_config(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err("설정 파일을 읽을 수 없습니다. 경로와 접근 권한을 확인하세요.".into())
        }
    };
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "설정 파일 읽기 실패")?;
    if bytes.len() as u64 > MAX_CONFIG {
        return Err("설정 파일이 2 MiB를 넘어 자동 등록을 중지했습니다.".into());
    }
    Ok(Some(bytes))
}

pub fn prepare(
    client: Client,
    path: &Path,
    exe: &Path,
    dir: &Path,
    endpoint: &str,
) -> Result<Plan, String> {
    crate::mcp_connection::endpoint(endpoint)?;
    if !path.is_absolute() || !exe.is_absolute() || !dir.is_absolute() {
        return Err("절대 경로를 입력하세요.".into());
    }
    let original = read_config(path)?;
    let source = std::str::from_utf8(original.as_deref().unwrap_or_default())
        .map_err(|_| "설정 파일은 UTF-8 형식이어야 합니다.")?
        .trim_start_matches('\u{feff}');
    let helper = helper_command(exe, dir, endpoint)?;
    let desired = match client {
        Client::Codex | Client::ChatGptDesktop => {
            json!({"url": endpoint, "http_headers_helper":helper})
        }
        Client::ClaudeCode => json!({"type":"http", "url":endpoint, "headersHelper":helper}),
        Client::ClaudeDesktop => json!({"command":exe, "args":["--mcp-stdio", "--data-dir", dir]}),
    };
    // Replace transport/auth fields of this entry only. Keep tool restrictions,
    // enablement, timeouts and all unrelated configuration.
    const TRANSPORT: &[&str] = &[
        "type",
        "url",
        "command",
        "args",
        "env",
        "env_vars",
        "cwd",
        "bearer_token_env_var",
        "http_headers",
        "env_http_headers",
        "http_headers_helper",
        "headers",
        "headersHelper",
    ];
    let (updated, exists) = if matches!(client, Client::Codex | Client::ChatGptDesktop) {
        let mut doc: toml_edit::DocumentMut = source.parse().map_err(|_| {
            "ChatGPT Desktop·Codex 설정의 TOML 형식이 잘못되었습니다. 원본은 변경하지 않았습니다."
        })?;
        if doc.get("mcp_servers").is_none() {
            doc["mcp_servers"] = toml_edit::table();
        }
        let servers = doc["mcp_servers"]
            .as_table_like_mut()
            .ok_or("mcp_servers가 테이블이 아닙니다.")?;
        let exists = servers.contains_key("timeschedule");
        if !exists {
            servers.insert("timeschedule", toml_edit::table());
        }
        let entry = servers
            .get_mut("timeschedule")
            .and_then(toml_edit::Item::as_table_like_mut)
            .ok_or("timeschedule 설정이 테이블이 아닙니다.")?;
        let same = TRANSPORT.iter().all(|key| match desired.get(*key) {
            Some(value) => entry.get(key).and_then(toml_edit::Item::as_str) == value.as_str(),
            None => !entry.contains_key(key),
        });
        for key in TRANSPORT {
            entry.remove(key);
        }
        entry.insert("url", toml_edit::value(endpoint));
        entry.insert("http_headers_helper", toml_edit::value(helper));
        (
            if same {
                original.clone().unwrap()
            } else {
                doc.to_string().into_bytes()
            },
            exists,
        )
    } else {
        let mut doc: Value = if source.trim().is_empty() && original.is_none() {
            json!({})
        } else {
            serde_json::from_str(source).map_err(|_| {
                "설정 파일의 JSON 형식이 잘못되었습니다. 원본은 변경하지 않았습니다."
            })?
        };
        let root = doc
            .as_object_mut()
            .ok_or("설정 파일의 최상위는 객체여야 합니다.")?;
        let servers = root
            .entry("mcpServers")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("mcpServers가 객체가 아닙니다.")?;
        let exists = servers.contains_key("timeschedule");
        let entry = servers
            .entry("timeschedule")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("timeschedule 설정이 객체가 아닙니다.")?;
        let before = entry.clone();
        for key in TRANSPORT {
            entry.remove(*key);
        }
        entry.extend(desired.as_object().unwrap().clone());
        let same = exists && &before == entry;
        let mut bytes = serde_json::to_vec_pretty(&doc).map_err(|_| "설정 생성 실패")?;
        bytes.push(b'\n');
        (
            if same {
                original.clone().unwrap()
            } else {
                bytes
            },
            exists,
        )
    };
    let replacing = exists && original.as_deref() != Some(updated.as_slice());
    let action = if original.as_deref() == Some(updated.as_slice()) {
        "이미 같은 설정입니다"
    } else if exists {
        "기존 timeschedule 연결·인증 설정을 갱신합니다"
    } else {
        "timeschedule 연결을 추가합니다"
    };
    let transport = if client == Client::ClaudeDesktop {
        "앱 실행 중 로컬 중계 · 토큰 복사 없음"
    } else {
        "로컬 HTTP · 연결할 때 Windows 인증정보 사용"
    };
    let shared = if matches!(client, Client::Codex | Client::ChatGptDesktop) {
        "\n같은 설정 파일을 쓰는 ChatGPT Desktop·Codex에 함께 적용됩니다."
    } else {
        ""
    };
    let preview = format!("{client}: {action}\n대상: {}\n서버: {endpoint}\n{transport}{shared}\n다른 서버·설정과 도구 제한은 보존합니다. 기존 파일은 같은 폴더에 백업합니다.", path.display());
    Ok(Plan {
        path: path.into(),
        preview,
        replacing,
        original,
        updated,
    })
}
pub fn apply(plan: &Plan) -> Result<Option<PathBuf>, String> {
    use std::io::{Seek, SeekFrom};
    #[cfg(not(windows))]
    return Err("연결 설정 등록은 Windows에서 지원합니다.".into());
    let parent = plan.path.parent().ok_or("설정 폴더가 없습니다.")?;
    std::fs::create_dir_all(parent).map_err(|_| "설정 폴더 생성 실패")?;
    if let Some(original) = &plan.original {
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        // Keep this handle through compare, backup and write. Other processes
        // cannot read partial data, write or replace this file while it is held.
        let mut file = options.open(&plan.path).map_err(|_| {
            "설정 파일이 사용 중이거나 열 수 없습니다. AI 앱의 설정 저장이 끝난 뒤 다시 시도하세요."
        })?;
        let mut current = Vec::new();
        Read::by_ref(&mut file)
            .take(MAX_CONFIG + 1)
            .read_to_end(&mut current)
            .map_err(|_| "설정 읽기 실패")?;
        if &current != original {
            return Err("미리보기 이후 설정이 바뀌었습니다. 변경 내용을 다시 확인하세요.".into());
        }
        if plan.unchanged() {
            return Ok(None);
        }
        let backup = parent.join(format!(
            "{}.timeschedule-{}.bak",
            plan.path.file_name().unwrap().to_string_lossy(),
            uuid::Uuid::new_v4()
        ));
        write_backup(&backup, original, &file)?;
        let write = |file: &mut std::fs::File, bytes: &[u8]| -> std::io::Result<()> {
            file.seek(SeekFrom::Start(0))?;
            file.write_all(bytes)?;
            file.set_len(bytes.len() as u64)?;
            file.sync_all()
        };
        if write(&mut file, &plan.updated).is_err() {
            let restored = write(&mut file, original).is_ok();
            return Err(format!(
                "설정 저장 실패 · {}\n백업: {}",
                if restored {
                    "원본 복원 완료"
                } else {
                    "백업에서 복원이 필요합니다"
                },
                backup.display()
            ));
        }
        return Ok(Some(backup));
    }
    let staged = parent.join(format!(".timeschedule-{}.tmp", uuid::Uuid::new_v4()));
    write_new(&staged, &plan.updated)?;
    // Atomically publish a fully flushed new file without replacing a file
    // another client created since preview. Same-directory hard links stay local.
    let result = std::fs::hard_link(&staged, &plan.path).map_err(|_| "설정 파일이 새로 생겼거나 이 폴더에서 등록을 지원하지 않습니다. 경로를 확인하고 다시 미리보기 하세요.");
    let _ = std::fs::remove_file(&staged);
    result?;
    Ok(None)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|_| "설정/백업 파일 생성 실패")?;
    write_created(path, bytes, &mut file)
}
fn write_created(path: &Path, bytes: &[u8], file: &mut std::fs::File) -> Result<(), String> {
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        return Err(format!("설정/백업 저장 실패: {}", path.display()));
    }
    Ok(())
}

#[cfg(windows)]
fn write_backup(path: &Path, bytes: &[u8], source: &std::fs::File) -> Result<(), String> {
    use std::os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    };
    use windows_sys::Win32::{
        Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Security::{
            GetKernelObjectSecurity, SetSecurityDescriptorControl, DACL_SECURITY_INFORMATION,
            SECURITY_ATTRIBUTES, SE_DACL_PROTECTED,
        },
        Storage::FileSystem::{CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL},
    };
    let mut size = 0;
    unsafe {
        GetKernelObjectSecurity(
            source.as_raw_handle(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut size,
        );
    }
    if size == 0 || size > 65536 {
        return Err("원본 설정의 접근 권한을 확인하지 못했습니다.".into());
    }
    let mut security_bytes = vec![0u8; size as usize];
    let descriptor = security_bytes.as_mut_ptr().cast();
    let ok = unsafe {
        GetKernelObjectSecurity(
            source.as_raw_handle(),
            DACL_SECURITY_INFORMATION,
            descriptor,
            size,
            &mut size,
        )
    };
    if ok == 0
        || unsafe { SetSecurityDescriptorControl(descriptor, SE_DACL_PROTECTED, SE_DACL_PROTECTED) }
            == 0
    {
        return Err("원본 설정의 접근 권한을 복사하지 못했습니다.".into());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // The DACL is supplied at creation, before any credential-bearing bytes exist.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            0,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err("원본과 같은 접근 권한으로 백업을 만들지 못했습니다.".into());
    }
    let mut file = unsafe { std::fs::File::from_raw_handle(handle) };
    write_created(path, bytes, &mut file)
}
#[cfg(not(windows))]
fn write_backup(_: &Path, _: &[u8], _: &std::fs::File) -> Result<(), String> {
    Err("Windows 전용 기능입니다.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn helper_command_passes_the_registered_endpoint_to_the_helper() {
        let exe = Path::new("C:\\app\\TimeSchedule.exe");
        let dir = Path::new("C:\\data");
        let command = helper_command(exe, dir, "http://127.0.0.1:40123/mcp").unwrap();
        let encoded = command.rsplit(' ').next().unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let script = String::from_utf16(&units).unwrap();
        assert!(
            script.contains("--endpoint 'http://127.0.0.1:40123/mcp'"),
            "{script}"
        );
        assert!(helper_command(exe, dir, "http://example.com:39281/mcp").is_err());
    }
    #[test]
    fn claude_desktop_uses_the_only_data_folder_it_has_run_from() {
        let f = Fixture::new();
        let appdata = f.0.join("Roaming");
        let local = f.0.join("Local");
        let config = |dir: &Path| dir.join("claude_desktop_config.json");
        // The Microsoft Store build keeps its live data (lockfile, config.json)
        // under Local\Claude-Data, while an older Roaming config stays behind.
        let roaming = appdata.join("Claude");
        let active = local.join("Claude-Data").join("claude_pzs8sxrjxfjjc");
        std::fs::create_dir_all(&roaming).unwrap();
        std::fs::create_dir_all(&active).unwrap();
        std::fs::write(config(&roaming), r#"{"mcpServers":{"timeschedule":{}}}"#).unwrap();
        std::fs::write(config(&active), r#"{"preferences":{}}"#).unwrap();
        std::fs::write(active.join("lockfile"), "").unwrap();
        std::fs::write(active.join("config.json"), "{}").unwrap();
        assert_eq!(
            claude_desktop_candidates(&appdata, Some(&local)),
            vec![config(&active)]
        );
        // Without any sign of a running app, every known location still counts.
        let quiet = f.0.join("quiet");
        let quiet_roaming = quiet.join("Roaming").join("Claude");
        let store = quiet
            .join("Local")
            .join("Packages")
            .join("Claude_abc")
            .join("LocalCache/Roaming/Claude");
        std::fs::create_dir_all(&quiet_roaming).unwrap();
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(config(&quiet_roaming), "{}").unwrap();
        let found = claude_desktop_candidates(&quiet.join("Roaming"), Some(&quiet.join("Local")));
        assert_eq!(found.len(), 2, "{found:?}");
        // Nothing at all: the documented default location.
        let empty = f.0.join("empty");
        assert_eq!(
            claude_desktop_candidates(&empty.join("Roaming"), Some(&empty.join("Local"))),
            vec![config(&empty.join("Roaming").join("Claude"))]
        );
    }
    #[test]
    fn competing_data_folders_are_decided_by_the_running_app_or_left_ambiguous() {
        let f = Fixture::new();
        let appdata = f.0.join("Roaming");
        let local = f.0.join("Local");
        let config = |dir: &Path| dir.join("claude_desktop_config.json");
        let regular = appdata.join("Claude");
        let store = local.join("Claude-Data").join("claude_store");
        for dir in [&regular, &store] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("lockfile"), "").unwrap();
            std::fs::write(dir.join("config.json"), "{}").unwrap();
        }
        // The regular install has run but has no MCP config yet: it still counts.
        assert!(!config(&regular).exists());
        // Two installs that have both run, neither running now: refuse to guess.
        let found = claude_desktop_candidates(&appdata, Some(&local));
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(default_path_from(found).is_err());
        // The running app holds its lockfile exclusively; that folder wins even
        // when the other one ran more recently.
        use std::os::windows::fs::OpenOptionsExt;
        let held = std::fs::OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(regular.join("lockfile"))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(store.join("config.json"), "{\"newer\":true}").unwrap();
        assert_eq!(
            claude_desktop_candidates(&appdata, Some(&local)),
            vec![config(&regular)]
        );
        drop(held);
    }
    #[test]
    fn a_roaming_folder_linked_to_the_store_folder_is_one_location() {
        let f = Fixture::new();
        let appdata = f.0.join("Roaming");
        let local = f.0.join("Local");
        let store = local
            .join("Packages")
            .join("Claude_abc")
            .join("LocalCache/Roaming/Claude");
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(&appdata).unwrap();
        std::fs::write(store.join("claude_desktop_config.json"), "{}").unwrap();
        // The Store installer links %APPDATA%\Claude to its own folder.
        let link = appdata.join("Claude");
        use std::os::windows::process::CommandExt;
        let made = std::process::Command::new("cmd")
            .raw_arg(format!(
                "/d /c mklink /J \"{}\" \"{}\"",
                link.display(),
                store.display()
            ))
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "could not create a junction for the test: {}",
            String::from_utf8_lossy(&made.stderr)
        );
        let found = claude_desktop_candidates(&appdata, Some(&local));
        assert_eq!(found.len(), 1, "one file reached two ways: {found:?}");
        assert!(
            default_path_from(found).is_ok(),
            "a single location is not ambiguous"
        );
    }
    #[test]
    fn chatgpt_desktop_shares_codex_registration_without_rewriting() {
        let chatgpt = Client::ALL
            .into_iter()
            .find(|client| client.to_string() == "ChatGPT Desktop")
            .expect("ChatGPT Desktop must be selectable in the assistant");
        assert_eq!(
            default_path(chatgpt).unwrap(),
            default_path(Client::Codex).unwrap()
        );
        for (first, second) in [(Client::Codex, chatgpt), (chatgpt, Client::Codex)] {
            let f = Fixture::new();
            let original = "# keep\n[mcp_servers.other]\ncommand='keep'\n[mcp_servers.timeschedule]\nenabled=false\ndisabled_tools=['calendar_create']\n";
            let plan = f.plan(first, original);
            apply(&plan).unwrap();
            let bytes = std::fs::read(&plan.path).unwrap();
            let doc: toml_edit::DocumentMut = std::str::from_utf8(&bytes).unwrap().parse().unwrap();
            assert_eq!(doc["mcp_servers"].as_table().unwrap().len(), 2);
            assert_eq!(
                doc["mcp_servers"]["timeschedule"]["enabled"].as_bool(),
                Some(false)
            );
            assert_eq!(
                doc["mcp_servers"]["timeschedule"]["disabled_tools"][0].as_str(),
                Some("calendar_create")
            );
            let next = prepare(
                second,
                &plan.path,
                Path::new("C:\\한글 & user's %DATA%\\TimeSchedule.exe"),
                &f.0,
                "http://127.0.0.1:39281/mcp",
            )
            .unwrap();
            assert!(next.unchanged());
            assert_eq!(apply(&next).unwrap(), None);
            assert_eq!(std::fs::read(&plan.path).unwrap(), bytes);
        }
    }
    #[test]
    fn client_json_formatting_does_not_invalidate_an_existing_registration() {
        let f = Fixture::new();
        let plan = f.plan(Client::ClaudeCode, "{}");
        apply(&plan).unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(&plan.path).unwrap()).unwrap();
        let compact = serde_json::to_vec(&value).unwrap();
        std::fs::write(&plan.path, &compact).unwrap();
        let next = prepare(
            Client::ClaudeCode,
            &plan.path,
            Path::new("C:\\한글 & user's %DATA%\\TimeSchedule.exe"),
            &f.0,
            "http://127.0.0.1:39281/mcp",
        )
        .unwrap();
        assert!(next.unchanged());
        assert_eq!(apply(&next).unwrap(), None);
        assert_eq!(std::fs::read(&plan.path).unwrap(), compact);
    }
    #[test]
    fn an_active_client_writer_blocks_registration_without_losing_its_file() {
        let f = Fixture::new();
        let plan = f.plan(Client::ClaudeCode, "{}");
        let _writer = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&plan.path)
            .unwrap();
        assert!(apply(&plan).is_err());
        assert_eq!(std::fs::read_to_string(&plan.path).unwrap(), "{}");
    }
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p =
                std::env::temp_dir().join(format!("timeschedule-setup-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn plan(&self, client: Client, source: &str) -> Plan {
            let path = self.0.join("client.config");
            std::fs::write(&path, source).unwrap();
            prepare(
                client,
                &path,
                Path::new("C:\\한글 & user's %DATA%\\TimeSchedule.exe"),
                &self.0,
                "http://127.0.0.1:39281/mcp",
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn codex_registration_preserves_other_servers_comments_and_backups() {
        let f = Fixture::new();
        let original = "# keep comment\nmodel = 'chosen-model'\n[mcp_servers.other]\nurl = 'https://example.test/mcp'\n";
        let plan = f.plan(Client::Codex, original);
        let backup = apply(&plan).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(backup).unwrap(), original);
        let result = std::fs::read_to_string(&plan.path).unwrap();
        assert!(result.contains("# keep comment"));
        assert!(result.contains("chosen-model"));
        assert!(result.contains("https://example.test/mcp"));
        assert!(result.contains("http_headers_helper"));
        assert!(!result.contains("bearer_token_env_var"));
    }
    #[test]
    fn json_clients_preserve_other_settings_and_use_distinct_transports() {
        for client in [Client::ClaudeCode, Client::ClaudeDesktop] {
            let f = Fixture::new();
            let plan = f.plan(
                client,
                r#"{"preferences":{"theme":"dark"},"mcpServers":{"other":{"command":"keep"}}}"#,
            );
            apply(&plan).unwrap();
            let value: Value = serde_json::from_slice(&std::fs::read(&plan.path).unwrap()).unwrap();
            assert_eq!(value["preferences"]["theme"], "dark");
            assert_eq!(value["mcpServers"]["other"]["command"], "keep");
            let entry = &value["mcpServers"]["timeschedule"];
            if client == Client::ClaudeCode {
                assert_eq!(entry["type"], "http");
                assert!(entry["headersHelper"]
                    .as_str()
                    .unwrap()
                    .contains("-EncodedCommand"));
            } else {
                assert_eq!(
                    entry["command"],
                    "C:\\한글 & user's %DATA%\\TimeSchedule.exe"
                );
                assert_eq!(entry["args"][0], "--mcp-stdio");
                assert!(entry.get("headers").is_none());
            }
        }
    }
    #[test]
    fn stale_preview_and_malformed_configuration_never_overwrite() {
        let f = Fixture::new();
        let plan = f.plan(Client::ClaudeCode, "{}");
        std::fs::write(&plan.path, "{\"newUserEdit\":true}").unwrap();
        assert!(apply(&plan).is_err());
        assert_eq!(
            std::fs::read_to_string(&plan.path).unwrap(),
            "{\"newUserEdit\":true}"
        );
        for invalid in ["{", "[]", "{\"mcpServers\":false}"] {
            std::fs::write(&plan.path, invalid).unwrap();
            assert!(prepare(
                Client::ClaudeCode,
                &plan.path,
                Path::new("C:\\app.exe"),
                &f.0,
                "http://127.0.0.1:39281/mcp"
            )
            .is_err());
            assert_eq!(std::fs::read_to_string(&plan.path).unwrap(), invalid);
        }
    }
    #[test]
    fn repeated_registration_is_noop_and_replaces_old_auth_only_in_own_entry() {
        let f = Fixture::new();
        let plan = f.plan(Client::Codex, "[mcp_servers.timeschedule]\nurl='http://127.0.0.1:39281/mcp'\nbearer_token_env_var='OLD_TOKEN'\n");
        assert!(plan.replacing);
        apply(&plan).unwrap();
        let next = prepare(
            Client::Codex,
            &plan.path,
            Path::new("C:\\한글 & user's %DATA%\\TimeSchedule.exe"),
            &f.0,
            "http://127.0.0.1:39281/mcp",
        )
        .unwrap();
        assert_eq!(apply(&next).unwrap(), None);
    }
    #[test]
    fn new_file_registration_and_debug_output_do_not_expose_original_credentials() {
        let f = Fixture::new();
        let path = f.0.join("new/client.json");
        let plan = prepare(
            Client::ClaudeCode,
            &path,
            Path::new("C:\\app.exe"),
            &f.0,
            "http://127.0.0.1:39281/mcp",
        )
        .unwrap();
        assert_eq!(apply(&plan).unwrap(), None);
        assert!(path.is_file());
        let secret_plan = f.plan(
            Client::ClaudeCode,
            r#"{"privateKey":"fixture-secret-do-not-print"}"#,
        );
        assert!(!format!("{secret_plan:?}").contains("fixture-secret-do-not-print"));
    }
}
