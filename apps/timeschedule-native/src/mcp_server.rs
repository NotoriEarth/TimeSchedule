use super::*;
use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    time::Duration,
};
use std::{
    net::{SocketAddr, TcpListener},
    path::Path,
    thread::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const MAX_CONNECTIONS: usize = 16;
const HEADER_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Server {
    cancel: CancellationToken,
    thread: Option<JoinHandle<()>>,
    pub address: SocketAddr,
}
impl Server {
    pub fn is_running(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }
    pub fn start(
        store: Arc<Storage>,
        changed: Arc<AtomicBool>,
        port: u16,
        token: String,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|_| "MCP 포트를 열 수 없습니다. 다른 앱의 사용 여부를 확인하세요.")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "MCP 소켓 초기화 실패")?;
        let address = listener.local_addr().map_err(|_| "MCP 주소 확인 실패")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(4)
            .build()
            .map_err(|_| "MCP 실행기 초기화 실패")?;
        let cancel = CancellationToken::new();
        let mut config = StreamableHttpServerConfig::default()
            .with_allowed_hosts([address.to_string()])
            .enforce_origin_validation();
        config.legacy_session_mode = false;
        config.json_response = true;
        config.sse_keep_alive = None;
        config.max_request_body_bytes = 128 * 1024;
        config.cancellation_token = cancel.clone();
        let api = CalendarTools {
            store,
            changed,
            work: Arc::new(tokio::sync::Semaphore::new(4)),
        };
        let service = StreamableHttpService::new(
            move || Ok(api.clone()),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let guard = Arc::new(Guard {
            token: format!("Bearer {token}"),
            host: address.to_string(),
            slots: tokio::sync::Semaphore::new(8),
        });
        let router = Router::new()
            .route_service("/mcp", service)
            .layer(middleware::from_fn_with_state(guard, protect));
        let shutdown = cancel.clone();
        let thread = std::thread::Builder::new()
            .name("calendar-mcp".into())
            .spawn(move || {
                runtime.block_on(async move {
                    if let Ok(listener) = tokio::net::TcpListener::from_std(listener) {
                        serve(listener, router, shutdown).await;
                    }
                });
                // Bounded shutdown even if a blocking SQLite/OS operation stalls. SQLite's
                // transaction remains atomic; callers must check an unacknowledged write.
                runtime.shutdown_timeout(Duration::from_secs(5));
            })
            .map_err(|_| "MCP 스레드 시작 실패")?;
        Ok(Self {
            cancel,
            thread: Some(thread),
            address,
        })
    }
}

