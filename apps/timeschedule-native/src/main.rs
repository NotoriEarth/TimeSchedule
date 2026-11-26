#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod calendar_ui;
mod calendar_viewport;
mod category_drag;
mod category_ui;
mod chat_review_ui;
mod date_choices;
mod date_input;
mod day_cell;
mod desktop_widget;
mod editor;
mod frozen;
mod icons;
mod launch;
mod mcp;
mod mcp_clients;
mod mcp_connection;
mod mcp_power;
mod mcp_setup;
mod mcp_setup_ui;
mod month_jump;
#[cfg(all(test, windows))]
mod native_alpha_tests;
mod popup;
mod portfolio_ui;
mod quick_editor;
mod record_cli;
mod record_inbox;
mod record_original;
mod record_sync;
mod shell;
mod text_layout;
mod theme;
mod time_input;
mod views;
mod widget_appearance;
mod widget_settings;
mod worker;

use app::App;
use iced::Font;
pub static DPI_INIT_ERROR: std::sync::OnceLock<String> = std::sync::OnceLock::new();
/// An argument that meant nothing; the window says so once it is open.
pub static IGNORED_ARGUMENT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match launch::route(&args) {
        launch::Route::Helper => std::process::exit(mcp_connection::run(&args)),
        launch::Route::Record => std::process::exit(record_cli::run(&args)),
        launch::Route::UnknownHelper => {
            eprintln!("알 수 없는 MCP 도우미 모드입니다. TimeSchedule 설정 → AI 연결 (MCP) → AI 연결 도우미에서 다시 등록하세요.");
            std::process::exit(2);
        }
        launch::Route::Window { ignored } => {
            if let Some(ignored) = ignored {
                let _ = IGNORED_ARGUMENT.set(ignored);
            }
        }
    }
    if let Err(error) = timeschedule_desktop::windows::initialize_process_dpi_for_shell() {
        let _ = DPI_INIT_ERROR.set(error);
    }
    if let Err(error) = iced::daemon(App::boot, App::update, App::view)
        .title(App::title)
        .default_font(Font::with_name("Malgun Gothic"))
        .theme(App::theme)
        .style(|_, theme| iced::theme::Style {
            background_color: iced::Color::TRANSPARENT,
            text_color: theme.palette().text,
        })
        .subscription(App::subscription)
        .run()
    {
        let message = format!("TimeSchedule 네이티브 앱을 시작하지 못했습니다.\n\n{error}");
        eprintln!("{message}");
        fatal_dialog(&message);
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn fatal_dialog(message: &str) {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, flags: u32) -> i32;
    }
    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = "TimeSchedule".encode_utf16().chain(Some(0)).collect();
    // Own-process startup failure; no existing app/window is modified.
    unsafe {
        MessageBoxW(0, message.as_ptr(), title.as_ptr(), 0x10);
    }
}
#[cfg(not(windows))]
fn fatal_dialog(_: &str) {}
