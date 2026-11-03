//! Experimental Explorer desktop attachment, targeting Windows 10 22H2.
//!
//! Only our widget is changed. The existing desktop host is discovered, never
//! created, restyled, hidden, restarted or subclassed. See docs/desktop-research.md.
//! Call mutations on the widget's UI thread; create it hidden and undecorated.

use super::NativeWindow as WebviewWindow;
pub use super::NativeWindow;

/// Registered before widget creation; the manager retains Tao's original class.
pub const WIDGET_CLASS: &str = "TimeScheduleDesktopLayered";
/// Contract with the opt-in Win32 presenter in vendor/softbuffer.
pub const PER_PIXEL_ALPHA_PROPERTY: &str = "TimeSchedule.PerPixelAlpha.v1";

#[cfg(all(windows, any(test, feature = "test-support")))]
#[allow(unused_imports)]
pub use native::test_support::verify_layered_child_fixture;

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub attached: bool,
    pub strategy: String,
    pub message: String,
    pub hwnd: String,
    pub parent_hwnd: String,
    pub shell_host_hwnd: String,
    pub shell_class: String,
    pub locked: bool,
    pub opacity_percent: Option<u8>,
}

/// Authored physical screen position and logical (96-DPI) dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct GeometryEvent {
    /// Only explicit user moves/resizes change the persisted preference.
    pub preferred: Option<WidgetGeometry>,
    pub actual_size: (u32, u32),
    pub error: Option<String>,
}

#[cfg(not(windows))]
pub fn take_geometry_events() -> Vec<GeometryEvent> {
    Vec::new()
}
#[cfg(not(windows))]
pub fn configure_geometry(_: &WebviewWindow, _: WidgetGeometry) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub fn observe_display_changes(_: &WebviewWindow) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
pub use native::{
    attach, configure_geometry, detach, enable_per_pixel_alpha, initialize_process_dpi_for_shell,
    logical_size, observe_display_changes, position, prepare_widget_class, recover_position,
    resize_logical, restore_position, set_locked, set_opacity, set_visible, status,
    take_geometry_events, with_shell_dpi_context,
};

#[cfg(not(windows))]
pub fn resize_logical(_: &WebviewWindow, _: u32, _: u32) -> Result<(), String> {
    Err("Windows 전용 크기 조정입니다.".into())
}
#[cfg(not(windows))]
pub fn logical_size(_: &WebviewWindow) -> Result<Option<(u32, u32)>, String> {
    Ok(None)
}

#[cfg(not(windows))]
pub fn enable_per_pixel_alpha(_: &WebviewWindow) -> Result<(), String> {
    Err("픽셀 투명도는 Windows에서만 지원합니다.".into())
}

#[cfg(not(windows))]
pub fn prepare_widget_class(_: &WebviewWindow) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn initialize_process_dpi_for_shell() -> Result<(), String> {
    Err("Windows 바탕화면 기능은 이 플랫폼에서 지원하지 않습니다.".into())
}

#[cfg(not(windows))]
pub fn position(_: &WebviewWindow) -> Result<(i32, i32), String> {
    Err("Windows 바탕화면 기능은 이 플랫폼에서 지원하지 않습니다.".into())
}

#[cfg(not(windows))]
pub fn restore_position(_: &WebviewWindow, _: i32, _: i32) -> Result<(), String> {
    Err("Windows 바탕화면 기능은 이 플랫폼에서 지원하지 않습니다.".into())
}

#[cfg(not(windows))]
pub fn set_opacity(_: &WebviewWindow, _: u8) -> Result<(), String> {
    Err("바탕화면 투명도는 Windows에서만 지원합니다.".into())
}

#[cfg(not(windows))]
pub fn with_shell_dpi_context<T>(create: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    create()
}

#[cfg(not(windows))]
pub fn attach(_: &WebviewWindow) -> Result<AttachmentInfo, String> {
    Err("바탕화면 연결은 Windows 10/11에서만 지원하는 실험 기능입니다.".into())
}

#[cfg(not(windows))]
pub fn status(window: &WebviewWindow) -> Result<AttachmentInfo, String> {
    attach(window)
}

#[cfg(not(windows))]
pub fn detach(_: &WebviewWindow) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn recover_position(_: &WebviewWindow) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub fn set_locked(_: &WebviewWindow, _: bool) -> Result<(), String> {
    Err("바탕화면 위치 잠금은 Windows에서만 지원합니다.".into())
}

#[cfg(not(windows))]
pub fn set_visible(window: &WebviewWindow, _: bool) -> Result<AttachmentInfo, String> {
    attach(window)
}

#[cfg(windows)]
mod native {
    use super::{AttachmentInfo, GeometryEvent, WebviewWindow, WidgetGeometry, WIDGET_CLASS};
    use std::{
        collections::HashMap,
        sync::{Mutex, OnceLock},
    };

    // Pointer-sized integer handles keep the registry Send without unsafe Send
    // implementations. The ABI is the Win32 HWND / DPI_AWARENESS_CONTEXT ABI.
    type Hwnd = isize;
    type SubclassProc = unsafe extern "system" fn(Hwnd, u32, usize, isize, usize, usize) -> isize;
    type WindowProc = unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize;
    #[link(name = "user32")]
    extern "system" {
        fn GetPropW(hwnd: Hwnd, name: *const u16) -> isize;
        fn SetPropW(hwnd: Hwnd, name: *const u16, value: isize) -> i32;
    }
    const GWL_STYLE: i32 = -16;
    const GWL_EXSTYLE: i32 = -20;
    const GCL_STYLE: i32 = -26;
    const CS_OWNDC: u32 = 0x0020;
    const CS_CLASSDC: u32 = 0x0040;
    const GA_PARENT: u32 = 1;
    const GW_HWNDNEXT: u32 = 2;
    const GW_OWNER: u32 = 4;
    const WS_CHILD: u32 = 0x4000_0000;
    const WS_POPUP: u32 = 0x8000_0000;
    const WS_VISIBLE: u32 = 0x1000_0000;
    const WS_CAPTION: u32 = 0x00c0_0000;
    const WS_THICKFRAME: u32 = 0x0004_0000;
    const WS_SYSMENU: u32 = 0x0008_0000;
    const WS_MINIMIZEBOX: u32 = 0x0002_0000;
    const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
    const WS_CLIPCHILDREN: u32 = 0x0200_0000;
    const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
    const WS_EX_TOPMOST: u32 = 0x0000_0008;
    const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
    const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
    const WS_EX_WINDOWEDGE: u32 = 0x0000_0100;
    const WS_EX_APPWINDOW: u32 = 0x0004_0000;
    const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
    const WS_EX_LAYERED: u32 = 0x0008_0000;
    const LWA_ALPHA: u32 = 0x0000_0002;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_FRAMECHANGED: u32 = 0x0020;
    const SWP_SHOWWINDOW: u32 = 0x0040;
    const SWP_HIDEWINDOW: u32 = 0x0080;
    const SWP_NOOWNERZORDER: u32 = 0x0200;
    const SUBCLASS_ID: usize = 0x5449_4d45;
    const RECOVERY_TIMER_ID: usize = 0x5449_4d46;
    const DISPLAY_TIMER_ID: usize = 0x5449_4d47;
    const OBSERVER_SUBCLASS_ID: usize = 0x5449_4d48;
    const DISPLAY_CHANGED: u32 = 0x8000 + 0x317;
    const RECOVERY_INTERVAL_MS: u32 = 30_000;
    const DISPLAY_SETTLE_MS: u32 = 200;
    const STRATEGY: &str = "explorer-icon-host-child";

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    fn clamp_requested_rect(current: Rect, x: i32, y: i32, bounds: Rect) -> Result<Rect, String> {
        let available_width = i64::from(bounds.right) - i64::from(bounds.left);
        let available_height = i64::from(bounds.bottom) - i64::from(bounds.top);
        let current_width = i64::from(current.right) - i64::from(current.left);
        let current_height = i64::from(current.bottom) - i64::from(current.top);
        if available_width <= 0
            || available_height <= 0
            || current_width <= 0
            || current_height <= 0
        {
            return Err("위젯 또는 모니터 영역이 유효하지 않습니다.".into());
        }
        let width = current_width.min(available_width);
        let height = current_height.min(available_height);
        let left = i64::from(x).clamp(i64::from(bounds.left), i64::from(bounds.right) - width);
        let top = i64::from(y).clamp(i64::from(bounds.top), i64::from(bounds.bottom) - height);
        Ok(Rect {
            left: left as i32,
            top: top as i32,
            right: (left + width) as i32,
            bottom: (top + height) as i32,
        })
    }

    #[cfg(test)]
    mod restore_tests {
        use super::*;
        #[test]
        fn restores_physical_negative_monitor_and_clamps_offscreen() {
            let current = Rect {
                left: 0,
                top: 0,
                right: 400,
                bottom: 300,
            };
            let negative = Rect {
                left: -1920,
                top: 0,
                right: 0,
                bottom: 1040,
            };
            assert_eq!(
                clamp_requested_rect(current, -1600, 80, negative).unwrap(),
                Rect {
                    left: -1600,
                    top: 80,
                    right: -1200,
                    bottom: 380
                }
            );
            assert_eq!(
                clamp_requested_rect(current, 40000, -40000, negative).unwrap(),
                Rect {
                    left: -400,
                    top: 0,
                    right: 0,
                    bottom: 300
                }
            );
        }
        #[test]
        fn oversized_widget_fits_small_work_area() {
            let bounds = Rect {
                left: 50,
                top: 60,
                right: 350,
                bottom: 260,
            };
            assert_eq!(
                clamp_requested_rect(
                    Rect {
                        left: 0,
                        top: 0,
                        right: 500,
                        bottom: 400
                    },
                    i32::MAX,
                    i32::MIN,
                    bounds
                )
                .unwrap(),
                bounds
            );
            assert!(clamp_requested_rect(bounds, 0, 0, Rect::default()).is_err());
        }
    }