/// Bound resources before Hyper parses headers and before authentication middleware runs.
async fn serve(listener: tokio::net::TcpListener, router: Router, shutdown: CancellationToken) {
    use hyper_util::{
        rt::{TokioIo, TokioTimer},
        service::TowerToHyperService,
    };
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            Some(_) = connections.join_next(), if !connections.is_empty() => {},
            accepted = listener.accept() => {
                let Ok((socket, _)) = accepted else { break };
                if connections.len() >= MAX_CONNECTIONS {
                    // Reject before allocating a task or HTTP parser. No request exists yet.
                    drop(socket);
                    continue;
                }
                let service = TowerToHyperService::new(router.clone());
                let cancel = shutdown.clone();
                connections.spawn(async move {
                    let mut builder = hyper::server::conn::http1::Builder::new();
                    builder.timer(TokioTimer::new()).header_read_timeout(HEADER_TIMEOUT);
                    let connection = builder.serve_connection(TokioIo::new(socket), service);
                    tokio::pin!(connection);
                    tokio::select! {
                        _ = &mut connection => {},
                        _ = cancel.cancelled() => {
                            connection.as_mut().graceful_shutdown();
                            let _ = tokio::time::timeout(Duration::from_secs(1), &mut connection).await;
                        }
                    }
                });
            }
        }
    }
    // Stop accepting immediately; each active connection has a bounded graceful drain.
    drop(listener);
    shutdown.cancel();
    while connections.join_next().await.is_some() {}
}
impl Drop for Server {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Guard {
    token: String,
    host: String,
    slots: tokio::sync::Semaphore,
}
async fn protect(State(guard): State<Arc<Guard>>, request: Request<Body>, next: Next) -> Response {
    let headers = request.headers();
    if headers.get_all("host").iter().count() != 1
        || headers.get("host").and_then(|v| v.to_str().ok()) != Some(&guard.host)
        || headers.contains_key("origin")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let authenticated = headers.get_all("authorization").iter().count() == 1
        && headers.get("authorization").is_some_and(|value| {
            let supplied = value.as_bytes();
            let expected = guard.token.as_bytes();
            supplied.len() == expected.len()
                && supplied
                    .iter()
                    .zip(expected)
                    .fold(0u8, |difference, (a, b)| difference | (a ^ b))
                    == 0
        });
    if !authenticated {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(_permit) = guard.slots.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let mut response = match tokio::time::timeout(Duration::from_secs(10), next.run(request)).await
    {
        Ok(response) => response,
        Err(_) => StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

pub(super) fn token(dir: &Path) -> Result<String, String> {
    let path = dir.join("mcp-token.dpapi");
    if path.exists() {
        let mut encrypted = Vec::new();
        fs::File::open(path)
            .and_then(|file| file.take(16 * 1024 + 1).read_to_end(&mut encrypted))
            .map_err(|_| "MCP 인증 파일 읽기 실패")?;
        if encrypted.len() > 16 * 1024 {
            return Err("MCP 인증 파일 크기가 잘못되었습니다.".into());
        }
        let plain = protect_token(&encrypted, false)?;
        let value = String::from_utf8(plain).map_err(|_| "MCP 인증 파일 형식 오류")?;
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("MCP 인증 파일 형식 오류".into());
        }
        return Ok(value);
    }
    let value = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let encrypted = protect_token(value.as_bytes(), true)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| "MCP 인증 파일 생성 실패")?;
    if file
        .write_all(&encrypted)
        .and_then(|_| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err("MCP 인증 파일 저장 실패".into());
    }
    Ok(value)
}

#[cfg(windows)]
fn protect_token(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // DPAPI CurrentUser scope; Windows allocates output with LocalAlloc.
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err("MCP 인증 보호/복호화 실패: 같은 Windows 계정을 사용하세요.".into());
    }
    let result =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        LocalFree(output.pbData.cast());
    }
    Ok(result)
}
#[cfg(not(windows))]
fn protect_token(_: &[u8], _: bool) -> Result<Vec<u8>, String> {
    Err("MCP 인증 저장은 Windows에서 지원됩니다.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    const TOKEN: &str = "fixture-only-secret-not-a-real-credential";
    fn request(address: SocketAddr, method: &str, headers: &str, body: &str) -> (u16, String) {
        let mut socket = std::net::TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(15)))
            .unwrap();
        write!(socket, "{method} /mcp HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n{headers}\r\n{body}",body.len()).unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        (
            response.split_whitespace().nth(1).unwrap().parse().unwrap(),
            response,
        )
    }
    fn auth() -> String {
        format!("Authorization: Bearer {TOKEN}\r\nMCP-Protocol-Version: 2025-11-25\r\n")
    }
    #[test]
    fn real_http_requires_auth_validates_origin_and_supports_mcp_lifecycle() {
        let server = Server::start(
            Arc::new(Storage::open(":memory:").unwrap()),
            Arc::new(AtomicBool::new(false)),
            0,
            TOKEN.into(),
        )
        .unwrap();
        let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
        assert_eq!(request(server.address, "POST", "", list).0, 401);
        assert_eq!(
            request(
                server.address,
                "POST",
                "Authorization: Bearer wrong\r\n",
                list
            )
            .0,
            401
        );
        assert_eq!(
            request(
                server.address,
                "POST",
                &(auth() + "Origin: https://evil.example\r\n"),
                list
            )
            .0,
            403
        );
        assert_eq!(
            request(
                server.address,
                "POST",
                &(auth() + "Host: evil.example\r\n"),
                list
            )
            .0,
            403
        );
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}}"#;
        let (status, response) = request(server.address, "POST", &auth(), init);
        assert_eq!(status, 200);
        assert!(response.contains("2025-11-25"));
        assert_eq!(
            request(
                server.address,
                "POST",
                &auth(),
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            )
            .0,
            202
        );
        let (status, response) = request(server.address, "POST", &auth(), list);
        assert_eq!(status, 200);
        for name in [
            "calendar_list",
            "calendar_get",
            "calendar_create",
            "calendar_update",
        ] {
            assert!(response.contains(name));
        }
        assert!(!response.contains("calendar_delete"));
        assert_eq!(request(server.address, "GET", &auth(), "").0, 405);
        // rmcp rejects malformed JSON as unsupported media before RPC dispatch.
        assert_eq!(request(server.address, "POST", &auth(), "{broken").0, 415);
        assert_eq!(
            request(
                server.address,
                "POST",
                &format!("Authorization: Bearer {TOKEN}\r\nMCP-Protocol-Version: 2000-01-01\r\n"),
                list
            )
            .0,
            400
        );
        let (_, unknown) = request(
            server.address,
            "POST",
            &auth(),
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"calendar_delete","arguments":{}}}"#,
        );
        assert!(unknown.contains("-32602"));
        assert_eq!(
            request(server.address, "POST", &auth(), &" ".repeat(128 * 1024 + 1)).0,
            413
        );
        let (status, response) = request(
            server.address,
            "POST",
            &auth(),
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"calendar_create","arguments":{"title":"fixture","date":"2026-11-02"}}}"#,
        );
        assert_eq!(status, 200);
        assert!(response.contains("updatedAt"));
        let (status, response) = request(
            server.address,
            "POST",
            &auth(),
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"calendar_create","arguments":{"title":"","date":"2026-02-30"}}}"#,
        );
        assert_eq!(status, 200);
        assert!(response.contains("\"isError\":true"));
        let address = server.address;
        drop(server);
        assert!(std::net::TcpStream::connect(address).is_err());
    }
    #[test]
    fn shutdown_is_bounded_with_incomplete_headers_and_slow_body() {
        for slow_body in [false, true] {
            let server = Server::start(
                Arc::new(Storage::open(":memory:").unwrap()),
                Arc::new(AtomicBool::new(false)),
                0,
                TOKEN.into(),
            )
            .unwrap();
            let mut socket = std::net::TcpStream::connect(server.address).unwrap();
            if slow_body {
                write!(socket,"POST /mcp HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: 1024\r\n\r\n{{",server.address).unwrap();
            } else {
                socket.write_all(b"POST /mcp HTTP/1.1\r\nHost: ").unwrap();
            }
            std::thread::sleep(Duration::from_millis(100));
            let (send, receive) = std::sync::mpsc::channel();
            let thread = std::thread::spawn(move || {
                drop(server);
                let _ = send.send(());
            });
            let result = receive.recv_timeout(Duration::from_secs(3));
            drop(socket);
            thread.join().unwrap();
            assert!(result.is_ok(), "shutdown stalled for slow_body={slow_body}");
        }
    }

    #[test]
    fn ordinary_http_requests_can_reuse_a_keep_alive_connection() {
        use std::io::{BufRead, BufReader};
        let server = Server::start(
            Arc::new(Storage::open(":memory:").unwrap()),
            Arc::new(AtomicBool::new(false)),
            0,
            TOKEN.into(),
        )
        .unwrap();
        let socket = std::net::TcpStream::connect(server.address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(socket);
        for id in 1..=2 {
            let body =
                format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list","params":{{}}}}"#);
            write!(reader.get_mut(),"POST /mcp HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\n{}\r\n{body}",server.address,body.len(),auth()).unwrap();
            let mut status = String::new();
            reader.read_line(&mut status).unwrap();
            assert!(status.starts_with("HTTP/1.1 200"));
            let mut length = None;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" {
                    break;
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = Some(value.trim().parse::<usize>().unwrap());
                }
            }
            let mut response = vec![0u8; length.expect("JSON response has bounded content length")];
            reader.read_exact(&mut response).unwrap();
            let response: Value = serde_json::from_slice(&response).unwrap();
            assert_eq!(response["id"], id);
            assert_eq!(response["result"]["tools"].as_array().unwrap().len(), 4);
        }
    }

    #[test]
    fn partial_header_connections_are_capped_before_authentication() {
        let server = Server::start(
            Arc::new(Storage::open(":memory:").unwrap()),
            Arc::new(AtomicBool::new(false)),
            0,
            TOKEN.into(),
        )
        .unwrap();
        let mut held = Vec::new();
        for _ in 0..20 {
            let mut socket = std::net::TcpStream::connect(server.address).unwrap();
            // No authentication or completed request exists for middleware to inspect.
            let _ = socket.write_all(b"POST /mcp HTTP/1.1\r\nHost: ");
            socket
                .set_read_timeout(Some(Duration::from_millis(100)))
                .unwrap();
            held.push(socket);
        }
        std::thread::sleep(Duration::from_millis(100));
        let open=held.iter_mut().filter(|socket| {
            let mut byte=[0u8;1];
            matches!(socket.peek(&mut byte),Err(error) if matches!(error.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut))
        }).count();
        drop(held);
        assert_eq!(
            open, 16,
            "pre-header sockets must consume the connection budget"
        );
    }

    #[test]
    fn incomplete_headers_expire_without_waiting_for_app_shutdown() {
        let server = Server::start(
            Arc::new(Storage::open(":memory:").unwrap()),
            Arc::new(AtomicBool::new(false)),
            0,
            TOKEN.into(),
        )
        .unwrap();
        let mut socket = std::net::TcpStream::connect(server.address).unwrap();
        socket.write_all(b"POST /mcp HTTP/1.1\r\nHost: ").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(7)))
            .unwrap();
        let mut response = String::new();
        let result = socket.read_to_string(&mut response);
        drop(socket);
        assert!(
            result.is_ok()
                || result.is_err_and(|error| matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                )),
            "incomplete headers stayed open past their deadline"
        );
        // Expiration frees capacity while the server remains usable.
        assert_eq!(
            request(
                server.address,
                "POST",
                &auth(),
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#
            )
            .0,
            200
        );
    }

    #[test]
    fn concurrent_slow_requests_are_rejected_at_the_http_limit() {
        let server = Server::start(
            Arc::new(Storage::open(":memory:").unwrap()),
            Arc::new(AtomicBool::new(false)),
            0,
            TOKEN.into(),
        )
        .unwrap();
        let mut held = Vec::new();
        for _ in 0..8 {
            let mut socket = std::net::TcpStream::connect(server.address).unwrap();
            write!(socket,"POST /mcp HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: 1024\r\n\r\n{{",server.address).unwrap();
            held.push(socket);
        }
        let mut status = 0;
        for _ in 0..30 {
            status = request(
                server.address,
                "POST",
                &auth(),
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            )
            .0;
            if status == 429 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(held);
        assert_eq!(status, 429);
    }

    #[test]
    fn bind_conflict_returns_an_error_and_keeps_existing_listener() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        assert!(Server::start(
            Arc::new(Storage::open(":memory:").unwrap()),
            Arc::new(AtomicBool::new(false)),
            listener.local_addr().unwrap().port(),
            TOKEN.into()
        )
        .is_err());
        assert!(std::net::TcpStream::connect(listener.local_addr().unwrap()).is_ok());
    }
    #[test]
    fn protected_token_is_reused_and_never_stored_as_plaintext() {
        let dir = std::env::temp_dir().join(format!("timeschedule-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let first = token(&dir).unwrap();
        assert_eq!(first, token(&dir).unwrap());
        assert_eq!(first.len(), 64);
        assert!(!std::fs::read(dir.join("mcp-token.dpapi"))
            .unwrap()
            .windows(first.len())
            .any(|bytes| bytes == first.as_bytes()));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
