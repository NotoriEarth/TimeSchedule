mod commands;
mod desktop;
mod storage;

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    widget_x: Option<i32>,
    widget_y: Option<i32>,
    widget_locked: bool,
    widget_visible: bool,
    #[serde(default = "default_widget_opacity")]
    widget_opacity: u8,
}

fn default_widget_opacity() -> u8 {
    82
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            widget_x: None,
            widget_y: None,
            widget_locked: true,
            widget_visible: true,
            widget_opacity: default_widget_opacity(),
        }
    }
}

#[derive(Clone, Default)]
struct DesktopState {
    attached: bool,
    message: String,
}

struct AppState {
    directory: PathBuf,
    desktop: Mutex<DesktopState>,
    preferences: Mutex<Preferences>,
    widget_class: Result<(), String>,
    quitting: AtomicBool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    storage_path: String,
    version: &'static str,
    desktop_attached: bool,
    desktop_message: String,
    widget_locked: bool,
    widget_visible: bool,
    widget_opacity: u8,
}

fn log(app: &AppHandle, message: &str) {
    use std::io::Write;
    let state = app.state::<AppState>();
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.directory.join("prototype.log"))
    {
        let _ = writeln!(file, "{} {}", chrono::Utc::now().to_rfc3339(), message);
    }
    eprintln!("{message}");
}

fn save_preferences(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let preferences = state
        .preferences
        .lock()
        .map_err(|_| "설정 잠금을 열 수 없습니다.")?;
    let text = serde_json::to_string_pretty(&*preferences).map_err(|error| error.to_string())?;
    let temporary = state.directory.join("preferences.json.tmp");
    fs::write(&temporary, text).map_err(|error| error.to_string())?;
    fs::rename(temporary, state.directory.join("preferences.json"))
        .map_err(|error| error.to_string())
}

fn remember_widget_position(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("widget") {
        if let Ok(position) = window.outer_position() {
            if let Ok(mut preferences) = app.state::<AppState>().preferences.lock() {
                preferences.widget_x = Some(position.x);
                preferences.widget_y = Some(position.y);
            }
        }
    }
    if let Err(error) = save_preferences(app) {
        log(app, &format!("Save preferences failed: {error}"));
    }
}

fn record_desktop(
    app: &AppHandle,
    result: Result<desktop::AttachmentInfo, String>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut status = state
        .desktop
        .lock()
        .map_err(|_| "바탕화면 상태 잠금을 열 수 없습니다.")?;
    match result {
        Ok(info) => {
            status.attached = info.attached;
            status.message = info.message.clone();
            drop(status);
            log(
                app,
                &format!(
                    "Desktop: {}",
                    serde_json::to_string(&info).unwrap_or_default()
                ),
            );
            let _ = app.emit("desktop-changed", ());
            Ok(())
        }
        Err(error) => {
            status.attached = false;
            status.message = format!("바탕화면 연결을 확인해 주세요: {error}");
            drop(status);
            log(app, &format!("Desktop attach failed: {error}"));
            let _ = app.emit("desktop-changed", ());
            Err(error)
        }
    }
}

fn show_widget_inner(app: &AppHandle) -> Result<(), String> {
    let result = create_and_show_widget(app);
    if let Err(error) = &result {
        let _ = record_desktop(app, Err(error.clone()));
    }
    result
}