    #[repr(C)]
    #[derive(Default)]
    struct MonitorInfo {
        size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct WindowPos {
        hwnd: Hwnd,
        insert_after: Hwnd,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct WindowClass {
        size: u32,
        style: u32,
        procedure: Option<WindowProc>,
        class_extra: i32,
        window_extra: i32,
        instance: isize,
        icon: isize,
        cursor: isize,
        background: isize,
        menu_name: *const u16,
        class_name: *const u16,
        small_icon: isize,
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetShellWindow() -> Hwnd;
        fn GetDesktopWindow() -> Hwnd;
        fn EnumWindows(callback: unsafe extern "system" fn(Hwnd, isize) -> i32, data: isize)
            -> i32;
        fn FindWindowExW(parent: Hwnd, after: Hwnd, class: *const u16, title: *const u16) -> Hwnd;
        fn GetClassNameW(hwnd: Hwnd, value: *mut u16, count: i32) -> i32;
        fn GetClassInfoExW(instance: isize, name: *const u16, class: *mut WindowClass) -> i32;
        fn RegisterClassExW(class: *const WindowClass) -> u16;
        fn GetClassLongW(hwnd: Hwnd, index: i32) -> u32;
        fn GetWindowThreadProcessId(hwnd: Hwnd, process: *mut u32) -> u32;
        fn GetAncestor(hwnd: Hwnd, flags: u32) -> Hwnd;
        fn GetParent(hwnd: Hwnd) -> Hwnd;
        fn GetWindow(hwnd: Hwnd, command: u32) -> Hwnd;
        fn IsWindow(hwnd: Hwnd) -> i32;
        fn IsWindowVisible(hwnd: Hwnd) -> i32;
        fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn MapWindowPoints(from: Hwnd, to: Hwnd, points: *mut Point, count: u32) -> i32;
        // GWL_STYLE and GWL_EXSTYLE are 32-bit on both x86 and x64.
        fn GetWindowLongW(hwnd: Hwnd, index: i32) -> i32;
        fn SetWindowLongW(hwnd: Hwnd, index: i32, value: i32) -> i32;
        fn SetLayeredWindowAttributes(hwnd: Hwnd, key: u32, alpha: u8, flags: u32) -> i32;
        fn GetLayeredWindowAttributes(
            hwnd: Hwnd,
            key: *mut u32,
            alpha: *mut u8,
            flags: *mut u32,
        ) -> i32;
        fn SetParent(hwnd: Hwnd, parent: Hwnd) -> Hwnd;
        fn SetWindowPos(
            hwnd: Hwnd,
            after: Hwnd,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            flags: u32,
        ) -> i32;
        fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
        fn PostMessageW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> i32;
        fn GetWindowDpiAwarenessContext(hwnd: Hwnd) -> isize;
        fn SetProcessDpiAwarenessContext(context: isize) -> i32;
        fn GetThreadDpiAwarenessContext() -> isize;
        fn GetDpiForWindow(hwnd: Hwnd) -> u32;
        #[cfg(any(test, feature = "test-support"))]
        fn MonitorFromWindow(hwnd: Hwnd, flags: u32) -> isize;
        fn MonitorFromRect(rect: *const Rect, flags: u32) -> isize;
        fn MonitorFromPoint(point: Point, flags: u32) -> isize;
        fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
        fn SetTimer(
            hwnd: Hwnd,
            id: usize,
            interval: u32,
            callback: Option<unsafe extern "system" fn(Hwnd, u32, usize, u32)>,
        ) -> usize;
        fn KillTimer(hwnd: Hwnd, id: usize) -> i32;
        fn SetThreadDpiAwarenessContext(context: isize) -> isize;
        fn AreDpiAwarenessContextsEqual(a: isize, b: isize) -> i32;
        fn SendMessageTimeoutW(
            hwnd: Hwnd,
            message: u32,
            wparam: usize,
            lparam: isize,
            flags: u32,
            timeout: u32,
            result: *mut usize,
        ) -> isize;
        fn SendMessageW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetLastError() -> u32;
        fn SetLastError(code: u32);
        fn GetCurrentThreadId() -> u32;
        fn GetCurrentProcessId() -> u32;
        fn GetModuleHandleW(name: *const u16) -> isize;
    }

    #[link(name = "comctl32")]
    extern "system" {
        fn SetWindowSubclass(hwnd: Hwnd, callback: SubclassProc, id: usize, data: usize) -> i32;
        fn RemoveWindowSubclass(hwnd: Hwnd, callback: SubclassProc, id: usize) -> i32;
        fn DefSubclassProc(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    }

    #[derive(Clone)]
    struct Attachment {
        host: Hwnd,
        view: Hwnd,
        shell_pid: u32,
        original_style: u32,
        original_exstyle: u32,
        original_rect: Rect,
        locked: bool,
        recovering: bool,
        moving: bool,
        geometry_error: Option<String>,
        preferred: WidgetGeometry,
        move_start: Option<WidgetGeometry>,
        pending_event: Option<GeometryEvent>,
        last_geometry: WidgetGeometry,
    }

    fn records() -> &'static Mutex<HashMap<Hwnd, Attachment>> {
        static RECORDS: OnceLock<Mutex<HashMap<Hwnd, Attachment>>> = OnceLock::new();
        RECORDS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn record(hwnd: Hwnd) -> Result<Option<Attachment>, String> {
        Ok(records()
            .lock()
            .map_err(|_| "바탕화면 상태 잠금 오류")?
            .get(&hwnd)
            .cloned())
    }

    fn error(operation: &str) -> String {
        let code = unsafe { GetLastError() };
        error_code(operation, code)
    }

    fn error_code(operation: &str, code: u32) -> String {
        format!(
            "{operation}: Win32 {code} ({})",
            std::io::Error::from_raw_os_error(code as i32)
        )
    }

    fn hwnd(window: &WebviewWindow) -> Result<Hwnd, String> {
        // Restrict this module to the dedicated widget, never the manager.
        if window.label() != "widget" {
            return Err("바탕화면 모듈은 widget 창에만 적용할 수 있습니다.".into());
        }
        let handle = window.hwnd().map_err(|e| e.to_string())?.0 as Hwnd;
        if unsafe { IsWindow(handle) } == 0 {
            return Err("위젯 창이 없어졌습니다. 위젯 창을 다시 만들어야 합니다.".into());
        }
        Ok(handle)
    }

    fn require_ui_thread(hwnd: Hwnd) -> Result<(), String> {
        let mut process = 0;
        let thread = unsafe { GetWindowThreadProcessId(hwnd, &mut process) };
        if thread != unsafe { GetCurrentThreadId() } || process != unsafe { GetCurrentProcessId() }
        {
            return Err("바탕화면 변경은 위젯 창의 UI 스레드에서 실행해야 합니다.".into());
        }
        Ok(())
    }

    fn class_name(hwnd: Hwnd) -> String {
        let mut value = [0u16; 256];
        let length = unsafe { GetClassNameW(hwnd, value.as_mut_ptr(), value.len() as i32) };
        String::from_utf16_lossy(&value[..length.max(0) as usize])
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn class_info(instance: isize, name: &str) -> Result<WindowClass, String> {
        // Every field is an integer, nullable function pointer or raw pointer.
        let mut class: WindowClass = unsafe { std::mem::zeroed() };
        class.size = std::mem::size_of::<WindowClass>() as u32;
        if unsafe { GetClassInfoExW(instance, wide(name).as_ptr(), &mut class) } == 0 {
            return Err(error("GetClassInfoExW"));
        }
        Ok(class)
    }

    fn compatible_class(actual: &WindowClass, expected: &WindowClass) -> bool {
        actual.style == expected.style
            && actual.procedure.map(|procedure| procedure as usize)
                == expected.procedure.map(|procedure| procedure as usize)
            && actual.instance == expected.instance
            && actual.class_extra == expected.class_extra
            && actual.window_extra == expected.window_extra
            && actual.icon == expected.icon
            && actual.small_icon == expected.small_icon
            && actual.cursor == expected.cursor
            && actual.background == expected.background
            && actual.menu_name == expected.menu_name
    }

    fn prepare_class(source: Hwnd, target_name: &str) -> Result<(), String> {
        require_ui_thread(source)?;
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        if instance == 0 {
            return Err(error("GetModuleHandleW"));
        }
        let mut expected = class_info(instance, &class_name(source))?;
        // Tao registers CS_OWNDC for general graphics clients. WebView2 does
        // not need a private GDI DC; layered HWNDs explicitly forbid that bit.
        // Clone to a separate name: never mutate the manager's existing class.
        expected.style &= !(CS_OWNDC | CS_CLASSDC);
        let name = wide(target_name);
        expected.class_name = name.as_ptr();
        match class_info(instance, target_name) {
            Ok(existing) => {
                if !compatible_class(&existing, &expected) {
                    return Err(
                        "위젯 전용 창 클래스 이름이 호환되지 않는 클래스와 충돌합니다.".into(),
                    );
                }
            }
            Err(_) => {
                if unsafe { RegisterClassExW(&expected) } == 0 {
                    return Err(error("RegisterClassExW(widget)"));
                }
            }
        }
        let actual = class_info(instance, target_name)?;
        if !compatible_class(&actual, &expected) {
            return Err("위젯 창 클래스 등록 후 속성 검증에 실패했습니다.".into());
        }
        Ok(())
    }

    /// Call once while the normal manager exists, before building the widget
    /// with `.window_classname(WIDGET_CLASS)`. The pinned Tao runtime tolerates
    /// this class already being registered and creates its HWND with this copy
    /// of its own WndProc and layout. All original manager-class fields stay put.
    pub fn prepare_widget_class(main: &WebviewWindow) -> Result<(), String> {
        if main.label() != "main" {
            return Err("위젯 창 클래스는 main 창을 기준으로 준비해야 합니다.".into());
        }
        let source = main.hwnd().map_err(|error| error.to_string())?.0 as Hwnd;
        prepare_class(source, WIDGET_CLASS)
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct LayerAttributes {
        key: u32,
        alpha: u8,
        flags: u32,
    }

    fn layer_attributes(hwnd: Hwnd) -> Result<LayerAttributes, String> {
        let mut attributes = LayerAttributes {
            key: 0,
            alpha: 0,
            flags: 0,
        };
        if unsafe {
            GetLayeredWindowAttributes(
                hwnd,
                &mut attributes.key,
                &mut attributes.alpha,
                &mut attributes.flags,
            )
        } == 0
        {
            return Err(error("GetLayeredWindowAttributes"));
        }
        Ok(attributes)
    }

    fn opacity_percent(hwnd: Hwnd, exstyle: u32) -> Result<Option<u8>, String> {
        if unsafe { GetPropW(hwnd, wide(super::PER_PIXEL_ALPHA_PROPERTY).as_ptr()) } != 0 {
            // Per-pixel presentation has no constant window-alpha attribute.
            return Ok(None);
        }
        if exstyle & WS_EX_LAYERED == 0 {
            return Ok(None);
        }
        let attributes = layer_attributes(hwnd)?;
        if attributes.flags & LWA_ALPHA == 0 {
            return Ok(None);
        }
        Ok(Some(
            ((u32::from(attributes.alpha) * 100 + 127) / 255) as u8,
        ))
    }

    /// Enable premultiplied BGRA presentation only on our native widget.
    /// No SetLayeredWindowAttributes call may follow: it disables ULW updates.
    pub fn enable_per_pixel_alpha(window: &WebviewWindow) -> Result<(), String> {
        let handle = hwnd(window)?;
        require_ui_thread(handle)?;
        if class_name(handle) != window.layered_class()
            || unsafe { GetClassLongW(handle, GCL_STYLE) } & (CS_OWNDC | CS_CLASSDC) != 0
        {
            return Err("위젯 창 클래스가 픽셀 투명도와 호환되지 않습니다.".into());
        }
        let property = wide(super::PER_PIXEL_ALPHA_PROPERTY);
        if unsafe { GetPropW(handle, property.as_ptr()) } != 0 {
            return configure_native_sizing(handle);
        }
        let previous = unsafe { GetWindowLongW(handle, GWL_EXSTYLE) } as u32;
        // Clear any constant-alpha redirection, then opt into the presenter.
        set_style(handle, GWL_EXSTYLE, previous & !WS_EX_LAYERED)?;
        set_style(handle, GWL_EXSTYLE, previous | WS_EX_LAYERED)?;
        if unsafe { SetPropW(handle, property.as_ptr(), 1) } == 0 {
            let failure = error("SetPropW(per-pixel alpha)");
            set_style(handle, GWL_EXSTYLE, previous)?;
            return Err(failure);
        }
        configure_native_sizing(handle)
    }

    fn configure_native_sizing(hwnd: Hwnd) -> Result<(), String> {
        require_ui_thread(hwnd)?;
        let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
        if style & WS_CHILD == 0
            || unsafe { GetPropW(hwnd, wide(super::PER_PIXEL_ALPHA_PROPERTY).as_ptr()) } == 0
        {
            return Err("네이티브 바탕화면 위젯만 크기 조정 프레임을 사용할 수 있습니다.".into());
        }
        // WM_NCCALCSIZE below keeps the entire window as client area. Retain
        // WS_THICKFRAME so DefWindowProc can actually enter the OS size loop.
        // Never use winit's style setters: they still cache a top-level window.
        set_style(hwnd, GWL_STYLE, style | WS_THICKFRAME)?;
        if unsafe {
            SetWindowPos(
                hwnd,
                0,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            )
        } == 0
        {
            return Err(error("SetWindowPos(native sizing frame)"));
        }
        Ok(())
    }

    fn set_native_opacity(hwnd: Hwnd, percent: u8, expected_class: &str) -> Result<(), String> {
        if !(40..=100).contains(&percent) {
            return Err("위젯 불투명도는 40~100% 범위여야 합니다.".into());
        }
        require_ui_thread(hwnd)?;
        if class_name(hwnd) != expected_class {
            return Err("반투명 표시는 준비된 위젯 전용 창 클래스에서만 지원합니다.".into());
        }
        if unsafe { GetClassLongW(hwnd, GCL_STYLE) } & (CS_OWNDC | CS_CLASSDC) != 0 {
            return Err(
                "위젯 창 클래스의 전용 DC 스타일은 Windows 반투명 창과 호환되지 않습니다.".into(),
            );
        }
        let previous_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
        let previous_attributes = if previous_style & WS_EX_LAYERED != 0 {
            Some(layer_attributes(hwnd)?)
        } else {
            None
        };
        let alpha = ((u32::from(percent) * 255 + 50) / 100) as u8;
        let result = (|| {
            set_style(hwnd, GWL_EXSTYLE, previous_style | WS_EX_LAYERED)?;
            if unsafe { IsWindow(hwnd) } == 0 {
                return Err("투명도 스타일 변경 후 위젯 HWND가 유효하지 않습니다.".into());
            }
            if unsafe { SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA) } == 0 {
                let failure = error("SetLayeredWindowAttributes");
                return Err(format!(
                    "{failure} (HWND={hwnd:#x}, valid={})",
                    unsafe { IsWindow(hwnd) } != 0
                ));
            }
            let actual = layer_attributes(hwnd)?;
            let applied_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
            if actual.alpha != alpha
                || actual.flags != LWA_ALPHA
                || applied_style != previous_style | WS_EX_LAYERED
            {
                return Err("위젯 투명도 설정 후 네이티브 값 검증에 실패했습니다.".into());
            }
            Ok(())
        })();
        if let Err(original_error) = result {
            let rollback = set_style(hwnd, GWL_EXSTYLE, previous_style).and_then(|_| {
                if let Some(previous) = previous_attributes {
                    if unsafe {
                        SetLayeredWindowAttributes(
                            hwnd,
                            previous.key,
                            previous.alpha,
                            previous.flags,
                        )
                    } == 0
                    {
                        return Err(error("SetLayeredWindowAttributes(rollback)"));
                    }
                }
                Ok(())
            });
            return Err(match rollback {
                Ok(()) => original_error,
                Err(error) => format!("{original_error}; 투명도 복구 실패: {error}"),
            });
        }
        Ok(())
    }

    /// Apply constant opacity to the entire calendar, including its text. This
    /// is native layered-child alpha, not per-pixel WebView or backdrop blur.
    pub fn set_opacity(window: &WebviewWindow, percent: u8) -> Result<(), String> {
        let handle = hwnd(window)?;
        if !status(window)?.attached {
            return Err("위젯이 바탕화면에 연결되어 있지 않습니다.".into());
        }
        set_native_opacity(handle, percent, window.layered_class())
    }

    fn icon_view(host: Hwnd) -> Hwnd {
        let class: Vec<u16> = "SHELLDLL_DefView\0".encode_utf16().collect();
        unsafe { FindWindowExW(host, 0, class.as_ptr(), std::ptr::null()) }
    }

    struct Search {
        pid: u32,
        hosts: Vec<Hwnd>,
    }

    unsafe extern "system" fn enum_host(candidate: Hwnd, data: isize) -> i32 {
        // EnumWindows invokes this synchronously while Search remains borrowed.
        let search = &mut *(data as *mut Search);
        let mut pid = 0;
        GetWindowThreadProcessId(candidate, &mut pid);
        if pid == search.pid && IsWindowVisible(candidate) != 0 {
            let class = class_name(candidate);
            if (class == "Progman" || class == "WorkerW") && icon_view(candidate) != 0 {
                search.hosts.push(candidate);
            }
        }
        1
    }

    fn desktop_host() -> Result<(Hwnd, u32), String> {
        let shell = unsafe { GetShellWindow() };
        let mut pid = 0;
        if shell == 0 || unsafe { GetWindowThreadProcessId(shell, &mut pid) } == 0 {
            return Err(
                "Explorer 바탕화면을 찾을 수 없습니다. 일반 Windows 바탕화면에서 다시 시도하세요."
                    .into(),
            );
        }
        let mut search = Search {
            pid,
            hosts: Vec::new(),
        };
        if unsafe { EnumWindows(enum_host, &mut search as *mut Search as isize) } == 0 {
            return Err(error("EnumWindows"));
        }
        // Ambiguity is unsupported rather than choosing an unrelated WorkerW.
        match search.hosts.as_slice() {
            [host] => Ok((*host, pid)),
            [] => Err("Explorer의 바탕화면 아이콘 호스트를 찾지 못했습니다. 이 셸 구성에서는 바탕화면 연결을 지원하지 않습니다.".into()),
            _ => Err("바탕화면 아이콘 호스트가 여러 개여서 안전하게 선택할 수 없습니다.".into()),
        }
    }

    struct DpiGuard(isize);
    impl DpiGuard {
        fn enter(context: isize) -> Result<Self, String> {
            if context == 0 {
                return Err("창의 DPI 컨텍스트를 확인할 수 없습니다.".into());
            }
            let old = unsafe { SetThreadDpiAwarenessContext(context) };
            if old == 0 {
                return Err(error("SetThreadDpiAwarenessContext"));
            }
            Ok(Self(old))
        }
    }
    impl Drop for DpiGuard {
        fn drop(&mut self) {
            unsafe {
                SetThreadDpiAwarenessContext(self.0);
            }
        }
    }

    /// Run a synchronous widget builder on Tauri's main thread. This changes only
    /// this thread for this scope; it does not change the manager/process context.
    pub fn with_shell_dpi_context<T>(
        create: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let (host, _) = desktop_host()?;
        let _guard = DpiGuard::enter(unsafe { GetWindowDpiAwarenessContext(host) })?;
        create()
    }

    /// Call before constructing the native GUI/event loop. This changes only
    /// our process; cross-process SetParent is still guarded by live HWND DPI
    /// equality. Framework attempts to change an already-set process context
    /// will be rejected by Windows rather than silently changing Explorer.
    pub fn initialize_process_dpi_for_shell() -> Result<(), String> {
        let (host, _) = desktop_host()?;
        let context = unsafe { GetWindowDpiAwarenessContext(host) };
        if context == 0 {
            return Err("Explorer DPI 컨텍스트를 확인하지 못했습니다.".into());
        }
        if unsafe { SetProcessDpiAwarenessContext(context) } == 0 {
            let current = unsafe { GetThreadDpiAwarenessContext() };
            if unsafe { AreDpiAwarenessContextsEqual(current, context) } == 0 {
                return Err("앱의 DPI 컨텍스트가 이미 설정되어 Explorer와 다릅니다. 바탕화면 연결은 안전하게 거부됩니다.".into());
            }
        }
        Ok(())
    }

    pub fn position(window: &WebviewWindow) -> Result<(i32, i32), String> {
        let hwnd = hwnd(window)?;
        require_ui_thread(hwnd)?;
        // Persist actual physical screen coordinates, independent of the
        // manager/window's awareness and DPI virtualization on other monitors.
        let _physical = DpiGuard::enter(-4)?; // PER_MONITOR_AWARE_V2 on this thread
        let rectangle = rect(hwnd)?;
        Ok((rectangle.left, rectangle.top))
    }

    /// Restore saved physical screen coordinates before parenting. The nearest
    /// surviving monitor is chosen from the requested rectangle, including
    /// negative virtual-desktop coordinates. Only our own unparented widget is
    /// moved; fully offscreen/oversized rectangles fit into its work area.
    pub fn restore_position(window: &WebviewWindow, x: i32, y: i32) -> Result<(), String> {
        let hwnd = hwnd(window)?;
        require_ui_thread(hwnd)?;
        if record(hwnd)?.is_some()
            || unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32 & WS_CHILD != 0
        {
            return Err("저장 위치 복원은 바탕화면 연결 전에 수행해야 합니다.".into());
        }
        let _physical = DpiGuard::enter(-4)?;
        let current = rect(hwnd)?;
        let width = (i64::from(current.right) - i64::from(current.left))
            .clamp(1, i64::from(i32::MAX)) as i32;
        let height = (i64::from(current.bottom) - i64::from(current.top))
            .clamp(1, i64::from(i32::MAX)) as i32;
        let left = x.min(i32::MAX - width);
        let top = y.min(i32::MAX - height);
        let requested = Rect {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let monitor = unsafe { MonitorFromRect(&requested, 2) };
        let mut info = MonitorInfo {
            size: std::mem::size_of::<MonitorInfo>() as u32,
            ..Default::default()
        };
        if monitor == 0 || unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return Err(error("GetMonitorInfoW(restore position)"));
        }
        let target = clamp_requested_rect(current, x, y, info.work)?;
        if unsafe {
            SetWindowPos(
                hwnd,
                0,
                target.left,
                target.top,
                target.right - target.left,
                target.bottom - target.top,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )
        } == 0
        {
            return Err(error("SetWindowPos(restore position)"));
        }
        if rect(hwnd)? != target {
            return Err("실제 위젯 위치가 요청한 물리 좌표와 다릅니다.".into());
        }
        Ok(())
    }

    fn rect(hwnd: Hwnd) -> Result<Rect, String> {
        let mut rect = Rect::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
            return Err(error("GetWindowRect"));
        }
        Ok(rect)
    }

    fn to_parent(screen: Rect, host: Hwnd) -> Result<Rect, String> {
        let mut points = [
            Point {
                x: screen.left,
                y: screen.top,
            },
            Point {
                x: screen.right,
                y: screen.bottom,
            },
        ];
        unsafe {
            SetLastError(0);
            if MapWindowPoints(0, host, points.as_mut_ptr(), 2) == 0 && GetLastError() != 0 {
                return Err(error("MapWindowPoints"));
            }
        }
        Ok(Rect {
            left: points[0].x,
            top: points[0].y,
            right: points[1].x,
            bottom: points[1].y,
        })
    }

    fn set_style(hwnd: Hwnd, index: i32, value: u32) -> Result<(), String> {
        unsafe {
            SetLastError(0);
            let previous = SetWindowLongW(hwnd, index, value as i32);
            let set_error = GetLastError();
            // Honor the API's documented success result. Windows may normalize
            // requested frame bits; callers verify their semantic invariants.
            if previous != 0 || set_error == 0 {
                return Ok(());
            }
            // A previous style of zero is a valid success return. Style-change
            // handlers run reentrantly and can leave their own last error; the
            // native fixture exposed that case (error 6 with the value applied).
            // Only rescue this ambiguous error when the live window has the
            // exact requested value, with a successful readback.
            SetLastError(0);
            let actual = GetWindowLongW(hwnd, index);
            let read_error = GetLastError();
            if IsWindow(hwnd) != 0 && actual as u32 == value && (actual != 0 || read_error == 0) {
                return Ok(());
            }
            Err(format!(
                "{} (HWND={hwnd:#x}, index={index}, 요청={value:#x}, 실제={:#x}, 읽기 오류={read_error})",
                error_code("SetWindowLongW", set_error),
                actual as u32,
            ))
        }
    }

    fn apply_child_styles(hwnd: Hwnd, style: u32, exstyle: u32) -> Result<(), String> {
        let child_style = (style
            & !(WS_POPUP
                | WS_CAPTION
                | WS_THICKFRAME
                | WS_SYSMENU
                | WS_MINIMIZEBOX
                | WS_MAXIMIZEBOX))
            | WS_CHILD
            | WS_CLIPCHILDREN
            | WS_CLIPSIBLINGS;
        set_style(hwnd, GWL_STYLE, child_style)?;
        // Windows removes WINDOWEDGE when the top-level frame becomes a
        // borderless child. Exclude the captured top-level edge explicitly so
        // the requested value agrees with the effective child-window style.
        set_style(
            hwnd,
            GWL_EXSTYLE,
            (exstyle & !(WS_EX_APPWINDOW | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_WINDOWEDGE))
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE,
        )
    }

    fn set_parent(hwnd: Hwnd, parent: Hwnd) -> Result<(), String> {
        unsafe {
            SetLastError(0);
            let previous = SetParent(hwnd, parent);
            if previous == 0 && GetLastError() != 0 {
                return Err(error("SetParent"));
            }
            if parent != 0 && (GetAncestor(hwnd, GA_PARENT) != parent || GetParent(hwnd) != parent)
            {
                return Err(
                    "SetParent 이후 실제 부모 HWND가 요청한 바탕화면 호스트와 다릅니다.".into(),
                );
            }
            // SetParent(NULL) restores the desktop as parent. WS_CHILD is
            // removed by restore() afterwards, as required by the API.
            if parent == 0 && GetAncestor(hwnd, GA_PARENT) != GetDesktopWindow() {
                return Err("위젯을 바탕화면 호스트에서 분리하지 못했습니다.".into());
            }
        }
        Ok(())
    }

    fn position_native(hwnd: Hwnd, after: Hwnd, r: Rect) -> Result<(), String> {
        let flags = SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_FRAMECHANGED;
        if unsafe {
            SetWindowPos(
                hwnd,
                after,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                flags,
            )
        } == 0
        {
            return Err(error("SetWindowPos"));
        }
        Ok(())
    }

    fn restore(hwnd: Hwnd, saved: &Attachment, target: Rect) -> Result<(), String> {
        // Remove our callback before restoring geometry; otherwise the lock
        // correctly refuses the restoration move. Only our HWND is touched.
        unsafe {
            KillTimer(hwnd, RECOVERY_TIMER_ID);
            KillTimer(hwnd, DISPLAY_TIMER_ID);
            RemoveWindowSubclass(hwnd, subclass, SUBCLASS_ID);
        }
        set_parent(hwnd, 0)?;
        set_style(hwnd, GWL_STYLE, saved.original_style & !WS_VISIBLE)?;
        set_style(hwnd, GWL_EXSTYLE, saved.original_exstyle)?;
        // Original windows are required to be non-topmost.
        position_native(hwnd, -2, target)
    }

    pub fn attach(window: &WebviewWindow) -> Result<AttachmentInfo, String> {
        let hwnd = hwnd(window)?;
        require_ui_thread(hwnd)?;
        if record(hwnd)?.is_some() {
            let info = status(window)?;
            if info.attached {
                return Ok(info);
            }
            return Err(
                "기존 바탕화면 연결이 변경되었습니다. 위젯을 분리하거나 다시 생성한 후 연결하세요."
                    .into(),
            );
        }
        let (host, shell_pid) = desktop_host()?;
        let own_dpi = unsafe { GetWindowDpiAwarenessContext(hwnd) };
        let host_dpi = unsafe { GetWindowDpiAwarenessContext(host) };
        if own_dpi == 0
            || host_dpi == 0
            || unsafe { AreDpiAwarenessContextsEqual(own_dpi, host_dpi) } == 0
        {
            return Err(format!("DPI 컨텍스트가 달라 바탕화면 연결을 중단했습니다 (widget={own_dpi:#x}, shell={host_dpi:#x}). with_shell_dpi_context 안에서 위젯을 생성하세요."));
        }
        let thread_dpi_before = unsafe { GetThreadDpiAwarenessContext() };
        let _dpi = DpiGuard::enter(own_dpi)?;
        let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
        let exstyle = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
        if style & WS_CHILD != 0 || unsafe { GetWindow(hwnd, GW_OWNER) } != 0 {
            return Err("바탕화면 연결에는 소유자 없는 독립 위젯 창이 필요합니다.".into());
        }
        if exstyle & WS_EX_TOPMOST != 0 {
            return Err("위젯의 alwaysOnTop을 해제한 후 바탕화면에 연결하세요.".into());
        }
        let screen = rect(hwnd)?;
        let local = to_parent(screen, host)?;
        let mut client = Rect::default();
        if unsafe { GetClientRect(host, &mut client) } == 0 {
            return Err(error("GetClientRect"));
        }
        if local.left < client.left
            || local.top < client.top
            || local.right > client.right
            || local.bottom > client.bottom
            || local.right <= local.left
            || local.bottom <= local.top
        {
            let mut widget_client = Rect::default();
            let widget_client_valid = unsafe { GetClientRect(hwnd, &mut widget_client) } != 0;
            return Err(format!(
                "위젯이 바탕화면 호스트 영역 밖에 있습니다. 표시 가능한 모니터 안으로 배치한 후 다시 시도하세요. \
                [widget={hwnd:#x}, host={host:#x}, screen={screen:?}, local={local:?}, \
                hostClient={client:?}, hostScreen={:?}, widgetClient={widget_client:?}, widgetClientValid={widget_client_valid}, \
                dpi(widget={}, host={}), awareness(widget={own_dpi:#x}, host={host_dpi:#x}, threadBefore={thread_dpi_before:#x}, threadNow={:#x}), \
                style={style:#x}, exstyle={exstyle:#x}]",
                rect(host),
                unsafe { GetDpiForWindow(hwnd) },
                unsafe { GetDpiForWindow(host) },
                unsafe { GetThreadDpiAwarenessContext() },
            ));
        }
        // Query, but do not change, Explorer's UI state. Bound the cross-process
        // message so an unresponsive Explorer does not stall attachment forever.
        let mut ui_state = 0usize;
        if unsafe { SendMessageTimeoutW(host, 0x0129, 0, 0, 0x0003, 250, &mut ui_state) } == 0 {
            return Err("Explorer가 응답하지 않아 바탕화면 연결을 중단했습니다.".into());
        }
        let saved = Attachment {
            host,
            view: icon_view(host),
            shell_pid,
            original_style: style,
            original_exstyle: exstyle,
            original_rect: screen,
            locked: true,
            recovering: false,
            moving: false,
            geometry_error: None,
            preferred: read_geometry(hwnd)?,
            move_start: None,
            pending_event: None,
            last_geometry: read_geometry(hwnd)?,
        };
        let result = (|| {
            // Child style is required *before* SetParent. Do not make the
            // Explorer host topmost: HWND_TOP here applies only to its children.
            apply_child_styles(hwnd, style, exstyle)?;
            set_parent(hwnd, host)?;
            position_native(hwnd, 0, local)?;
            unsafe {
                // Synchronize only our tree with the existing parent state.
                SendMessageW(hwnd, 0x0128, 2 | (7 << 16), 0);
                SendMessageW(hwnd, 0x0128, 1 | ((ui_state & 7) << 16), 0);
            }
            if unsafe { SetWindowSubclass(hwnd, subclass, SUBCLASS_ID, 0) } == 0 {
                return Err("위젯의 마우스/위치 잠금 핸들러를 설치하지 못했습니다.".into());
            }
            records()
                .lock()
                .map_err(|_| "바탕화면 상태 잠금 오류")?
                .insert(hwnd, saved.clone());
            if unsafe { SetTimer(hwnd, RECOVERY_TIMER_ID, RECOVERY_INTERVAL_MS, None) } == 0 {
                return Err(error("SetTimer(widget recovery)"));
            }
            let info = status(window)?;
            if !info.attached {
                return Err(info.message);
            }
            Ok(info)
        })();
        if result.is_err() {
            // Fail closed: an unattached widget must not become an ordinary
            // floating window. Keep it hidden even if rollback itself fails.
            unsafe {
                ShowWindow(hwnd, 0);
            }
            let rollback = restore(hwnd, &saved, saved.original_rect);
            if let Ok(mut entries) = records().lock() {
                entries.remove(&hwnd);
            }
            if let Err(rollback_error) = rollback {
                return Err(format!(
                    "{}; 복구 실패: {rollback_error}. 위젯은 숨겼습니다.",
                    result.unwrap_err()
                ));
            }
        }
        result
    }

    /// Live structural status. This confirms HWND parenting and styles, not a
    /// visual claim that Win+D or WebView2 input has been manually verified.
    pub fn status(window: &WebviewWindow) -> Result<AttachmentInfo, String> {
        let hwnd = hwnd(window)?;
        let saved = record(hwnd)?;
        let parent = unsafe { GetAncestor(hwnd, GA_PARENT) };
        let mut attached = false;
        let mut host = 0;
        let mut locked = true;
        if let Some(saved) = &saved {
            host = saved.host;
            locked = saved.locked;
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(host, &mut pid);
            }
            let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
            let exstyle = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
            attached = unsafe { IsWindow(host) } != 0
                && parent == host
                && unsafe { GetParent(hwnd) } == host
                && pid == saved.shell_pid
                && icon_view(host) == saved.view
                && saved.view != 0
                && above_sibling(hwnd, saved.view)
                && style & WS_CHILD != 0
                && style & WS_POPUP == 0
                && exstyle & (WS_EX_TOPMOST | WS_EX_TRANSPARENT) == 0;
        }
        let shell_class = class_name(host);
        let opacity_percent =
            opacity_percent(hwnd, unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32)?;
        let mut message = if attached {
            format!("바탕화면 부모 연결 확인 ({shell_class}, HWND={host:#x}). Win+D·클릭 동작은 실제 환경 검증이 필요한 실험 기능입니다.")
        } else {
            format!("바탕화면 연결 안 됨 (실제 부모={parent:#x}, 예상 호스트={host:#x}). 위젯을 다시 연결해야 합니다.")
        };
        if let Some(error) = saved
            .as_ref()
            .and_then(|entry| entry.geometry_error.as_ref())
        {
            message.push_str(&format!(" 화면 안 위치 복구 실패: {error}"));
        }
        Ok(AttachmentInfo {
            attached,
            strategy: STRATEGY.into(),
            message,
            hwnd: format!("{hwnd:#x}"),
            parent_hwnd: format!("{parent:#x}"),
            shell_host_hwnd: format!("{host:#x}"),
            shell_class,
            locked,
            opacity_percent,
        })
    }

    fn above_sibling(hwnd: Hwnd, sibling: Hwnd) -> bool {
        let mut next = unsafe { GetWindow(hwnd, GW_HWNDNEXT) };
        // Bounded traversal also fails cleanly if Explorer changes the tree
        // concurrently. No cursor hook or per-frame polling is used.
        for _ in 0..1024 {
            if next == 0 {
                return false;
            }
            if next == sibling {
                return true;
            }
            next = unsafe { GetWindow(next, GW_HWNDNEXT) };
        }
        false
    }

    /// Restore original top-level styles while retaining the current screen
    /// position. Detach hides the widget; the caller may destroy or reuse it.
    pub fn detach(window: &WebviewWindow) -> Result<(), String> {
        let hwnd = hwnd(window)?;
        require_ui_thread(hwnd)?;
        let Some(saved) = record(hwnd)? else {
            return Ok(());
        };
        let _dpi = DpiGuard::enter(unsafe { GetWindowDpiAwarenessContext(hwnd) })?;
        let screen = rect(hwnd)?;
        unsafe {
            ShowWindow(hwnd, 0);
        }
        restore(hwnd, &saved, screen)?;
        records()
            .lock()
            .map_err(|_| "바탕화면 상태 잠금 오류")?
            .remove(&hwnd);
        Ok(())
    }

    // Preferred physical screen coordinates are mapped in one physical-DPI
    // scope. Both display notifications and the sparse fallback use this path.
    fn recover_geometry(hwnd: Hwnd) -> Result<(), String> {
        require_ui_thread(hwnd)?;
        let saved = record(hwnd)?.ok_or("위젯 연결 상태가 없습니다.")?;
        if saved.moving || saved.recovering {
            return Ok(());
        }
        if unsafe { GetParent(hwnd) } != saved.host || unsafe { IsWindow(saved.host) } == 0 {
            return Err("바탕화면 호스트가 변경되어 위치를 복구할 수 없습니다.".into());
        }
        let _dpi = DpiGuard::enter(-4)?;
        let preferred = saved.preferred;
        let monitor = unsafe {
            MonitorFromPoint(
                Point {
                    x: preferred.x,
                    y: preferred.y,
                },
                2,
            )
        };
        let mut info = MonitorInfo {
            size: std::mem::size_of::<MonitorInfo>() as u32,
            ..Default::default()
        };
        if monitor == 0 || unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return Err(error("GetMonitorInfoW"));
        }
        // The widget must fit both the monitor work area and its parent's client
        // area; clipping by either can otherwise hide the top-right controls.
        let work = to_parent(info.work, saved.host)?;
        let mut client = Rect::default();
        if unsafe { GetClientRect(saved.host, &mut client) } == 0 {
            return Err(error("GetClientRect"));
        }
        let bounds = Rect {
            left: work.left.max(client.left),
            top: work.top.max(client.top),
            right: work.right.min(client.right),
            bottom: work.bottom.min(client.bottom),
        };
        if bounds.right <= bounds.left || bounds.bottom <= bounds.top {
            return Err("바탕화면 호스트와 모니터의 표시 영역이 겹치지 않습니다.".into());
        }
        let current = to_parent(rect(hwnd)?, saved.host)?;
        let scale = f64::from(unsafe { GetDpiForWindow(hwnd) }.max(96)) / 96.;
        let desired = Rect {
            left: preferred.x,
            top: preferred.y,
            right: preferred
                .x
                .saturating_add((f64::from(preferred.width) * scale).round() as i32),
            bottom: preferred
                .y
                .saturating_add((f64::from(preferred.height) * scale).round() as i32),
        };
        let desired = to_parent(desired, saved.host)?;
        let target = clamp_requested_rect(desired, desired.left, desired.top, bounds)?;
        let (left, top) = (target.left, target.top);
        let (width, height) = (target.right - left, target.bottom - top);
        if current == target {
            return Ok(());
        }
        records()
            .lock()
            .map_err(|_| "바탕화면 상태 잠금 오류")?
            .get_mut(&hwnd)
            .ok_or("위젯 연결 상태가 없습니다.")?
            .recovering = true;
        // Never hold the registry mutex across SetWindowPos: it invokes our
        // subclass synchronously. Only this move bypasses the user's lock.
        let result = if unsafe {
            SetWindowPos(
                hwnd,
                0,
                left,
                top,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )
        } == 0
        {
            Err(error("SetWindowPos(widget recovery)"))
        } else {
            rect(hwnd)
                .and_then(|screen| to_parent(screen, saved.host))
                .and_then(|actual| {
                    if actual == target {
                        Ok(())
                    } else {
                        Err("창 위치 보정 결과가 요청한 영역과 다릅니다.".into())
                    }
                })
        };
        if let Some(entry) = records()
            .lock()
            .map_err(|_| "바탕화면 상태 잠금 오류")?
            .get_mut(&hwnd)
        {
            entry.recovering = false;
        }
        result
    }

