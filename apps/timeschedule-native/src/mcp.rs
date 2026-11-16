//! Authenticated, process-owned local calendar MCP. No model or external service calls.
use rmcp::{model::*, service::RequestContext, ErrorData, RoleServer, ServerHandler};
use serde::Deserialize;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use timeschedule_core::storage::EventInput;
use timeschedule_core::storage::Storage;
#[path = "mcp_server.rs"]
mod server;
pub(crate) use server::{proof, read_token, CHALLENGE_HEADER};

pub struct LocalMcp {
    server: Option<server::Server>,
    status_path: std::path::PathBuf,
    pub status: String,
}
impl LocalMcp {
    pub fn start(
        store: Arc<Storage>,
        changed: Arc<AtomicBool>,
        dir: &std::path::Path,
    ) -> Result<Option<Self>, String> {
        if std::env::var("TIMESCHEDULE_MCP_DISABLED").as_deref() == Ok("1") {
            return Ok(None);
        }
        let port = std::env::var("TIMESCHEDULE_MCP_PORT")
            .unwrap_or_else(|_| "39281".into())
            .parse::<u16>()
            .map_err(|_| "TIMESCHEDULE_MCP_PORT는 0..65535 정수여야 합니다.")?;
        let server = server::Server::start(store, changed, port, server::token(dir)?)?;
        let endpoint = format!("http://{}/mcp", server.address);
        let status_path = dir.join("mcp-status.json");
        std::fs::write(&status_path,json!({"endpoint":endpoint,"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION")}).to_string()).map_err(|_|"MCP 상태 파일 저장 실패")?;
        Ok(Some(Self {
            server: Some(server),
            status_path,
            status: format!("실행 중 · {endpoint} · 토큰 인증"),
        }))
    }
    pub fn is_running(&self) -> bool {
        self.server.as_ref().is_some_and(server::Server::is_running)
    }
}
impl Drop for LocalMcp {
    fn drop(&mut self) {
        self.server.take();
        let _ = std::fs::remove_file(&self.status_path);
    }
}

