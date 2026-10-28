use crate::storage::{CalendarEvent, EventInput, Note, NoteInput, Storage, Task, TaskInput};
use tauri::{AppHandle, Emitter, State};

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
