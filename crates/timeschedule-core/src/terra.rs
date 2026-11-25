use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

pub const MODEL: &str = "gpt-6.1-terra";

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

/// One model call through `codex exec`.
pub struct Call<'a> {
    pub model: &'a str,
    /// `model_reasoning_effort`: `low`, `medium` or `high`.
    pub effort: &'a str,
    /// A JSON Schema the answer must follow (`--output-schema`).
    pub schema: Option<&'a serde_json::Value>,
    /// Bounds the whole call: the CLI, and the wait for its pipes after it ends.
    pub timeout: Duration,
    /// Bytes of answer accepted.
    pub answer_limit: u64,
    /// Whether the usage is read from the CLI's output (`--json`). Reading it takes a
    /// thread, and that thread outlives the call for as long as a process the CLI
    /// started holds the output pipe. Without it the output is discarded, no thread
    /// reads it, and the reply's usage is empty.
    pub read_usage: bool,
    /// Whether a Markdown `json` fence around the answer is taken off. Without it the
    /// answer is returned as the CLI wrote it.
    pub unfence: bool,
}

/// What Codex reported the call used (`turn.completed`). A counter Codex did not
/// report is `None`, which is not the same as a reported 0.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub text: String,
    pub usage: Usage,
    pub seconds: f64,
}

/// A call that gave no usable answer. It still says how long it took and what it
/// used, as far as Codex reported before it failed.
#[derive(Debug, Clone)]
pub struct Failed {
    pub message: String,
    pub usage: Usage,
    pub seconds: f64,
}

impl Failed {
    /// A failure with no usage known.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            usage: Usage::default(),
            seconds: 0.0,
        }
    }
}

/// Bytes kept of one line of Codex's `--json` output. A `turn.completed` line is a few
/// hundred bytes; a longer line is an event that holds text, and is not kept.
const EVENT_LINE_LIMIT: usize = 64 * 1024;
/// How long the pipes are waited for after the CLI ends. They end with it unless a
/// process it started still holds them.
const PIPE_GRACE: Duration = Duration::from_secs(2);

/// Reads Codex's `--json` output as it comes, keeping the usage of the last
/// `turn.completed` event and at most `EVENT_LINE_LIMIT` bytes of the line being read.
#[derive(Default)]
struct Events {
    line: Vec<u8>,
    /// The line being read is over the limit: the rest of it is skipped.
    over: bool,
    usage: Usage,
}

impl Events {
    fn feed(&mut self, mut chunk: &[u8]) {
        while !chunk.is_empty() {
            let end = chunk.iter().position(|b| *b == b'\n');
            let part = &chunk[..end.unwrap_or(chunk.len())];
            if !self.over {
                if self.line.len() + part.len() > EVENT_LINE_LIMIT {
                    self.over = true;
                    self.line.clear();
                } else {
                    self.line.extend_from_slice(part);
                }
            }
            let Some(end) = end else { break };
            self.end_line();
            chunk = &chunk[end + 1..];
        }
    }

    fn end_line(&mut self) {
        if !self.over {
            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(&self.line) {
                if event["type"] == "turn.completed" {
                    let n = |key: &str| event["usage"][key].as_u64();
                    self.usage = Usage {
                        input_tokens: n("input_tokens"),
                        cached_input_tokens: n("cached_input_tokens"),
                        output_tokens: n("output_tokens"),
                        reasoning_output_tokens: n("reasoning_output_tokens"),
                    };
                }
            }
        }
        self.line.clear();
        self.over = false;
    }
}

fn strip_fence(body: &str) -> String {
    let body = body.trim();
    body.strip_prefix("```json")
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(body)
        .trim()
        .to_owned()
}

/// Sends `prompt` to the model with tools, web search, plugins and MCP off, in an
/// empty temporary folder and a read-only sandbox, and returns its last message.
pub fn run(prompt: &str, call: &Call) -> Result<Reply, Failed> {
    let started = Instant::now();
    let seen = Arc::new(Mutex::new(Usage::default()));
    let result = exec(prompt, call, &seen);
    let usage = seen.lock().map(|u| u.clone()).unwrap_or_default();
    let seconds = started.elapsed().as_secs_f64();
    match result {
        Ok(text) => Ok(Reply {
            text,
            usage,
            seconds,
        }),
        Err(message) => Err(Failed {
            message,
            usage,
            seconds,
        }),
    }
}

