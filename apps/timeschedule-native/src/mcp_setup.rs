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
            with_backup_hint(path, "ChatGPT Desktop·Codex 설정의 TOML 형식이 잘못되었습니다. 원본은 변경하지 않았습니다.")
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
                with_backup_hint(
                    path,
                    "설정 파일의 JSON 형식이 잘못되었습니다. 원본은 변경하지 않았습니다.",
                )
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
    let preview = format!("{client}: {action}\n대상: {}\n서버: {endpoint}\n{transport}{shared}\n다른 서버·설정과 도구 제한은 보존합니다. 기존 파일은 같은 폴더에 백업하고 최근 {KEPT_BACKUPS}개만 남깁니다.", path.display());
    Ok(Plan {
        path: path.into(),
        preview,
        replacing,
        original,
        updated,
    })
}
/// What a registration did.
#[derive(Debug, PartialEq)]
pub struct Applied {
    /// The backup of the configuration that was replaced. `None` when nothing
    /// had to change or the file is new.
    pub backup: Option<PathBuf>,
    /// Older backups that should have been removed and could not be. Each is
    /// a full copy of the configuration.
    pub kept_too_many: Vec<PathBuf>,
}
pub fn register(plan: &Plan) -> Result<Applied, String> {
    #[cfg(not(windows))]
    return Err("연결 설정 등록은 Windows에서 지원합니다.".into());
    let parent = plan.path.parent().ok_or("설정 폴더가 없습니다.")?;
    std::fs::create_dir_all(parent).map_err(|_| "설정 폴더 생성 실패")?;
    if plan.original.is_some() {
        return match Staged::new(plan)? {
            Stage::Ready(staged) => staged.publish(),
            Stage::Unchanged { kept_too_many } => Ok(Applied {
                backup: None,
                kept_too_many,
            }),
        };
    }
    let name = plan
        .path
        .file_name()
        .ok_or("설정 파일 이름이 없습니다.")?
        .to_string_lossy()
        .into_owned();
    listable(&[&plan.path])?;
    // A first registration that was interrupted leaves its working file too.
    sweep(parent, &name);
    let staged = parent.join(format!("{name}.timeschedule-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&staged)
        .map_err(|_| "설정 파일 생성 실패")?;
    let written = write_created(&staged, &plan.updated, &mut file);
    drop(file);
    // One step gives the finished file its name, and fails if a file has
    // appeared there since the preview. Unlike linking the name to the working
    // file and then removing that, a stop right after it leaves no second name
    // on the configuration.
    if let Err(error) = written.and_then(|_| publish_new(&staged, &plan.path)) {
        return Err(removing(error, &staged));
    }
    // The configuration may have existed before (and been reset by its app):
    // backups made then are bounded like any others.
    Ok(Applied {
        backup: None,
        kept_too_many: prune_backups(&[&plan.path]),
    })
}
/// `register`, for callers that only want the backup's path.
#[cfg(test)]
pub fn apply(plan: &Plan) -> Result<Option<PathBuf>, String> {
    register(plan).map(|applied| applied.backup)
}
/// Renames `staged` to `path` unless something already has that name.
#[cfg(windows)]
fn publish_new(staged: &Path, path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let wide =
        |path: &Path| -> Vec<u16> { path.as_os_str().encode_wide().chain(Some(0)).collect() };
    let (from, to) = (wide(staged), wide(path));
    // Without MOVEFILE_REPLACE_EXISTING an existing file makes the call fail.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err("설정 파일이 새로 생겼거나 이 폴더에서 등록을 지원하지 않습니다. 경로를 확인하고 다시 미리보기 하세요.".into());
    }
    Ok(())
}
#[cfg(not(windows))]
fn publish_new(_: &Path, _: &Path) -> Result<(), String> {
    Err("Windows 전용 기능입니다.".into())
}
fn write_created(path: &Path, bytes: &[u8], file: &mut std::fs::File) -> Result<(), String> {
    if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
        return Err(format!("설정/백업 저장 실패: {}", path.display()));
    }
    Ok(())
}

