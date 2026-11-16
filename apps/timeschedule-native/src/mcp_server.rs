use super::*;
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
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
    /// A server whose connection log lives only in memory (tests).
    #[cfg(test)]
    pub fn start(
        store: Arc<Storage>,
        changed: Arc<AtomicBool>,
        port: u16,
        token: String,
    ) -> Result<Self, String> {
        Self::start_with_clients(
            store,
            changed,
            port,
            token,
            Arc::new(crate::mcp_clients::ClientLog::memory()),
        )
    }
    pub fn start_with_clients(
        store: Arc<Storage>,
        changed: Arc<AtomicBool>,
        port: u16,
        token: String,
        clients: Arc<crate::mcp_clients::ClientLog>,
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
            clients,
        };
        let service = StreamableHttpService::new(
            move || Ok(api.clone()),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let guard = Arc::new(Guard {
            token: format!("Bearer {token}"),
            secret: token,
            host: address.to_string(),
            slots: tokio::sync::Semaphore::new(8),
        });
        let router = Router::new()
            .route_service("/mcp", service)
            .layer(middleware::from_fn_with_state(guard.clone(), protect))
            // Added after the token layer: a client asks for this proof before it
            // sends the token, so the route cannot itself require the token.
            .route("/proof", axum::routing::get(prove))
            .with_state(guard);
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
    secret: String,
    host: String,
    slots: tokio::sync::Semaphore,
}
/// Exactly this listener's Host and no browser Origin (DNS-rebinding defence).
fn addressed_locally(guard: &Guard, headers: &HeaderMap) -> bool {
    headers.get_all("host").iter().count() == 1
        && headers.get("host").and_then(|v| v.to_str().ok()) == Some(guard.host.as_str())
        && !headers.contains_key("origin")
}

