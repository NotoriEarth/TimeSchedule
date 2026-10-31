# Local softbuffer 0.4.8 patch

Source: the published `softbuffer 0.4.8` crate, upstream revision recorded in `.cargo_vcs_info.json`. Upstream source and MIT/Apache licenses are retained. Cargo resolves this copy only in the root native workspace; the separate Tauri workspace is unchanged.

Only `src/backends/win32.rs` differs from the upstream source. An HWND with nonzero `TimeSchedule.PerPixelAlpha.v1` opts into premultiplied BGRA presentation via `UpdateLayeredWindow(ULW_ALPHA)`. The high byte is alpha only in this opt-in mode. Normal windows keep the upstream 0x00RRGGBB / BitBlt contract. The caller must enable WS_EX_LAYERED, avoid CS_OWNDC/CS_CLASSDC and must not call SetLayeredWindowAttributes after opting in. The presenter reports ULW errors rather than falling back to opaque rendering. NULL destination preserves window position; the buffer dimensions set the presentation size.

The native app enables the property only on its own calendar HWND, on its UI thread. Other apps and Explorer are not changed. Tiny-skia's `engine::into_color` swaps red/blue and retains alpha, producing the required premultiplied BGRA pixels. Clear color is transparent, cell fills use the background opacity preference, and text retains its own alpha.

Verification: `cargo test -p timeschedule-native per_pixel_presenter --locked` exercises real hidden parent/child HWNDs through public softbuffer APIs, rejection of a non-layered target, successful layered-child updates, resize, and unchanged RGB mode. `widget_background_alpha_changes_without_fading_solid_ink` checks actual renderer output in both themes. These checks do not by themselves verify desktop hit testing or visual composition on all Windows versions.

API references: [UpdateLayeredWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow), [SetLayeredWindowAttributes](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setlayeredwindowattributes). Re-evaluate this patch when upgrading Iced or softbuffer.
