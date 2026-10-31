//! Physical positioning for our own top-level popup windows.

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// A snapshot in physical screen coordinates, safe to pass between UI callbacks.
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    client: Rect,
    work: Rect,
}

fn placement(anchor: Anchor, width: i32, height: i32) -> Result<(i32, i32), String> {
    let c = anchor.client;
    let w = anchor.work;
    if width <= 0
        || height <= 0
        || c.right <= c.left
        || c.bottom <= c.top
        || w.right <= w.left
        || w.bottom <= w.top
    {
        return Err("창 또는 화면 영역이 유효하지 않습니다.".into());
    }
    let x = (i64::from(c.left) + i64::from(c.right) - i64::from(width)) / 2;
    let y = (i64::from(c.top) + i64::from(c.bottom) - i64::from(height)) / 2;
    Ok((
        x.clamp(
            i64::from(w.left),
            (i64::from(w.right) - i64::from(width)).max(i64::from(w.left)),
        ) as i32,
        y.clamp(
            i64::from(w.top),
            (i64::from(w.bottom) - i64::from(height)).max(i64::from(w.top)),
        ) as i32,
    ))
}

#[cfg(windows)]
pub use native::{anchor, center};
#[cfg(not(windows))]
pub fn anchor(_: isize) -> Result<Anchor, String> {
    Err("팝업 위치 지정은 Windows에서 지원합니다.".into())
}
#[cfg(not(windows))]
pub fn center(_: isize, _: Anchor) -> Result<(), String> {
    Err("팝업 위치 지정은 Windows에서 지원합니다.".into())
}

#[cfg(windows)]
mod native {
    use super::*;
    #[repr(C)]
    #[derive(Default)]
    struct MonitorInfo {
        size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetWindowThreadProcessId(hwnd: isize, process: *mut u32) -> u32;
        fn SetThreadDpiAwarenessContext(context: isize) -> isize;
        fn GetClientRect(hwnd: isize, rect: *mut Rect) -> i32;
        fn GetWindowRect(hwnd: isize, rect: *mut Rect) -> i32;
        fn MapWindowPoints(from: isize, to: isize, points: *mut Point, count: u32) -> i32;
        fn MonitorFromWindow(hwnd: isize, flags: u32) -> isize;
        fn GetMonitorInfoW(monitor: isize, info: *mut MonitorInfo) -> i32;
        fn GetWindowLongW(hwnd: isize, index: i32) -> i32;
        fn SetWindowPos(
            hwnd: isize,
            after: isize,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            flags: u32,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThreadId() -> u32;
        fn GetCurrentProcessId() -> u32;
        fn SetLastError(code: u32);
        fn GetLastError() -> u32;
    }
    fn error(action: &str) -> String {
        format!("{action}: {}", std::io::Error::last_os_error())
    }
    fn require_owned(hwnd: isize) -> Result<(), String> {
        let mut pid = 0;
        let tid = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if hwnd == 0
            || tid != unsafe { GetCurrentThreadId() }
            || pid != unsafe { GetCurrentProcessId() }
        {
            return Err("현재 UI 스레드가 소유한 앱 창만 배치할 수 있습니다.".into());
        }
        Ok(())
    }
    struct Physical(isize);
    impl Physical {
        fn enter() -> Result<Self, String> {
            let old = unsafe { SetThreadDpiAwarenessContext(-4) };
            if old == 0 {
                Err(error("SetThreadDpiAwarenessContext"))
            } else {
                Ok(Self(old))
            }
        }
    }
    impl Drop for Physical {
        fn drop(&mut self) {
            unsafe {
                SetThreadDpiAwarenessContext(self.0);
            }
        }
    }