pub(crate) const CHALLENGE_HEADER: &str = "timeschedule-challenge";
/// What a listener on `host` that holds `token` answers to `challenge`. The
/// token never travels, and binding the host stops another port from relaying
/// the genuine server's answer.
pub(crate) fn proof(token: &str, host: &str, challenge: &str) -> Result<String, String> {
    let message = format!("TimeSchedule MCP server proof v1\n{host}\n{challenge}");
    let mac = hmac_sha256(token.as_bytes(), message.as_bytes())?;
    Ok(mac.iter().map(|byte| format!("{byte:02x}")).collect())
}
async fn prove(State(guard): State<Arc<Guard>>, headers: HeaderMap) -> Response {
    if !addressed_locally(&guard, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut challenges = headers.get_all(CHALLENGE_HEADER).iter();
    let challenge = match (challenges.next(), challenges.next()) {
        (Some(value), None) => value.to_str().ok(),
        _ => None,
    }
    .filter(|v| v.len() == 64 && v.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
    let Some(challenge) = challenge else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match proof(&guard.secret, &guard.host, challenge) {
        Ok(value) => ([("cache-control", "no-store")], value).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
async fn protect(State(guard): State<Arc<Guard>>, request: Request<Body>, next: Next) -> Response {
    let headers = request.headers();
    if !addressed_locally(&guard, headers) {
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
        return read_token(dir);
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

pub(crate) fn read_token(dir: &Path) -> Result<String, String> {
    let path = dir.join("mcp-token.dpapi");
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

#[cfg(windows)]
fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<[u8; 32], String> {
    use windows_sys::Win32::Security::Cryptography::{BCryptHash, BCRYPT_HMAC_SHA256_ALG_HANDLE};
    let mut output = [0u8; 32];
    // One-shot CNG HMAC through the built-in pseudo-handle: nothing to open or free.
    let status = unsafe {
        BCryptHash(
            BCRYPT_HMAC_SHA256_ALG_HANDLE,
            key.as_ptr(),
            key.len() as u32,
            data.as_ptr(),
            data.len() as u32,
            output.as_mut_ptr(),
            output.len() as u32,
        )
    };
    if status != 0 {
        return Err("MCP 서버 확인값 계산 실패".into());
    }
    Ok(output)
}
#[cfg(not(windows))]
fn hmac_sha256(_: &[u8], _: &[u8]) -> Result<[u8; 32], String> {
    Err("MCP 서버 확인은 Windows에서 지원됩니다.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    const TOKEN: &str = "fixture-only-secret-not-a-real-credential";
    #[test]
    #[ignore = "requires freshly built release EXE in TIMESCHEDULE_MCP_TEST_EXE; isolated client/process integration"]
    fn release_connection_helper_and_desktop_bridge_round_trip() {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        let exe = std::path::PathBuf::from(
            std::env::var_os("TIMESCHEDULE_MCP_TEST_EXE").expect("release EXE path required"),
        );
        assert!(exe.is_absolute() && exe.is_file());
        let dir = std::env::temp_dir().join(format!(
            "TimeSchedule 한글 & user's %PATH% {}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Arc::new(Storage::open(":memory:").unwrap());
        let changed = Arc::new(AtomicBool::new(false));
        let server =
            Server::start(store.clone(), changed.clone(), 0, token(&dir).unwrap()).unwrap();
        let endpoint = format!("http://{}/mcp", server.address);
        std::fs::write(
            dir.join("mcp-status.json"),
            json!({"endpoint":endpoint}).to_string(),
        )
        .unwrap();
        let helper = crate::mcp_setup::helper_command(&exe, &dir, &endpoint).unwrap();
        let mut parts = helper.split_whitespace();
        let header_output = Command::new(parts.next().unwrap())
            .args(parts)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            header_output.status.success(),
            "header process failed (credential output omitted)"
        );
        let headers: Value =
            serde_json::from_slice(&header_output.stdout).expect("helper must output JSON only");
        assert!(headers["Authorization"]
            .as_str()
            .is_some_and(|s| s.starts_with("Bearer ") && s.len() == 71));

        // A deliberately restricted file in an ordinary temp directory proves
        // that backups retain per-file protection rather than parent inheritance.
        let config = dir.join("desktop.json");
        std::fs::write(&config, "{\"fixtureSecret\":\"not-a-real-secret\"}").unwrap();
        let escaped = config.to_string_lossy().replace('\'', "''");
        let acl_script = format!("$ErrorActionPreference='Stop'; $p='{escaped}'; $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User; $acl=[IO.File]::GetAccessControl($p); $acl.SetAccessRuleProtection($true,$false); $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','Allow')); [IO.File]::SetAccessControl($p,$acl)");
        assert!(Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &acl_script])
            .creation_flags(0x08000000)
            .status()
            .unwrap()
            .success());
        let plan = crate::mcp_setup::prepare(
            crate::mcp_setup::Client::ClaudeDesktop,
            &config,
            &exe,
            &dir,
            &endpoint,
        )
        .unwrap();
        let backup = crate::mcp_setup::apply(&plan).unwrap().unwrap();
        let escaped_backup = backup.to_string_lossy().replace('\'', "''");
        let acl_check = format!("$ErrorActionPreference='Stop'; $b=[IO.File]::GetAccessControl('{escaped_backup}'); $rules=$b.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]); $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User; if (!$b.AreAccessRulesProtected -or $rules.Count -ne 1 -or $rules[0].IdentityReference -ne $sid -or $rules[0].FileSystemRights -ne 'FullControl' -or $rules[0].AccessControlType -ne 'Allow' -or $rules[0].IsInherited) {{ exit 1 }}");
        assert!(Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &acl_check])
            .creation_flags(0x08000000)
            .status()
            .unwrap()
            .success());
        let value: Value = serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
        let entry = &value["mcpServers"]["timeschedule"];
        let mut bridge = Command::new(entry["command"].as_str().unwrap())
            .args(
                entry["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap()),
            )
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":11,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":12,\"method\":\"tools/list\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":13,\"method\":\"tools/call\",\"params\":{\"name\":\"calendar_create\",\"arguments\":{\"title\":\"isolated bridge fixture\",\"date\":\"2026-11-03\"}}}\n"
        );
        bridge
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        // Sample the owned bridge while waiting for stdin, not a user process.
        let sample = format!("$p=[Diagnostics.Process]::GetProcessById({}); $c=$p.TotalProcessorTime.TotalSeconds; [Threading.Thread]::Sleep(2000); $p.Refresh(); [pscustomobject]@{{workingSetBytes=$p.WorkingSet64;privateBytes=$p.PrivateMemorySize64;idleCpuSeconds=($p.TotalProcessorTime.TotalSeconds-$c);sampleSeconds=2}} | ConvertTo-Json -Compress", bridge.id());
        let memory = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &sample])
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(memory.status.success());
        println!(
            "BRIDGE_RESOURCE {}",
            String::from_utf8_lossy(&memory.stdout).trim()
        );
        drop(bridge.stdin.take());
        let output = bridge.wait_with_output().unwrap();
        assert!(output.status.success(), "bridge process failed");
        let replies: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0]["result"]["serverInfo"]["name"], "TimeSchedule");
        assert_eq!(replies[1]["result"]["tools"].as_array().unwrap().len(), 4);
        assert_ne!(replies[2]["result"]["isError"], true);
        assert!(changed.load(Ordering::Acquire));
        assert_eq!(
            store
                .list_event_headers_limited("2026-11-03", "2026-11-03", 10)
                .unwrap()
                .0
                .len(),
            1
        );
        drop(server);
        assert!(crate::mcp_connection::probe(&dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn setup_probe_uses_authenticated_real_server_without_schedule_mutations() {
        let dir = std::env::temp_dir().join(format!("timeschedule-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let changed = Arc::new(AtomicBool::new(false));
        let server = Server::start(
            Arc::new(Storage::open(":memory:").unwrap()),
            changed.clone(),
            0,
            token(&dir).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("mcp-status.json"),
            json!({"endpoint":format!("http://{}/mcp", server.address)}).to_string(),
        )
        .unwrap();
        assert!(crate::mcp_connection::probe(&dir).unwrap().contains("4"));
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":11,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":12,\"method\":\"tools/list\",\"params\":{}}\n"
        );
        let mut output = Vec::new();
        crate::mcp_connection::bridge(&dir, std::io::Cursor::new(input), &mut output).unwrap();
        let replies: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(
            replies.len(),
            2,
            "notifications must not produce stdout replies"
        );
        assert_eq!(replies[0]["id"], 11);
        assert_eq!(replies[0]["result"]["serverInfo"]["name"], "TimeSchedule");
        assert_eq!(replies[1]["id"], 12);
        assert_eq!(replies[1]["result"]["tools"].as_array().unwrap().len(), 4);
        assert!(!changed.load(Ordering::Acquire));
        std::fs::remove_file(dir.join("mcp-token.dpapi")).unwrap();
        assert!(crate::mcp_connection::probe(&dir).is_err());
        assert!(!dir.join("mcp-token.dpapi").exists());
        drop(server);
        std::fs::remove_dir_all(dir).unwrap();
    }
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
    /// A local listener that is not TimeSchedule. It records every request it
    /// receives and answers each one with 200 and the body `reply` returns for it.
    fn impostor(
        reply: impl Fn(&str) -> String + Send + 'static,
    ) -> (SocketAddr, Arc<std::sync::Mutex<String>>) {
        impostor_raw(move |request| {
            let body = reply(request);
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
        })
    }
    /// Like `impostor`, but `reply` returns the whole HTTP response.
    fn impostor_raw(
        reply: impl Fn(&str) -> String + Send + 'static,
    ) -> (SocketAddr, Arc<std::sync::Mutex<String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let seen = Arc::new(std::sync::Mutex::new(String::new()));
        let record = seen.clone();
        std::thread::spawn(move || {
            for socket in listener.incoming() {
                let Ok(mut socket) = socket else { break };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut chunk = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut chunk) {
                        Ok(n) if n > 0 => request.extend_from_slice(&chunk[..n]),
                        _ => break,
                    }
                }
                let request = String::from_utf8_lossy(&request).into_owned();
                record.lock().unwrap().push_str(&request);
                let _ = socket.write_all(reply(&request).as_bytes());
            }
        });
        (address, seen)
    }
    /// A listener that holds `secret`, so it passes the proof, and serves further
    /// requests on the same connection. Every other request is answered with the
    /// raw response `reply` returns, and the log records whether the connection
    /// it arrived on had proven itself first.
    fn proving_listener(
        secret: &str,
        reply: impl Fn(&str) -> String + Send + 'static,
    ) -> (SocketAddr, Arc<std::sync::Mutex<Vec<bool>>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = log.clone();
        let secret = secret.to_string();
        std::thread::spawn(move || {
            for socket in listener.incoming() {
                let Ok(mut socket) = socket else { break };
                socket
                    .set_read_timeout(Some(Duration::from_secs(20)))
                    .unwrap();
                let mut proved = false;
                loop {
                    let mut request = Vec::new();
                    let mut byte = [0u8; 1];
                    while !request.ends_with(b"\r\n\r\n") {
                        match socket.read(&mut byte) {
                            Ok(1) => request.push(byte[0]),
                            _ => break,
                        }
                    }
                    if !request.ends_with(b"\r\n\r\n") {
                        break;
                    }
                    let request = String::from_utf8_lossy(&request).into_owned();
                    let header = |name: &str| {
                        request
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix(name)
                                    .map(|value| value.trim().to_string())
                            })
                            .unwrap_or_default()
                    };
                    let mut body = vec![0u8; header("content-length:").parse().unwrap_or(0)];
                    if socket.read_exact(&mut body).is_err() {
                        break;
                    }
                    if request.starts_with("GET /proof") {
                        let answer = proof(
                            &secret,
                            &header("host:"),
                            &header("timeschedule-challenge:"),
                        )
                        .unwrap();
                        let sent = write!(
                            socket,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{answer}",
                            answer.len()
                        );
                        if sent.is_err() {
                            break;
                        }
                        proved = true;
                    } else {
                        record.lock().unwrap().push(proved);
                        let _ = socket.write_all(reply(&request).as_bytes());
                        // Keep the connection open until the client lets go of it.
                        let _ = socket.read(&mut byte);
                        break;
                    }
                }
            }
        });
        (address, log)
    }
    fn data_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("timeschedule-proof-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn memory_server(token: &str) -> Server {
        Server::start(
            Arc::new(Storage::open(":memory:").unwrap()),
            Arc::new(AtomicBool::new(false)),
            0,
            token.into(),
        )
        .unwrap()
    }
    fn proof_request(address: SocketAddr, host: &str, headers: &str) -> (u16, String) {
        let mut socket = std::net::TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            socket,
            "GET /proof HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{headers}\r\n"
        )
        .unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        let status = response.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = response
            .split_once("\r\n\r\n")
            .map(|(_, body)| body.to_string())
            .unwrap_or_default();
        (status, body)
    }
    const INITIALIZE: &str = "{\"jsonrpc\":\"2.0\",\"id\":11,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}}\n";

    #[test]
    fn each_ai_app_connection_is_recorded_but_the_self_check_is_not() {
        use crate::mcp_clients::{AiApp, ClientLog};
        let dir = data_dir();
        let secret = token(&dir).unwrap();
        let clients = Arc::new(ClientLog::memory());
        let server = Server::start_with_clients(
            Arc::new(Storage::open(":memory:").unwrap()),
            Arc::new(AtomicBool::new(false)),
            0,
            secret.clone(),
            clients.clone(),
        )
        .unwrap();
        std::fs::write(
            dir.join("mcp-status.json"),
            json!({"endpoint":format!("http://{}/mcp", server.address)}).to_string(),
        )
        .unwrap();
        // TimeSchedule's own connection check initializes too, under its own name.
        crate::mcp_connection::probe(&dir).unwrap();
        assert!(clients.snapshot().is_empty(), "{:?}", clients.snapshot());
        // Claude Desktop reaches the server through the stdio bridge.
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"claude-ai","version":"1.2.3"}}}"#;
        let mut output = Vec::new();
        crate::mcp_connection::bridge(&dir, std::io::Cursor::new(format!("{init}\n")), &mut output)
            .unwrap();
        // Claude Code talks HTTP directly.
        let direct = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"claude-code","version":"2.0.0"}}}"#;
        let auth =
            format!("Authorization: Bearer {secret}\r\nMCP-Protocol-Version: 2025-11-25\r\n");
        assert_eq!(request(server.address, "POST", &auth, direct).0, 200);
        let desktop = clients
            .last_for(AiApp::ClaudeDesktop)
            .expect("bridge connection");
        assert_eq!(
            (desktop.name.as_str(), desktop.version.as_str()),
            ("claude-ai", "1.2.3")
        );
        let code = clients
            .last_for(AiApp::ClaudeCode)
            .expect("direct connection");
        assert_eq!(code.version, "2.0.0");
        assert!(clients.last_for(AiApp::CodexChatGpt).is_none());
        // A rejected (unauthenticated) initialize is not a connection.
        let stranger = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"codex-mcp-client","version":"9"}}}"#;
        assert_eq!(request(server.address, "POST", "", stranger).0, 401);
        assert!(clients.last_for(AiApp::CodexChatGpt).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn hmac_sha256_matches_the_published_test_vector() {
        let mac = hmac_sha256(b"key", b"The quick brown fox jumps over the lazy dog").unwrap();
        let hex: String = mac.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            hex,
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }
    #[test]
    fn proof_matches_the_vector_pinned_for_the_powershell_status_script() {
        // tests/local-mcp.test.mjs pins the same inputs and answer for scripts/mcp-session.ps1.
        assert_eq!(
            proof(
                &"0123456789abcdef".repeat(4),
                "127.0.0.1:39281",
                &"ab".repeat(32)
            )
            .unwrap(),
            "9c488ddb357b469f4d2205932eb32a03afd257e7f518ac9ccf79326b91c6e978"
        );
    }
    #[test]
    fn auth_headers_are_withheld_from_a_listener_that_cannot_prove_the_token() {
        let dir = data_dir();
        let secret = token(&dir).unwrap();
        let (address, seen) = impostor(|_| "0".repeat(64));
        let result = crate::mcp_connection::auth_headers(&dir, &format!("http://{address}/mcp"));
        assert!(
            result.is_err(),
            "the credential was released for an unverified listener"
        );
        assert!(!seen.lock().unwrap().contains(&secret));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn auth_headers_are_withheld_while_nothing_listens_on_the_endpoint() {
        let dir = data_dir();
        token(&dir).unwrap();
        let closed = TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap();
        assert!(
            crate::mcp_connection::auth_headers(&dir, &format!("http://{closed}/mcp")).is_err()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn auth_headers_are_released_for_the_running_server_that_proves_the_token() {
        let dir = data_dir();
        let secret = token(&dir).unwrap();
        let server = memory_server(&secret);
        let headers: Value = serde_json::from_str(
            &crate::mcp_connection::auth_headers(&dir, &format!("http://{}/mcp", server.address))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(headers["Authorization"], format!("Bearer {secret}"));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_proof_relayed_from_another_port_does_not_release_auth_headers() {
        let dir = data_dir();
        let secret = token(&dir).unwrap();
        let server = memory_server(&secret);
        let real = server.address;
        // The impostor forwards the challenge to the genuine server on a different port.
        let (address, seen) = impostor(move |request| {
            let challenge = request
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("timeschedule-challenge:")
                        .map(|value| value.trim().to_string())
                })
                .unwrap_or_default();
            proof_request(
                real,
                &real.to_string(),
                &format!("TimeSchedule-Challenge: {challenge}\r\n"),
            )
            .1
        });
        let result = crate::mcp_connection::auth_headers(&dir, &format!("http://{address}/mcp"));
        assert!(result.is_err(), "a relayed proof unlocked the credential");
        let seen = seen.lock().unwrap().to_ascii_lowercase();
        assert!(
            seen.contains("timeschedule-challenge:"),
            "the relay was not exercised"
        );
        assert!(!seen.contains(&secret));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_registration_without_an_endpoint_gets_no_token_even_while_the_app_runs() {
        // Registrations made before 0.13.1 do not say where the client will send
        // the token, so proving any one server cannot make releasing it safe.
        let dir = data_dir();
        let secret = token(&dir).unwrap();
        let server = memory_server(&secret);
        let url = format!("http://{}/mcp", server.address);
        std::fs::write(
            dir.join("mcp-status.json"),
            json!({"endpoint":url}).to_string(),
        )
        .unwrap();
        let args = |extra: &[&str]| -> Vec<String> {
            ["--mcp-auth-headers", "--data-dir", dir.to_str().unwrap()]
                .iter()
                .chain(extra)
                .map(|value| value.to_string())
                .collect()
        };
        let refused = crate::mcp_connection::helper_auth_headers(&args(&[])).unwrap_err();
        assert!(refused.contains("다시 등록"), "{refused}");
        assert!(!refused.contains(&secret));
        let released =
            crate::mcp_connection::helper_auth_headers(&args(&["--endpoint", &url])).unwrap();
        assert!(released.contains(&secret));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_redirect_to_the_genuine_proof_does_not_release_auth_headers() {
        let dir = data_dir();
        let secret = token(&dir).unwrap();
        let server = memory_server(&secret);
        let real = server.address;
        let (address, seen) = impostor_raw(move |_| {
            format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{real}/proof\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        });
        let result = crate::mcp_connection::auth_headers(&dir, &format!("http://{address}/mcp"));
        assert!(
            result.is_err(),
            "a redirected proof unlocked the credential"
        );
        assert!(!seen.lock().unwrap().contains(&secret));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn an_authentication_failure_is_reported_without_waiting_for_its_body() {
        // A listener that holds the token (so it passes the proof) but rejects the
        // request with a body that never arrives in full.
        let dir = data_dir();
        let secret = token(&dir).unwrap();
        // The fixture keeps the connection open after these few body bytes.
        let (address, _) = proving_listener(&secret, |_| {
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: 4000000\r\n\r\npartial".to_string()
        });
        std::fs::write(
            dir.join("mcp-status.json"),
            json!({"endpoint":format!("http://{address}/mcp")}).to_string(),
        )
        .unwrap();
        let started = std::time::Instant::now();
        let error = crate::mcp_connection::probe(&dir).unwrap_err();
        assert!(error.contains("인증 실패"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the status was held back by an unfinished body"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn the_token_travels_only_on_the_connection_that_proved_itself() {
        // Proving on one connection and sending on another would hand the token to
        // whichever process owns the port by the time the second one is opened.
        let dir = data_dir();
        let secret = token(&dir).unwrap();
        let (address, log) = proving_listener(&secret, |_| {
            let body = r#"{"jsonrpc":"2.0","id":11,"result":{}}"#;
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
        });
        std::fs::write(
            dir.join("mcp-status.json"),
            json!({"endpoint":format!("http://{address}/mcp")}).to_string(),
        )
        .unwrap();
        let mut output = Vec::new();
        crate::mcp_connection::bridge(&dir, std::io::Cursor::new(INITIALIZE), &mut output).unwrap();
        let reply: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(reply["id"], 11, "{reply}");
        assert_eq!(*log.lock().unwrap(), vec![true]);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn bridge_does_not_send_the_token_to_an_unproven_listener() {
        let dir = data_dir();
        let secret = token(&dir).unwrap();
        let (address, seen) = impostor(|_| r#"{"jsonrpc":"2.0","id":11,"result":{}}"#.to_string());
        std::fs::write(
            dir.join("mcp-status.json"),
            json!({"endpoint":format!("http://{address}/mcp")}).to_string(),
        )
        .unwrap();
        let mut output = Vec::new();
        crate::mcp_connection::bridge(&dir, std::io::Cursor::new(INITIALIZE), &mut output).unwrap();
        let reply: Value = serde_json::from_slice(&output).unwrap();
        assert!(reply["error"].is_object(), "{reply}");
        assert!(!seen.lock().unwrap().contains(&secret));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn probe_does_not_send_the_token_to_an_unproven_listener() {
        let dir = data_dir();
        let secret = token(&dir).unwrap();
        let (address, seen) = impostor(|_| {
            r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"TimeSchedule"},"protocolVersion":"2025-11-25"}}"#.to_string()
        });
        std::fs::write(
            dir.join("mcp-status.json"),
            json!({"endpoint":format!("http://{address}/mcp")}).to_string(),
        )
        .unwrap();
        assert!(crate::mcp_connection::probe(&dir).is_err());
        assert!(!seen.lock().unwrap().contains(&secret));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn proof_endpoint_needs_no_token_but_exact_host_no_origin_and_a_well_formed_challenge() {
        let server = memory_server(TOKEN);
        let host = server.address.to_string();
        let challenge = |value: &str| format!("TimeSchedule-Challenge: {value}\r\n");
        let (status, first) = proof_request(server.address, &host, &challenge(&"ab".repeat(32)));
        assert_eq!(status, 200);
        assert!(first.len() == 64 && first.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(!first.contains(TOKEN));
        let (_, second) = proof_request(server.address, &host, &challenge(&"cd".repeat(32)));
        assert_ne!(first, second, "the proof must depend on the challenge");
        assert_eq!(
            proof_request(server.address, "evil.example", &challenge(&"ab".repeat(32))).0,
            403
        );
        assert_eq!(
            proof_request(
                server.address,
                &host,
                &(challenge(&"ab".repeat(32)) + "Origin: https://evil.example\r\n")
            )
            .0,
            403
        );
        for malformed in ["", &"ab".repeat(31), &"AB".repeat(32), &"zz".repeat(32)] {
            assert_eq!(
                proof_request(server.address, &host, &challenge(malformed)).0,
                400,
                "{malformed}"
            );
        }
        assert_eq!(proof_request(server.address, &host, "").0, 400);
        assert_eq!(
            proof_request(
                server.address,
                &host,
                &(challenge(&"ab".repeat(32)) + &challenge(&"ab".repeat(32)))
            )
            .0,
            400,
            "an ambiguous repeated challenge must be rejected"
        );
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