const CHANGED: &str = "미리보기 이후 설정이 바뀌었습니다. 변경 내용을 다시 확인하세요.";
/// An existing configuration with its replacement written beside it but not
/// yet swapped in. Until `publish` the configuration is exactly what it was,
/// and `publish` swaps the names in one step. So whenever the PC stops, the
/// configuration's name leads to one whole version, the old or the new, never
/// to a cut-off or mixed file. What a stop can leave is a stray working file
/// or backup beside it, removed by the next registration.
struct Staged {
    target: PathBuf,
    /// Held from the comparison to the swap. Nobody else can read the file or
    /// write to it in place meanwhile.
    original: std::fs::File,
    temporary: PathBuf,
    backup: PathBuf,
    /// Whether this attempt created each of the two. Only what it created is
    /// its own to remove.
    made_temporary: bool,
    made_backup: bool,
    /// The file that was compared: one name, not a link.
    compared: Identity,
    /// The symbolic link the user keeps the configuration behind, if any.
    link: Option<PathBuf>,
    /// The rules and label both copies were made with.
    access: Vec<u8>,
}
enum Stage {
    /// The configuration already says what the plan wants.
    Unchanged {
        kept_too_many: Vec<PathBuf>,
    },
    Ready(Staged),
}
impl Staged {
    fn new(plan: &Plan) -> Result<Stage, String> {
        Self::with_id(plan, uuid::Uuid::new_v4())
    }
    /// `unique` names this attempt's backup and working file.
    fn with_id(plan: &Plan, unique: uuid::Uuid) -> Result<Stage, String> {
        let expected = plan.original.as_ref().ok_or(CHANGED)?;
        let link = std::fs::symlink_metadata(&plan.path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
            .then(|| plan.path.clone());
        let target = as_stored(&real_target(&plan.path)?);
        let folder = target
            .parent()
            .ok_or("설정 폴더가 없습니다.")?
            .to_path_buf();
        // Copies are named after the file as it is stored, however the path
        // was typed: Windows finds one file under any capitals.
        let name = target
            .file_name()
            .ok_or("설정 파일 이름이 없습니다.")?
            .to_string_lossy()
            .into_owned();
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const GENERIC_READ: u32 = 0x8000_0000;
            const DELETE: u32 = 0x0001_0000;
            const FILE_SHARE_DELETE: u32 = 4;
            // The right to delete is asked for before anything is written:
            // replacing the file needs it, and so does removing the copies this
            // attempt puts beside it, which get the file's own rules.
            // Sharing nothing but deletion: a rename may replace the file (that
            // is how the swap works), reading and writing must wait.
            options
                .access_mode(GENERIC_READ | DELETE)
                .share_mode(FILE_SHARE_DELETE);
        }
        let mut original = options.open(&target).map_err(|error| {
            const ERROR_ACCESS_DENIED: i32 = 5;
            if error.raw_os_error() == Some(ERROR_ACCESS_DENIED) {
                "설정 파일을 교체할 권한이 없습니다(읽기와 삭제 권한이 필요합니다). 원본은 변경하지 않았습니다."
            } else {
                "설정 파일이 사용 중이거나 열 수 없습니다. AI 앱의 설정 저장이 끝난 뒤 다시 시도하세요."
            }
        })?;
        let mut current = Vec::new();
        Read::by_ref(&mut original)
            .take(MAX_CONFIG + 1)
            .read_to_end(&mut current)
            .map_err(|_| "설정 읽기 실패")?;
        if &current != expected {
            return Err(CHANGED.into());
        }
        listable(&known_as(&target, link.as_deref()))?;
        if plan.unchanged() {
            // Nothing to write, but what earlier attempts left is still tidied:
            // a stop right after a swap, or an earlier version, leaves copies.
            let compared = identity(&original);
            sweep(&folder, &name);
            let kept_too_many = prune_backups(&known_as(&target, link.as_deref()));
            // Tidying takes a moment. If an AI app saved a version of its own
            // over the file meanwhile, the registration is not known to be there.
            if !compared.is_some_and(|compared| still_there(&target, link.as_deref(), &compared)) {
                return Err(CHANGED.into());
            }
            return Ok(Stage::Unchanged { kept_too_many });
        }
        let compared = identity(&original).ok_or("설정 파일 정보를 확인하지 못했습니다.")?;
        if compared.links > 1 {
            // A new file under this name would leave the other names on the old content.
            return Err("설정 파일이 하드 링크로 다른 이름과 연결되어 있어 자동 등록하지 않습니다. 원본은 변경하지 않았습니다.".into());
        }
        sweep(&folder, &name);
        // Read once: both copies get these rules, and the swap goes ahead only
        // if the original still has them.
        let access = access_of(&original)?;
        let mut staged = Self {
            backup: folder.join(format!("{name}.timeschedule-{unique}.bak")),
            temporary: folder.join(format!("{name}.timeschedule-{unique}.tmp")),
            target,
            original,
            made_temporary: false,
            made_backup: false,
            compared,
            link,
            access,
        };
        let written = write_backup(
            &staged.backup,
            expected,
            &staged.original,
            &staged.access,
            &mut staged.made_backup,
        )
        .and_then(|_| {
            write_replacement(
                &staged.temporary,
                &plan.updated,
                &staged.original,
                &staged.access,
                &mut staged.made_temporary,
            )
        });
        match written {
            Ok(()) => Ok(Stage::Ready(staged)),
            Err(error) => Err(staged.discard(error)),
        }
    }
    /// Removes what this attempt made (the configuration was not changed, so
    /// none of it is needed) and returns `error`, naming any copy of the
    /// configuration that could not be removed.
    fn discard(&self, error: impl Into<String>) -> String {
        let mut message = error.into();
        for (made, path) in [
            (self.made_temporary, &self.temporary),
            (self.made_backup, &self.backup),
        ] {
            if made {
                message = removing(message, path);
            }
        }
        message
    }
    /// Swaps the replacement in and returns the backup of what it replaced.
    fn publish(self) -> Result<Applied, String> {
        // The held handle lets a rename replace the file, so another program
        // may have swapped in its own version since the comparison, moved the
        // file away and left a link, or given it a second name. Then the name
        // no longer leads to the one file that was compared. Windows has no
        // single step that both checks this and swaps, so the check comes
        // immediately before the swap (see the limits in docs/local-mcp.md).
        // Rules or a label changed meanwhile (closed to others, say) are not
        // in the copies. A handle held for reading does not stop that change.
        let rules_changed = access_of(&self.original).map_or(true, |now| now != self.access);
        // Last of all, so that as little as possible lies between it and the swap.
        if rules_changed || !still_there(&self.target, self.link.as_deref(), &self.compared) {
            return Err(self.discard(CHANGED));
        }
        // One step: the name leads to the old file or to the new one.
        if std::fs::rename(&self.temporary, &self.target).is_err() {
            return Err(self.discard("설정 파일을 교체하지 못했습니다. 원본은 그대로입니다. 파일이 읽기 전용이거나 다른 프로그램이 사용 중인지 확인하세요."));
        }
        drop(self.original);
        Ok(Applied {
            kept_too_many: prune_backups(&known_as(&self.target, self.link.as_deref())),
            backup: Some(self.backup),
        })
    }
}
/// Backups are counted and removed by listing the folder of every name the
/// configuration is known by. Where that is not allowed they could neither be
/// bounded nor, if nothing changes, be known to be within bounds, so nothing
/// is registered there.
fn listable(names: &[&Path]) -> Result<(), String> {
    for name in names {
        let folder = name.parent().ok_or("설정 폴더가 없습니다.")?;
        std::fs::read_dir(folder).map_err(|_| {
            "설정 폴더의 파일 목록을 읽을 수 없어 등록하지 않았습니다(백업을 정리할 수 없습니다). 원본은 변경하지 않았습니다."
        })?;
    }
    Ok(())
}
/// What the name `target` itself is right now. A link put there is seen as a
/// link, not as the file it leads to.
fn named(target: &Path) -> Option<Identity> {
    let mut options = std::fs::OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        // Metadata only, which no sharing mode refuses.
        options
            .access_mode(0)
            .share_mode(7)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    }
    #[cfg(not(windows))]
    options.read(true);
    options.open(target).ok().and_then(|file| identity(&file))
}
/// Whether the configuration's name still leads to the one file that was
/// compared, and the link the user keeps it behind, if any, still leads to
/// that name. Otherwise an AI app or the user has put something else there.
fn still_there(target: &Path, link: Option<&Path>, compared: &Identity) -> bool {
    link.is_none_or(|link| real_target(link).ok().as_deref() == Some(target))
        && named(target).as_ref() == Some(compared)
}
/// The names a configuration's backups may lie beside.
fn known_as<'a>(target: &'a Path, link: Option<&'a Path>) -> Vec<&'a Path> {
    std::iter::once(target).chain(link).collect()
}
/// `path` with its file name spelled as the file is stored, if it exists.
fn as_stored(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(stored) => match stored.file_name() {
            Some(name) => path.with_file_name(name),
            None => path.into(),
        },
        Err(_) => path.into(),
    }
}
/// Removes `path`, a copy of the configuration this module made and no longer
/// needs, and returns `message`, with the path added if it is still there.
fn removing(mut message: String, path: &Path) -> String {
    if std::fs::remove_file(path).is_err() && !gone(path) {
        message.push_str(&format!(
            "\n지우지 못한 설정 사본이 남았습니다: {}",
            path.display()
        ));
    }
    message
}
/// The file a registration replaces: the configuration itself, or what it
/// points to when the user keeps it elsewhere behind a symbolic link.
fn real_target(path: &Path) -> Result<PathBuf, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::canonicalize(path)
            .map_err(|_| "설정 파일 링크의 대상을 찾지 못했습니다.".into()),
        _ => Ok(path.into()),
    }
}
/// Whether `file` is one this module made for the configuration `config`:
/// `<config>.timeschedule-<uuid>.<extension>`.
fn ours(file: &str, config: &str, extension: &str) -> bool {
    file.strip_prefix(config)
        .and_then(|rest| rest.strip_prefix(".timeschedule-"))
        .and_then(|rest| rest.strip_suffix(extension))
        .and_then(|rest| rest.strip_suffix('.'))
        .is_some_and(|id| id.len() == 36 && uuid::Uuid::parse_str(id).is_ok())
}
/// This module's files for `config` in `folder`, with their modification times.
/// Names are matched exactly. Copies are named after the file as it is stored
/// (see `Staged::with_id`), so one file has one set whatever capitals its path
/// was typed with, and in a folder with case-sensitive names two files that
/// differ in capitals alone never share one.
fn made(folder: &Path, config: &str, extension: &str) -> Vec<(std::time::SystemTime, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| ours(&entry.file_name().to_string_lossy(), config, extension))
        .filter_map(|entry| {
            // Regular files only: never follow a link out of the folder.
            let metadata = entry.metadata().ok()?;
            metadata
                .is_file()
                .then(|| Some((metadata.modified().ok()?, entry.path())))?
        })
        .collect()
}
/// Removes working files a registration left when the PC stopped before the
/// swap. One still being written is held by its writer and stays.
fn sweep(folder: &Path, config: &str) {
    for (_, path) in made(folder, config, "tmp") {
        let _ = std::fs::remove_file(path);
    }
}
/// Backups of one configuration, newest first. It can be known by more than
/// one name: earlier versions put the backup beside a symbolic link, this one
/// beside the file the link leads to.
fn backups(names: &[&Path]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for name in names {
        if let (Some(folder), Some(file)) = (name.parent(), name.file_name()) {
            found.extend(made(folder, &file.to_string_lossy(), "bak"));
        }
    }
    found.sort_by(|a, b| b.cmp(a));
    found.dedup();
    found.into_iter().map(|(_, path)| path).collect()
}
/// Removes all but the newest backups and returns those it could not remove:
/// full copies of the configuration that are still there.
fn prune_backups(targets: &[&Path]) -> Vec<PathBuf> {
    backups(targets)
        .into_iter()
        .skip(KEPT_BACKUPS)
        .filter(|old| std::fs::remove_file(old).is_err() && !gone(old))
        .collect()
}
/// Whether nothing has this name. An error other than "not found" (access
/// denied, say) is not taken for absence.
fn gone(path: &Path) -> bool {
    matches!(std::fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}
/// `message`, naming the newest backup when there is one to restore from.
fn with_backup_hint(path: &Path, message: &str) -> String {
    let target = as_stored(&real_target(path).unwrap_or_else(|_| path.into()));
    match backups(&[&target, path]).first() {
        Some(backup) => format!(
            "{message}\nTimeSchedule이 만든 가장 최근 백업: {}",
            backup.display()
        ),
        None => message.into(),
    }
}

const SE_DACL_PRESENT: u16 = 0x0004;
const SE_SACL_PRESENT: u16 = 0x0010;
const SE_DACL_PROTECTED_BIT: u16 = 0x1000;
/// The control word of a self-relative security descriptor.
fn control(descriptor: &[u8]) -> u16 {
    descriptor
        .get(2..4)
        .map_or(0, |bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
}
const INHERITED_ACE: u8 = 0x10;
/// Where the rules of one list of a self-relative security descriptor lie:
/// the access rules (`present` = `SE_DACL_PRESENT`, its offset stored at byte
/// 16) or the integrity label (`SE_SACL_PRESENT`, byte 12). `Ok(None)`: the
/// descriptor has no such list. `Err`: it does not parse.
fn rule_spans(
    descriptor: &[u8],
    present: u16,
    offset_at: usize,
) -> Result<Option<Vec<std::ops::Range<usize>>>, ()> {
    let word = |at: usize| -> Result<usize, ()> {
        let pair = descriptor.get(at..at + 2).ok_or(())?;
        Ok(u16::from_le_bytes([pair[0], pair[1]]) as usize)
    };
    if control(descriptor) & present == 0 {
        return Ok(None);
    }
    let offset = descriptor.get(offset_at..offset_at + 4).ok_or(())?;
    let offset = u32::from_le_bytes([offset[0], offset[1], offset[2], offset[3]]) as usize;
    if offset == 0 {
        return Ok(None);
    }
    let count = word(offset + 4)?;
    let mut at = offset + 8;
    let mut found = Vec::new();
    for _ in 0..count {
        let size = word(at + 2)?;
        if size < 8 || at + size > descriptor.len() {
            return Err(());
        }
        found.push(at..at + size);
        at += size;
    }
    Ok(Some(found))
}
/// The rules of one list, each without its "inherited" mark: who, which
/// rights or which integrity level, allow or deny.
fn rule_list(
    descriptor: &[u8],
    present: u16,
    offset_at: usize,
) -> Result<Option<Vec<Vec<u8>>>, ()> {
    Ok(rule_spans(descriptor, present, offset_at)?.map(|spans| {
        spans
            .into_iter()
            .map(|span| {
                let mut rule = descriptor[span].to_vec();
                rule[1] &= !INHERITED_ACE;
                rule
            })
            .collect()
    }))
}
/// Makes an inherited integrity label the descriptor's own. Windows drops an
/// inherited label when a file is created from the descriptor, and asking it
/// to keep the list as given needs a privilege ordinary accounts lack.
fn own_label(descriptor: &mut [u8]) {
    if let Ok(Some(spans)) = rule_spans(descriptor, SE_SACL_PRESENT, 12) {
        for span in spans {
            descriptor[span.start + 1] &= !INHERITED_ACE;
        }
    }
}
/// Whether two descriptors grant and deny the same things in the same order
/// and carry the same integrity label.
fn same_rules(a: &[u8], b: &[u8]) -> bool {
    let lists = |descriptor: &[u8]| {
        Some((
            rule_list(descriptor, SE_DACL_PRESENT, 16).ok()?,
            // No label list and an empty one both mean: no label of its own.
            rule_list(descriptor, SE_SACL_PRESENT, 12)
                .ok()?
                .unwrap_or_default(),
        ))
    };
    matches!((lists(a), lists(b)), (Some(a), Some(b)) if a == b)
}

/// What decides who may open an open file, as a self-relative descriptor: its
/// access rules (DACL) and its integrity label, which stops processes of
/// lower integrity and is kept in a separate list.
#[cfg(windows)]
fn access_of(file: &std::fs::File) -> Result<Vec<u8>, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Security::{GetKernelObjectSecurity, DACL_SECURITY_INFORMATION};
    const LABEL_SECURITY_INFORMATION: u32 = 0x10;
    let wanted = DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION;
    let mut size = 0;
    unsafe {
        GetKernelObjectSecurity(
            file.as_raw_handle(),
            wanted,
            std::ptr::null_mut(),
            0,
            &mut size,
        );
    }
    if size == 0 || size > 65536 {
        return Err("원본 설정의 접근 권한을 확인하지 못했습니다.".into());
    }
    let mut descriptor = vec![0u8; size as usize];
    let ok = unsafe {
        GetKernelObjectSecurity(
            file.as_raw_handle(),
            wanted,
            descriptor.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    };
    if ok == 0 {
        return Err("원본 설정의 접근 권한을 확인하지 못했습니다.".into());
    }
    Ok(descriptor)
}
/// A new file that nobody else can open while it is written. With `access`,
/// it gets exactly those rules and that label and takes no rules from its
/// folder, and they are in place before the first byte; without, it takes its
/// folder's. `made` is set once the file exists.
#[cfg(windows)]
fn create_with(
    path: &Path,
    access: Option<&mut Vec<u8>>,
    attributes: u32,
    made: &mut bool,
) -> Result<std::fs::File, String> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows_sys::Win32::{
        Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Security::{SetSecurityDescriptorControl, SECURITY_ATTRIBUTES, SE_DACL_PROTECTED},
        Storage::FileSystem::{CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL},
    };
    // To read the new file's own rules back.
    const READ_CONTROL: u32 = 0x0002_0000;
    let mut security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 0,
    };
    if let Some(descriptor) = access {
        own_label(descriptor);
        let descriptor = descriptor.as_mut_ptr().cast();
        // The access rules are taken exactly as given, inherited ones included.
        if unsafe { SetSecurityDescriptorControl(descriptor, SE_DACL_PROTECTED, SE_DACL_PROTECTED) }
            == 0
        {
            return Err("원본 설정의 접근 권한을 복사하지 못했습니다.".into());
        }
        security.lpSecurityDescriptor = descriptor;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE | READ_CONTROL,
            0,
            if security.lpSecurityDescriptor.is_null() {
                std::ptr::null()
            } else {
                &security
            },
            CREATE_NEW,
            if attributes == 0 {
                FILE_ATTRIBUTE_NORMAL
            } else {
                attributes
            },
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err("원본과 같은 접근 권한으로 설정/백업 파일을 만들지 못했습니다.".into());
    }
    *made = true;
    Ok(unsafe { std::fs::File::from_raw_handle(handle) })
}
/// Stored encrypted for this account (EFS), like the original.
#[cfg(windows)]
const FILE_ATTRIBUTE_ENCRYPTED: u32 = 0x4000;
/// The marks a copy is created with. Windows ignores a request for encryption
/// made together with the system mark, so that mark gives way.
#[cfg(windows)]
fn marks(source: &std::fs::File, kept: u32) -> Result<u32, String> {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_SYSTEM;
    let attributes = identity(source)
        .ok_or("설정 파일 정보를 확인하지 못했습니다.")?
        .attributes
        & (kept | FILE_ATTRIBUTE_ENCRYPTED);
    Ok(if attributes & FILE_ATTRIBUTE_ENCRYPTED != 0 {
        attributes & !FILE_ATTRIBUTE_SYSTEM
    } else {
        attributes
    })
}
/// Hands `file` back only if it is as closed as the original: the same rules
/// and label, and encrypted if asked. Otherwise the still empty file is
/// removed and nothing of the configuration is written to it.
#[cfg(windows)]
fn proven(
    file: std::fs::File,
    path: &Path,
    wanted: &[u8],
    attributes: u32,
    made: &mut bool,
) -> Result<std::fs::File, String> {
    let closed = access_of(&file).is_ok_and(|actual| same_rules(&actual, wanted))
        && (attributes & FILE_ATTRIBUTE_ENCRYPTED == 0
            || identity(&file)
                .is_some_and(|actual| actual.attributes & FILE_ATTRIBUTE_ENCRYPTED != 0));
    if closed {
        return Ok(file);
    }
    drop(file);
    if std::fs::remove_file(path).is_ok() {
        *made = false;
    }
    Err("원본과 같은 접근 권한·암호화를 가진 파일을 만들 수 없어 등록하지 않았습니다. 원본은 변경하지 않았습니다.".into())
}
/// A file with exactly the original's rules and label, proven before use.
#[cfg(windows)]
fn create_like(
    path: &Path,
    access: &[u8],
    attributes: u32,
    made: &mut bool,
) -> Result<std::fs::File, String> {
    let mut exact = access.to_vec();
    let file = create_with(path, Some(&mut exact), attributes, made)?;
    proven(file, path, access, attributes, made)
}
/// A backup always carries its own copy of the original's rules and label, so
/// it stays as private as the original wherever its folder's rules go later.
/// It is encrypted if the original is.
#[cfg(windows)]
fn write_backup(
    path: &Path,
    bytes: &[u8],
    source: &std::fs::File,
    access: &[u8],
    made: &mut bool,
) -> Result<(), String> {
    let mut file = create_like(path, access, marks(source, 0)?, made)?;
    write_created(path, bytes, &mut file)
}
/// Writes the file that will take the original's place, with the original's
/// access rules, integrity label, encryption and hidden/system marks.
#[cfg(windows)]
fn write_replacement(
    path: &Path,
    bytes: &[u8],
    source: &std::fs::File,
    access: &[u8],
    made: &mut bool,
) -> Result<(), String> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NOT_CONTENT_INDEXED, FILE_ATTRIBUTE_SYSTEM,
    };
    let attributes = marks(
        source,
        FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED,
    )?;
    if control(access) & SE_DACL_PROTECTED_BIT == 0 {
        // The usual case: the original takes its rules from its folder, and so
        // does a new file made beside it. Checked, not assumed.
        let file = create_with(path, None, attributes, made)?;
        let follows =
            access_of(&file).is_ok_and(|actual| control(&actual) & SE_DACL_PROTECTED_BIT == 0);
        if follows {
            if let Ok(mut file) = proven(file, path, access, attributes, made) {
                return write_created(path, bytes, &mut file);
            }
        } else {
            drop(file);
            if std::fs::remove_file(path).is_ok() {
                *made = false;
            }
        }
        if *made {
            return Err("설정 교체 준비 실패".into());
        }
        // The original also has rules or a label of its own. They are kept
        // exactly, at the price that the new file no longer follows its
        // folder's rules.
    }
    let mut file = create_like(path, access, attributes, made)?;
    write_created(path, bytes, &mut file)
}
#[cfg(not(windows))]
fn write_backup(
    _: &Path,
    _: &[u8],
    _: &std::fs::File,
    _: &[u8],
    _: &mut bool,
) -> Result<(), String> {
    Err("Windows 전용 기능입니다.".into())
}
#[cfg(not(windows))]
fn write_replacement(
    _: &Path,
    _: &[u8],
    _: &std::fs::File,
    _: &[u8],
    _: &mut bool,
) -> Result<(), String> {
    Err("Windows 전용 기능입니다.".into())
}
#[cfg(not(windows))]
fn access_of(_: &std::fs::File) -> Result<Vec<u8>, String> {
    Err("Windows 전용 기능입니다.".into())
}

