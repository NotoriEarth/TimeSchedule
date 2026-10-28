# Experimental Windows desktop attachment

Target host: Windows 10 Pro 22H2, build 19045.7807. Windows 11 is a best-effort compatibility target. This note describes the implementation and its limits; it is not a record of successful visual acceptance testing.

## Chosen hierarchy and input behavior

`src-tauri/src/desktop.rs` discovers the existing visible `Progman` or `WorkerW` belonging to the process returned by `GetShellWindow`, with a direct `SHELLDLL_DefView` child. It requires exactly one candidate. The widget becomes a child of that host and is ordered above `SHELLDLL_DefView` within that host. The module never raises the host itself.

```text
Explorer desktop host: Progman or WorkerW
  widget: WS_CHILD, above the icon surface within its rectangle
  SHELLDLL_DefView
    desktop icon list
Ordinary application windows remain separate top-level windows.
```

The widget intentionally covers desktop icons inside its own rectangle. It must be placed in an unused area of the desktop. Native pointer events can reach the widget directly; there is no global mouse hook, input forwarding, cursor polling, or click-through mode. Locked means position and size are fixed; date clicks remain available. `WM_MOUSEACTIVATE` returns `MA_NOACTIVATE`, which preserves the click without activating the desktop host. Text editing and keyboard-heavy interaction belong in the ordinary manager window. See [Microsoft's WM_MOUSEACTIVATE behavior](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mouseactivate).

The common live-wallpaper arrangement places content behind the icon surface. That arrangement is unsuitable for an interactive calendar unless input is forwarded separately. Lively's upstream implementation documents both the classic separate WorkerW hierarchy and the newer layered Progman hierarchy. Its implementation also demonstrates that shell topology changes need explicit handling. This prototype does not copy Lively's code, spawn wallpaper hosts, change wallpaper, or alter any Explorer window styles. See [Lively WinDesktopCore](https://github.com/rocksdanister/lively/blob/core-separation/src/Lively/Lively/Core/WinDesktopCore.cs).

Rainmeter implements desktop positioning through its own Z-order machinery and Show Desktop handling. Its implementation does not establish that `alwaysOnBottom` alone is sufficient. This prototype uses actual child parenting instead. See [Rainmeter Skin.cpp](https://github.com/rainmeter/rainmeter/blob/master/Library/Skin.cpp).

## Public integration API

No new Cargo dependency is required. The module links the Windows system libraries user32, kernel32 and comctl32 through small raw FFI declarations. Windows 10 version 1607 or newer is required by the DPI APIs; the actual target is 22H2.

```rust
pub fn with_shell_dpi_context<T>(create: impl FnOnce() -> Result<T, String>) -> Result<T, String>;
pub fn attach(window: &tauri::WebviewWindow) -> Result<AttachmentInfo, String>;
pub fn status(window: &tauri::WebviewWindow) -> Result<AttachmentInfo, String>;
pub fn detach(window: &tauri::WebviewWindow) -> Result<(), String>;
pub fn set_locked(window: &tauri::WebviewWindow, locked: bool) -> Result<(), String>;
pub fn set_visible(window: &tauri::WebviewWindow, visible: bool) -> Result<AttachmentInfo, String>;
```

`AttachmentInfo` serializes camelCase and contains `attached`, `strategy`, `message`, `hwnd`, `parentHwnd`, `shellHostHwnd`, `shellClass`, and `locked`. Handles are hexadecimal strings, avoiding JavaScript integer precision assumptions. The strategy is `explorer-icon-host-child`.

1. Create the manager normally.
2. On the Tauri main/UI thread, call `with_shell_dpi_context` around synchronous `WebviewWindowBuilder::build()`. Use the exact window label `widget`. Configure the widget hidden, undecorated, non-topmost, skipped from the taskbar, and nonresizable. Place its initial screen rectangle fully inside an attached monitor before calling `attach`.
3. Call `attach` on that same UI thread. It starts locked. Use `desktop::set_visible(&widget, true)` only after success. Keep it hidden and expose the error in the manager when attachment fails.
4. Dispatch command-driven attachment, detachment, lock, and visibility changes with `run_on_main_thread`. The module checks the owning thread and process before changing a window. Do not set topmost, bottommost, decorations, focusability, resizability, or window ownership through Tauri after attachment. Use `desktop::set_visible` for both show and hide: inspected Tao 0.37.2 `WindowFlags::apply_diff` rewrites native styles from its cached top-level flags even on a visibility change, so Tauri's ordinary `show()`/`hide()` would remove `WS_CHILD`. The manager continues to use normal Tauri window APIs.
5. `status` queries current native state without changing anything. Use it when reporting application status or responding to Show Widget; do not report a cached initial success forever. It checks actual parent handles, child/pop-up/topmost styles, Explorer process identity, the icon-view handle, and sibling ordering above the icon view. `attached` means these structural checks passed, not that all visual acceptance cases have been verified.
6. `detach` hides the widget and restores its original top-level styles while keeping its current screen position. It can then be destroyed or deliberately repositioned and attached again. A failure is returned; it must not be reported as success. Normal application exit can simply destroy the widget; the subclass removes its record on `WM_NCDESTROY`.

## DPI, positioning, and rollback

Cross-process `SetParent` with mismatched DPI awareness can reset the child process's DPI awareness, potentially disturbing the manager as well. The module compares the two real window contexts with `AreDpiAwarenessContextsEqual` and refuses a mismatch. The scoped creation helper temporarily adopts the shell's context on the calling UI thread, then restores the prior context. If the framework overrides that during window creation, attachment still refuses the mismatch; changing thread context after creation is not a fix for an existing HWND. See [Microsoft SetParent](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setparent), [DPI context comparison](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-aredpiawarenesscontextsequal), and [thread DPI scopes](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setthreaddpiawarenesscontext).

`WS_POPUP` is cleared and `WS_CHILD` is set before parenting, followed by a frame refresh. Parent changes check both the last-error result and the actual parent HWND. Style writes clear last error before checking the zero-return case. Failures trigger restoration and leave the widget hidden; restoration failures are included in the returned error. See [Microsoft window styles and SetWindowLong](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongw).

The initial window rectangle is captured and converted from screen coordinates into the shell host's client coordinates with `MapWindowPoints`, using a matching thread DPI scope. Negative virtual-desktop monitor coordinates are therefore not treated as primary-monitor-relative coordinates. A rectangle that would be clipped by the host is rejected. Revision 0.1.2 adds ongoing position recovery when monitor geometry changes, as described below. Unlock or detach before deliberately changing the widget geometry, because native position locking blocks movement and resizing. See [Microsoft MapWindowPoints](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-mapwindowpoints) and [SetWindowPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos).

Only the widget is subclassed, on its owning thread, to keep pointer clicks non-activating and implement the move/size lock. No Explorer procedure is subclassed. See [Microsoft SetWindowSubclass](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-setwindowsubclass).

## Known limits and required acceptance checks

- Explorer's `Progman`, `WorkerW`, and `SHELLDLL_DefView` topology is an implementation detail rather than a documented desktop-widget contract. Windows updates, third-party shells, wallpaper applications, or shell transitions can change it.
- Windows 11 24H2 and later have changed desktop composition. This implementation only accepts an existing matching hierarchy; it does not force the layered wallpaper path. Windows 11 must be tested separately and is not certified by Windows 10 results.
- A shell restart may destroy the widget HWND. A later `status` returns a missing-window or detached error. There is no watchdog or automatic window recreation in this module. Recreate the widget through the app's own normal window lifecycle, or restart the app; do not kill/restart Explorer as a test shortcut.
- The shell can reorder/reparent the icon view during wallpaper or display changes. A later structural check detects a changed sibling order or icon host. Recovery requires deliberately recreating or detaching/re-attaching the widget. The geometry timer introduced in 0.1.2 does not recreate destroyed windows or repair a changed shell hierarchy.
- Parent attachment does not prove WebView2 has rendered, that pointer events work, or that Win+D behavior is correct. Only tests on the actual host can establish those points.
- Keyboard navigation and screen-reader behavior of the non-activating widget have not been established. The manager is the primary interactive interface.

Acceptance on the actual host must cover: widget visible on the desktop; Win+D in both directions; taskbar Show Desktop; ordinary window overlap; clicking a calendar date opens and focuses the normal manager; locking preserves date clicks and prevents drag; unlocking permits the intended drag region; tray Hide/Show preserves desktop parenting; clean Quit; and at least one secondary-monitor/DPI configuration if that configuration is to be supported. Record which cases were actually observed rather than equating a successful build with desktop behavior.

## Revision 0.1.1: discoverable movement and whole-widget translucency

The user reported being unable to move the widget and then clarified that the small lock icon was not discoverable. The wide month view now exposes text controls (`이동하기` / `위치 고정`) and a persistent movement hint. Tauri's deep drag-region option includes header descendants, while interactive controls explicitly opt out. An unlocked native drag failure has not been established, so the native drag mechanism was retained.

The widget uses constant native alpha, adjustable from 40 to 100 percent (default 82). This fades the entire widget, including its text, over the actual desktop; it is not background-only per-pixel transparency. A normal opaque WebView avoids relying on DWM blur for a reparented child window. [SetLayeredWindowAttributes](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setlayeredwindowattributes) supports child layered windows on Windows 8 and later, with an appropriate [compatibility manifest](https://learn.microsoft.com/en-us/windows/win32/winmsg/using-windows#using-layered-windows).

Tao 0.37.2 registers its standard class with `CS_OWNDC`, which is incompatible with [WS_EX_LAYERED](https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles). Before constructing the widget, the app clones its own manager's actual class into a dedicated class name, preserving its window procedure, module, extra bytes and other attributes, and removing only `CS_OWNDC`/`CS_CLASSDC`. The manager's class is never changed. The current Tao implementation tolerates a pre-registered custom class; dependency upgrades must recheck that behavior. Preparation failures prevent widget construction while leaving the manager usable.

The separate `desktop_native` integration executable embeds the same Windows 8/10 manifest and exercises hidden windows owned only by the test process. It checks class isolation, collision handling, alpha readback, and parent/style preservation without touching Explorer. These tests do not establish how WebView2 renders on a particular GPU or whether mouse dragging works on the actual desktop.

Actual application startup exposed two additional integration conditions. Tao's undecorated shadows add nonclient insets outside the requested inner size, so the desktop widget explicitly disables native shadows before creation. Windows also normalizes frame styles during child conversion; the generic style setter accepts documented API success and uses exact readback only to resolve an ambiguous zero-return/last-error case. Child conversion removes `WS_EX_WINDOWEDGE`, while attachment and opacity retain their own explicit invariants. The native integration fixture covers this top-level-to-child transition. After these corrections, the current Windows 10 application reported both `attached=true` and `opacityPercent=82` on 2026-10-28. This is native state evidence, not a screenshot or a mouse-interaction acceptance result.

On 2026-10-28 the user separately confirmed that the desktop background shows through the revised calendar and dragging its upper area moves it. The final release executable was then rebuilt from the same corrected source, restarted, and confirmed to report attachment and 82% opacity. Other acceptance cases listed above remain scoped separately.


## Revision 0.1.2: recovery after display geometry changes

A resolution change left the locked widget clipped, making its top-right Move/Lock controls unreachable. Initial placement was clamped only when constructing a window, while WM_WINDOWPOSCHANGING rejected subsequent position and size changes when locked. Re-showing an existing window did not fit it to the new display.

Each attached widget now owns a 1-second WM_TIMER on its UI thread. It intersects the nearest surviving monitor work area with the host client rectangle in the widget DPI context, clamps the existing rectangle, and changes only the widget geometry with NOZORDER and NOACTIVATE. A narrowly scoped recovery flag bypasses movement locking during that call without changing the saved lock preference. Native drag loops defer recovery. Detach and destruction remove the timer. Errors are included in the live desktop status. The timer does not call an AI model or use any tokens.

The manager and tray also expose a position-recovery action using the same show-and-recover path. This remains useful when the widget controls are off-screen. Alpha, parent, z-order and visibility are preserved by automatic recovery. Shrunk widgets retain their smaller size until recreated; Explorer-restart recovery is outside this change.

The Windows native fixture reproduces the failure using a locked, offscreen child and a shrunken hidden parent owned by the test process. Before the fix it reported Rect { left: 150, top: -20, right: 230, bottom: 40 } after the recovery message. After the fix it verifies containment, lock retention, unchanged alpha and parent, and no unintended display of a hidden widget. This test does not change the real screen resolution.

On 2026-10-28 the 0.1.2 release was rebuilt and launched on the current PC. The persisted widget Y coordinate changed from -133 to 0, live startup status confirmed attachment and 82% opacity, and the manager accessibility tree exposed the recovery button and version 0.1.2. The user subsequently confirmed that the controls were visible again. Native screenshot capture timed out and coordinate input geometry was unavailable, so neither clicking the recovery control nor a real resolution-switch acceptance run is claimed.
