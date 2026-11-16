//! Local-only, bounded MCP client used by setup and Claude Desktop's stdio bridge.
use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, Request};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use std::{
    io::{BufRead, Read, Write},
    net::SocketAddr,
    path::Path,
    time::Duration,
};

const PROTOCOL: &str = "2025-11-25";
const MAX_MESSAGE: usize = 128 * 1024;

pub fn endpoint(value: &str) -> Result<SocketAddr, String> {
    let port = value
        .strip_prefix("http://127.0.0.1:")
        .and_then(|s| s.strip_suffix("/mcp"))
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|p| *p != 0)
        .ok_or("MCP 주소는 http://127.0.0.1:포트/mcp 형식이어야 합니다.")?;
    Ok(SocketAddr::from(([127, 0, 0, 1], port)))
}
pub fn current_endpoint(dir: &Path) -> Result<String, String> {
    let file = std::fs::File::open(dir.join("mcp-status.json"))
        .map_err(|_| "TimeSchedule MCP가 꺼져 있습니다. 앱 실행과 MCP 상태를 확인하세요.")?;
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "MCP 상태를 읽지 못했습니다.")?;
    if bytes.len() > 4096 {
        return Err("MCP 상태 파일 크기가 잘못되었습니다.".into());
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "MCP 상태 파일 형식이 잘못되었습니다.")?;
    let address = endpoint(value["endpoint"].as_str().ok_or("MCP 주소가 없습니다.")?)?;
    Ok(format!("http://{address}/mcp"))
}
struct Connection<'a> {
    address: SocketAddr,
    dir: &'a Path,
}
struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
/// One HTTP/1 connection to a loopback listener. Every request sent on it reaches
/// the process that accepted it, whatever happens to the port afterwards.
struct Link {
    sender: hyper::client::conn::http1::SendRequest<Full<Bytes>>,
    _task: AbortOnDrop,
}
impl Link {
    async fn open(address: SocketAddr) -> Result<Self, &'static str> {
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .map_err(|_| "TimeSchedule MCP에 연결하지 못했습니다. 앱 실행 상태를 확인하세요.")?;
        let (sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(socket))
            .await
            .map_err(|_| "MCP 연결 초기화 실패")?;
        Ok(Self {
            sender,
            _task: AbortOnDrop(tokio::spawn(async move {
                let _ = connection.await;
            })),
        })
    }
    /// Status and, for a success, at most `limit` body bytes.
    async fn send(
        &mut self,
        request: Request<Full<Bytes>>,
        limit: usize,
    ) -> Result<(u16, Vec<u8>), &'static str> {
        const NO_RESPONSE: &str =
            "MCP 응답을 받지 못했습니다. 저장 요청이었다면 재시도 전에 결과를 확인하세요.";
        self.sender.ready().await.map_err(|_| NO_RESPONSE)?;
        let response = self
            .sender
            .send_request(request)
            .await
            .map_err(|_| NO_RESPONSE)?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            // The status is the whole answer; never wait on or size-check an error body.
            return Ok((status, Vec::new()));
        }
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        while let Some(frame) = body.frame().await {
            if let Ok(data) = frame.map_err(|_| "MCP 응답 읽기 실패")?.into_data() {
                if bytes.len() + data.len() > limit {
                    return Err("MCP 응답 크기 초과");
                }
                bytes.extend_from_slice(&data);
            }
        }
        Ok((status, bytes))
    }
}
/// A connection whose far end has proven that it holds `token`. A process that
/// merely occupies the port cannot answer, and the token itself is not sent.
/// Callers that send the token themselves must send it on the returned link:
/// a second connection could reach a different process.
async fn proven_link(address: SocketAddr, token: &str) -> Result<Link, String> {
    const UNVERIFIED: &str = "이 주소에서 실행 중인 TimeSchedule을 확인하지 못했습니다. 앱 실행 상태를 확인하세요. 인증 정보는 보내지 않았습니다.";
    let challenge = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let host = address.to_string();
    let expected = crate::mcp::proof(token, &host, &challenge)?;
    let request = Request::get("/proof")
        .header("Host", &host)
        .header(crate::mcp::CHALLENGE_HEADER, &challenge)
        .body(Full::new(Bytes::new()))
        .map_err(|_| UNVERIFIED)?;
    let (link, status, answer) = tokio::time::timeout(Duration::from_secs(3), async {
        let mut link = Link::open(address).await?;
        let (status, answer) = link.send(request, 256).await?;
        Ok::<_, &'static str>((link, status, answer))
    })
    .await
    .map_err(|_| UNVERIFIED)?
    .map_err(|_| UNVERIFIED)?;
    let matches = answer.len() == expected.len()
        && answer
            .iter()
            .zip(expected.as_bytes())
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0;
    if status != 200 || !matches {
        return Err(UNVERIFIED.into());
    }
    Ok(link)
}
/// `proven_link` with the token `read` returns, and that token. The token file
/// can be reissued between reading it and asking for the proof; the server then
/// answers with the new token and the proof fails. So after a failure the token
/// is read once more and, only if it changed, tried once more. Each attempt
/// must prove itself before anything is sent, exactly like the first.
async fn proven(
    address: SocketAddr,
    read: &mut dyn FnMut() -> Result<String, String>,
) -> Result<(Link, String), String> {
    let token = read()?;
    match proven_link(address, &token).await {
        Ok(link) => Ok((link, token)),
        Err(error) => {
            let reread = read()?;
            if reread == token {
                return Err(error);
            }
            Ok((proven_link(address, &reread).await?, reread))
        }
    }
}
impl<'a> Connection<'a> {
    /// The token is read for each request, so a long-lived bridge follows a
    /// reissue without being restarted.
    fn load(dir: &'a Path) -> Result<Self, String> {
        Ok(Self {
            address: endpoint(&current_endpoint(dir)?)?,
            dir,
        })
    }
    async fn post(&self, value: &Value, protocol: &str) -> Result<Option<Value>, String> {
        tokio::time::timeout(Duration::from_secs(12), async {
            // The status file can outlive a crashed app; never trust its port with the
            // token. The request goes out on the very connection that proved itself.
            let (mut link, token) =
                proven(self.address, &mut || crate::mcp::read_token(self.dir)).await?;
            let request = Request::post("/mcp")
                .header("Host", self.address.to_string())
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .header("Accept", "application/json, text/event-stream")
                .header("MCP-Protocol-Version", protocol)
                .header("Connection", "close")
                .body(Full::new(Bytes::from(value.to_string())))
                .map_err(|_| "MCP 요청 생성 실패")?;
            let (status, bytes) = link.send(request, 2 * 1024 * 1024).await?;
            if !(200..300).contains(&status) {
                return Err(match status {
                    401 | 403 => "MCP 인증 실패. TimeSchedule과 같은 Windows 계정인지 확인하세요.",
                    _ => "MCP 요청 실패. 저장 요청이었다면 재시도 전에 결과를 확인하세요.",
                }
                .into());
            }
            if bytes.is_empty() {
                Ok(None)
            } else {
                serde_json::from_slice(&bytes)
                    .map(Some)
                    .map_err(|_| "MCP 응답 형식 오류".into())
            }
        })
        .await
        .map_err(|_| {
            "MCP 응답 시간 초과. 저장 요청이었다면 재시도 전에 결과를 확인하세요.".to_string()
        })?
    }
}
fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "MCP 검사 실행기 생성 실패".into())
}
pub fn probe(dir: &Path) -> Result<String, String> {
    let connection = Connection::load(dir)?;
    runtime()?.block_on(async {
        let initialized = connection.post(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"TimeSchedule connection check","version":env!("CARGO_PKG_VERSION")}}}), PROTOCOL).await?.ok_or("MCP 초기화 응답 없음")?;
        if initialized["result"]["serverInfo"]["name"] != "TimeSchedule" { return Err("TimeSchedule 서버를 확인하지 못했습니다.".into()); }
        let protocol = initialized["result"]["protocolVersion"].as_str().ok_or("MCP 버전 응답 없음")?;
        connection.post(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}), protocol).await?;
        let list = connection.post(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}), protocol).await?.ok_or("MCP 도구 응답 없음")?;
        let tools = list["result"]["tools"].as_array().ok_or("MCP 도구 목록을 확인하지 못했습니다.")?;
        for name in ["calendar_list", "calendar_get", "calendar_create", "calendar_update"] {
            if !tools.iter().any(|t| t["name"] == name) { return Err("필요한 일정 도구가 없습니다.".into()); }
        }
        Ok(format!("서버 연결·인증 정상 · 일정 도구 {}개 확인 (일정 읽기·수정 없음)", tools.len()))
    })
}