fn create_and_show_widget(app: &AppHandle) -> Result<(), String> {
    app.state::<AppState>().widget_class.clone()?;
    let window = if let Some(window) = app.get_webview_window("widget") {
        if !desktop::status(&window)?.attached {
            desktop::set_visible(&window, false)?;
            desktop::detach(&window)?;
        }
        window
    } else {
        let monitor = app.primary_monitor().map_err(|error| error.to_string())?;
        let preferences = app
            .state::<AppState>()
            .preferences
            .lock()
            .map_err(|_| "설정 잠금 오류")?
            .clone();
        let (x, y, width, height) = if let Some(monitor) = monitor {
            let scale = monitor.scale_factor();
            widget_layout(
                (monitor.position().x as f64, monitor.position().y as f64),
                (monitor.size().width as f64, monitor.size().height as f64),
                scale,
                (preferences.widget_x, preferences.widget_y),
            )
        } else {
            (80.0, 80.0, 900.0, 560.0)
        };
        desktop::with_shell_dpi_context(|| {
            WebviewWindowBuilder::new(
                app,
                "widget",
                WebviewUrl::App("index.html?view=widget".into()),
            )
            .title("TimeSchedule 바탕화면 달력")
            .window_classname(desktop::WIDGET_CLASS)
            .inner_size(width, height)
            .position(x, y)
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .skip_taskbar(true)
            .always_on_top(false)
            .visible(false)
            .focused(false)
            .build()
            .map_err(|error| error.to_string())
        })?
    };
    record_desktop(app, desktop::attach(&window))?;
    let locked = app
        .state::<AppState>()
        .preferences
        .lock()
        .map_err(|_| "설정 잠금 오류")?
        .widget_locked;
    desktop::set_locked(&window, locked)?;
    desktop::recover_position(&window)?;
    let opacity = app
        .state::<AppState>()
        .preferences
        .lock()
        .map_err(|_| "설정 잠금 오류")?
        .widget_opacity
        .clamp(40, 100);
    if let Err(error) = desktop::set_opacity(&window, opacity) {
        log(app, &format!("Widget translucency unavailable: {error}"));
        let _ = app.emit(
            "app-error",
            format!("반투명 표시를 적용하지 못했어요. {error}"),
        );
    }
    record_desktop(app, desktop::set_visible(&window, true))?;
    app.state::<AppState>()
        .preferences
        .lock()
        .map_err(|_| "설정 잠금 오류")?
        .widget_visible = true;
    remember_widget_position(app);
    save_preferences(app)
}

fn widget_layout(
    origin: (f64, f64),
    size: (f64, f64),
    scale: f64,
    saved: (Option<i32>, Option<i32>),
) -> (f64, f64, f64, f64) {
    let left = origin.0 / scale;
    let top = origin.1 / scale;
    let available_width = size.0 / scale;
    let available_height = size.1 / scale;
    let width = 900.0_f64.min((available_width - 48.0).max(1.0));
    let height = 560.0_f64.min((available_height - 80.0).max(1.0));
    let right = left + available_width - width;
    let bottom = top + available_height - height;
    let x = saved
        .0
        .map(|v| f64::from(v) / scale)
        .unwrap_or(right - 24.0)
        .clamp(left, right);
    let y = saved
        .1
        .map(|v| f64::from(v) / scale)
        .unwrap_or(top + 36.0)
        .clamp(top, bottom);
    (x, y, width, height)
}

fn show_manager_inner(app: &AppHandle, date: Option<String>) -> Result<(), String> {
    if let Some(ref value) = date {
        storage::validate_date(value)?;
    }
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|error| error.to_string())?;
        window.unminimize().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        if let Some(date) = date {
            window
                .emit("select-date", date)
                .map_err(|error| error.to_string())?;
        }
    } else {
        let url = date
            .map(|value| format!("index.html?date={value}"))
            .unwrap_or_else(|| "index.html".into());
        WebviewWindowBuilder::new(app, "main", WebviewUrl::App(url.into()))
            .title("TimeSchedule")
            .inner_size(1160.0, 820.0)
            .min_inner_size(800.0, 620.0)
            .center()
            .build()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn on_main_thread<T: Send + 'static>(
    app: AppHandle,
    action: impl FnOnce(AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = sender.send(action(handle));
    })
    .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv_timeout(std::time::Duration::from_secs(15))
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??
}

#[tauri::command]
async fn get_app_info(app: AppHandle) -> Result<AppInfo, String> {
    on_main_thread(app, |handle| {
        let state = handle.state::<AppState>();
        let mut status = state
            .desktop
            .lock()
            .map_err(|_| "바탕화면 상태 잠금을 열 수 없습니다.")?;
        let mut actually_visible = false;
        let mut actual_opacity = None;
        if let Some(window) = handle.get_webview_window("widget") {
            match desktop::status(&window) {
                Ok(info) => {
                    status.attached = info.attached;
                    actual_opacity = info.opacity_percent;
                    status.message = info.message;
                }
                Err(error) => {
                    status.attached = false;
                    status.message = error;
                }
            }
            actually_visible = status.attached && window.is_visible().unwrap_or(false);
        } else {
            status.attached = false;
            if status.message.is_empty() {
                status.message = "바탕화면 달력이 열려 있지 않습니다.".into();
            }
        }
        let preferences = state.preferences.lock().map_err(|_| "설정 잠금 오류")?;
        if status.attached && actual_opacity.is_none() && preferences.widget_opacity < 100 {
            status
                .message
                .push_str(" 반투명 처리가 적용되지 않아 불투명하게 표시합니다.");
        }
        Ok(AppInfo {
            storage_path: state.directory.join("schedule.db").display().to_string(),
            version: env!("CARGO_PKG_VERSION"),
            desktop_attached: status.attached,
            desktop_message: status.message.clone(),
            widget_locked: preferences.widget_locked,
            widget_visible: actually_visible,
            widget_opacity: actual_opacity.unwrap_or(100),
        })
    })
    .await
}

