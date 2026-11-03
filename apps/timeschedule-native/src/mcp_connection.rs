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
struct Connection {
    address: SocketAddr,
    token: String,
}
struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl Connection {
    fn load(dir: &Path) -> Result<Self, String> {
        Ok(Self {
            address: endpoint(&current_endpoint(dir)?)?,
            token: crate::mcp::read_token(dir)?,
        })
    }
    async fn post(&self, value: &Value, protocol: &str) -> Result<Option<Value>, String> {
        tokio::time::timeout(Duration::from_secs(12), async {
            let socket = tokio::net::TcpStream::connect(self.address)
                .await
                .map_err(|_| {
                    "TimeSchedule MCP에 연결하지 못했습니다. 앱 실행 상태를 확인하세요."
                })?;
            let (mut sender, connection) =
                hyper::client::conn::http1::handshake(TokioIo::new(socket))
                    .await
                    .map_err(|_| "MCP 연결 초기화 실패")?;
            let task = AbortOnDrop(tokio::spawn(async move {
                let _ = connection.await;
            }));
            let result = async {
                let request = Request::post("/mcp")
                    .header("Host", self.address.to_string())
                    .header("Authorization", format!("Bearer {}", self.token))
                    .header("Content-Type", "application/json")
                    .header("Accept", "application/json, text/event-stream")
                    .header("MCP-Protocol-Version", protocol)
                    .header("Connection", "close")
                    .body(Full::new(Bytes::from(value.to_string())))
                    .map_err(|_| "MCP 요청 생성 실패")?;
                let response = sender.send_request(request).await.map_err(|_| {
                    "MCP 응답을 받지 못했습니다. 저장 요청이었다면 재시도 전에 결과를 확인하세요."
                })?;
                if !response.status().is_success() {
                    return Err(match response.status().as_u16() {
                        401 | 403 => {
                            "MCP 인증 실패. TimeSchedule과 같은 Windows 계정인지 확인하세요."
                        }
                        _ => "MCP 요청 실패. 저장 요청이었다면 재시도 전에 결과를 확인하세요.",
                    }
                    .into());
                }
                let mut body = response.into_body();
                let mut bytes = Vec::new();
                while let Some(frame) = body.frame().await {
                    if let Ok(data) = frame.map_err(|_| "MCP 응답 읽기 실패")?.into_data() {
                        if bytes.len() + data.len() > 2 * 1024 * 1024 {
                            return Err("MCP 응답 크기 초과".into());
                        }
                        bytes.extend_from_slice(&data);
                    }
                }
                if bytes.is_empty() {
                    Ok(None)
                } else {
                    serde_json::from_slice(&bytes)
                        .map(Some)
                        .map_err(|_| "MCP 응답 형식 오류".into())
                }
            }
            .await;
            drop(task);
            result
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
pub fn try_run() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first()?.as_str();
    if !["--mcp-auth-headers", "--mcp-stdio", "--mcp-probe"].contains(&mode) {
        return None;
    }
    let result = (|| {
        if args.len() != 3 || args[1] != "--data-dir" || !Path::new(&args[2]).is_absolute() {
            return Err("MCP 도우미에 올바른 --data-dir 절대 경로가 필요합니다.".into());
        }
        let dir = Path::new(&args[2]);
        match mode {
            "--mcp-auth-headers" => {
                let token = crate::mcp::read_token(dir)?;
                // This stdout is the credential channel consumed by the MCP client.
                println!("{}", json!({"Authorization":format!("Bearer {token}")}));
                Ok(())
            }
            "--mcp-probe" => {
                println!("{}", json!({"status":probe(dir)?}));
                Ok(())
            }
            _ => bridge(dir, std::io::stdin().lock(), std::io::stdout().lock()),
        }
    })();
    Some(match result {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
