use crate::diary::{
    Entry, EntryInput, EntrySummary, ImportResult, Project, ProjectInput, SearchHit,
};
use crate::storage::{CalendarEvent, EventInput, Note, NoteInput, Storage, Task, TaskInput};
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub fn open_source_url(url: String) -> Result<(), String> {
    crate::diary::open_source_url(&url)
}

#[tauri::command]
pub fn list_projects(storage: State<'_, Storage>) -> Result<Vec<Project>, String> {
    storage.list_projects()
}
#[tauri::command]
pub fn save_project(
    app: AppHandle,
    storage: State<'_, Storage>,
    input: ProjectInput,
) -> Result<Project, String> {
    let p = storage.save_project(input)?;
    notify_data_changed(&app)?;
    Ok(p)
}
#[tauri::command]
pub fn delete_project(
    app: AppHandle,
    storage: State<'_, Storage>,
    id: String,
) -> Result<(), String> {
    storage.delete_project(&id)?;
    notify_data_changed(&app)
}
#[tauri::command]
pub fn list_entries(
    storage: State<'_, Storage>,
    from: String,
    to: String,
    project_id: Option<String>,
    offset: u32,
) -> Result<Vec<EntrySummary>, String> {
    storage.list_entries(&from, &to, project_id.as_deref(), offset)
}
#[tauri::command]
pub fn get_entry(storage: State<'_, Storage>, id: String) -> Result<Entry, String> {
    storage.get_entry(&id)
}
#[tauri::command]
pub fn save_entry(
    app: AppHandle,
    storage: State<'_, Storage>,
    input: EntryInput,
) -> Result<Entry, String> {
    let e = storage.save_entry(input)?;
    notify_data_changed(&app)?;
    Ok(e)
}
#[tauri::command]
pub fn delete_entry(app: AppHandle, storage: State<'_, Storage>, id: String) -> Result<(), String> {
    storage.delete_entry(&id)?;
    notify_data_changed(&app)
}
#[tauri::command]
pub async fn search_diary(
    app: AppHandle,
    query: String,
    limit: u32,
) -> Result<Vec<SearchHit>, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Storage>().search_diary(&query, limit))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn import_git(app: AppHandle, project_id: String) -> Result<ImportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let result = app.state::<Storage>().import_git(&project_id)?;
        notify_data_changed(&app)?;
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn list_note_headers(storage: State<'_, Storage>) -> Result<Vec<Note>, String> {
    storage.list_note_headers()
}
#[tauri::command]
pub fn get_note(storage: State<'_, Storage>, id: String) -> Result<Note, String> {
    storage.get_note(&id)
}
#[tauri::command]
pub fn list_event_headers(
    storage: State<'_, Storage>,
    from: String,
    to: String,
) -> Result<Vec<CalendarEvent>, String> {
    storage.list_event_headers(&from, &to)
}

#[tauri::command]
pub fn list_events(
    storage: State<'_, Storage>,
    from: String,
    to: String,
) -> Result<Vec<CalendarEvent>, String> {
    storage.list_events(&from, &to)
}

#[tauri::command]
pub fn save_event(
    app: AppHandle,
    storage: State<'_, Storage>,
    input: EventInput,
) -> Result<CalendarEvent, String> {
    let event = storage.save_event(input)?;
    notify_data_changed(&app)?;
    Ok(event)
}

#[tauri::command]
pub fn delete_event(app: AppHandle, storage: State<'_, Storage>, id: String) -> Result<(), String> {
    storage.delete_event(&id)?;
    notify_data_changed(&app)
}

#[tauri::command]
pub fn list_tasks(storage: State<'_, Storage>) -> Result<Vec<Task>, String> {
    storage.list_tasks()
}

#[tauri::command]
pub fn save_task(
    app: AppHandle,
    storage: State<'_, Storage>,
    input: TaskInput,
) -> Result<Task, String> {
    let task = storage.save_task(input)?;
    notify_data_changed(&app)?;
    Ok(task)
}

#[tauri::command]
pub fn delete_task(app: AppHandle, storage: State<'_, Storage>, id: String) -> Result<(), String> {
    storage.delete_task(&id)?;
    notify_data_changed(&app)
}

#[tauri::command]
pub fn list_notes(storage: State<'_, Storage>) -> Result<Vec<Note>, String> {
    storage.list_notes()
}

#[tauri::command]
pub fn save_note(
    app: AppHandle,
    storage: State<'_, Storage>,
    input: NoteInput,
) -> Result<Note, String> {
    let note = storage.save_note(input)?;
    notify_data_changed(&app)?;
    Ok(note)
}

#[tauri::command]
pub fn delete_note(app: AppHandle, storage: State<'_, Storage>, id: String) -> Result<(), String> {
    storage.delete_note(&id)?;
    notify_data_changed(&app)
}

fn notify_data_changed(app: &AppHandle) -> Result<(), String> {
    app.emit("data-changed", ()).map_err(|error| {
        format!("Your change was saved, but other windows could not refresh. Reopen the window to see it. Details: {error}")
    })
}