#[tauri::command]
async fn show_manager(app: AppHandle, date: Option<String>) -> Result<(), String> {
    on_main_thread(app, move |handle| show_manager_inner(&handle, date)).await
}

#[tauri::command]
async fn show_widget(app: AppHandle) -> Result<(), String> {
    on_main_thread(app, |handle| show_widget_inner(&handle)).await
}

#[tauri::command]
async fn hide_widget(app: AppHandle) -> Result<(), String> {
    on_main_thread(app, |handle| hide_widget_inner(&handle)).await
}

fn hide_widget_inner(app: &AppHandle) -> Result<(), String> {
    remember_widget_position(app);
    if let Some(window) = app.get_webview_window("widget") {
        desktop::set_visible(&window, false)?;
    }
    app.state::<AppState>()
        .preferences
        .lock()
        .map_err(|_| "설정 잠금 오류")?
        .widget_visible = false;
    save_preferences(app)?;
    let _ = app.emit("desktop-changed", ());
    Ok(())
}

#[tauri::command]
async fn set_widget_locked(app: AppHandle, locked: bool) -> Result<(), String> {
    on_main_thread(app, move |handle| {
        let window = handle
            .get_webview_window("widget")
            .ok_or("달력 위젯이 열려 있지 않습니다.")?;
        desktop::set_locked(&window, locked)?;
        handle
            .state::<AppState>()
            .preferences
            .lock()
            .map_err(|_| "설정 잠금 오류")?
            .widget_locked = locked;
        remember_widget_position(&handle);
        save_preferences(&handle)?;
        let _ = handle.emit("desktop-changed", ());
        Ok(())
    })
    .await
}

#[tauri::command]
async fn set_widget_opacity(app: AppHandle, opacity: u8) -> Result<(), String> {
    if !(40..=100).contains(&opacity) {
        return Err("불투명도는 40~100%로 설정해 주세요.".into());
    }
    on_main_thread(app, move |handle| {
        let window = handle
            .get_webview_window("widget")
            .ok_or("달력 위젯이 열려 있지 않습니다.")?;
        desktop::set_opacity(&window, opacity)?;
        handle
            .state::<AppState>()
            .preferences
            .lock()
            .map_err(|_| "설정 잠금 오류")?
            .widget_opacity = opacity;
        save_preferences(&handle)?;
        let _ = handle.emit("desktop-changed", ());
        Ok(())
    })
    .await
}

fn quit(app: &AppHandle) {
    remember_widget_position(app);
    app.state::<AppState>()
        .quitting
        .store(true, Ordering::SeqCst);
    app.exit(0);
}