#[derive(Clone)]
struct CalendarTools {
    store: Arc<Storage>,
    changed: Arc<AtomicBool>,
    work: Arc<tokio::sync::Semaphore>,
}
impl CalendarTools {
    fn execute(&self, name: &str, args: Value) -> Result<Value, String> {
        if !args.is_object() || args.as_object().unwrap().values().any(Value::is_null) {
            return Err(
                "Arguments must be an object; omit optional fields instead of null.".into(),
            );
        }
        match name {
            "calendar_list" => {
                let input: List = parse(args)?;
                let (events, truncated) =
                    self.store
                        .list_event_headers_limited(&input.from, &input.to, input.limit)?;
                let events: Vec<Value> = events
                    .into_iter()
                    .map(|event| {
                        let mut value = json!(event);
                        value.as_object_mut().unwrap().remove("notes");
                        value
                    })
                    .collect();
                Ok(json!({"events":events,"truncated":truncated}))
            }
            "calendar_get" => {
                let input: Get = parse(args)?;
                Ok(json!(self.store.get_event(&input.id)?))
            }
            "calendar_create" => {
                let input: Create = parse(args)?;
                let event = self.store.save_event(EventInput {
                    id: None,
                    title: input.title,
                    date: input.date,
                    start_time: input.start_time,
                    end_time: input.end_time,
                    all_day: input.all_day,
                    notes: input.notes,
                    color: input.color,
                    completed: input.completed,
                })?;
                self.changed.store(true, Ordering::Release);
                Ok(json!(event))
            }
            "calendar_update" => {
                if args.as_object().unwrap().len() <= 2 {
                    return Err("Provide at least one field to update.".into());
                }
                let input: Update = parse(args)?;
                if input.expected_updated_at.len() > 64 {
                    return Err("Invalid expectedUpdatedAt.".into());
                }
                let old = self.store.get_event(&input.id)?;
                let event = self.store.save_event_if_unchanged(
                    EventInput {
                        id: Some(old.id),
                        title: input.title.unwrap_or(old.title),
                        date: input.date.unwrap_or(old.date),
                        start_time: input.start_time.or(old.start_time),
                        end_time: input.end_time.or(old.end_time),
                        all_day: input.all_day.unwrap_or(old.all_day),
                        notes: input.notes.unwrap_or(old.notes),
                        color: input.color.unwrap_or(old.color),
                        completed: input.completed.unwrap_or(old.completed),
                    },
                    Some(&input.expected_updated_at),
                )?;
                self.changed.store(true, Ordering::Release);
                Ok(json!(event))
            }
            _ => Err("Unknown calendar tool.".into()),
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value)
        .map_err(|_| "Invalid or unknown tool arguments; check tools/list for the schema.".into())
}
fn default_limit() -> usize {
    100
}
fn all_day() -> bool {
    true
}
fn sage() -> String {
    "sage".into()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    from: String,
    to: String,
    #[serde(default = "default_limit")]
    limit: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Get {
    id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Create {
    title: String,
    date: String,
    start_time: Option<String>,
    end_time: Option<String>,
    #[serde(default = "all_day")]
    all_day: bool,
    #[serde(default)]
    notes: String,
    #[serde(default = "sage")]
    color: String,
    #[serde(default)]
    completed: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Update {
    id: String,
    expected_updated_at: String,
    title: Option<String>,
    date: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    all_day: Option<bool>,
    notes: Option<String>,
    color: Option<String>,
    completed: Option<bool>,
}

fn tools() -> Vec<Tool> {
    let fields = json!({
        "title":{"type":"string","minLength":1,"maxLength":200},
        "date":{"type":"string","description":"Local calendar date, YYYY-MM-DD"},
        "startTime":{"type":"string","description":"HH:mm, required when allDay is false"},
        "endTime":{"type":"string","description":"HH:mm, after startTime on the same day"},
        "allDay":{"type":"boolean"},"notes":{"type":"string","maxLength":20000},
        "color":{"type":"string","enum":["sage","blue","amber","rose"]},"completed":{"type":"boolean"}
    });
    let mut update = fields.clone();
    update["id"] = json!({"type":"string","format":"uuid"});
    update["expectedUpdatedAt"] = json!({"type":"string","description":"Exact updatedAt from the last detail/create/update response. Conflicts require re-reading before applying changes."});
    [
        ("calendar_list","List event headers for an inclusive range of at most 366 days. Notes are omitted; if truncated, narrow the dates. Dates/times are local, without timezone conversion.",json!({"from":{"type":"string"},"to":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":100,"default":100}}),vec!["from","to"],true),
        ("calendar_get","Read one full event including notes and updatedAt.",json!({"id":{"type":"string","format":"uuid"}}),vec!["id"],true),
        ("calendar_create","Create an event. Defaults: allDay=true, notes empty, color=sage, completed=false. A retry after a lost response can create a duplicate: list before retrying.",fields,vec!["title","date"],false),
        ("calendar_update","Partially update one event using expectedUpdatedAt. Omitted fields are preserved. Set allDay=true to clear times. Null is not accepted. Never blindly retry a conflict.",update,vec!["id","expectedUpdatedAt"],false),
    ].into_iter().map(|(name,description,properties,required,read_only)|serde_json::from_value(json!({
        "name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},
        "annotations":{"readOnlyHint":read_only,"destructiveHint":name=="calendar_update","idempotentHint":read_only,"openWorldHint":false}
    })).expect("static MCP tool schema")).collect()
}

impl ServerHandler for CalendarTools {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        info.protocol_version = ProtocolVersion::V_2025_11_25;
        info.server_info = Implementation::new("TimeSchedule", env!("CARGO_PKG_VERSION"));
        info
    }
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(vec![
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_03_26,
        ])
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(tools()))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if ![
            "calendar_list",
            "calendar_get",
            "calendar_create",
            "calendar_update",
        ]
        .contains(&request.name.as_ref())
        {
            return Err(ErrorData::invalid_params("Unknown calendar tool.", None));
        }
        let api = self.clone();
        let Ok(permit) = self.work.clone().try_acquire_owned() else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "Calendar is busy; retry after the current operations finish.",
            )])
            .into());
        };
        let result = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            api.execute(
                &request.name,
                Value::Object(request.arguments.unwrap_or_default()),
            )
        })
        .await;
        let result = match result {
            Ok(Ok(value)) => CallToolResult::success(vec![ContentBlock::text(value.to_string())]),
            Ok(Err(error)) => CallToolResult::error(vec![ContentBlock::text(
                if error.starts_with("Database operation") {
                    "Calendar storage is unavailable.".into()
                } else {
                    error
                },
            )]),
            Err(_) => CallToolResult::error(vec![ContentBlock::text("Calendar operation failed.")]),
        };
        Ok(result.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> CalendarTools {
        CalendarTools {
            store: Arc::new(Storage::open(":memory:").unwrap()),
            changed: Arc::new(AtomicBool::new(false)),
            work: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }
    #[test]
    fn tools_preserve_omitted_fields_reject_stale_writes_and_exclude_notes_from_lists() {
        let api = fixture();
        let created = api.execute("calendar_create", json!({"title":"회의","date":"2026-11-02","notes":"긴 메모","allDay":false,"startTime":"09:00","endTime":"10:00","color":"rose"})).unwrap();
        assert!(api.changed.swap(false, Ordering::AcqRel));
        let updated = api
            .execute(
                "calendar_update",
                json!({"id":created["id"],"expectedUpdatedAt":created["updatedAt"],"title":"수정"}),
            )
            .unwrap();
        assert_eq!(updated["notes"], "긴 메모");
        assert_eq!(updated["startTime"], "09:00");
        assert_eq!(updated["color"], "rose");
        assert!(api.changed.swap(false, Ordering::AcqRel));
        assert!(api
            .execute(
                "calendar_update",
                json!({"id":created["id"],"expectedUpdatedAt":created["updatedAt"],"title":"stale"})
            )
            .unwrap_err()
            .contains("conflict"));
        assert!(!api.changed.load(Ordering::Acquire));
        assert_eq!(
            api.execute("calendar_get", json!({"id":created["id"]}))
                .unwrap(),
            updated
        );
        let list = api
            .execute(
                "calendar_list",
                json!({"from":"2026-11-02","to":"2026-11-02","limit":1}),
            )
            .unwrap();
        assert!(list["events"][0].get("notes").is_none());
        assert!(api
            .execute("calendar_delete", json!({"id":created["id"]}))
            .is_err());
    }
    #[test]
    fn tools_reject_bad_input_without_writes() {
        let api = fixture();
        for args in [
            json!({"title":"","date":"2026-11-02"}),
            json!({"title":"x","date":"2026-02-30"}),
            json!({"title":"x","date":"2026-11-02","allDay":false,"startTime":"11:00","endTime":"10:00"}),
            json!({"title":"x","date":"2026-11-02","unexpected":true}),
            json!({"title":"x","date":"2026-11-02","notes":null}),
        ] {
            assert!(api.execute("calendar_create", args).is_err());
        }
        assert!(api
            .execute(
                "calendar_list",
                json!({"from":"2026-01-01","to":"2028-01-01"})
            )
            .is_err());
        assert!(!api.changed.load(Ordering::Acquire));
        assert!(api
            .store
            .list_events("2026-01-01", "2026-12-31")
            .unwrap()
            .is_empty());
    }
    #[test]
    fn open_gui_draft_cannot_overwrite_a_newer_calendar_update() {
        let api = fixture();
        let saved = api
            .store
            .save_event(timeschedule_core::storage::EventInput {
                id: None,
                title: "original".into(),
                date: "2026-11-02".into(),
                start_time: None,
                end_time: None,
                all_day: true,
                notes: "body".into(),
                color: "sage".into(),
                completed: false,
            })
            .unwrap();
        let mut draft = crate::editor::Form::event(saved.clone());
        draft.title = "my unsaved draft".into();
        api.store.set_event_completed(&saved.id, true).unwrap();
        assert!(draft.save(&api.store).unwrap_err().contains("conflict"));
        assert_eq!(draft.title, "my unsaved draft");
        assert_eq!(api.store.get_event(&saved.id).unwrap().title, "original");
    }
}