fn exec(prompt: &str, call: &Call, seen: &Arc<Mutex<Usage>>) -> Result<String, String> {
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
        let effort = format!("model_reasoning_effort=\"{}\"", call.effort);
        cmd.current_dir(&dir).arg("exec");
        if call.read_usage {
            cmd.arg("--json");
        }
        cmd.args([
            "--ignore-user-config",
            "--ephemeral",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "-m",
            call.model,
            "-c",
            &effort,
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
        ]);
        if let Some(schema) = call.schema {
            let path = dir.join("schema.json");
            fs::write(&path, schema.to_string()).map_err(|e| e.to_string())?;
            cmd.arg("--output-schema").arg(path);
        }
        cmd.arg("--output-last-message").arg(&answer).arg("-");
        let seen = call.read_usage.then_some(seen);
        drive(cmd, prompt, call.model, call.timeout, seen)?;
        let body = read_answer(&answer, call.answer_limit)?;
        Ok(if call.unfence {
            strip_fence(&body)
        } else {
            body
        })
    })();
    let _ = fs::remove_dir_all(dir);
    result
}

/// Runs `cmd` with `prompt` on its stdin, and returns within `timeout` and a moment.
/// With `seen`, the usage in its stdout is read into it; without, stdout is discarded.
/// Threads of their own serve the pipes, so this can time out even if the program
/// never reads stdin or fills stdout. A thread whose pipe is still held by a process
/// the program started is left behind: it ends when that process does.
fn drive(
    mut cmd: Command,
    prompt: &str,
    name: &str,
    timeout: Duration,
    seen: Option<&Arc<Mutex<Usage>>>,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(if seen.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Codex CLI를 시작하지 못했습니다. {e}"))?;
    let mut input = child.stdin.take().expect("piped stdin");
    let bytes = prompt.as_bytes().to_owned();
    let (wrote, written) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = wrote.send(input.write_all(&bytes));
    });
    let drained = seen.map(|seen| {
        let mut output = child.stdout.take().expect("piped stdout");
        let (done, drained) = mpsc::channel();
        let shared = Arc::clone(seen);
        std::thread::spawn(move || {
            let mut events = Events::default();
            let mut chunk = [0_u8; 8192];
            while let Ok(read @ 1..) = output.read(&mut chunk) {
                events.feed(&chunk[..read]);
                if let Ok(mut usage) = shared.lock() {
                    usage.clone_from(&events.usage);
                }
            }
            let _ = done.send(());
        });
        drained
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            other => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match other {
                    Err(e) => format!("Codex 실행 상태를 확인할 수 없습니다. {e}"),
                    _ => format!(
                        "{name} 응답 대기 시간({}초)을 초과했습니다.",
                        timeout.as_secs()
                    ),
                });
            }
        }
    };
    let until = deadline.min(Instant::now() + PIPE_GRACE);
    let left = || until.saturating_duration_since(Instant::now());
    // The usage read so far stands whether or not the output ends in time.
    if let Some(drained) = drained {
        let _ = drained.recv_timeout(left());
    }
    if !status.success() {
        return Err(format!(
            "{name} 호출 실패({status}). Codex CLI 버전, 로그인 및 네트워크를 확인해 주세요."
        ));
    }
    match written.recv_timeout(left()) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(format!("Codex 입력 전달 실패. {e}")),
        Err(_) => Err("Codex 입력 전달이 끝나지 않았습니다.".into()),
    }
}

fn read_answer(path: &Path, limit: u64) -> Result<String, String> {
    let mut body = String::new();
    fs::File::open(path)
        .and_then(|f| f.take(limit + 1).read_to_string(&mut body))
        .map_err(|e| format!("모델 응답을 읽지 못했습니다. {e}"))?;
    if body.len() as u64 > limit {
        return Err("모델 응답 크기를 초과했습니다.".into());
    }
    Ok(body)
}