/// How many backups of one configuration file are kept. Each is a full copy,
/// secrets included, so older ones are removed once a newer one exists.
const KEPT_BACKUPS: usize = 3;
/// Which file an open handle refers to, and what the replacement must match.
#[derive(Debug, PartialEq)]
struct Identity {
    volume: u32,
    index: u64,
    links: u32,
    attributes: u32,
}
#[cfg(windows)]
fn identity(file: &std::fs::File) -> Option<Identity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // Reads metadata of a handle this process owns.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return None;
    }
    Some(Identity {
        volume: info.dwVolumeSerialNumber,
        index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        links: info.nNumberOfLinks,
        attributes: info.dwFileAttributes,
    })
}
#[cfg(not(windows))]
fn identity(_: &std::fs::File) -> Option<Identity> {
    None
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

    const EXE: &str = "C:\\한글 & user's %DATA%\\TimeSchedule.exe";
    fn staged(plan: &Plan) -> Staged {
        match Staged::new(plan).unwrap() {
            Stage::Ready(staged) => staged,
            Stage::Unchanged { .. } => panic!("the plan changes nothing"),
        }
    }
    /// Makes `link` a symbolic link to the file `real`, or says that this
    /// account may not (it takes Developer Mode or elevation). On CI that is a
    /// failure, so these tests cannot pass there by being skipped.
    fn linked(real: &Path, link: &Path) -> bool {
        if std::os::windows::fs::symlink_file(real, link).is_ok() {
            return true;
        }
        assert!(
            std::env::var_os("GITHUB_ACTIONS").is_none(),
            "CI must be able to create symbolic links"
        );
        eprintln!("skipped: this account may not create symbolic links");
        false
    }
    fn plan_for(path: &Path, dir: &Path, port: u16) -> Plan {
        prepare(
            Client::ClaudeCode,
            path,
            Path::new(EXE),
            dir,
            &format!("http://127.0.0.1:{port}/mcp"),
        )
        .unwrap()
    }
    fn which_file(path: &Path) -> (u32, u64) {
        let id = identity(&std::fs::File::open(path).unwrap()).unwrap();
        (id.volume, id.index)
    }
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
    fn powershell(script: &str) -> String {
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn registration_swaps_in_a_whole_new_file_instead_of_rewriting_the_old_one() {
        // Rewriting in place leaves a cut-off or mixed configuration if the PC
        // loses power part way. A new file that takes the old one's place in one
        // step cannot: the name always leads to one whole version.
        let f = Fixture::new();
        let plan = f.plan(Client::ClaudeCode, r#"{"keep":true}"#);
        let before = which_file(&plan.path);
        let backup = apply(&plan).unwrap().unwrap();
        assert_ne!(
            which_file(&plan.path),
            before,
            "the old file was rewritten in place"
        );
        let value: Value = serde_json::from_slice(&std::fs::read(&plan.path).unwrap()).unwrap();
        assert_eq!(value["keep"], true);
        assert!(value["mcpServers"]["timeschedule"].is_object());
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            r#"{"keep":true}"#
        );
        // Only the configuration and its backup are left: no working file.
        assert_eq!(
            names(&f.0),
            [
                "client.config".to_string(),
                backup.file_name().unwrap().to_string_lossy().into_owned()
            ]
        );
    }
    #[test]
    fn only_the_newest_backups_of_a_configuration_are_kept() {
        let f = Fixture::new();
        let path = f.0.join("client.config");
        std::fs::write(&path, "{}").unwrap();
        // Files that merely look similar are not ours to delete.
        let strangers = [
            "client.config.timeschedule-notes.bak".to_string(),
            format!("other.config.timeschedule-{}.bak", uuid::Uuid::new_v4()),
            "client.config.bak".to_string(),
        ];
        for name in &strangers {
            std::fs::write(f.0.join(name), "not ours").unwrap();
        }
        let mut made = Vec::new();
        for port in 40001..40006 {
            made.push(apply(&plan_for(&path, &f.0, port)).unwrap().unwrap());
            // Distinct modification times, whatever the clock resolution.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let kept: Vec<String> = names(&f.0)
            .into_iter()
            .filter(|name| name.starts_with("client.config.timeschedule-") && name.len() > 60)
            .collect();
        let mut newest: Vec<String> = made[made.len() - KEPT_BACKUPS..]
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        newest.sort();
        assert_eq!(kept, newest, "older backups must go, newer ones stay");
        for name in &strangers {
            assert_eq!(std::fs::read_to_string(f.0.join(name)).unwrap(), "not ours");
        }
    }
    #[test]
    fn a_configuration_with_a_second_name_is_refused_rather_than_split_from_it() {
        // Swapping in a new file would leave the other name on the old content.
        let f = Fixture::new();
        let plan = f.plan(Client::ClaudeCode, "{}");
        let twin = f.0.join("same file, second name");
        std::fs::hard_link(&plan.path, &twin).unwrap();
        let error = apply(&plan).unwrap_err();
        assert!(error.contains("하드 링크"), "{error}");
        assert_eq!(std::fs::read_to_string(&plan.path).unwrap(), "{}");
        assert_eq!(std::fs::read_to_string(&twin).unwrap(), "{}");
        assert_eq!(
            names(&f.0),
            ["client.config", "same file, second name"],
            "nothing may be left behind by a refusal"
        );
    }
    #[test]
    fn a_registration_that_cannot_replace_the_file_leaves_it_and_nothing_else() {
        let f = Fixture::new();
        let plan = f.plan(Client::ClaudeCode, r#"{"keep":true}"#);
        let mut permissions = std::fs::metadata(&plan.path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&plan.path, permissions.clone()).unwrap();
        assert!(apply(&plan).is_err());
        assert_eq!(
            std::fs::read_to_string(&plan.path).unwrap(),
            r#"{"keep":true}"#
        );
        assert_eq!(names(&f.0), ["client.config"], "no backup or working file");
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        std::fs::set_permissions(&plan.path, permissions).unwrap();
    }
    #[test]
    fn a_damaged_configuration_points_to_the_newest_backup() {
        let f = Fixture::new();
        let path = f.0.join("client.config");
        let damaged = "{\"mcpServers\":";
        let attempt = |path: &Path| {
            prepare(
                Client::ClaudeCode,
                path,
                Path::new(EXE),
                &f.0,
                "http://127.0.0.1:39281/mcp",
            )
        };
        // Without a backup there is nothing to point to.
        std::fs::write(&path, damaged).unwrap();
        let error = attempt(&path).unwrap_err();
        assert!(!error.contains(".bak"), "{error}");
        std::fs::write(&path, "{}").unwrap();
        apply(&plan_for(&path, &f.0, 40001)).unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let newest = apply(&plan_for(&path, &f.0, 40002)).unwrap().unwrap();
        // What a save cut off by an earlier version could leave behind.
        std::fs::write(&path, damaged).unwrap();
        let error = attempt(&path).unwrap_err();
        assert!(error.contains("JSON"), "{error}");
        assert!(
            error.contains(&newest.display().to_string()),
            "the newest backup is not named: {error}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            damaged,
            "unchanged"
        );
    }
    #[test]
    fn a_stop_before_the_swap_leaves_the_whole_original_and_is_tidied_up_next_time() {
        let f = Fixture::new();
        let original = r#"{"keep":true,"mcpServers":{"other":{"command":"x"}}}"#;
        let plan = f.plan(Client::ClaudeCode, original);
        let staged = staged(&plan);
        // While the replacement is prepared nobody reads the file or writes to it.
        assert!(std::fs::File::open(&plan.path).is_err());
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(&plan.path)
            .is_err());
        let (temporary, backup) = (staged.temporary.clone(), staged.backup.clone());
        // The replacement is already whole on disk, beside the original.
        let ready: Value = serde_json::from_slice(&std::fs::read(&temporary).unwrap()).unwrap();
        assert!(ready["mcpServers"]["timeschedule"].is_object());
        // The PC stops here: the handle closes and nothing is cleaned up.
        drop(staged);
        assert_eq!(std::fs::read_to_string(&plan.path).unwrap(), original);
        assert!(temporary.is_file() && backup.is_file());
        // The next registration starts from the untouched original and removes
        // the stray working file.
        let kept = apply(&plan_for(&plan.path, &f.0, 39281)).unwrap().unwrap();
        assert!(!temporary.exists(), "the working file was left behind");
        let mut expected = vec![
            "client.config".to_string(),
            backup.file_name().unwrap().to_string_lossy().into_owned(),
            kept.file_name().unwrap().to_string_lossy().into_owned(),
        ];
        expected.sort();
        assert_eq!(names(&f.0), expected);
        let value: Value = serde_json::from_slice(&std::fs::read(&plan.path).unwrap()).unwrap();
        assert_eq!(value["mcpServers"]["other"]["command"], "x");
        assert!(value["mcpServers"]["timeschedule"].is_object());
    }
    #[test]
    fn a_version_another_program_swapped_in_meanwhile_is_not_overwritten() {
        let f = Fixture::new();
        let plan = f.plan(Client::ClaudeCode, "{}");
        let staged = staged(&plan);
        // An AI app saves its own new version the same way: written beside the
        // file, then renamed over it.
        let theirs = f.0.join("their working file");
        std::fs::write(&theirs, r#"{"theirNewSetting":1}"#).unwrap();
        std::fs::rename(&theirs, &plan.path).unwrap();
        let error = staged.publish().unwrap_err();
        assert!(error.contains("바뀌었습니다"), "{error}");
        assert_eq!(
            std::fs::read_to_string(&plan.path).unwrap(),
            r#"{"theirNewSetting":1}"#
        );
        assert_eq!(
            names(&f.0),
            ["client.config"],
            "nothing of the attempt stays"
        );
    }
    /// Who is granted or denied what, without the "inherited" mark.
    fn grants(sddl: &str) -> Vec<String> {
        sddl.split('(')
            .skip(1)
            .map(|rule| {
                let fields: Vec<&str> = rule.trim_end_matches(')').split(';').collect();
                format!("{};{};{}", fields[0], fields[2], fields[5])
            })
            .collect()
    }
    #[test]
    fn the_new_file_keeps_the_access_rules_and_marks_of_the_one_it_replaces() {
        let f = Fixture::new();
        let all = [
            "inherited.json",
            "private.json",
            "mixed.json",
            "hidden.json",
        ];
        for name in all {
            std::fs::write(f.0.join(name), "{}").unwrap();
        }
        let at = |name: &str| f.0.join(name).to_string_lossy().replace('\'', "''");
        // private: this user only, nothing from the folder. mixed: the folder's
        // rules plus one of its own (read access for the Guests group).
        powershell(&format!(
            "$ErrorActionPreference='Stop'; \
             $me=[Security.Principal.WindowsIdentity]::GetCurrent().User; \
             $a=[IO.File]::GetAccessControl('{private}'); $a.SetAccessRuleProtection($true,$false); \
             $a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($me,'FullControl','Allow')); \
             [IO.File]::SetAccessControl('{private}',$a); \
             $b=[IO.File]::GetAccessControl('{mixed}'); \
             $b.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-5-32-546'),'Read','Allow')); \
             [IO.File]::SetAccessControl('{mixed}',$b); \
             [IO.File]::SetAttributes('{hidden}','Hidden')",
            private = at("private.json"),
            mixed = at("mixed.json"),
            hidden = at("hidden.json"),
        ));
        let read_rules = || -> Vec<String> {
            powershell(&format!(
                "foreach ($p in '{}','{}','{}') {{ [IO.File]::GetAccessControl($p).GetSecurityDescriptorSddlForm('Access') }}",
                at("inherited.json"),
                at("private.json"),
                at("mixed.json")
            ))
            .lines()
            .map(|line| line.trim().to_string())
            .collect()
        };
        let before = read_rules();
        assert_eq!(grants(&before[1]).len(), 1, "{}", before[1]);
        assert_eq!(
            grants(&before[2]).len(),
            grants(&before[0]).len() + 1,
            "{}",
            before[2]
        );
        for name in all {
            let path = f.0.join(name);
            let was = which_file(&path);
            apply(&plan_for(&path, &f.0, 39281)).unwrap().unwrap();
            assert_ne!(which_file(&path), was, "{name} was not replaced");
        }
        let after = read_rules();
        // What decides access: whether the folder's rules apply ("P" = they do
        // not) and each rule in order. The "AI" mark only records that a list
        // was once computed by inheritance and changes nothing.
        let meaning = |sddl: &str| {
            let (marks, rules) = sddl.split_once('(').unwrap();
            (marks.contains('P'), rules.to_string())
        };
        assert_eq!(after[0], before[0], "rules taken from the folder");
        assert_eq!(meaning(&after[1]), meaning(&before[1]), "private rules");
        assert!(meaning(&before[1]).0 && !meaning(&before[0]).0);
        assert_eq!(
            grants(&after[2]),
            grants(&before[2]),
            "own rules on top of the folder's: {} -> {}",
            before[2],
            after[2]
        );
        const FILE_ATTRIBUTE_HIDDEN: u32 = 2;
        let hidden = identity(&std::fs::File::open(f.0.join("hidden.json")).unwrap()).unwrap();
        assert_ne!(
            hidden.attributes & FILE_ATTRIBUTE_HIDDEN,
            0,
            "no longer hidden"
        );
        // The backup of the private file is as private as the file.
        let backup = names(&f.0)
            .into_iter()
            .find(|name| name.starts_with("private.json.timeschedule-"))
            .unwrap();
        let backup_rules = powershell(&format!(
            "[IO.File]::GetAccessControl('{}').GetSecurityDescriptorSddlForm('Access')",
            at(&backup)
        ));
        assert_eq!(meaning(&backup_rules), meaning(&before[1]));
    }
    #[test]
    fn a_configuration_kept_behind_a_symbolic_link_is_replaced_where_it_really_is() {
        let f = Fixture::new();
        let elsewhere = f.0.join("dotfiles");
        std::fs::create_dir(&elsewhere).unwrap();
        let real = elsewhere.join("claude.json");
        std::fs::write(&real, r#"{"keep":true}"#).unwrap();
        let link = f.0.join("client.config");
        if !linked(&real, &link) {
            return;
        }
        let backup = apply(&plan_for(&link, &f.0, 39281)).unwrap().unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the link itself was replaced by a file"
        );
        let value: Value = serde_json::from_slice(&std::fs::read(&real).unwrap()).unwrap();
        assert_eq!(value["keep"], true);
        assert!(value["mcpServers"]["timeschedule"].is_object());
        assert_eq!(
            std::fs::canonicalize(backup.parent().unwrap()).unwrap(),
            std::fs::canonicalize(&elsewhere).unwrap(),
            "the backup belongs beside the real file"
        );
        assert_eq!(names(&f.0), ["client.config", "dotfiles"]);
    }
    fn tool(program: &str, arguments: &[&std::ffi::OsStr]) -> Option<String> {
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new(program)
            .args(arguments)
            .creation_flags(0x08000000)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }
    #[test]
    fn an_integrity_label_on_the_original_is_carried_over() {
        // A label is a second kind of access rule, kept apart from the ordinary
        // list: it stops processes of lower integrity. A new file has none.
        let f = Fixture::new();
        let path = f.0.join("labelled.json");
        std::fs::write(&path, "{}").unwrap();
        tool(
            "icacls.exe",
            &[
                path.as_os_str(),
                "/setintegritylevel".as_ref(),
                "M".as_ref(),
            ],
        )
        .expect("could not label the fixture");
        // "(NW)" is how icacls prints a label's policy, in every display language.
        let labelled = |path: &Path| {
            tool("icacls.exe", &[path.as_os_str()]).is_some_and(|s| s.contains("(NW)"))
        };
        assert!(labelled(&path));
        let backup = apply(&plan_for(&path, &f.0, 39281)).unwrap().unwrap();
        assert!(labelled(&path), "the new file lost the label");
        assert!(labelled(&backup), "the backup lost the label");
    }
    #[test]
    fn an_encrypted_configuration_stays_encrypted() {
        let f = Fixture::new();
        let path = f.0.join("encrypted.json");
        std::fs::write(&path, "{}").unwrap();
        const FILE_ATTRIBUTE_ENCRYPTED: u32 = 0x4000;
        let encrypted = |path: &Path| {
            identity(&std::fs::File::open(path).unwrap())
                .unwrap()
                .attributes
                & FILE_ATTRIBUTE_ENCRYPTED
                != 0
        };
        let quoted = path.to_string_lossy().replace('\'', "''");
        let script = format!("[IO.File]::Encrypt('{quoted}')");
        let done = tool(
            "powershell.exe",
            &[
                "-NoProfile".as_ref(),
                "-NonInteractive".as_ref(),
                "-Command".as_ref(),
                script.as_ref(),
            ],
        );
        if done.is_none() || !encrypted(&path) {
            // Windows Home, or a volume or policy without file encryption.
            eprintln!("skipped: this PC cannot encrypt a file");
            return;
        }
        let backup = apply(&plan_for(&path, &f.0, 39281)).unwrap().unwrap();
        assert!(encrypted(&path), "the new file is stored in the clear");
        assert!(encrypted(&backup), "the backup is stored in the clear");
    }
    #[test]
    fn an_unchanged_registration_still_tidies_up_old_backups_and_working_files() {
        // A stop right after a swap, or an earlier version, can leave more
        // backups than are kept. Registering the same thing again must not
        // leave those copies of the configuration lying around for good.
        let f = Fixture::new();
        let path = f.0.join("client.config");
        std::fs::write(&path, "{}").unwrap();
        apply(&plan_for(&path, &f.0, 39281)).unwrap().unwrap();
        for _ in 0..4 {
            std::fs::write(
                f.0.join(format!(
                    "client.config.timeschedule-{}.bak",
                    uuid::Uuid::new_v4()
                )),
                "{}",
            )
            .unwrap();
        }
        let stray = f.0.join(format!(
            "client.config.timeschedule-{}.tmp",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&stray, "{").unwrap();
        let same = plan_for(&path, &f.0, 39281);
        assert!(same.unchanged());
        assert_eq!(apply(&same).unwrap(), None);
        assert!(!stray.exists(), "the working file was left behind");
        assert_eq!(names(&f.0).len(), 1 + KEPT_BACKUPS, "{:?}", names(&f.0));
    }
    #[test]
    fn a_failed_attempt_removes_only_what_it_made_itself() {
        let f = Fixture::new();
        let plan = f.plan(Client::ClaudeCode, "{}");
        // Something else already has the name this attempt would give its backup.
        let id = uuid::Uuid::new_v4();
        let taken = f.0.join(format!("client.config.timeschedule-{id}.bak"));
        std::fs::write(&taken, "someone else's file").unwrap();
        assert!(Staged::with_id(&plan, id).is_err());
        assert_eq!(
            std::fs::read_to_string(&taken).unwrap(),
            "someone else's file",
            "a file this attempt did not make was removed"
        );
        assert_eq!(std::fs::read_to_string(&plan.path).unwrap(), "{}");
        assert_eq!(names(&f.0).len(), 2);
    }
    #[test]
    fn without_the_right_to_replace_the_file_nothing_is_written_at_all() {
        // May read and write the file, but not delete it, and may not delete
        // files in its folder either. A working file or backup made here could
        // not be removed again, and every attempt would leave another copy.
        let f = Fixture::new();
        let folder = f.0.join("locked");
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("client.config");
        std::fs::write(&path, "{}").unwrap();
        let plan = plan_for(&path, &f.0, 39281);
        let quoted = |path: &Path| path.to_string_lossy().replace('\'', "''");
        let rule = "[Security.AccessControl.FileSystemAccessRule]::new";
        powershell(&format!(
            "$ErrorActionPreference='Stop'; \
             $me=[Security.Principal.WindowsIdentity]::GetCurrent().User; \
             $d=[IO.Directory]::GetAccessControl('{folder}'); $d.SetAccessRuleProtection($true,$false); \
             $d.AddAccessRule({rule}($me,'Modify','None','None','Allow')); \
             [IO.Directory]::SetAccessControl('{folder}',$d); \
             $a=[IO.File]::GetAccessControl('{file}'); $a.SetAccessRuleProtection($true,$false); \
             $a.AddAccessRule({rule}($me,'Read,Write','Allow')); \
             [IO.File]::SetAccessControl('{file}',$a)",
            folder = quoted(&folder),
            file = quoted(&path),
        ));
        let result = apply(&plan);
        let left = names(&folder);
        // Give the fixture back before judging, so it can be removed.
        powershell(&format!(
            "$ErrorActionPreference='Stop'; \
             $me=[Security.Principal.WindowsIdentity]::GetCurrent().User; \
             $d=[IO.Directory]::GetAccessControl('{folder}'); \
             $d.AddAccessRule({rule}($me,'FullControl','None','None','Allow')); \
             [IO.Directory]::SetAccessControl('{folder}',$d); \
             $a=[IO.File]::GetAccessControl('{file}'); \
             $a.AddAccessRule({rule}($me,'FullControl','Allow')); \
             [IO.File]::SetAccessControl('{file}',$a)",
            folder = quoted(&folder),
            file = quoted(&path),
        ));
        let error = result.unwrap_err();
        assert!(error.contains("권한"), "{error}");
        assert_eq!(
            left,
            ["client.config"],
            "a copy was left that cannot be removed"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }
    /// The integrity label of a file: who it stops and how, byte for byte.
    fn label_of(path: &Path) -> Vec<Vec<u8>> {
        let access = access_of(&std::fs::File::open(path).unwrap()).unwrap();
        rule_list(&access, SE_SACL_PRESENT, 12)
            .unwrap()
            .unwrap_or_default()
    }
    #[test]
    fn a_label_taken_from_an_earlier_folder_is_carried_over_exactly() {
        // The file got its label from a labelled folder and was then moved to
        // an ordinary one. It keeps the label, marked as inherited, although
        // nothing here would give a new file one.
        let f = Fixture::new();
        let labelled = f.0.join("labelled folder");
        std::fs::create_dir(&labelled).unwrap();
        tool(
            "icacls.exe",
            &[
                labelled.as_os_str(),
                "/setintegritylevel".as_ref(),
                "(OI)(CI)M".as_ref(),
            ],
        )
        .expect("could not label the fixture folder");
        std::fs::write(labelled.join("client.config"), "{}").unwrap();
        let path = f.0.join("client.config");
        std::fs::rename(labelled.join("client.config"), &path).unwrap();
        let before = label_of(&path);
        assert_eq!(before.len(), 1, "the fixture has no label to lose");
        let backup = apply(&plan_for(&path, &f.0, 39281)).unwrap().unwrap();
        assert_eq!(label_of(&path), before, "the new file's label differs");
        assert_eq!(label_of(&backup), before, "the backup's label differs");
        // A label set on the file itself, compared the same exact way.
        let own = f.0.join("own.json");
        std::fs::write(&own, "{}").unwrap();
        tool(
            "icacls.exe",
            &[own.as_os_str(), "/setintegritylevel".as_ref(), "M".as_ref()],
        )
        .unwrap();
        let before = label_of(&own);
        assert_eq!(before.len(), 1);
        apply(&plan_for(&own, &f.0, 39281)).unwrap().unwrap();
        assert_eq!(label_of(&own), before);
    }
    #[test]
    fn an_encrypted_file_with_the_system_mark_is_still_written_encrypted() {
        // Windows ignores a request for encryption when the system mark is
        // asked for in the same call.
        let f = Fixture::new();
        let path = f.0.join("encrypted.json");
        std::fs::write(&path, "{}").unwrap();
        let attributes = |path: &Path| {
            identity(&std::fs::File::open(path).unwrap())
                .unwrap()
                .attributes
        };
        let quoted = path.to_string_lossy().replace('\'', "''");
        let script = format!(
            "[IO.File]::Encrypt('{quoted}'); [IO.File]::SetAttributes('{quoted}','System')"
        );
        let done = tool(
            "powershell.exe",
            &[
                "-NoProfile".as_ref(),
                "-NonInteractive".as_ref(),
                "-Command".as_ref(),
                script.as_ref(),
            ],
        );
        const ENCRYPTED: u32 = 0x4000;
        const SYSTEM: u32 = 0x4;
        if done.is_none() || attributes(&path) & (ENCRYPTED | SYSTEM) != ENCRYPTED | SYSTEM {
            eprintln!("skipped: this PC cannot encrypt a file");
            return;
        }
        let backup = apply(&plan_for(&path, &f.0, 39281)).unwrap().unwrap();
        assert_ne!(attributes(&path) & ENCRYPTED, 0, "written in the clear");
        assert_ne!(attributes(&backup) & ENCRYPTED, 0);
    }
    #[test]
    fn a_name_rearranged_while_the_replacement_was_prepared_is_not_overwritten() {
        // Another program gives the file a second name. The name still leads to
        // the file that was compared, but swapping in a new one would now split
        // the two names apart.
        let f = Fixture::new();
        let plan = f.plan(Client::ClaudeCode, "{}");
        let staged = staged(&plan);
        let twin = f.0.join("second name");
        std::fs::hard_link(&plan.path, &twin).unwrap();
        let error = staged.publish().unwrap_err();
        assert!(error.contains("바뀌었습니다"), "{error}");
        assert_eq!(std::fs::read_to_string(&plan.path).unwrap(), "{}");
        assert_eq!(std::fs::read_to_string(&twin).unwrap(), "{}");
        assert_eq!(names(&f.0), ["client.config", "second name"]);
    }
    #[test]
    fn a_link_put_in_the_files_place_meanwhile_is_not_replaced_by_a_file() {
        let f = Fixture::new();
        let plan = f.plan(Client::ClaudeCode, "{}");
        let staged = staged(&plan);
        // Another program moves the file away and leaves a link to it.
        let moved = f.0.join("moved");
        std::fs::rename(&plan.path, &moved).unwrap();
        if !linked(&moved, &plan.path) {
            std::fs::rename(&moved, &plan.path).unwrap();
            return;
        }
        let error = staged.publish().unwrap_err();
        assert!(error.contains("바뀌었습니다"), "{error}");
        assert!(std::fs::symlink_metadata(&plan.path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "{}");
        assert_eq!(names(&f.0), ["client.config", "moved"]);
    }
    #[test]
    fn old_backups_that_cannot_be_removed_are_reported_not_passed_over() {
        use std::os::windows::fs::OpenOptionsExt;
        let f = Fixture::new();
        let path = f.0.join("client.config");
        std::fs::write(&path, "{}").unwrap();
        let mut made = Vec::new();
        for port in 40001..40004 {
            made.push(apply(&plan_for(&path, &f.0, port)).unwrap().unwrap());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // Another program holds the oldest backup open, so it cannot be removed.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&made[0])
            .unwrap();
        let applied = register(&plan_for(&path, &f.0, 40004)).unwrap();
        assert!(
            applied.backup.is_some(),
            "the registration itself succeeded"
        );
        assert_eq!(applied.kept_too_many, [made[0].clone()]);
        // The same when nothing has to change.
        let same = register(&plan_for(&path, &f.0, 40004)).unwrap();
        assert_eq!(same.backup, None);
        assert_eq!(same.kept_too_many, [made[0].clone()]);
        drop(held);
        let tidy = register(&plan_for(&path, &f.0, 40004)).unwrap();
        assert!(tidy.kept_too_many.is_empty());
        assert!(!made[0].exists());
    }
    #[test]
    fn a_new_configuration_gets_one_name_and_never_replaces_a_file_made_meanwhile() {
        let f = Fixture::new();
        let path = f.0.join("fresh.json");
        let plan = plan_for(&path, &f.0, 39281);
        assert_eq!(register(&plan).unwrap().backup, None);
        // One name only: a second one left behind would make every later
        // registration refuse the file as hard-linked.
        assert_eq!(
            identity(&std::fs::File::open(&path).unwrap())
                .unwrap()
                .links,
            1
        );
        assert_eq!(names(&f.0), ["fresh.json"]);
        assert!(apply(&plan_for(&path, &f.0, 40001)).unwrap().is_some());
        // Another program creates the file between the preview and the registration.
        let raced = f.0.join("raced.json");
        let plan = plan_for(&raced, &f.0, 39281);
        std::fs::write(&raced, r#"{"theirs":true}"#).unwrap();
        assert!(register(&plan).is_err());
        assert_eq!(
            std::fs::read_to_string(&raced).unwrap(),
            r#"{"theirs":true}"#
        );
        assert_eq!(
            names(&f.0)
                .iter()
                .filter(|name| name.contains("raced") || name.starts_with(".timeschedule-"))
                .count(),
            1,
            "a working file was left: {:?}",
            names(&f.0)
        );
    }
    #[test]
    fn a_folder_whose_contents_cannot_be_listed_gets_no_copies() {
        // Files can be made here but not listed, so backups could neither be
        // counted nor removed and would pile up with every registration.
        let f = Fixture::new();
        let folder = f.0.join("unlistable");
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("client.config");
        std::fs::write(&path, "{}").unwrap();
        let plan = plan_for(&path, &f.0, 39281);
        let quoted = |path: &Path| path.to_string_lossy().replace('\'', "''");
        let rule = "[Security.AccessControl.FileSystemAccessRule]::new";
        powershell(&format!(
            "$ErrorActionPreference='Stop'; \
             $me=[Security.Principal.WindowsIdentity]::GetCurrent().User; \
             $a=[IO.File]::GetAccessControl('{file}'); $a.SetAccessRuleProtection($true,$false); \
             $a.AddAccessRule({rule}($me,'FullControl','Allow')); \
             [IO.File]::SetAccessControl('{file}',$a); \
             $d=[IO.Directory]::GetAccessControl('{folder}'); $d.SetAccessRuleProtection($true,$false); \
             $d.AddAccessRule({rule}($me,'CreateFiles,Traverse,ReadAttributes,ReadPermissions,DeleteSubdirectoriesAndFiles','None','None','Allow')); \
             [IO.Directory]::SetAccessControl('{folder}',$d)",
            folder = quoted(&folder),
            file = quoted(&path),
        ));
        assert!(
            std::fs::read_dir(&folder).is_err(),
            "the fixture is listable"
        );
        let result = register(&plan);
        powershell(&format!(
            "$ErrorActionPreference='Stop'; \
             $me=[Security.Principal.WindowsIdentity]::GetCurrent().User; \
             $d=[IO.Directory]::GetAccessControl('{folder}'); \
             $d.AddAccessRule({rule}($me,'FullControl','None','None','Allow')); \
             [IO.Directory]::SetAccessControl('{folder}',$d)",
            folder = quoted(&folder),
        ));
        let error = result.unwrap_err();
        assert!(error.contains("목록"), "{error}");
        assert_eq!(names(&folder), ["client.config"]);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }
    #[test]
    fn a_link_pointed_elsewhere_meanwhile_is_not_reported_as_registered() {
        // The user keeps the configuration behind a link. While the replacement
        // is prepared, the link is pointed at another file: the AI app now
        // reads that one, and replacing the old target would register nothing.
        let f = Fixture::new();
        let (first, second) = (f.0.join("first.json"), f.0.join("second.json"));
        std::fs::write(&first, "{}").unwrap();
        std::fs::write(&second, r#"{"second":true}"#).unwrap();
        let link = f.0.join("client.config");
        if !linked(&first, &link) {
            return;
        }
        let ready = staged(&plan_for(&link, &f.0, 39281));
        std::fs::remove_file(&link).unwrap();
        std::os::windows::fs::symlink_file(&second, &link).unwrap();
        let error = ready.publish().unwrap_err();
        assert!(error.contains("바뀌었습니다"), "{error}");
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "{}");
        assert_eq!(
            std::fs::read_to_string(&second).unwrap(),
            r#"{"second":true}"#
        );
    }
    #[test]
    fn rules_tightened_while_the_replacement_was_prepared_are_not_undone() {
        // The replacement and the backup were made with the rules the file had
        // a moment ago. The user has since closed the file to everyone else.
        let f = Fixture::new();
        let plan = f.plan(Client::ClaudeCode, "{}");
        let ready = staged(&plan);
        let quoted = plan.path.to_string_lossy().replace('\'', "''");
        powershell(&format!(
            "$ErrorActionPreference='Stop'; \
             $me=[Security.Principal.WindowsIdentity]::GetCurrent().User; \
             $a=[Security.AccessControl.FileSecurity]::new(); $a.SetAccessRuleProtection($true,$false); \
             $a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($me,'FullControl','Allow')); \
             [IO.FileInfo]::new('{quoted}').SetAccessControl($a)"
        ));
        let error = ready.publish().unwrap_err();
        assert!(error.contains("바뀌었습니다"), "{error}");
        assert_eq!(std::fs::read_to_string(&plan.path).unwrap(), "{}");
        assert_eq!(names(&f.0), ["client.config"], "a wider copy was left");
    }
    #[test]
    fn backups_beside_the_link_and_beside_the_real_file_count_together() {
        // Earlier versions put the backup beside the link, this one beside the
        // file the link leads to.
        let f = Fixture::new();
        let elsewhere = f.0.join("dotfiles");
        std::fs::create_dir(&elsewhere).unwrap();
        let (link, real) = (f.0.join(".claude.json"), elsewhere.join("claude.json"));
        let mut all = Vec::new();
        for folder_and_name in [&link, &real, &link, &real, &link] {
            let backup = folder_and_name.with_file_name(format!(
                "{}.timeschedule-{}.bak",
                folder_and_name.file_name().unwrap().to_string_lossy(),
                uuid::Uuid::new_v4()
            ));
            std::fs::write(&backup, "{}").unwrap();
            all.push(backup);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(backups(&[&real, &link]), {
            let mut newest_first = all.clone();
            newest_first.reverse();
            newest_first
        });
        assert!(prune_backups(&[&real, &link]).is_empty());
        let left: Vec<bool> = all.iter().map(|path| path.exists()).collect();
        assert_eq!(left, [false, false, true, true, true]);
        // The same path given twice is one location.
        assert_eq!(backups(&[&real, &real]).len(), 1);
    }
    #[test]
    fn an_unlistable_folder_is_reported_even_when_nothing_has_to_change() {
        let f = Fixture::new();
        let folder = f.0.join("unlistable");
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("client.config");
        std::fs::write(&path, "{}").unwrap();
        apply(&plan_for(&path, &f.0, 39281)).unwrap().unwrap();
        let same = plan_for(&path, &f.0, 39281);
        assert!(same.unchanged());
        let quoted = folder.to_string_lossy().replace('\'', "''");
        let rule = "[Security.AccessControl.FileSystemAccessRule]::new";
        powershell(&format!(
            "$ErrorActionPreference='Stop'; \
             $me=[Security.Principal.WindowsIdentity]::GetCurrent().User; \
             $d=[IO.Directory]::GetAccessControl('{quoted}'); $d.SetAccessRuleProtection($true,$true); \
             $d.AddAccessRule({rule}($me,'ListDirectory','None','None','Deny')); \
             [IO.Directory]::SetAccessControl('{quoted}',$d)"
        ));
        assert!(
            std::fs::read_dir(&folder).is_err(),
            "the fixture is listable"
        );
        let result = register(&same);
        powershell(&format!(
            "$ErrorActionPreference='Stop'; \
             $me=[Security.Principal.WindowsIdentity]::GetCurrent().User; \
             $d=[IO.Directory]::GetAccessControl('{quoted}'); \
             [void]$d.RemoveAccessRule({rule}($me,'ListDirectory','None','None','Deny')); \
             [IO.Directory]::SetAccessControl('{quoted}',$d)"
        ));
        let error = result.unwrap_err();
        assert!(error.contains("목록"), "{error}");
    }
    #[test]
    fn a_working_file_left_by_an_interrupted_first_registration_is_removed() {
        let f = Fixture::new();
        let path = f.0.join("fresh.json");
        // The PC stopped while the very first registration was writing.
        let stray = f.0.join(format!(
            "fresh.json.timeschedule-{}.tmp",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&stray, "{").unwrap();
        register(&plan_for(&path, &f.0, 39281)).unwrap();
        assert_eq!(names(&f.0), ["fresh.json"], "the working file was left");
    }
    #[test]
    fn registering_a_configuration_that_is_gone_still_bounds_its_old_backups() {
        // The AI app reset its settings: the configuration is gone, the
        // backups earlier registrations made are not.
        let f = Fixture::new();
        let path = f.0.join("client.config");
        for _ in 0..5 {
            std::fs::write(
                f.0.join(format!(
                    "client.config.timeschedule-{}.bak",
                    uuid::Uuid::new_v4()
                )),
                "{}",
            )
            .unwrap();
        }
        let applied = register(&plan_for(&path, &f.0, 39281)).unwrap();
        assert_eq!(applied.backup, None);
        assert!(applied.kept_too_many.is_empty());
        assert_eq!(names(&f.0).len(), 1 + KEPT_BACKUPS, "{:?}", names(&f.0));
    }
    #[test]
    fn backups_beside_a_link_whose_folder_cannot_be_listed_are_not_passed_over() {
        let f = Fixture::new();
        let (home, elsewhere) = (f.0.join("home"), f.0.join("dotfiles"));
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&elsewhere).unwrap();
        let real = elsewhere.join("claude.json");
        std::fs::write(&real, "{}").unwrap();
        let link = home.join("client.config");
        if !linked(&real, &link) {
            return;
        }
        let plan = plan_for(&link, &f.0, 39281);
        let quoted = home.to_string_lossy().replace('\'', "''");
        let rule = "[Security.AccessControl.FileSystemAccessRule]::new";
        let deny = |verb: &str| {
            powershell(&format!(
                "$ErrorActionPreference='Stop'; \
                 $me=[Security.Principal.WindowsIdentity]::GetCurrent().User; \
                 $d=[IO.Directory]::GetAccessControl('{quoted}'); \
                 [void]$d.{verb}({rule}($me,'ListDirectory','None','None','Deny')); \
                 [IO.Directory]::SetAccessControl('{quoted}',$d)"
            ));
        };
        deny("AddAccessRule");
        let result = register(&plan);
        deny("RemoveAccessRule");
        let error = result.unwrap_err();
        assert!(error.contains("목록"), "{error}");
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "{}");
        assert_eq!(names(&elsewhere), ["claude.json"]);
    }
    #[test]
    fn one_file_has_one_set_of_backups_whatever_capitals_its_path_is_typed_with() {
        // Windows finds `CLIENT.CONFIG` and `client.config` to be one file, so
        // a path typed with other capitals must not start a second set.
        let f = Fixture::new();
        std::fs::write(f.0.join("client.config"), "{}").unwrap();
        let spellings = ["CLIENT.CONFIG", "client.config", "Client.Config"];
        for (round, port) in (40001..40007).enumerate() {
            let typed = f.0.join(spellings[round % spellings.len()]);
            let backup = apply(&plan_for(&typed, &f.0, port)).unwrap().unwrap();
            assert!(
                backup
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("client.config.timeschedule-"),
                "not named after the file as stored: {}",
                backup.display()
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(names(&f.0).len(), 1 + KEPT_BACKUPS, "{:?}", names(&f.0));
        // A backup that only resembles this file's in other capitals is left
        // alone: in a folder with case-sensitive names it is another file's.
        let other = f.0.join(format!(
            "Client.Config.timeschedule-{}.bak",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&other, "another file's backup").unwrap();
        apply(&plan_for(&f.0.join("client.config"), &f.0, 40010))
            .unwrap()
            .unwrap();
        assert!(other.exists());
    }
    #[test]
    fn a_name_that_no_longer_leads_to_the_compared_file_is_noticed() {
        let f = Fixture::new();
        let path = f.0.join("client.config");
        std::fs::write(&path, "{}").unwrap();
        let compared = identity(&std::fs::File::open(&path).unwrap()).unwrap();
        assert!(still_there(&path, None, &compared));
        // An AI app saves a version of its own by renaming it over the file.
        let theirs = f.0.join("their working file");
        std::fs::write(&theirs, r#"{"theirs":true}"#).unwrap();
        std::fs::rename(&theirs, &path).unwrap();
        assert!(!still_there(&path, None, &compared));
    }
}