    pub fn anchor(hwnd: isize) -> Result<Anchor, String> {
        require_owned(hwnd)?;
        let _physical = Physical::enter()?;
        let mut client = Rect::default();
        if unsafe { GetClientRect(hwnd, &mut client) } == 0 {
            return Err(error("GetClientRect"));
        }
        // Includes the Explorer parent's screen offset for WS_CHILD widgets.
        unsafe {
            SetLastError(0);
            if MapWindowPoints(hwnd, 0, (&mut client as *mut Rect).cast(), 2) == 0
                && GetLastError() != 0
            {
                return Err(error("MapWindowPoints"));
            }
        }
        let mut info = MonitorInfo {
            size: std::mem::size_of::<MonitorInfo>() as u32,
            ..Default::default()
        };
        let monitor = unsafe { MonitorFromWindow(hwnd, 2) };
        if monitor == 0 || unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return Err(error("GetMonitorInfoW"));
        }
        Ok(Anchor {
            client,
            work: info.work,
        })
    }
    pub fn center(hwnd: isize, anchor: Anchor) -> Result<(), String> {
        require_owned(hwnd)?;
        if unsafe { GetWindowLongW(hwnd, -16) } as u32 & 0x4000_0000 != 0 {
            return Err("팝업은 독립된 최상위 창이어야 합니다.".into());
        }
        let _physical = Physical::enter()?;
        // A monitor transition can update the decorated outer size through DPI
        // messages. Measure again before the final placement; never resize here.
        for _ in 0..2 {
            let mut rect = Rect::default();
            if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
                return Err(error("GetWindowRect"));
            }
            let (x, y) = placement(anchor, rect.right - rect.left, rect.bottom - rect.top)?;
            if unsafe { SetWindowPos(hwnd, 0, x, y, 0, 0, 0x0001 | 0x0004 | 0x0010) } == 0 {
                return Err(error("SetWindowPos"));
            }
        }
        Ok(())
    }
    #[cfg(test)]
    mod native_tests {
        use super::*;
        #[link(name = "user32")]
        extern "system" {
            fn CreateWindowExW(
                ex: u32,
                class: *const u16,
                name: *const u16,
                style: u32,
                x: i32,
                y: i32,
                width: i32,
                height: i32,
                parent: isize,
                menu: isize,
                instance: isize,
                param: *const std::ffi::c_void,
            ) -> isize;
            fn DestroyWindow(hwnd: isize) -> i32;
        }
        struct Owned(isize);
        impl Drop for Owned {
            fn drop(&mut self) {
                unsafe {
                    DestroyWindow(self.0);
                }
            }
        }
        fn create(style: u32, x: i32, y: i32, w: i32, h: i32, parent: isize) -> Owned {
            let class: Vec<_> = "STATIC".encode_utf16().chain(Some(0)).collect();
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    std::ptr::null(),
                    style,
                    x,
                    y,
                    w,
                    h,
                    parent,
                    0,
                    0,
                    std::ptr::null(),
                )
            };
            assert_ne!(hwnd, 0, "{}", error("CreateWindowExW"));
            Owned(hwnd)
        }
        #[test]
        fn real_child_origin_maps_to_screen_and_centers_owned_popup() {
            let _physical = Physical::enter().unwrap();
            let host = create(0x8000_0000, 200, 100, 800, 600, 0);
            let child = create(0x4000_0000, 30, 50, 400, 300, host.0);
            let popup = create(0x8000_0000 | 0x00c0_0000, 0, 0, 200, 100, 0);
            let a = anchor(child.0).unwrap();
            assert_eq!(
                (a.client.left, a.client.top, a.client.right, a.client.bottom),
                (230, 150, 630, 450)
            );
            center(popup.0, a).unwrap();
            let mut actual = Rect::default();
            assert_ne!(unsafe { GetWindowRect(popup.0, &mut actual) }, 0);
            assert_eq!((actual.left, actual.top), (330, 250));
            assert_eq!(
                (actual.right - actual.left, actual.bottom - actual.top),
                (200, 100)
            );
            // Diary top-level origin uses its client, rather than monitor center.
            center(popup.0, anchor(host.0).unwrap()).unwrap();
            assert_ne!(unsafe { GetWindowRect(popup.0, &mut actual) }, 0);
            assert_eq!((actual.left, actual.top), (500, 350));
            assert!(center(child.0, a).is_err());
            assert!(anchor(0).is_err());
            let raw = host.0;
            assert!(std::thread::spawn(move || anchor(raw))
                .join()
                .unwrap()
                .is_err());
            drop(host);
            assert!(anchor(raw).is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn centers_on_origin_not_monitor_and_keeps_negative_coordinates() {
        let anchor = Anchor {
            client: Rect {
                left: -1700,
                top: 100,
                right: -900,
                bottom: 700,
            },
            work: Rect {
                left: -1920,
                top: 0,
                right: 0,
                bottom: 1040,
            },
        };
        assert_eq!(placement(anchor, 400, 300).unwrap(), (-1500, 250));
        // A smaller source does not turn the popup into a child or shrink it.
        let small = Anchor {
            client: Rect {
                left: -1500,
                top: 300,
                right: -1300,
                bottom: 400,
            },
            ..anchor
        };
        assert_eq!(placement(small, 440, 480).unwrap(), (-1620, 110));
    }
    #[test]
    fn clamps_to_origin_work_area_without_losing_title_bar() {
        let anchor = Anchor {
            client: Rect {
                left: 1800,
                top: 900,
                right: 2000,
                bottom: 1100,
            },
            work: Rect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            },
        };
        assert_eq!(placement(anchor, 440, 480).unwrap(), (1480, 560));
        assert_eq!(placement(anchor, 2200, 1200).unwrap(), (0, 0));
        assert!(placement(anchor, 0, 400).is_err());
    }
}
