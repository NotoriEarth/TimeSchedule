#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    OpenDiary,
    ShowWidget,
    HideWidget,
    RecoverWidget,
    Quit,
}

#[cfg(windows)]
pub struct NativeTray {
    _icon: tray_icon::TrayIcon,
}
#[cfg(windows)]
impl NativeTray {
    /// Create, retain and drop on the framework's UI thread with a Windows
    /// message loop. Holding the tray in App prevents premature disposal.
    pub fn new() -> Result<Self, String> {
        use tray_icon::{
            menu::{Menu, MenuItem},
            Icon, TrayIconBuilder,
        };
        let menu = Menu::new();
        for (id, title) in [
            ("native-open", "다이어리 열기"),
            ("native-show", "위젯 표시"),
            ("native-hide", "위젯 숨기기"),
            ("native-recover", "위젯 위치 복구"),
            ("native-quit", "TimeSchedule Native 종료"),
        ] {
            menu.append(&MenuItem::with_id(id, title, true, None))
                .map_err(|e| e.to_string())?;
        }
        let mut pixels = vec![0u8; 16 * 16 * 4];
        for y in 0..16 {
            for x in 0..16 {
                let offset = (y * 16 + x) * 4;
                let color = if (2..14).contains(&x) && (2..14).contains(&y) {
                    if y < 5 {
                        [75, 107, 85, 255]
                    } else {
                        [246, 241, 220, 255]
                    }
                } else {
                    [0, 0, 0, 0]
                };
                pixels[offset..offset + 4].copy_from_slice(&color);
            }
        }
        let icon = Icon::from_rgba(pixels, 16, 16).map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_id("timeschedule-native")
            .with_tooltip("TimeSchedule Native · 다이어리")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_icon(icon)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self { _icon: tray })
    }
    pub fn try_recv(&self) -> Option<TrayAction> {
        use tray_icon::{menu::MenuEvent, MouseButton, MouseButtonState, TrayIconEvent};
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(action) = action_for_id(event.id.as_ref()) {
                return Some(action);
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                return Some(TrayAction::OpenDiary);
            }
        }
        None
    }
}
fn action_for_id(id: &str) -> Option<TrayAction> {
    match id {
        "native-open" => Some(TrayAction::OpenDiary),
        "native-show" => Some(TrayAction::ShowWidget),
        "native-hide" => Some(TrayAction::HideWidget),
        "native-recover" => Some(TrayAction::RecoverWidget),
        "native-quit" => Some(TrayAction::Quit),
        _ => None,
    }
}
#[cfg(not(windows))]
pub struct NativeTray;
#[cfg(not(windows))]
impl NativeTray {
    pub fn new() -> Result<Self, String> {
        Err("트레이는 Windows 빌드에서 지원합니다.".into())
    }
    pub fn try_recv(&self) -> Option<TrayAction> {
        None
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn unknown_ids_do_not_trigger_quit() {
        assert_eq!(super::action_for_id("quit"), None);
        assert_eq!(
            super::action_for_id("native-quit"),
            Some(super::TrayAction::Quit)
        );
    }
}
