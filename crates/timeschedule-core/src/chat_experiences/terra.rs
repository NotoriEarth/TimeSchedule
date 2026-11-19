use std::{
    env, fs,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use super::model::MODEL;

fn executable() -> Result<PathBuf, String> {
    let name = if cfg!(windows) { "codex.exe" } else { "codex" };
    if let Some(path) = env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    }) {
        return Ok(path);
    }
    // A GUI launched from Explorer may not inherit the desktop CLI's versioned PATH.
    #[cfg(windows)]
    if let Some(root) = env::var_os("LOCALAPPDATA") {
        let bin = PathBuf::from(root).join("OpenAI/Codex/bin");
        if let Ok(entries) = fs::read_dir(bin) {
            let latest = entries
                .flatten()
                .map(|entry| entry.path().join(name))
                .filter_map(|path| Some((path.metadata().ok()?.modified().ok()?, path)))
                .max();
            if let Some((_, path)) = latest {
                return Ok(path);
            }
        }
    }
    Err("Codex CLI를 찾을 수 없습니다. Codex를 설치하고 로그인해 주세요.".into())
}

pub(super) fn infer(request: &serde_json::Value) -> Result<String, String> {
    let prompt = request["messages"][0]["content"]
        .as_str()
        .ok_or("Terra 입력이 비었습니다.")?;
    let mut cmd = Command::new(executable()?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let dir = env::temp_dir().join(format!("timeschedule-terra-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let answer = dir.join("answer.json");
    let result = (|| {
        let mut child = cmd
            .current_dir(&dir)
            .args([
                "exec",
                "--ignore-user-config",
                "--ephemeral",
                "--skip-git-repo-check",
                "--sandbox",
                "read-only",
                "-m",
                MODEL,
                "-c",
                "model_reasoning_effort=\"medium\"",
                "-c",
                "web_search=\"disabled\"",
                "-c",
                "mcp_servers={}",
                "--disable",
                "shell_tool",
                "--disable",
                "plugins",
                "--disable",
                "apps",
                "--disable",
                "multi_agent",
                "--output-last-message",
            ])
            .arg(&answer)
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Codex CLI를 시작하지 못했습니다. {e}"))?;
        let mut input = child.stdin.take().expect("piped stdin");
        let bytes = prompt.as_bytes().to_owned();
        // The UI worker must be able to time out even if the CLI never reads stdin.
        let writer = std::thread::spawn(move || input.write_all(&bytes));
        let deadline = Instant::now() + Duration::from_secs(180);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100))
                }
                other => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(match other {
                        Err(e) => format!("Codex 실행 상태를 확인할 수 없습니다. {e}"),
                        _ => "Terra 응답 대기 시간(180초)을 초과했습니다.".into(),
                    });
                }
            }
        };
        if !status.success() {
            return Err(format!(
                "Terra 호출 실패({status}). Codex CLI 버전, 로그인 및 네트워크를 확인해 주세요."
            ));
        }
        writer
            .join()
            .map_err(|_| "Codex 입력 전달 실패")?
            .map_err(|e| e.to_string())?;
        let mut body = String::new();
        fs::File::open(&answer)
            .and_then(|f| f.take(65537).read_to_string(&mut body))
            .map_err(|e| format!("Terra 응답을 읽지 못했습니다. {e}"))?;
        if body.len() > 65536 {
            return Err("Terra 응답 크기를 초과했습니다.".into());
        }
        let body = body.trim();
        Ok(body
            .strip_prefix("```json")
            .and_then(|s| s.strip_suffix("```"))
            .unwrap_or(body)
            .trim()
            .to_owned())
    })();
    let _ = fs::remove_file(answer);
    let _ = fs::remove_dir(dir);
    result
}
