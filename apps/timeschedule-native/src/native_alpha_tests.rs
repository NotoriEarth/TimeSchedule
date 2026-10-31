//! Real Win32/softbuffer contract, using only test-owned hidden HWNDs.
use raw_window_handle::*;
use std::{num::NonZeroIsize, ptr, rc::Rc};
use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};

struct OwnedWindow(HWND);
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0);
        }
    }
}
impl HasWindowHandle for OwnedWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(
                NonZeroIsize::new(self.0 as isize).unwrap(),
            )))
        })
    }
}
impl HasDisplayHandle for OwnedWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Windows(WindowsDisplayHandle::new()))
        })
    }
}

#[test]
fn per_pixel_presenter_requires_layered_style_and_supports_child_windows() {
    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    let make = |parent: HWND, style| {
        let handle = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                style,
                0,
                0,
                32,
                16,
                parent,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        assert!(!handle.is_null(), "{}", std::io::Error::last_os_error());
        Rc::new(OwnedWindow(handle))
    };
    let parent = make(ptr::null_mut(), WS_POPUP);
    let child = make(parent.0, WS_CHILD);
    let context = softbuffer::Context::new(parent.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, child.clone()).unwrap();
    surface
        .resize(32.try_into().unwrap(), 16.try_into().unwrap())
        .unwrap();
    let present = |surface: &mut softbuffer::Surface<_, _>| {
        let mut buffer = surface.buffer_mut().unwrap();
        buffer.fill(0x80000080); // half-transparent premultiplied blue
        buffer[0] = 0xffffffff; // opaque ink
        buffer.present()
    };
    present(&mut surface).unwrap(); // upstream RGB path
    let property: Vec<u16> = timeschedule_desktop::windows::PER_PIXEL_ALPHA_PROPERTY
        .encode_utf16()
        .chain(Some(0))
        .collect();
    assert_ne!(
        unsafe { SetPropW(child.0, property.as_ptr(), 1usize as _) },
        0
    );
    assert!(
        present(&mut surface).is_err(),
        "opted-in alpha must report an invalid non-layered HWND, never silently use RGB"
    );
    unsafe {
        SetWindowLongW(child.0, GWL_EXSTYLE, WS_EX_LAYERED as i32);
    }
    present(&mut surface).expect("premultiplied alpha on a layered child");
    surface
        .resize(40.try_into().unwrap(), 24.try_into().unwrap())
        .unwrap();
    present(&mut surface).expect("resized alpha surface");
    let mut rect = windows_sys::Win32::Foundation::RECT::default();
    assert_ne!(unsafe { GetWindowRect(child.0, &mut rect) }, 0);
    assert_eq!((rect.right - rect.left, rect.bottom - rect.top), (40, 24));
    unsafe {
        RemovePropW(child.0, property.as_ptr());
        SetWindowLongW(child.0, GWL_EXSTYLE, 0);
    }
    present(&mut surface).expect("ordinary RGB presentation is preserved");
}
