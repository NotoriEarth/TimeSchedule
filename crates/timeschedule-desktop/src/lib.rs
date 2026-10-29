pub mod paths;
pub mod preferences;
pub mod tray;
pub mod windows;

/// Borrowed HWND valid only during its framework's UI-thread callback.
/// !Send and !Sync: neither this reference nor its methods may cross threads.
pub struct NativeWindow {
    hwnd: isize,
    label: &'static str,
    layered_class: &'static str,
    _ui_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
pub struct NativeHandle(pub isize);
impl NativeWindow {
    /// # Safety
    /// hwnd must be this process's live Iced widget, on its owning UI thread,
    /// and must remain alive throughout every use of the returned adapter.
    pub unsafe fn iced_widget(hwnd: isize) -> Self {
        Self {
            hwnd,
            label: "widget",
            layered_class: "Window Class",
            _ui_thread: Default::default(),
        }
    }
    /// # Safety
    /// Same lifetime/ownership/thread requirements as iced_widget. The caller
    /// must supply the real Tauri label and use its prepared layered class.
    pub unsafe fn tauri_window(hwnd: isize, label: &'static str) -> Self {
        Self {
            hwnd,
            label,
            layered_class: windows::WIDGET_CLASS,
            _ui_thread: Default::default(),
        }
    }
    fn hwnd(&self) -> Result<NativeHandle, String> {
        Ok(NativeHandle(self.hwnd))
    }
    fn label(&self) -> &str {
        self.label
    }
    fn layered_class(&self) -> &str {
        self.layered_class
    }
}

pub fn open_source_url(url: &str) -> Result<(), String> {
    timeschedule_core::diary::validate_url(url)?;
    if url.is_empty() {
        return Err("출처 링크가 없습니다.".into());
    }
    #[cfg(windows)]
    {
        #[link(name = "shell32")]
        extern "system" {
            fn ShellExecuteW(
                hwnd: isize,
                operation: *const u16,
                file: *const u16,
                parameters: *const u16,
                directory: *const u16,
                show: i32,
            ) -> isize;
        }
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let address: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
        let result = unsafe {
            ShellExecuteW(
                0,
                verb.as_ptr(),
                address.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result <= 32 {
            return Err(format!("기본 브라우저를 열지 못했습니다 ({result})."));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("이 빌드의 외부 링크 열기는 Windows에서 지원합니다.".into())
    }
}