pub fn run() {
    let application = tauri::Builder::default()
        .setup(|app| {
            let directory = std::env::var_os("TIMESCHEDULE_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?);
            fs::create_dir_all(&directory)?;
            let store = storage::Storage::open(directory.join("schedule.db"))
                .map_err(std::io::Error::other)?;
            let preferences = fs::read_to_string(directory.join("preferences.json"))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            let widget_class = app
                .get_webview_window("main")
                .ok_or_else(|| "위젯 창 클래스를 준비할 관리자 창이 없습니다.".to_string())
                .and_then(|main| desktop::prepare_widget_class(&main));
            app.manage(store);
            app.manage(AppState {
                directory,
                desktop: Mutex::new(DesktopState::default()),
                preferences: Mutex::new(preferences),
                widget_class,
                quitting: AtomicBool::new(false),
            });
            if let Err(error) = &app.state::<AppState>().widget_class {
                log(
                    app.handle(),
                    &format!("Widget class preparation failed: {error}"),
                );
            }

            let open = MenuItem::with_id(app, "open", "일정 관리 열기", true, None::<&str>)?;
            let widget =
                MenuItem::with_id(app, "widget", "바탕화면 달력 표시", true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", "바탕화면 달력 숨기기", true, None::<&str>)?;
            let recover = MenuItem::with_id(
                app,
                "recover",
                "바탕화면 달력 위치 복구",
                true,
                None::<&str>,
            )?;
            let exit = MenuItem::with_id(app, "quit", "완전 종료", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &widget, &recover, &hide, &exit])?;
            let mut tray = TrayIconBuilder::with_id("timeschedule-tray")
                .tooltip("TimeSchedule · 오늘을 차분하게")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    let result = match event.id.as_ref() {
                        "open" => show_manager_inner(app, None),
                        "widget" | "recover" => show_widget_inner(app),
                        "hide" => hide_widget_inner(app),
                        "quit" => {
                            quit(app);
                            Ok(())
                        }
                        _ => Ok(()),
                    };
                    if let Err(error) = result {
                        log(app, &error);
                        let _ = app.emit("app-error", error);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Err(error) = show_manager_inner(tray.app_handle(), None) {
                            log(tray.app_handle(), &error);
                        }
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            if app
                .state::<AppState>()
                .preferences
                .lock()
                .map_err(|_| std::io::Error::other("설정 잠금 오류"))?
                .widget_visible
            {
                if let Err(error) = show_widget_inner(app.handle()) {
                    log(app.handle(), &error);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "widget" && matches!(event, WindowEvent::Destroyed) {
                if let Ok(mut status) = window.state::<AppState>().desktop.lock() {
                    status.attached = false;
                    status.message =
                        "바탕화면 달력 연결이 종료되었습니다. 다시 표시해 주세요.".into();
                }
                let _ = window.app_handle().emit("desktop-changed", ());
            }
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    if !window.state::<AppState>().quitting.load(Ordering::SeqCst) {
                        api.prevent_close();
                        remember_widget_position(window.app_handle());
                        if let Err(error) = window.destroy() {
                            log(window.app_handle(), &error.to_string());
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_events,
            commands::save_event,
            commands::delete_event,
            commands::list_tasks,
            commands::save_task,
            commands::delete_task,
            commands::list_notes,
            commands::save_note,
            commands::delete_note,
            get_app_info,
            show_manager,
            show_widget,
            hide_widget,
            set_widget_locked,
            set_widget_opacity
        ])
        .build(tauri::generate_context!())
        .expect("TimeSchedule could not start");
    application.run(|app, event| {
        if let tauri::RunEvent::Ready = event {
            if let Some(window) = app.get_webview_window("main") {
                log(
                    app,
                    &format!(
                        "Main ready: visible={:?}, minimized={:?}, size={:?}, position={:?}",
                        window.is_visible(),
                        window.is_minimized(),
                        window.outer_size(),
                        window.outer_position()
                    ),
                );
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if !app.state::<AppState>().quitting.load(Ordering::SeqCst) {
                api.prevent_exit();
            }
        }
    });
}

#[cfg(test)]
mod preference_tests {
    use super::*;

    #[test]
    fn existing_preferences_keep_position_and_lock_when_opacity_is_added() {
        let old = r#"{"widgetX":2120,"widgetY":72,"widgetLocked":false,"widgetVisible":true}"#;
        let preferences: Preferences = serde_json::from_str(old).unwrap();
        assert_eq!(preferences.widget_x, Some(2120));
        assert!(!preferences.widget_locked);
        assert_eq!(preferences.widget_opacity, 82);
    }

    #[test]
    fn wider_calendar_clamps_old_small_widget_position_inside_monitor() {
        let (x, y, width, height) =
            widget_layout((0.0, 0.0), (2880.0, 1800.0), 2.0, (Some(2120), Some(72)));
        assert_eq!((width, height), (900.0, 560.0));
        assert_eq!(x, 540.0);
        assert_eq!(y, 36.0);
        assert!(x + width <= 1440.0 && y + height <= 900.0);
    }

    #[test]
    fn calendar_fits_small_monitor_and_recovers_offscreen_saved_position() {
        let (x, y, width, height) = widget_layout(
            (-1280.0, 0.0),
            (1280.0, 720.0),
            1.5,
            (Some(9000), Some(-9000)),
        );
        assert!(x >= -1280.0 / 1.5 && x + width <= 0.0);
        assert!(y >= 0.0 && y + height <= 480.0);
        assert!(width > 0.0 && height > 0.0);
    }
}
