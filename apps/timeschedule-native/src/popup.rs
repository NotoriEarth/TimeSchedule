//! Show separate windows only after positioning them over their origin.
use crate::app::{App, Message};
use iced::{window, Task};
use raw_window_handle::RawWindowHandle;
use timeschedule_desktop::popup;

fn hwnd(window: &dyn iced::window::Window) -> Result<isize, String> {
    match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get()),
        _ => Err("이 플랫폼에서는 팝업 위치를 지정할 수 없습니다.".into()),
    }
}
impl App {
    pub(crate) fn place_popup(&mut self, source: window::Id, target: window::Id) -> Task<Message> {
        let token = window::Id::unique();
        self.placing_popups.insert(target, token);
        let position = window::run(source, |window| hwnd(window).and_then(popup::anchor))
            .then(move |anchor| {
                window::run(target, move |window| {
                    anchor.and_then(|anchor| {
                        hwnd(window).and_then(|handle| popup::center(handle, anchor))
                    })
                })
            })
            .map(move |result| Message::PopupPositioned(target, token, result));
        // A source may disappear (Explorer recovery) before its UI callback runs.
        // Iced drops that callback; never leave the new popup permanently hidden.
        let fallback = Task::perform(
            async {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            },
            move |_| {
                Message::PopupPositioned(
                    target,
                    token,
                    Err("원본 창 위치를 확인하지 못해 기본 위치에 표시합니다.".into()),
                )
            },
        );
        Task::batch([position, fallback])
    }
    pub(crate) fn popup_positioned(
        &mut self,
        id: window::Id,
        token: window::Id,
        result: Result<(), String>,
    ) -> Task<Message> {
        if self.placing_popups.get(&id) != Some(&token) {
            return Task::none();
        }
        self.placing_popups.remove(&id);
        let jump = self
            .widget_ui
            .jump
            .as_ref()
            .filter(|jump| jump.window == id);
        if jump.is_none() && self.quick_window != Some(id) {
            return Task::none();
        }
        self.log(&format!("popup positioned {id:?}: {result:?}"));
        let reveal = window::set_mode(id, window::Mode::Windowed)
            .chain(window::minimize(id, false))
            .chain(window::gain_focus(id));
        if jump.is_some_and(|j| j.direct) {
            reveal.chain(iced::widget::operation::focus("jump-date"))
        } else if jump.is_none() && self.editor_in_quick {
            reveal.chain(iced::widget::operation::focus("editor-title"))
        } else {
            reveal
        }
    }
}
