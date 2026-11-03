#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod calendar_viewport;
mod desktop_widget;
mod editor;
mod frozen;
mod icons;
mod mcp;
mod month_jump;
#[cfg(all(test, windows))]
mod native_alpha_tests;
mod popup;
mod quick_editor;
mod record_cli;
mod record_inbox;
mod record_sync;
mod shell;
mod text_layout;
mod theme;
mod views;
mod widget_appearance;
mod widget_settings;
mod worker;

use app::App;
use iced::Font;
pub static DPI_INIT_ERROR: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn main() {
    if let Some(code) = record_cli::try_run() {
        std::process::exit(code);
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