    fn read_geometry(hwnd: Hwnd) -> Result<WidgetGeometry, String> {
        let _physical = DpiGuard::enter(-4)?;
        let r = rect(hwnd)?;
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        Ok(WidgetGeometry {
            x: r.left,
            y: r.top,
            width: ((r.right - r.left) as f64 * 96. / f64::from(dpi)).round() as u32,
            height: ((r.bottom - r.top) as f64 * 96. / f64::from(dpi)).round() as u32,
        })
    }

    pub fn configure_geometry(
        window: &WebviewWindow,
        preferred: WidgetGeometry,
    ) -> Result<(), String> {
        let hwnd = hwnd(window)?;
        require_ui_thread(hwnd)?;
        if preferred.width == 0
            || preferred.height == 0
            || preferred.width > 3840
            || preferred.height > 2160
        {
            return Err("위젯 크기가 허용 범위를 벗어났습니다.".into());
        }
        let previous = {
            let mut entries = records().lock().map_err(|_| "바탕화면 상태 잠금 오류")?;
            let entry = entries.get_mut(&hwnd).ok_or("위젯 연결 상태가 없습니다.")?;
            std::mem::replace(&mut entry.preferred, preferred)
        };
        let result = recover_geometry(hwnd);
        if result.is_err() {
            if let Ok(mut entries) = records().lock() {
                if let Some(entry) = entries.get_mut(&hwnd) {
                    entry.preferred = previous;
                }
            }
        }
        emit_geometry(hwnd, None, result.clone().err());
        result
    }