pub fn bridge(dir: &Path, mut input: impl BufRead, mut output: impl Write) -> Result<(), String> {
    let runtime = runtime()?;
    let mut protocol = PROTOCOL.to_string();
    loop {
        let mut line = Vec::new();
        let n = input
            .by_ref()
            .take((MAX_MESSAGE + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| "MCP 입력 읽기 실패")?;
        if n == 0 {
            return Ok(());
        }
        if n > MAX_MESSAGE {
            return Err("MCP 입력 크기 초과".into());
        }
        let request: Value =
            serde_json::from_slice(&line).map_err(|_| "MCP JSON 입력 형식 오류")?;
        let response = Connection::load(dir)
            .and_then(|connection| runtime.block_on(connection.post(&request, &protocol)));
        let reply = match response {
            Ok(value) => value,
            Err(message) if request.get("id").is_some() => Some(
                json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":message}}),
            ),
            Err(_) => None,
        };
        if let Some(reply) = reply {
            if request["method"] == "initialize" {
                if let Some(version) = reply["result"]["protocolVersion"].as_str() {
                    protocol = version.into();
                }
            }
            serde_json::to_writer(&mut output, &reply).map_err(|_| "MCP 출력 실패")?;
            output
                .write_all(b"\n")
                .and_then(|_| output.flush())
                .map_err(|_| "MCP 출력 실패")?;
        }
    }
}
/// Authorization header JSON for an MCP client that will send it to `url`. The
/// token is released only after the listener at `url` proves it is this user's
/// running TimeSchedule; a closed app or a port squatter gets an error instead.
pub fn auth_headers(dir: &Path, url: &str) -> Result<String, String> {
    released(endpoint(url)?, &mut || crate::mcp::read_token(dir))
}
/// `auth_headers` over any source of the token, read as often as `proven` asks.
pub(crate) fn released(
    address: SocketAddr,
    read: &mut dyn FnMut() -> Result<String, String>,
) -> Result<String, String> {
    let (_, token) = runtime()?.block_on(proven(address, read))?;
    Ok(json!({"Authorization":format!("Bearer {token}")}).to_string())
}
/// `--data-dir <absolute path> [--endpoint <url>]`. The bridge and probe read the
/// running endpoint themselves; registrations made before 0.13.1 carry none.
fn helper_arguments(args: &[String]) -> Result<(&Path, Option<&str>), String> {
    if !matches!(args.len(), 3 | 5) || args[1] != "--data-dir" || !Path::new(&args[2]).is_absolute()
    {
        return Err("MCP 도우미에 올바른 --data-dir 절대 경로가 필요합니다.".into());
    }
    let url = if args.len() == 5 {
        if args[3] != "--endpoint" {
            return Err("MCP 도우미 인자를 확인하세요. 설정에서 AI 연결을 다시 등록하세요.".into());
        }
        endpoint(&args[4])?;
        Some(args[4].as_str())
    } else {
        None
    };
    Ok((Path::new(&args[2]), url))
}
/// What `--mcp-auth-headers` prints for these helper arguments. The client sends
/// the result to the URL in its own configuration, which this process cannot
/// see: without the registered endpoint no server can be proven to be that
/// destination, so the token stays here.
pub(crate) fn helper_auth_headers(args: &[String]) -> Result<String, String> {
    let (dir, url) = helper_arguments(args)?;
    let url = url.ok_or("0.13.1 이전에 만든 등록이라 인증 정보를 보낼 주소를 확인할 수 없습니다. TimeSchedule 설정 → 로컬 일정 MCP → AI 연결 도우미에서 다시 등록하세요. 인증 정보는 보내지 않았습니다.")?;
    auth_headers(dir, url)
}
/// Runs the helper mode in `args[0]` (see `launch::HELPER_MODES`) and returns
/// the exit code.
pub fn run(args: &[String]) -> i32 {
    let mode = args.first().map(String::as_str).unwrap_or_default();
    let result = (|| {
        let (dir, _) = helper_arguments(args)?;
        match mode {
            "--mcp-auth-headers" => {
                // This stdout is the credential channel consumed by the MCP client.
                println!("{}", helper_auth_headers(args)?);
                Ok(())
            }
            "--mcp-probe" => {
                println!("{}", json!({"status":probe(dir)?}));
                Ok(())
            }
            "--mcp-stdio" => bridge(dir, std::io::stdin().lock(), std::io::stdout().lock()),
            _ => Err("알 수 없는 MCP 도우미 모드입니다.".into()),
        }
    })();
    match result {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_reads_the_registered_endpoint_and_never_assumes_one() {
        let dir = if cfg!(windows) { "C:\\data" } else { "/data" };
        let args = |extra: &[&str]| -> Vec<String> {
            ["--mcp-auth-headers", "--data-dir", dir]
                .iter()
                .chain(extra)
                .map(|value| value.to_string())
                .collect()
        };
        // Registrations made before 0.13.1 carry no endpoint; the bridge and
        // probe forms never do. The destination is then unknown, not the default.
        let legacy = args(&[]);
        assert_eq!(helper_arguments(&legacy).unwrap(), (Path::new(dir), None));
        let registered = args(&["--endpoint", "http://127.0.0.1:40123/mcp"]);
        assert_eq!(
            helper_arguments(&registered).unwrap(),
            (Path::new(dir), Some("http://127.0.0.1:40123/mcp"))
        );
        for extra in [
            &["--endpoint"][..],
            &["--endpoint", "http://example.com:39281/mcp"],
            &["--endpoint", "http://127.0.0.1:39281/other"],
            &["--port", "39281"],
            &["--endpoint", "http://127.0.0.1:40123/mcp", "extra"],
        ] {
            assert!(helper_arguments(&args(extra)).is_err(), "{extra:?}");
        }
    }
    #[test]
    fn credentials_cannot_leave_exact_loopback_endpoint() {
        assert_eq!(
            endpoint("http://127.0.0.1:39281/mcp").unwrap().port(),
            39281
        );
        for value in [
            "http://example.com:39281/mcp",
            "https://127.0.0.1:39281/mcp",
            "http://127.0.0.1:0/mcp",
            "http://127.0.0.1:65536/mcp",
            "http://127.0.0.1:39281/mcp?redirect=x",
            "http://127.0.0.1:39281/mcp#x",
            "http://127.0.0.1:39281/mcp\r\nX: y",
            "http://127.0.0.1:39281@remote/mcp",
        ] {
            assert!(endpoint(value).is_err(), "{value}");
        }
    }
}