/// The call the chat review and the portfolio writer make: Terra at medium effort,
/// no schema, three minutes, an answer of at most 64 KiB with its fence taken off.
/// The CLI's output is not read.
pub fn generate(prompt: &str) -> Result<String, String> {
    run(
        prompt,
        &Call {
            model: MODEL,
            effort: "medium",
            schema: None,
            timeout: Duration::from_secs(180),
            answer_limit: 65536,
            read_usage: false,
            unfence: true,
        },
    )
    .map(|reply| reply.text)
    .map_err(|failed| failed.message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fed(chunks: &[&[u8]]) -> Events {
        let mut events = Events::default();
        for chunk in chunks {
            events.feed(chunk);
        }
        events
    }

    #[test]
    fn usage_is_read_from_the_last_completed_turn() {
        let events = concat!(
            "{\"type\":\"thread.started\"}\n",
            "not json\n",
            "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}\n",
            "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":11485,\"cached_input_tokens\":8960,\"output_tokens\":19,\"reasoning_output_tokens\":3}}\n",
        );
        // The same whether it comes whole or a few bytes at a time.
        let pieces: Vec<&[u8]> = events.as_bytes().chunks(7).collect();
        for chunks in [vec![events.as_bytes()], pieces] {
            assert_eq!(
                fed(&chunks).usage,
                Usage {
                    input_tokens: Some(11485),
                    cached_input_tokens: Some(8960),
                    output_tokens: Some(19),
                    reasoning_output_tokens: Some(3),
                }
            );
        }
        assert_eq!(
            fed(&[b"{\"type\":\"turn.started\"}\n"]).usage,
            Usage::default()
        );
    }

    #[test]
    fn a_counter_codex_did_not_report_is_not_zero() {
        let usage = fed(&[
            b"{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":0,\"output_tokens\":\"7\"}}\n",
        ])
        .usage;
        assert_eq!(
            usage,
            Usage {
                input_tokens: Some(0),
                ..Usage::default()
            }
        );
        assert_eq!(
            serde_json::to_value(&usage).unwrap(),
            serde_json::json!({"inputTokens": 0, "cachedInputTokens": null,
                "outputTokens": null, "reasoningOutputTokens": null})
        );
    }

    #[test]
    fn a_long_event_is_skipped_without_being_kept() {
        let mut events = Events::default();
        events.feed(b"{\"type\":\"item.completed\",\"text\":\"");
        for _ in 0..1024 {
            events.feed(&[b'x'; 8192]);
            assert!(events.line.len() <= EVENT_LINE_LIMIT);
        }
        assert!(events.line.capacity() <= 2 * EVENT_LINE_LIMIT);
        events.feed(b"\"}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":5}}\n");
        assert_eq!(events.usage.input_tokens, Some(5));
    }

    #[test]
    fn a_fenced_answer_is_unwrapped() {
        assert_eq!(strip_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_fence(" {\"a\":1} "), "{\"a\":1}");
    }

    /// `cmd` ends at once and leaves `ping` running with the pipes it inherited.
    #[cfg(windows)]
    fn leaving_a_process_behind() -> Command {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "/B", "ping", "-n", "20", "127.0.0.1"]);
        cmd
    }

    #[cfg(windows)]
    #[test]
    fn a_call_ends_though_a_process_it_started_keeps_the_pipe_open() {
        let started = Instant::now();
        let seen = Arc::new(Mutex::new(Usage::default()));
        let cmd = leaving_a_process_behind();
        let result = drive(cmd, "prompt", "test", Duration::from_secs(60), Some(&seen));
        assert_eq!(result, Ok(()));
        let took = started.elapsed();
        // `cmd` itself ends in a fraction of a second, and the output would end with
        // it if nothing else held the pipe. The call lasting the whole grace shows the
        // output had not ended: `ping` held the pipe when the call gave up waiting.
        assert!(took >= PIPE_GRACE, "nothing held the pipe: {took:?}");
        assert!(took < Duration::from_secs(10), "{took:?}");
    }

    /// The same program, which with a reader takes the whole grace, takes no grace.
    #[cfg(windows)]
    #[test]
    fn a_call_that_reads_no_usage_waits_for_no_pipe() {
        let started = Instant::now();
        let cmd = leaving_a_process_behind();
        let result = drive(cmd, "prompt", "test", Duration::from_secs(60), None);
        assert_eq!(result, Ok(()));
        let took = started.elapsed();
        assert!(took < PIPE_GRACE, "{took:?}");
    }

    #[test]
    fn an_answer_is_read_as_it_was_written() {
        let path = env::temp_dir().join(format!("timeschedule-answer-{}", uuid::Uuid::new_v4()));
        let fenced = " ```json\n{\"a\":1}\n```\n";
        fs::write(&path, fenced).unwrap();
        let read = read_answer(&path, 64);
        let over = read_answer(&path, 8);
        fs::remove_file(&path).unwrap();
        assert_eq!(read.as_deref(), Ok(fenced));
        assert!(over.unwrap_err().contains("크기"));
    }

    #[cfg(windows)]
    #[test]
    fn a_call_that_outlives_its_timeout_is_stopped() {
        let mut cmd = Command::new("ping");
        cmd.args(["-n", "30", "127.0.0.1"]);
        let started = Instant::now();
        let seen = Arc::new(Mutex::new(Usage::default()));
        let result = drive(cmd, "prompt", "test", Duration::from_secs(1), Some(&seen));
        assert!(result.unwrap_err().contains("초과"));
        let took = started.elapsed();
        assert!(took < Duration::from_secs(10), "{took:?}");
    }
}
