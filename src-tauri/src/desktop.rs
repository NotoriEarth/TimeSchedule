//! Tauri window adapter for the shared, guarded Windows desktop module.
use tauri::WebviewWindow;
#[cfg(all(windows, test))]
#[allow(unused_imports)]
pub(crate) use timeschedule_desktop::windows::verify_layered_child_fixture;
pub use timeschedule_desktop::windows::{with_shell_dpi_context, AttachmentInfo, WIDGET_CLASS};
fn native(window: &WebviewWindow) -> Result<timeschedule_desktop::NativeWindow, String> {
    #[cfg(windows)]
    {
        let label = match window.label() {
            "main" => "main",
            "widget" => "widget",
            _ => return Err("알 수 없는 창입니다.".into()),
        };
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
        // Only used synchronously by the same UI-thread callers as before.
        Ok(unsafe { timeschedule_desktop::NativeWindow::tauri_window(hwnd, label) })
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Err("Windows에서만 지원합니다.".into())
    }
}
pub fn prepare_widget_class(w: &WebviewWindow) -> Result<(), String> {
    timeschedule_desktop::windows::prepare_widget_class(&native(w)?)
}
pub fn attach(w: &WebviewWindow) -> Result<AttachmentInfo, String> {
    timeschedule_desktop::windows::attach(&native(w)?)
}
pub fn status(w: &WebviewWindow) -> Result<AttachmentInfo, String> {
    timeschedule_desktop::windows::status(&native(w)?)
}
pub fn detach(w: &WebviewWindow) -> Result<(), String> {
    timeschedule_desktop::windows::detach(&native(w)?)
}
pub fn recover_position(w: &WebviewWindow) -> Result<(), String> {
    timeschedule_desktop::windows::recover_position(&native(w)?)
}
pub fn set_locked(w: &WebviewWindow, value: bool) -> Result<(), String> {
    timeschedule_desktop::windows::set_locked(&native(w)?, value)
}
pub fn set_opacity(w: &WebviewWindow, value: u8) -> Result<(), String> {
    timeschedule_desktop::windows::set_opacity(&native(w)?, value)
}
pub fn set_visible(w: &WebviewWindow, value: bool) -> Result<AttachmentInfo, String> {
    timeschedule_desktop::windows::set_visible(&native(w)?, value)
}
