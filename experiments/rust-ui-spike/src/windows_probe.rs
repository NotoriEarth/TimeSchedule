//! Experimental adapter only. Keep all HWND operations inside window::run,
//! which executes on the owning event-loop thread. No stored raw window handles.
use raw_window_handle::RawWindowHandle;

use timeschedule_desktop::windows as desktop;

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Inspect,
    Attach,
    Detach,
    Opacity(u8),
    Lock(bool),
}

#[derive(Debug, Clone)]
pub struct Report {
    pub message: String,
    pub locked: bool,
}

pub fn run(window: &dyn iced::window::Window, action: Action) -> Result<Report, String> {
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return Err("Windows 전용 시제품입니다.".into());
    };
    let owned = unsafe { desktop::NativeWindow::iced_widget(raw.hwnd.get()) };
    match action {
        Action::Attach => {
            desktop::attach(&owned)?;
            desktop::set_visible(&owned, true)?;
        }
        Action::Detach => {
            desktop::detach(&owned)?;
            return Ok(Report {
                message: "위젯을 분리하고 숨겼습니다. 연결 버튼으로 다시 연결할 수 있습니다."
                    .into(),
                locked: false,
            });
        }
        Action::Opacity(percent) => desktop::set_opacity(&owned, percent)?,
        Action::Lock(value) => desktop::set_locked(&owned, value)?,
        Action::Inspect => {}
    }
    let info = desktop::status(&owned)?;
    Ok(Report {
        message: format!(
            "연결={} · 잠금={} · 투명도={:?}% · {}",
            info.attached, info.locked, info.opacity_percent, info.message
        ),
        locked: info.locked,
    })
}

#[cfg(all(windows, test))]
mod tests {
    #[test]
    fn owned_layered_child_preserves_parent_class() {
        super::desktop::verify_layered_child_fixture();
    }
}
