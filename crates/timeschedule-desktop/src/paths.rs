use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

/// The data folder, made if it is missing.
pub fn resolve_data_dir(
    override_path: Option<PathBuf>,
    experimental: bool,
) -> Result<PathBuf, String> {
    let path = data_dir(override_path, experimental)?;
    fs::create_dir_all(&path).map_err(|e| format!("데이터 폴더를 만들지 못했습니다: {e}"))?;
    fs::canonicalize(path).map_err(|e| e.to_string())
}

/// Where the data folder is. Nothing is made, so a command that only reads can use it.
pub fn data_dir(override_path: Option<PathBuf>, experimental: bool) -> Result<PathBuf, String> {
    let path = override_path
        .or_else(|| std::env::var_os("TIMESCHEDULE_DATA_DIR").map(PathBuf::from))
        .unwrap_or_else(|| {
            let base = std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("timeschedule"));
            base.join(if experimental {
                "com.timeschedule.personal.native-experiment"
            } else {
                "com.timeschedule.personal.native"
            })
        });
    if path.as_os_str().is_empty() {
        return Err("데이터 폴더 경로가 비어 있습니다.".into());
    }
    Ok(path)
}

/// Keep this guard alive until all storage workers have stopped. On Windows,
/// share_mode(0) denies concurrent handles, including other processes; the OS
/// releases the lock on crash. The persistent empty lock file is harmless.
pub struct InstanceLock {
    _file: File,
}
impl InstanceLock {
    pub fn acquire(directory: &Path) -> Result<Self, String> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        #[cfg(not(windows))]
        {
            return Err("현재 단일 실행 잠금은 Windows에서 지원합니다.".into());
        }
        #[cfg(windows)]
        {
            let file=options.open(directory.join("native-instance.lock")).map_err(|e|format!("같은 데이터 폴더의 앱이 이미 실행 중이거나 잠금 파일을 열 수 없습니다: {e}"))?;
            Ok(Self { _file: file })
        }
    }
}
#[cfg(all(windows, test))]
mod tests {
    use super::*;
    #[test]
    fn directory_lock_excludes_duplicate_and_releases() {
        let dir =
            std::env::temp_dir().join(format!("timeschedule-lock-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let lock = InstanceLock::acquire(&dir).unwrap();
        assert!(InstanceLock::acquire(&dir).is_err());
        drop(lock);
        let again = InstanceLock::acquire(&dir).unwrap();
        drop(again);
        fs::remove_file(dir.join("native-instance.lock")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
