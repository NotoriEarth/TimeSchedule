use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    node: PathBuf,
    script: PathBuf,
    state_dir: PathBuf,
}

fn read_small(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(16385)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 16384 {
        return Err("재시도 설정 크기가 너무 큽니다.".into());
    }
    Ok(bytes)
}
impl Config {
    fn validate(&self) -> Result<(), String> {
        if !self.node.is_absolute()
            || !self.script.is_absolute()
            || !self.state_dir.is_absolute()
            || !self.node.is_file()
            || !self.script.is_file()
            || !self.state_dir.is_dir()
            || self.script.file_name().and_then(|v| v.to_str()) != Some("project-records.mjs")
        {
            return Err("등록된 Node·기록 스크립트·상태 폴더를 확인해 주세요.".into());
        }
        Ok(())
    }
}
pub fn register(dir: &Path, node: &Path, script: &Path, state_dir: &Path) -> Result<(), String> {
    let config = Config {
        node: node.into(),
        script: script.into(),
        state_dir: state_dir.into(),
    };
    config.validate()?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = dir.join("project-record-sync.json");
    let bytes = serde_json::to_vec(&config).map_err(|e| e.to_string())?;
    if fs::read(&file).ok().as_deref() == Some(bytes.as_slice()) {
        return Ok(());
    }
    let temp = dir.join(format!(".record-sync-{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    let result = fs::rename(&temp, &file).map_err(|e| e.to_string());
    let _ = fs::remove_file(temp);
    result
}

// The job owns only this retry process tree, including its Codex child. Closing
// the app or hitting the deadline cannot leave another publisher running.
#[cfg(windows)]
struct Job(isize);
#[cfg(windows)]
impl Job {
    fn attach(child: &Child) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle()) == 0
            {
                let error = std::io::Error::last_os_error().to_string();
                CloseHandle(handle);
                return Err(error);
            }
            Ok(Self(handle as isize))
        }
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0 as _);
        }
    }
}
pub struct Runner {
    child: Option<Child>,
    started: Instant,
    next_check: Instant,
    #[cfg(windows)]
    job: Option<Job>,
}
impl Default for Runner {
    fn default() -> Self {
        Self {
            child: None,
            started: Instant::now(),
            next_check: Instant::now(),
            #[cfg(windows)]
            job: None,
        }
    }
}
impl Runner {
    fn stop(&mut self) {
        #[cfg(windows)]
        {
            self.job.take();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    pub fn poll(&mut self, dir: &Path) -> Option<String> {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.stop();
                    return Some(format!("대기 기록 재시도 종료: {status}"));
                }
                Ok(None) if self.started.elapsed() < Duration::from_secs(240) => return None,
                _ => {
                    self.stop();
                    return Some("대기 기록 재시도 제한 시간 또는 실행 오류. 원문과 전달 상태를 보존했습니다.".into());
                }
            }
        }
        if dir.as_os_str().is_empty() || Instant::now() < self.next_check {
            return None;
        }
        self.next_check = Instant::now() + Duration::from_secs(60);
        let file = dir.join("project-record-sync.json");
        if !file.exists() {
            return None;
        }
        let start = (|| -> Result<bool, String> {
            let config: Config =
                serde_json::from_slice(&read_small(&file)?).map_err(|e| e.to_string())?;
            config.validate()?;
            let wake = config.state_dir.join("wake.json");
            if !wake.exists() {
                return Ok(false);
            }
            let wake: serde_json::Value =
                serde_json::from_slice(&read_small(&wake)?).map_err(|e| e.to_string())?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis() as u64;
            if wake["pending"].as_u64().unwrap_or(0) == 0
                || wake["nextAttemptAt"].as_u64().unwrap_or(u64::MAX) > now
            {
                return Ok(false);
            }
            let mut command = Command::new(&config.node);
            command
                .arg(&config.script)
                .args(["sync", "--max-jobs", "1", "--state-dir"])
                .arg(&config.state_dir)
                .arg("--data-dir")
                .arg(dir)
                .arg("--exe")
                .arg(std::env::current_exe().map_err(|e| e.to_string())?)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command.spawn().map_err(|e| e.to_string())?;
            #[cfg(windows)]
            {
                match Job::attach(&child) {
                    Ok(job) => self.job = Some(job),
                    Err(error) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(error);
                    }
                }
            }
            self.child = Some(child);
            self.started = Instant::now();
            Ok(true)
        })();
        match start {
            Ok(true) => Some("대기 기록의 로컬 저장을 재시도합니다. Space는 선택 설정을 따릅니다.".into()),
            Ok(false) => None,
            Err(e) => Some(format!("대기 기록 재시도 실패: {e}")),
        }
    }
}
impl Drop for Runner {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_inbox::tests::TestDir;
    fn node() -> PathBuf {
        PathBuf::from("C:/Program Files/nodejs/node.exe")
    }
    #[test]
    fn retry_runner_starts_only_for_due_work_and_only_one_child() {
        let dir = TestDir::new();
        let state = dir.0.join("state");
        fs::create_dir(&state).unwrap();
        let script = dir.0.join("project-records.mjs");
        fs::write(&script,"import fs from 'node:fs';import path from 'node:path';const dir=process.argv[process.argv.indexOf('--state-dir')+1];fs.appendFileSync(path.join(dir,'calls'),'one\\n');setTimeout(()=>{fs.writeFileSync(path.join(dir,'wake.json'),JSON.stringify({pending:0,nextAttemptAt:null}));},200);").unwrap();
        register(&dir.0, &node(), &script, &state).unwrap();
        fs::write(
            state.join("wake.json"),
            r#"{"pending":1,"nextAttemptAt":9999999999999}"#,
        )
        .unwrap();
        let mut runner = Runner::default();
        assert!(runner.poll(&dir.0).is_none());
        assert!(runner.child.is_none());
        fs::write(
            state.join("wake.json"),
            r#"{"pending":1,"nextAttemptAt":0}"#,
        )
        .unwrap();
        runner.next_check = Instant::now();
        assert!(runner.poll(&dir.0).unwrap().contains("재시도"));
        let started = Instant::now();
        while runner.child.is_some() && started.elapsed() < Duration::from_secs(5) {
            runner.next_check = Instant::now();
            runner.poll(&dir.0);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(runner.child.is_none());
        assert_eq!(fs::read_to_string(state.join("calls")).unwrap(), "one\n");
    }
    #[test]
    fn retry_configuration_rejects_relative_paths() {
        let dir = TestDir::new();
        assert!(register(
            &dir.0,
            Path::new("node.exe"),
            Path::new("project-records.mjs"),
            &dir.0
        )
        .is_err());
    }
}