    /// Drain only the bounded in-memory notifications. No HWND/desktop polling.
    /// Destroy/detach removes its record and any pending event before HWND reuse.
    pub fn take_geometry_events() -> Vec<GeometryEvent> {
        records()
            .lock()
            .map(|mut entries| {
                entries
                    .values_mut()
                    .filter_map(|entry| entry.pending_event.take())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn emit_geometry(hwnd: Hwnd, preferred: Option<WidgetGeometry>, error: Option<String>) {
        let actual = read_geometry(hwnd).ok();
        if let Ok(mut entries) = records().lock() {
            if let Some(entry) = entries.get_mut(&hwnd) {
                let actual = actual.unwrap_or(entry.last_geometry);
                if preferred.is_none()
                    && actual == entry.last_geometry
                    && error == entry.geometry_error
                {
                    return;
                }
                entry.last_geometry = actual;
                entry.geometry_error = error.clone();
                // A later automatic correction must not erase an unconsumed user move.
                let preferred =
                    preferred.or_else(|| entry.pending_event.as_ref().and_then(|e| e.preferred));
                entry.pending_event = Some(GeometryEvent {
                    preferred,
                    actual_size: (actual.width, actual.height),
                    error,
                });
            }
        }
    }

    fn recover_and_report(hwnd: Hwnd) {
        let error = recover_geometry(hwnd).err();
        emit_geometry(hwnd, None, error);
    }

    /// Observe broadcast notifications on our existing top-level manager, which
    /// remains alive while hidden in the tray. Desktop children miss broadcasts.
    pub fn observe_display_changes(window: &WebviewWindow) -> Result<(), String> {
        let hwnd = hwnd(window)?;
        require_ui_thread(hwnd)?;
        if unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32 & WS_CHILD != 0 {
            return Err("화면 알림에는 앱의 최상위 창이 필요합니다.".into());
        }
        if unsafe { SetWindowSubclass(hwnd, display_observer, OBSERVER_SUBCLASS_ID, 0) } == 0 {
            return Err(error("SetWindowSubclass(display observer)"));
        }
        Ok(())
    }

    fn is_display_change(message: u32, wparam: usize) -> bool {
        message == 0x007e
            || (message == 0x001a && wparam == 0x002f)
            || matches!(message, 0x02e0 | 0x02e2 | 0x02e3)
    }

    unsafe extern "system" fn display_observer(
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        _: usize,
        _: usize,
    ) -> isize {
        if message == 0x0082 {
            RemoveWindowSubclass(hwnd, display_observer, OBSERVER_SUBCLASS_ID);
        } else if is_display_change(message, wparam) {
            let handles = records()
                .lock()
                .map(|entries| entries.keys().copied().collect::<Vec<_>>())
                .unwrap_or_default();
            for widget in handles {
                // Our app's widget UI thread receives this; no cross-process work.
                PostMessageW(widget, DISPLAY_CHANGED, 0, 0);
            }
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }

    fn finish_user_move(hwnd: Hwnd) {
        let actual = read_geometry(hwnd);
        let preferred = if let Ok(mut entries) = records().lock() {
            entries.get_mut(&hwnd).and_then(|entry| {
                entry.moving = false;
                let start = entry.move_start.take()?;
                let actual = actual.as_ref().ok()?;
                if entry.locked || *actual == start {
                    return None;
                }
                let before = entry.preferred;
                if (actual.x, actual.y) != (start.x, start.y) {
                    entry.preferred.x = actual.x;
                    entry.preferred.y = actual.y;
                }
                if (actual.width, actual.height) != (start.width, start.height) {
                    entry.preferred.width = actual.width;
                    entry.preferred.height = actual.height;
                }
                (before != entry.preferred).then_some(entry.preferred)
            })
        } else {
            None
        };
        emit_geometry(hwnd, preferred, actual.err());
        recover_and_report(hwnd);
    }

    pub fn recover_position(window: &WebviewWindow) -> Result<(), String> {
        recover_geometry(hwnd(window)?)
    }

    pub fn logical_size(window: &WebviewWindow) -> Result<Option<(u32, u32)>, String> {
        let hwnd = hwnd(window)?;
        require_ui_thread(hwnd)?;
        if record(hwnd)?.is_some_and(|s| s.moving || s.recovering) {
            return Ok(None);
        }
        let _physical = DpiGuard::enter(-4)?;
        let r = rect(hwnd)?;
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        Ok(Some((
            ((r.right - r.left) as f64 * 96.0 / f64::from(dpi)).round() as u32,
            ((r.bottom - r.top) as f64 * 96.0 / f64::from(dpi)).round() as u32,
        )))
    }

    /// Numeric size changes touch only our unlocked child and retain parent/style.
    pub fn resize_logical(window: &WebviewWindow, width: u32, height: u32) -> Result<(), String> {
        if !(320..=3840).contains(&width) || !(220..=2160).contains(&height) {
            return Err("위젯 크기가 허용 범위를 벗어났습니다.".into());
        }
        let hwnd = hwnd(window)?;
        require_ui_thread(hwnd)?;
        let saved = record(hwnd)?.ok_or("바탕화면 연결이 필요합니다.")?;
        if saved.locked || saved.moving {
            return Err("잠금 해제 후 이동을 마치고 크기를 조절하세요.".into());
        }
        if unsafe { GetParent(hwnd) } != saved.host || unsafe { IsWindow(saved.host) } == 0 {
            return Err("바탕화면 호스트가 변경되었습니다.".into());
        }
        let mut preferred = saved.preferred;
        preferred.width = width;
        preferred.height = height;
        configure_geometry(window, preferred)
    }

    /// Lock position/size, preserving normal pointer clicks on calendar dates.
    pub fn set_locked(window: &WebviewWindow, locked: bool) -> Result<(), String> {
        let hwnd = hwnd(window)?;
        require_ui_thread(hwnd)?;
        if !status(window)?.attached {
            return Err("위젯이 바탕화면에 연결되어 있지 않습니다.".into());
        }
        let mut entries = records().lock().map_err(|_| "바탕화면 상태 잠금 오류")?;
        let saved = entries.get_mut(&hwnd).ok_or("위젯 연결 상태가 없습니다.")?;
        saved.locked = locked;
        Ok(())
    }

    /// Use this instead of Tauri show()/hide(): Tao's cached window flags still
    /// describe a top-level window, and changing them would overwrite WS_CHILD.
    pub fn set_visible(window: &WebviewWindow, visible: bool) -> Result<AttachmentInfo, String> {
        let handle = hwnd(window)?;
        require_ui_thread(handle)?;
        let before = status(window)?;
        if visible && !before.attached {
            return Err(before.message);
        }
        if visible {
            recover_geometry(handle)?;
        }
        let flags = SWP_NOMOVE
            | SWP_NOSIZE
            | SWP_NOZORDER
            | SWP_NOACTIVATE
            | SWP_NOOWNERZORDER
            | if visible {
                SWP_SHOWWINDOW
            } else {
                SWP_HIDEWINDOW
            };
        if unsafe { SetWindowPos(handle, 0, 0, 0, 0, 0, flags) } == 0 {
            return Err(error("SetWindowPos(visibility)"));
        }
        let after = status(window)?;
        if visible && !after.attached {
            return Err(after.message);
        }
        Ok(after)
    }

    unsafe extern "system" fn subclass(
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        _: usize,
        _: usize,
    ) -> isize {
        if message == 0x0083 && GetPropW(hwnd, wide(super::PER_PIXEL_ALPHA_PROPERTY).as_ptr()) != 0
        {
            // Preserve a borderless client while retaining the native sizing style.
            // https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-nccalcsize
            return 0;
        }
        if message == 0x0082 {
            // WM_NCDESTROY; Explorer can destroy its children.
            KillTimer(hwnd, RECOVERY_TIMER_ID);
            KillTimer(hwnd, DISPLAY_TIMER_ID);
            RemoveWindowSubclass(hwnd, subclass, SUBCLASS_ID);
            if let Ok(mut entries) = records().lock() {
                entries.remove(&hwnd);
            }
            return DefSubclassProc(hwnd, message, wparam, lparam);
        }
        if message == 0x0021 {
            return 3;
        } // MA_NOACTIVATE, keep the click.
        if message == 0x0231 {
            let start = read_geometry(hwnd).ok();
            if let Ok(mut entries) = records().lock() {
                if let Some(entry) = entries.get_mut(&hwnd) {
                    entry.moving = true;
                    entry.move_start = start;
                }
            }
        } else if message == 0x0232 {
            finish_user_move(hwnd);
        }
        if message == DISPLAY_CHANGED || is_display_change(message, wparam) {
            if SetTimer(hwnd, DISPLAY_TIMER_ID, DISPLAY_SETTLE_MS, None) == 0 {
                emit_geometry(hwnd, None, Some(error("SetTimer(display settle)")));
            }
        }
        if message == 0x0113 && matches!(wparam, RECOVERY_TIMER_ID | DISPLAY_TIMER_ID) {
            if wparam == DISPLAY_TIMER_ID {
                KillTimer(hwnd, DISPLAY_TIMER_ID);
            }
            recover_and_report(hwnd);
            return 0;
        }
        let locked = records()
            .lock()
            .ok()
            .and_then(|entries| {
                entries
                    .get(&hwnd)
                    .map(|entry| entry.locked && !entry.recovering)
            })
            .unwrap_or(false);
        // Native widget's custom grip uses the OS size loop. Keep its logical
        // limits here, while allowing monitor recovery below them when needed.
        if message == 0x0214
            && lparam != 0
            && !locked
            && GetPropW(hwnd, wide(super::PER_PIXEL_ALPHA_PROPERTY).as_ptr()) != 0
        {
            let r = &mut *(lparam as *mut Rect);
            let scale = f64::from(GetDpiForWindow(hwnd).max(96)) / 96.0;
            let width = (i64::from(r.right) - i64::from(r.left)).clamp(
                (320.0 * scale).round() as i64,
                (3840.0 * scale).round() as i64,
            );
            let height = (i64::from(r.bottom) - i64::from(r.top)).clamp(
                (220.0 * scale).round() as i64,
                (2160.0 * scale).round() as i64,
            );
            if matches!(wparam, 1 | 4 | 7) {
                r.left =
                    (i64::from(r.right) - width).clamp(i32::MIN.into(), i32::MAX.into()) as i32;
            } else {
                r.right =
                    (i64::from(r.left) + width).clamp(i32::MIN.into(), i32::MAX.into()) as i32;
            }
            if matches!(wparam, 3 | 4 | 5) {
                r.top =
                    (i64::from(r.bottom) - height).clamp(i32::MIN.into(), i32::MAX.into()) as i32;
            } else {
                r.bottom =
                    (i64::from(r.top) + height).clamp(i32::MIN.into(), i32::MAX.into()) as i32;
            }
        }
        if locked {
            if message == 0x0112 && matches!(wparam & 0xfff0, 0xf010 | 0xf000) {
                return 0; // SC_MOVE / SC_SIZE, including Tauri startDragging.
            }
            if message == 0x0046 && lparam != 0 {
                // WM_WINDOWPOSCHANGING
                let change = &mut *(lparam as *mut WindowPos);
                change.flags |= SWP_NOMOVE | SWP_NOSIZE;
            }
            if message == 0x0084 {
                // WM_NCHITTEST
                let hit = DefSubclassProc(hwnd, message, wparam, lparam);
                return if hit == 2 || (10..=17).contains(&hit) {
                    1
                } else {
                    hit
                };
            }
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub(crate) mod test_support {
        use super::*;

        #[link(name = "user32")]
        extern "system" {
            fn CreateWindowExW(
                exstyle: u32,
                class: *const u16,
                title: *const u16,
                style: u32,
                x: i32,
                y: i32,
                width: i32,
                height: i32,
                parent: Hwnd,
                menu: isize,
                instance: isize,
                parameter: *const std::ffi::c_void,
            ) -> Hwnd;
            fn DestroyWindow(hwnd: Hwnd) -> i32;
            fn UnregisterClassW(class: *const u16, instance: isize) -> i32;
            fn DefWindowProcW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
        }

        #[repr(C)]
        #[derive(Default)]
        struct TestMessage {
            hwnd: Hwnd,
            message: u32,
            wparam: usize,
            lparam: isize,
            time: u32,
            point: Point,
            private: u32,
        }
        #[link(name = "user32")]
        extern "system" {
            fn PeekMessageW(
                message: *mut TestMessage,
                hwnd: Hwnd,
                min: u32,
                max: u32,
                remove: u32,
            ) -> i32;
            fn DispatchMessageW(message: *const TestMessage) -> isize;
        }
        fn pump_for(duration: std::time::Duration) {
            let start = std::time::Instant::now();
            while start.elapsed() < duration {
                let mut msg = TestMessage::default();
                while unsafe { PeekMessageW(&mut msg, 0, 0, 0, 1) } != 0 {
                    unsafe {
                        DispatchMessageW(&msg);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        struct HiddenWindows {
            source: Hwnd,
            widget: Hwnd,
            instance: isize,
            source_class: Vec<u16>,
            widget_class: Vec<u16>,
        }

        impl Drop for HiddenWindows {
            fn drop(&mut self) {
                unsafe {
                    if self.widget != 0 {
                        DestroyWindow(self.widget);
                    }
                    if self.source != 0 {
                        DestroyWindow(self.source);
                    }
                    UnregisterClassW(self.widget_class.as_ptr(), self.instance);
                    UnregisterClassW(self.source_class.as_ptr(), self.instance);
                }
            }
        }

        // Real native seam, using only this test process's hidden windows:
        // validates cloned class isolation and layered-child alpha readback.
        // It does not inspect, automate, or change Explorer or the real desktop.
        #[cfg_attr(test, test)]
        pub fn verify_layered_child_fixture() {
            // Match the real app's DPI context before creating fixture HWNDs.
            // A DPI-unaware test window virtualizes coordinates at 125%/150%.
            let _fixture_dpi = DpiGuard::enter(-4).unwrap();
            let suffix = format!("{}_{}", unsafe { GetCurrentProcessId() }, unsafe {
                GetCurrentThreadId()
            });
            let source_name = format!("TimeScheduleTestSource_{suffix}");
            let widget_name = format!("TimeScheduleTestLayered_{suffix}");
            let mut windows = HiddenWindows {
                source: 0,
                widget: 0,
                instance: unsafe { GetModuleHandleW(std::ptr::null()) },
                source_class: wide(&source_name),
                widget_class: wide(&widget_name),
            };
            let mut source_class: WindowClass = unsafe { std::mem::zeroed() };
            source_class.size = std::mem::size_of::<WindowClass>() as u32;
            source_class.style = 0x0003 | CS_OWNDC;
            source_class.procedure = Some(DefWindowProcW);
            source_class.instance = windows.instance;
            source_class.class_name = windows.source_class.as_ptr();
            assert_ne!(
                unsafe { RegisterClassExW(&source_class) },
                0,
                "{}",
                error("RegisterClassExW(test)")
            );
            windows.source = unsafe {
                CreateWindowExW(
                    0,
                    windows.source_class.as_ptr(),
                    std::ptr::null(),
                    WS_POPUP,
                    0,
                    0,
                    200,
                    100,
                    0,
                    0,
                    windows.instance,
                    std::ptr::null(),
                )
            };
            assert_ne!(windows.source, 0, "{}", error("CreateWindowExW(source)"));

            assert!(
                prepare_class(windows.source, &source_name).is_err(),
                "must reject a conflicting original class"
            );
            prepare_class(windows.source, &widget_name).unwrap();
            prepare_class(windows.source, &widget_name).unwrap(); // safe repeated preparation
            windows.widget = unsafe {
                CreateWindowExW(
                    WS_EX_APPWINDOW | WS_EX_WINDOWEDGE | 0x10, // ACCEPTFILES, as on Tao's widget
                    windows.widget_class.as_ptr(),
                    std::ptr::null(),
                    WS_POPUP | WS_CAPTION | WS_THICKFRAME | WS_CLIPCHILDREN,
                    10,
                    10,
                    80,
                    60,
                    0,
                    0,
                    windows.instance,
                    std::ptr::null(),
                )
            };
            assert_ne!(windows.widget, 0, "{}", error("CreateWindowExW(widget)"));
            // Exercise saved physical-position recovery using only our hidden
            // fixture window. Never move/resize the actual desktop or monitors.
            let adapter = WebviewWindow {
                hwnd: windows.widget,
                label: "widget",
                layered_class: WIDGET_CLASS,
                _ui_thread: Default::default(),
            };
            restore_position(&adapter, 100_000, -100_000).unwrap();
            {
                let _physical = DpiGuard::enter(-4).unwrap();
                let actual = rect(windows.widget).unwrap();
                let mut monitor = MonitorInfo {
                    size: std::mem::size_of::<MonitorInfo>() as u32,
                    ..Default::default()
                };
                assert_ne!(
                    unsafe { GetMonitorInfoW(MonitorFromWindow(windows.widget, 2), &mut monitor) },
                    0
                );
                assert!(
                    actual.left >= monitor.work.left
                        && actual.top >= monitor.work.top
                        && actual.right <= monitor.work.right
                        && actual.bottom <= monitor.work.bottom
                );
                assert_eq!(position(&adapter).unwrap(), (actual.left, actual.top));
            }
            restore_position(&adapter, 10, 10).unwrap();
            assert_eq!(
                unsafe { GetClassLongW(windows.source, GCL_STYLE) },
                source_class.style
            );
            assert_eq!(
                unsafe { GetClassLongW(windows.widget, GCL_STYLE) } & (CS_OWNDC | CS_CLASSDC),
                0
            );

            // Exercise the actual top-level-to-child order with a captured
            // WINDOWEDGE bit, which Windows normalizes away after frame removal.
            let original_style = unsafe { GetWindowLongW(windows.widget, GWL_STYLE) } as u32;
            let original_exstyle = unsafe { GetWindowLongW(windows.widget, GWL_EXSTYLE) } as u32;
            assert_ne!(original_exstyle & WS_EX_WINDOWEDGE, 0);
            apply_child_styles(windows.widget, original_style, original_exstyle).unwrap();
            set_parent(windows.widget, windows.source).unwrap();
            assert!(
                restore_position(&adapter, 100, 100).is_err(),
                "attached widgets must use geometry recovery instead"
            );
            let child_style = unsafe { GetWindowLongW(windows.widget, GWL_STYLE) } as u32;
            let child_exstyle = unsafe { GetWindowLongW(windows.widget, GWL_EXSTYLE) } as u32;
            assert_ne!(child_style & WS_CHILD, 0);
            assert_eq!(child_style & (WS_POPUP | WS_CAPTION | WS_THICKFRAME), 0);
            assert_eq!(child_exstyle, WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | 0x10);
            assert_eq!(unsafe { GetParent(windows.widget) }, windows.source);
            // A documented successful setter may normalize a frame flag. The
            // generic wrapper must accept it; child/alpha checks remain exact.
            set_style(
                windows.widget,
                GWL_EXSTYLE,
                child_exstyle | WS_EX_WINDOWEDGE,
            )
            .unwrap();
            assert_eq!(
                unsafe { GetWindowLongW(windows.widget, GWL_EXSTYLE) } as u32,
                child_exstyle
            );

            assert!(set_native_opacity(windows.widget, 39, &widget_name).is_err());
            assert_eq!(
                unsafe { GetWindowLongW(windows.widget, GWL_EXSTYLE) } as u32 & WS_EX_LAYERED,
                0
            );
            // Retain coverage for SetWindowLongW's valid zero previous-style
            // return, including reentrant handlers that leave a last error.
            set_style(windows.widget, GWL_EXSTYLE, 0).unwrap();
            set_native_opacity(windows.widget, 40, &widget_name).unwrap();
            apply_child_styles(windows.widget, child_style, child_exstyle | WS_EX_LAYERED).unwrap();
            for percent in [40, 82, 100] {
                set_native_opacity(windows.widget, percent, &widget_name).unwrap();
                let exstyle = unsafe { GetWindowLongW(windows.widget, GWL_EXSTYLE) } as u32;
                assert_eq!(
                    opacity_percent(windows.widget, exstyle).unwrap(),
                    Some(percent)
                );
                assert_eq!(exstyle & (WS_EX_TRANSPARENT | WS_EX_TOPMOST), 0);
                assert_eq!(unsafe { GetParent(windows.widget) }, windows.source);
                assert_eq!(unsafe { IsWindowVisible(windows.widget) }, 0);
            }
            let before = layer_attributes(windows.widget).unwrap();
            assert!(set_native_opacity(windows.widget, 101, &widget_name).is_err());
            assert_eq!(layer_attributes(windows.widget).unwrap(), before);
            assert_eq!(
                unsafe { GetClassLongW(windows.source, GCL_STYLE) },
                source_class.style
            );

            // A locked child must recover when its desktop host becomes smaller.
            // Use only this process's hidden windows, never resize the real display.
            position_native(
                windows.widget,
                0,
                Rect {
                    left: 150,
                    top: -20,
                    right: 230,
                    bottom: 40,
                },
            )
            .unwrap();
            records().lock().unwrap().insert(
                windows.widget,
                Attachment {
                    host: windows.source,
                    view: 0,
                    shell_pid: unsafe { GetCurrentProcessId() },
                    original_style,
                    original_exstyle,
                    original_rect: rect(windows.widget).unwrap(),
                    locked: true,
                    recovering: false,
                    moving: false,
                    geometry_error: None,
                    preferred: read_geometry(windows.widget).unwrap(),
                    move_start: None,
                    pending_event: None,
                    last_geometry: read_geometry(windows.widget).unwrap(),
                },
            );
            assert_ne!(
                unsafe { SetWindowSubclass(windows.widget, subclass, SUBCLASS_ID, 0) },
                0
            );
            assert_ne!(
                unsafe {
                    SetWindowPos(
                        windows.source,
                        0,
                        0,
                        0,
                        60,
                        40,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                },
                0
            );
            let before_recovery = to_parent(rect(windows.widget).unwrap(), windows.source).unwrap();
            assert!(before_recovery.top < 0 && before_recovery.right > 60);
            // WM_TIMER, the widget's geometry recovery timer.
            unsafe {
                SendMessageW(windows.widget, 0x0113, 0x5449_4d46, 0);
            }
            let recovered = to_parent(rect(windows.widget).unwrap(), windows.source).unwrap();
            assert!(
                recovered.left >= 0
                    && recovered.top >= 0
                    && recovered.right <= 60
                    && recovered.bottom <= 40,
                "move/lock controls remain clipped after display shrink: {recovered:?}"
            );
            assert!(record(windows.widget).unwrap().unwrap().locked);
            assert_eq!(unsafe { GetParent(windows.widget) }, windows.source);
            assert_eq!(unsafe { IsWindowVisible(windows.widget) }, 0);
            assert_eq!(layer_attributes(windows.widget).unwrap(), before);
            assert_eq!(
                (
                    recovered.right - recovered.left,
                    recovered.bottom - recovered.top
                ),
                (60, 40)
            );
            unsafe {
                SendMessageW(windows.widget, 0x0113, 0x5449_4d46, 0);
            }
            assert_eq!(
                to_parent(rect(windows.widget).unwrap(), windows.source).unwrap(),
                recovered
            );
            // A temporary clamp must not destroy the preferred dimensions.
            unsafe {
                SetWindowPos(
                    windows.source,
                    0,
                    0,
                    0,
                    300,
                    200,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                SendMessageW(windows.widget, 0x0113, RECOVERY_TIMER_ID, 0);
            }
            let expanded = to_parent(rect(windows.widget).unwrap(), windows.source).unwrap();
            assert_eq!(
                (
                    expanded.right - expanded.left,
                    expanded.bottom - expanded.top
                ),
                (80, 60),
                "temporary shrink destroyed the preferred size"
            );
            unsafe {
                SetWindowPos(
                    windows.source,
                    0,
                    0,
                    0,
                    60,
                    40,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                SendMessageW(windows.widget, 0x0113, RECOVERY_TIMER_ID, 0);
            }
            // Recovery must not remove the user's movement lock.
            position_native(
                windows.widget,
                0,
                Rect {
                    left: 500,
                    top: 500,
                    right: 800,
                    bottom: 800,
                },
            )
            .unwrap();
            let still_locked = to_parent(rect(windows.widget).unwrap(), windows.source).unwrap();
            assert_eq!(
                (
                    still_locked.left,
                    still_locked.top,
                    still_locked.right,
                    still_locked.bottom
                ),
                (
                    recovered.left,
                    recovered.top,
                    recovered.right,
                    recovered.bottom
                )
            );
            let owned = unsafe { WebviewWindow::iced_widget(windows.widget) };
            let locked_size = rect(windows.widget).unwrap();
            assert!(
                resize_logical(&owned, 400, 300).is_err(),
                "locked numeric resize must fail"
            );
            assert_eq!(rect(windows.widget).unwrap(), locked_size);
            records()
                .lock()
                .unwrap()
                .get_mut(&windows.widget)
                .unwrap()
                .locked = false;
            assert_ne!(
                unsafe {
                    SetWindowPos(
                        windows.source,
                        0,
                        0,
                        0,
                        1000,
                        800,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                },
                0
            );
            resize_logical(&owned, 400, 300).unwrap();
            assert_eq!(logical_size(&owned).unwrap(), Some((400, 300)));
            // Real OS move-loop notifications must yield exactly one final
            // authored change; intermediate WINDOWPOSCHANGED is display-only.
            take_geometry_events();
            unsafe {
                SendMessageW(windows.widget, 0x0231, 0, 0);
            }
            for x in [25, 30, 35] {
                unsafe {
                    SetWindowPos(
                        windows.widget,
                        0,
                        x,
                        25,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                assert!(
                    take_geometry_events().is_empty(),
                    "do not save intermediate moves"
                );
            }
            unsafe {
                SendMessageW(windows.widget, 0x0232, 0, 0);
            }
            let events = take_geometry_events();
            assert_eq!(
                events.len(),
                1,
                "move end must notify the app without position polling"
            );
            assert_eq!(
                events[0].preferred,
                Some(read_geometry(windows.widget).unwrap())
            );
            unsafe {
                SendMessageW(windows.widget, 0x0231, 0, 0);
                SendMessageW(windows.widget, 0x0232, 0, 0);
            }
            assert!(
                take_geometry_events().is_empty(),
                "unchanged move must not save twice"
            );
            let resized = rect(windows.widget).unwrap();
            assert!(resize_logical(&owned, 0, 0).is_err());
            assert_eq!(rect(windows.widget).unwrap(), resized);
            assert_eq!(unsafe { GetParent(windows.widget) }, windows.source);
            assert_eq!(layer_attributes(windows.widget).unwrap(), before);
            records()
                .lock()
                .unwrap()
                .get_mut(&windows.widget)
                .unwrap()
                .moving = true;
            assert_eq!(
                logical_size(&owned).unwrap(),
                None,
                "do not persist intermediate drag sizes"
            );
            records()
                .lock()
                .unwrap()
                .get_mut(&windows.widget)
                .unwrap()
                .moving = false;
            // Deliver display/work-area/DPI notifications to an owned TOP-LEVEL
            // window and pump real posted messages/timers. No system display changes.
            let observer = unsafe { WebviewWindow::iced_widget(windows.source) };
            observe_display_changes(&observer).unwrap();
            assert!(observe_display_changes(&owned).is_err());
            take_geometry_events();
            let preferred = record(windows.widget).unwrap().unwrap().preferred;
            records()
                .lock()
                .unwrap()
                .get_mut(&windows.widget)
                .unwrap()
                .locked = true;
            for (message, wparam) in [(0x007e, 0), (0x001a, 0x002f), (0x02e0, 0)] {
                unsafe {
                    SetWindowPos(
                        windows.source,
                        0,
                        0,
                        0,
                        250,
                        180,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    for _ in 0..20 {
                        SendMessageW(windows.source, message, wparam, 0);
                    }
                }
                let before = rect(windows.widget).unwrap();
                pump_for(std::time::Duration::from_millis(60));
                assert_eq!(
                    rect(windows.widget).unwrap(),
                    before,
                    "must coalesce the notification burst"
                );
                pump_for(std::time::Duration::from_millis(300));
                let actual = to_parent(rect(windows.widget).unwrap(), windows.source).unwrap();
                assert!(
                    actual.left >= 0
                        && actual.top >= 0
                        && actual.right <= 250
                        && actual.bottom <= 180,
                    "top-level notification was not forwarded to child: {actual:?}"
                );
                assert_eq!(
                    record(windows.widget).unwrap().unwrap().preferred,
                    preferred
                );
                let events = take_geometry_events();
                assert_eq!(events.len(), 1);
                assert!(
                    events[0].preferred.is_none(),
                    "display corrections are not user edits"
                );
                unsafe {
                    SetWindowPos(
                        windows.source,
                        0,
                        0,
                        0,
                        1000,
                        800,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    SendMessageW(windows.source, message, wparam, 0);
                }
                pump_for(std::time::Duration::from_millis(350));
                assert_eq!(
                    read_geometry(windows.widget).unwrap(),
                    preferred,
                    "restore authored geometry when area returns"
                );
                take_geometry_events();
            }
            assert_eq!(RECOVERY_INTERVAL_MS, 30_000);
            assert_eq!(DISPLAY_SETTLE_MS, 200);
            // Queued user movement survives a later automatic correction.
            records()
                .lock()
                .unwrap()
                .get_mut(&windows.widget)
                .unwrap()
                .locked = false;
            unsafe {
                SendMessageW(windows.widget, 0x0231, 0, 0);
                SetWindowPos(
                    windows.widget,
                    0,
                    50,
                    40,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                SendMessageW(windows.widget, 0x0232, 0, 0);
                SetWindowPos(
                    windows.source,
                    0,
                    0,
                    0,
                    250,
                    180,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                SendMessageW(windows.source, 0x007e, 0, 0);
            }
            pump_for(std::time::Duration::from_millis(350));
            let event = take_geometry_events().pop().unwrap();
            assert_eq!(
                event.preferred.unwrap(),
                record(windows.widget).unwrap().unwrap().preferred
            );
            assert!(event.actual_size.0 < preferred.width);
            // A move while temporarily small must retain the authored dimensions.
            unsafe {
                SetWindowPos(
                    windows.source,
                    0,
                    0,
                    0,
                    300,
                    200,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                SendMessageW(windows.widget, 0x0231, 0, 0);
                SetWindowPos(
                    windows.widget,
                    0,
                    5,
                    6,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                SendMessageW(windows.widget, 0x0232, 0, 0);
            }
            let event = take_geometry_events().pop().unwrap();
            assert_eq!(
                (
                    event.preferred.unwrap().width,
                    event.preferred.unwrap().height
                ),
                (400, 300)
            );
            // A recreated adapter starts with clamped actual geometry, then
            // configures the same persisted preference before showing the child.
            let persisted = record(windows.widget).unwrap().unwrap().preferred;
            let mut restarted = records().lock().unwrap().remove(&windows.widget).unwrap();
            restarted.preferred = read_geometry(windows.widget).unwrap();
            restarted.pending_event = None;
            records().lock().unwrap().insert(windows.widget, restarted);
            configure_geometry(&owned, persisted).unwrap();
            assert_eq!(
                record(windows.widget).unwrap().unwrap().preferred,
                persisted
            );
            // Restore room for the existing native sizing checks.
            unsafe {
                SetWindowPos(
                    windows.source,
                    0,
                    0,
                    0,
                    1000,
                    800,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                SendMessageW(windows.widget, 0x0113, RECOVERY_TIMER_ID, 0);
            }
            take_geometry_events();
            assert_eq!(
                read_geometry(windows.widget).unwrap(),
                persisted,
                "restart must restore persisted authored size/position"
            );
            let marker = wide(super::super::PER_PIXEL_ALPHA_PROPERTY);
            assert_ne!(unsafe { SetPropW(windows.widget, marker.as_ptr(), 1) }, 0);
            configure_native_sizing(windows.widget).unwrap();
            let sizing_style = unsafe { GetWindowLongW(windows.widget, GWL_STYLE) } as u32;
            assert_ne!(
                sizing_style & WS_THICKFRAME,
                0,
                "custom grip must be able to start the OS sizing loop"
            );
            assert_ne!(sizing_style & WS_CHILD, 0);
            assert_eq!(sizing_style & (WS_POPUP | WS_CAPTION), 0);
            let outer = rect(windows.widget).unwrap();
            let mut client = Rect::default();
            assert_ne!(unsafe { GetClientRect(windows.widget, &mut client) }, 0);
            assert_eq!(
                (client.right, client.bottom),
                (outer.right - outer.left, outer.bottom - outer.top),
                "resizing style must not add a visible frame"
            );
            let mut sizing = Rect {
                left: 0,
                top: 0,
                right: 12,
                bottom: 10,
            };
            unsafe {
                SendMessageW(windows.widget, 0x0214, 8, &mut sizing as *mut Rect as isize);
            }
            let scale = f64::from(unsafe { GetDpiForWindow(windows.widget) }.max(96)) / 96.;
            assert_eq!(sizing.right, (320. * scale).round() as i32);
            assert_eq!(sizing.bottom, (220. * scale).round() as i32);
            emit_geometry(
                windows.widget,
                Some(read_geometry(windows.widget).unwrap()),
                None,
            );
            let previous = record(windows.widget).unwrap().unwrap().preferred;
            set_parent(windows.widget, 0).unwrap();
            assert!(configure_geometry(
                &owned,
                WidgetGeometry {
                    width: 800,
                    ..previous
                }
            )
            .is_err());
            assert_eq!(
                record(windows.widget).unwrap().unwrap().preferred,
                previous,
                "failed numeric resize must not silently replace the authored preference"
            );
            set_parent(windows.widget, windows.source).unwrap();
            let destroyed = windows.widget;
            unsafe {
                DestroyWindow(destroyed);
            }
            windows.widget = 0;
            assert!(record(destroyed).unwrap().is_none());
            assert!(
                take_geometry_events().is_empty(),
                "destroyed HWND cannot publish stale preferences"
            );
        }
    }
}
