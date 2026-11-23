//! Windows: the title bar and frame of a window without OS decorations (#1381, #13996).
//!
//! Such a window keeps its native thick frame (`WS_THICKFRAME`, so Aero Snap, Win+arrows and the
//! OS's own move and size loops work). winit (vendored) gives it Windows' own frame on the left,
//! right and bottom (the invisible resize borders and the shadow) and none at the top: the top of
//! the window is client area, where the UI draws its title bar. One window procedure
//! ([`caption_proc`]) answers the system for every such window, from the bar the window's UI
//! published (`solidcraft_ui_egui::window_chrome::caption`):
//!
//! - `WM_NCHITTEST`: the band along the top edge resizes (Windows' resize-border width for the
//!   window's current DPI, 8 px at 100 %, with corners at its ends), the bare title bar is
//!   `HTCAPTION` (Windows drags the window from the first pixel, maximizes it on a double-click
//!   and snaps it) and the maximize button is `HTMAXBUTTON` (Windows 11's snap layouts). The bar's
//!   controls stay the UI's.
//! - What Windows then keeps for itself is told to the UI: where the pointer is over the caption
//!   (the UI sees it there as anywhere else in the window), a press on the caption or on the
//!   maximize button (to the UI a press on its bar, so its menus close as on a bar of its own), a
//!   right-click on the caption (the UI's own menu for its bar opens, as before) and the maximize
//!   button held down. A press on a resize border is the system's alone.
//!
//! The main window gets the procedure when it is created ([`install`]), so the OS shows its
//! resize cursors and runs its own resize from the first message, whatever egui has laid out;
//! every other window gets it when it first draws the bar ([`register`]).
//!
//! The procedure also answers `WM_GETDPISCALEDSIZE` with the size the window will really have at
//! the new DPI (the same logical size, as winit computes it, with the frame around it), so the
//! rect Windows suggests while the title bar is dragged onto a monitor with another scale is the
//! rect winit then applies: the window rescales once at the crossing and stays under the pointer.
//! A snapped window leaves the answer to Windows: it derives the size to restore to from this
//! one, so the snapped size would replace the size from before the snap (#9336).
//!
//! Replacing a window procedure and setting a window class's icon have no safe API, hence the
//! `unsafe_code` allowance on this module (the app crate denies it; see `native_crash.rs` and
//! `open_documents.rs` for the others).
//! The procedure is installed on the thread that owns the window, once per window, and calls the
//! one that window had before for everything it doesn't answer: winit's, or for the main window
//! the accessibility adapter's, which stands in front of winit's.
//!
//! [`set_class_icon`] gives the window's class the exe's icon, so the taskbar button shows the
//! mark even when the window is too busy to answer the taskbar's request for its icon.
//!
//! [`WindowMove`] passes Preferences › Layout's `windows_smooth_window_drag` to winit: on, the app
//! draws nothing while the window is dragged by its title bar and catches up when the drag ends;
//! off (the default), it keeps drawing during the drag, as before.
#![allow(unsafe_code)]

use std::sync::{Mutex, MutexGuard, PoisonError};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use solidcraft_ui_egui::titlebar;
use solidcraft_ui_egui::window_chrome::caption::{self, CaptionHit, PxRect};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, ScreenToClient};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent};
use windows_sys::Win32::UI::Shell::{ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETAUTOHIDEBAREX, APPBARDATA, SHAppBarMessage};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, GCLP_HICON, GCLP_HICONSM, GWL_EXSTYLE, GWL_STYLE, GWLP_WNDPROC, GetClientRect, GetWindowLongPtrW,
    GetWindowPlacement, GetWindowRect, GetWindowThreadProcessId, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTMAXBUTTON,
    HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IMAGE_ICON, IsIconic, IsZoomed, LR_DEFAULTCOLOR, LoadImageW, NCCALCSIZE_PARAMS, SM_CXICON,
    SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SM_CXSMICON, SW_MAXIMIZE, SW_RESTORE, SetWindowLongPtrW, ShowWindow, TITLEBARINFOEX, WINDOWPLACEMENT,
    WM_GETDPISCALEDSIZE, WM_GETTITLEBARINFOEX, WM_MOUSEMOVE, WM_NCCALCSIZE, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN,
    WM_NCLBUTTONUP, WM_NCMBUTTONDBLCLK, WM_NCMBUTTONDOWN, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_NCRBUTTONDBLCLK, WM_NCRBUTTONDOWN, WM_NCRBUTTONUP,
    WM_NCXBUTTONDBLCLK, WM_NCXBUTTONDOWN, WNDPROC, WS_THICKFRAME,
};
// windows-sys has SetClassLongPtrW only on 64-bit targets; on 32-bit Windows it is SetClassLongW.
#[cfg(target_pointer_width = "64")]
use windows_sys::Win32::UI::WindowsAndMessaging::SetClassLongPtrW;
#[cfg(target_pointer_width = "32")]
use windows_sys::Win32::UI::WindowsAndMessaging::SetClassLongW as SetClassLongPtrW;

/// The work area (the monitor without the taskbar) of the monitor nearest the window, in physical
/// pixels: left, top, width, height.
pub fn work_area(window: &impl HasWindowHandle) -> Option<(i32, i32, i32, i32)> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(w) = handle.as_raw() else { return None };
    let hwnd = w.hwnd.get() as HWND;
    // SAFETY: `hwnd` is the live main window; `info` is a MONITORINFO with its size set, as
    // GetMonitorInfoW requires, and the monitor handle comes from the call before it.
    let rc = unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if monitor.is_null() || GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        info.rcWork
    };
    Some((rc.left, rc.top, rc.right - rc.left, rc.bottom - rc.top))
}

/// The exe's icon resource (`build.rs` embeds it as `1 ICON`).
const ICON_RESOURCE: usize = 1;

/// Give the main window's class the exe's icon, big and small. Call it before the window is
/// shown (eframe's app creator).
///
/// The taskbar asks a window for its icon when the window appears. eframe sets the window's
/// icon before that, while the window is still hidden, and then shows the window at the end of
/// its first frame; if that frame is slow, or startup kept the thread busy long enough for
/// Windows to mark the window as not responding, the request isn't answered in time. The taskbar then
/// takes the window class's icon and doesn't ask again. winit registers its class without one,
/// so the button showed Windows' generic icon for as long as the window was open.
/// `false` if it isn't a Win32 window or the exe's icon doesn't load; SetClassLongPtrW's own
/// failure can't be told from a class that had no icon (both return 0), so it isn't reported.
pub fn set_class_icon(window: &impl HasWindowHandle) -> bool {
    let Ok(handle) = window.window_handle() else { return false };
    let RawWindowHandle::Win32(w) = handle.as_raw() else { return false };
    let hwnd = w.hwnd.get() as HWND;
    // SAFETY: a plain query on the live main window.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let (Some(big), Some(small)) = (exe_icon(SM_CXICON, dpi), exe_icon(SM_CXSMICON, dpi)) else { return false };
    // SAFETY: `hwnd` is the live main window and both icons are valid handles. The class keeps
    // them for the rest of the process, so they are never destroyed. `as _`: on 32-bit Windows
    // this is SetClassLongW (see the imports), which takes an i32.
    unsafe {
        SetClassLongPtrW(hwnd, GCLP_HICON, big as _);
        SetClassLongPtrW(hwnd, GCLP_HICONSM, small as _);
    }
    true
}

/// The exe's icon at the size of system metric `metric` (`SM_CXICON`, `SM_CXSMICON`) at `dpi`.
fn exe_icon(metric: i32, dpi: u32) -> Option<windows_sys::Win32::Foundation::HANDLE> {
    // SAFETY: GetModuleHandleW(null) is this exe, loaded for the life of the process; the name
    // is an integer resource id (MAKEINTRESOURCE), which LoadImageW accepts in place of a string.
    let icon = unsafe {
        let size = GetSystemMetricsForDpi(metric, dpi);
        LoadImageW(GetModuleHandleW(std::ptr::null()), std::ptr::without_provenance(ICON_RESOURCE), IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
    };
    (!icon.is_null()).then_some(icon)
}

/// The window procedure ours replaced, by window ([`window_id`]), as an address. The windows of
/// the app do not all have the same one: the main window's is the accessibility adapter's, which
/// stands in front of winit's, and a pop-out view's is winit's own.
static PREVIOUS: Mutex<Vec<(u64, isize)>> = Mutex::new(Vec::new());

/// The list of replaced procedures. A panic while it was held leaves plain numbers, so a poisoned
/// lock is used as it is.
fn replaced() -> MutexGuard<'static, Vec<(u64, isize)>> {
    PREVIOUS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Let every window that draws SolidCraft's title bar hand it to Windows: the UI asks for the
/// procedure when a window first publishes its bar ([`hook`]), and publishes from the first
/// frame of `ctx`. Call it once, before that frame.
pub fn register(ctx: &egui::Context) {
    caption::set_platform(hook);
    solidcraft_ui_egui::window_chrome::install(ctx);
}

/// Answer the main window's hit test and DPI-size questions from now on. Call it on the thread
/// that created the window (eframe's app creator), so the resize border is there from the first
/// message. `false` if it isn't a Win32 window, or the procedure could not be put in place.
pub fn install(window: &impl HasWindowHandle) -> bool {
    let Ok(handle) = window.window_handle() else { return false };
    let RawWindowHandle::Win32(w) = handle.as_raw() else { return false };
    attach(w.hwnd.get() as HWND)
}

/// The UI's [`caption::Hook`]: native window `window` has drawn the title bar.
fn hook(window: u64) {
    attach(window as usize as HWND);
}

/// The window as the UI names it (`egui::ViewportInfo::native_window`).
fn window_id(hwnd: HWND) -> u64 {
    hwnd as usize as u64
}

/// Put [`caption_proc`] in place for `hwnd` and tell the UI's side; `true` if it answers for the
/// window now, also when it already did.
fn attach(hwnd: HWND) -> bool {
    let ours = caption_proc as *const () as isize;
    // SAFETY: plain queries; for a handle that is no window both answer 0.
    let (thread, current) = unsafe { (GetWindowThreadProcessId(hwnd, std::ptr::null_mut()), GetWindowLongPtrW(hwnd, GWLP_WNDPROC) as isize) };
    // A procedure is replaced on the thread that owns the window only.
    // SAFETY: a plain query.
    if current == 0 || thread == 0 || thread != unsafe { GetCurrentThreadId() } {
        return false;
    }
    if current != ours {
        // The procedure this window has now is the one ours calls for every other message. It is
        // on the list before ours is in place, so no message finds ours without it.
        let window = window_id(hwnd);
        {
            let mut replaced = replaced();
            replaced.retain(|(listed, _)| *listed != window);
            replaced.push((window, current));
        }
        // SAFETY: `hwnd` is a live window of this thread; `caption_proc` has the WNDPROC signature
        // and stays valid for the life of the process. SetWindowLongPtrW returns the previous
        // procedure. `as _`: on 32-bit Windows SetWindowLongPtrW is SetWindowLongW, which takes
        // and returns i32.
        let previous = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours as _) } as isize;
        let mut replaced = replaced();
        if previous == 0 {
            replaced.retain(|(listed, _)| *listed != window);
            return false;
        }
        if let Some(entry) = replaced.iter_mut().find(|(listed, _)| *listed == window) {
            entry.1 = previous;
        }
    }
    caption::hooked(window_id(hwnd));
    true
}

/// The address of the procedure ours replaced for `hwnd`; 0 for a window that is not on the list.
fn previous_address(hwnd: HWND) -> isize {
    let window = window_id(hwnd);
    replaced().iter().find(|(listed, _)| *listed == window).map_or(0, |(_, address)| *address)
}

/// The procedure ours replaced for `hwnd`.
fn previous(hwnd: HWND) -> WNDPROC {
    let p = previous_address(hwnd);
    // SAFETY: a nonzero value is the WNDPROC this window had when `attach` replaced it.
    (p != 0).then(|| unsafe { std::mem::transmute::<isize, unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>(p) })
}

/// Answers the system for one window from the title bar its UI published: the resize band along
/// the top, the caption and the maximize button. The system frame answers for the left, right
/// and bottom.
unsafe extern "system" fn caption_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let prev = previous(hwnd);
    // SAFETY: the arguments are the message as Windows delivered it to this window. A window
    // with ours in place is on the list (`attach`); were it not, the system's default answers.
    let call_previous = || unsafe {
        match prev {
            Some(_) => CallWindowProcW(prev, hwnd, msg, wparam, lparam),
            None => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    };
    let window = window_id(hwnd);
    match msg {
        WM_NCHITTEST => {
            let default = call_previous();
            // The system frame (left, right, bottom) answers for itself.
            if default != HTCLIENT as LRESULT {
                return default;
            }
            let (x, y) = screen_point(lparam);
            // The client area: the resize band along the top, then the title bar the UI published.
            if let Some(hit) = top_border_hit(hwnd, x, y) {
                return hit as LRESULT;
            }
            client_point(hwnd, x, y).and_then(|(cx, cy)| caption_code(caption::hit(window, cx, cy))).map_or(default, |hit| hit as LRESULT)
        }
        // The pointer over the caption, the maximize button or the top band is Windows': the UI
        // is told where it is. A press dragged off the maximize button is no click.
        WM_NCMOUSEMOVE => {
            let result = call_previous();
            let (x, y) = screen_point(lparam);
            caption::pointer(window, client_point(hwnd, x, y));
            watch_for_leave(hwnd);
            if wparam != HTMAXBUTTON as WPARAM {
                caption::set_maximize_pressed(window, false);
            }
            result
        }
        // In the client area the UI sees the pointer itself; off the window nobody does.
        WM_MOUSEMOVE | WM_NCMOUSELEAVE => {
            caption::pointer(window, None);
            caption::set_maximize_pressed(window, false);
            call_previous()
        }
        // A press on the maximize button stops here: the default handling would run its own loop
        // and draw a classic button. The UI draws its button held down.
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if wparam == HTMAXBUTTON as WPARAM => {
            report_press(hwnd, lparam);
            caption::set_maximize_pressed(window, true);
            0
        }
        // egui sees no press on the caption: the UI is told where it was, as the press on its bar
        // that it is on a bar of its own (so its open menus close), and the press does what it
        // does. A press on a resize border is the system's alone, as on the system's own frame.
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK | WM_NCMBUTTONDOWN | WM_NCMBUTTONDBLCLK | WM_NCXBUTTONDOWN | WM_NCXBUTTONDBLCLK => {
            if wparam == HTCAPTION as WPARAM {
                report_press(hwnd, lparam);
            }
            call_previous()
        }
        WM_NCLBUTTONUP if wparam == HTMAXBUTTON as WPARAM => {
            if caption::set_maximize_pressed(window, false) {
                // SAFETY: plain calls on a window handle the system passed in.
                unsafe { ShowWindow(hwnd, if IsZoomed(hwnd) != 0 { SW_RESTORE } else { SW_MAXIMIZE }) };
            }
            0
        }
        // Windows opens its window menu on a right-click only over a caption it draws itself. This
        // caption is the UI's bar, so the click goes back to the UI when the button comes up: its
        // own menu for the bar opens, as on a bar the system does not answer for.
        WM_NCRBUTTONDOWN | WM_NCRBUTTONDBLCLK if wparam == HTCAPTION as WPARAM => 0,
        WM_NCRBUTTONUP if wparam == HTCAPTION as WPARAM => {
            let (x, y) = screen_point(lparam);
            if let Some((cx, cy)) = client_point(hwnd, x, y) {
                caption::secondary_click(window, cx, cy);
            }
            0
        }
        // The system derives the title bar and its buttons from the non-client frame, which has
        // no top here: report the ones the UI drew.
        WM_GETTITLEBARINFOEX => {
            let result = call_previous();
            // SAFETY: for this message lParam points to a TITLEBARINFOEX the caller reads back.
            if let Some(info) = unsafe { (lparam as *mut TITLEBARINFOEX).as_mut() } {
                report_title_bar(hwnd, info);
            }
            result
        }
        WM_GETDPISCALEDSIZE => {
            let new_dpi = wparam as u32;
            // SAFETY: for this message lParam points to a SIZE the system reads back.
            match (resizable(hwnd) && !snapped(hwnd), client_size(hwnd), unsafe { (lparam as *mut SIZE).as_mut() }) {
                (true, Some(size), Some(out)) => {
                    // SAFETY: a window handle the system passed in.
                    let old_dpi = unsafe { GetDpiForWindow(hwnd) };
                    match framed_size(titlebar::dpi_scaled_size(size, old_dpi, new_dpi), side_frame(hwnd, new_dpi)) {
                        Some(size) => {
                            *out = size;
                            1
                        }
                        None => call_previous(),
                    }
                }
                _ => call_previous(),
            }
        }
        WM_NCCALCSIZE => {
            let result = call_previous();
            // SAFETY: for a nonzero wParam, lParam points to the NCCALCSIZE_PARAMS the system
            // passed, whose first rect winit has just set (the maximized window's client area).
            // For a zero wParam lParam is a bare RECT, so no NCCALCSIZE_PARAMS reference is made.
            if wparam != 0
                && let Some(params) = unsafe { (lparam as *mut NCCALCSIZE_PARAMS).as_mut() }
            {
                leave_auto_hide_taskbar_reachable(hwnd, &mut params.rgrc[0]);
            }
            result
        }
        WM_NCDESTROY => {
            caption::closed(window);
            // Hand the window back to the procedure it had, for its last message, and forget it.
            let address = previous_address(hwnd);
            replaced().retain(|(listed, _)| *listed != window);
            if address != 0 {
                // SAFETY: restores the procedure this window had before `attach`.
                unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, address as _) };
            }
            call_previous()
        }
        _ => call_previous(),
    }
}

/// The screen point in a mouse message's `lParam`, signed (monitors left of or above the primary
/// one).
fn screen_point(lparam: LPARAM) -> (i32, i32) {
    (i32::from(lparam as u16 as i16), i32::from((lparam >> 16) as u16 as i16))
}

/// Screen point (`x`, `y`) in the window's client pixels, when it is inside the client area (the
/// title bar and the top band are: the frame has no top).
fn client_point(hwnd: HWND, x: i32, y: i32) -> Option<(i32, i32)> {
    let mut p = POINT { x, y };
    let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: `p` and `client` are valid to fill; the handle is the system's.
    let known = unsafe { ScreenToClient(hwnd, &mut p) != 0 && GetClientRect(hwnd, &mut client) != 0 };
    (known && p.x >= 0 && p.y >= 0 && p.x < client.right && p.y < client.bottom).then_some((p.x, p.y))
}

/// Tell the UI of a press the system takes on the caption or the maximize button, where it was
/// (the screen point in a mouse message's `lParam`).
fn report_press(hwnd: HWND, lparam: LPARAM) {
    let (x, y) = screen_point(lparam);
    if let Some((cx, cy)) = client_point(hwnd, x, y) {
        caption::pressed(window_id(hwnd), cx, cy);
    }
}

/// Ask for `WM_NCMOUSELEAVE` when the pointer leaves the caption. Without it a pointer that goes
/// straight off the window from there would stay on the caption for the UI.
fn watch_for_leave(hwnd: HWND) {
    let mut track = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE | TME_NONCLIENT,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    // SAFETY: a TRACKMOUSEEVENT with its size set, for a window handle the system passed in.
    unsafe { TrackMouseEvent(&mut track) };
}

/// Write the title bar the UI published and its buttons into `info`, in screen coordinates:
/// minimize, maximize and close (indexes 2, 3 and 5) sit side by side, each as wide as the
/// maximize button.
fn report_title_bar(hwnd: HWND, info: &mut TITLEBARINFOEX) {
    let Some((bar, Some(max))) = caption::title_bar(window_id(hwnd)) else { return };
    let screen = |r: PxRect| -> RECT {
        let mut a = POINT { x: r.min_x, y: r.min_y };
        let mut b = POINT { x: r.max_x, y: r.max_y };
        // SAFETY: valid POINTs to convert; the handle is the system's.
        unsafe {
            ClientToScreen(hwnd, &mut a);
            ClientToScreen(hwnd, &mut b);
        }
        RECT { left: a.x, top: a.y, right: b.x, bottom: b.y }
    };
    let width = max.max_x - max.min_x;
    let beside = |dx: i32| PxRect::new(max.min_x + dx, max.min_y, max.max_x + dx, max.max_y);
    info.rcTitleBar = screen(bar);
    info.rgrect[2] = screen(beside(-width));
    info.rgrect[3] = screen(max);
    info.rgrect[5] = screen(beside(width));
}

/// The `HT*` code for what the published title bar has at a point: the caption, the maximize
/// button, or `None` (the UI's controls, or off the bar: the default answer stands).
fn caption_code(hit: CaptionHit) -> Option<u32> {
    match hit {
        CaptionHit::Drag => Some(HTCAPTION),
        CaptionHit::MaximizeButton => Some(HTMAXBUTTON),
        CaptionHit::Client | CaptionHit::Outside => None,
    }
}

/// The window's size at a new DPI: the system frame ([`side_frame`]) around the scaled client
/// size. `None` when either is unknown (the default handling answers).
fn framed_size(client: Option<(i32, i32)>, frame: Option<(i32, i32, i32)>) -> Option<SIZE> {
    let ((cx, cy), (left, right, bottom)) = (client?, frame?);
    Some(SIZE { cx: cx + left + right, cy: cy + bottom })
}

/// The window's frame at `dpi` as winit's `WM_NCCALCSIZE` keeps it: left, right and bottom.
fn side_frame(hwnd: HWND, dpi: u32) -> Option<(i32, i32, i32)> {
    let mut r = RECT { left: 0, top: 0, right: 100, bottom: 100 };
    // SAFETY: plain queries on a window handle the system passed in; `r` is a valid RECT.
    let ok = unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        AdjustWindowRectExForDpi(&mut r, style, 0, ex_style, dpi)
    };
    (ok != 0).then_some((-r.left, r.right - 100, r.bottom - 100))
}

/// A maximized window fills the monitor's work area, which is the whole monitor when the taskbar
/// auto-hides; Windows then takes it for a fullscreen window and the taskbar never comes back
/// (#18473). Leave 1 px free at each edge that holds an auto-hide taskbar, as maximized windows
/// with a frame do, so pointing at that edge shows the bar.
fn leave_auto_hide_taskbar_reachable(hwnd: HWND, client: &mut RECT) {
    // SAFETY: plain queries on the window handle the system passed in.
    let (zoomed, monitor) = unsafe { (IsZoomed(hwnd) != 0, MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)) };
    if !zoomed || monitor.is_null() {
        return;
    }
    // SAFETY: `info` has its size set, as GetMonitorInfoW requires.
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    // SAFETY: the monitor handle is the one just returned.
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return;
    }
    let auto_hide = |edge: u32| {
        // SAFETY: zeroed APPBARDATA with its size and the queried edge and monitor rect set, as
        // ABM_GETAUTOHIDEBAREX requires; the call only reads them and returns a window handle.
        unsafe {
            let mut data: APPBARDATA = std::mem::zeroed();
            data.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
            data.uEdge = edge;
            data.rc = info.rcMonitor;
            SHAppBarMessage(ABM_GETAUTOHIDEBAREX, &mut data) != 0
        }
    };
    let edges = AutoHideEdges { left: auto_hide(ABE_LEFT), top: auto_hide(ABE_TOP), right: auto_hide(ABE_RIGHT), bottom: auto_hide(ABE_BOTTOM) };
    let (l, t, r, b) = shrink_for_auto_hide((client.left, client.top, client.right, client.bottom), edges);
    *client = RECT { left: l, top: t, right: r, bottom: b };
}

/// Which edges of a monitor have an auto-hide taskbar.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct AutoHideEdges {
    left: bool,
    top: bool,
    right: bool,
    bottom: bool,
}

/// A maximized client rect (left, top, right, bottom) with 1 px taken off each edge that has an
/// auto-hide taskbar. A rect too small to lose the pixel is left as it is.
fn shrink_for_auto_hide(rect: (i32, i32, i32, i32), edges: AutoHideEdges) -> (i32, i32, i32, i32) {
    let (l, t, r, b) = rect;
    let (nl, nt) = (l + i32::from(edges.left), t + i32::from(edges.top));
    let (nr, nb) = (r - i32::from(edges.right), b - i32::from(edges.bottom));
    if nr > nl && nb > nt { (nl, nt, nr, nb) } else { rect }
}

/// The window can be resized by its border now: it has the thick frame (not fullscreen, which
/// drops it) and is neither maximized nor minimized.
fn resizable(hwnd: HWND) -> bool {
    // SAFETY: plain queries on a window handle the system passed in.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        style & WS_THICKFRAME != 0 && IsZoomed(hwnd) == 0 && IsIconic(hwnd) == 0
    }
}

/// The window is snapped (Aero Snap, Win+arrows): it is not the size it will restore to.
fn snapped(hwnd: HWND) -> bool {
    let empty = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let mut window = empty;
    // SAFETY: both structs are valid to fill, and WINDOWPLACEMENT has its length set, as
    // GetWindowPlacement requires.
    let normal = unsafe {
        let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
        placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        if GetWindowPlacement(hwnd, &mut placement) == 0 || GetWindowRect(hwnd, &mut window) == 0 {
            return false;
        }
        placement.rcNormalPosition
    };
    differs_in_size(window, normal)
}

/// Sizes only: the restored rect is in work-area coordinates, the window's in screen coordinates.
fn differs_in_size(a: RECT, b: RECT) -> bool {
    (a.right - a.left, a.bottom - a.top) != (b.right - b.left, b.bottom - b.top)
}

/// The client area's size in pixels (the window without the system frame on its left, right and
/// bottom).
fn client_size(hwnd: HWND) -> Option<(i32, i32)> {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: `r` is a valid RECT to fill.
    (unsafe { GetClientRect(hwnd, &mut r) } != 0).then_some((r.right - r.left, r.bottom - r.top))
}

/// Windows' resize border at the window's DPI: the sizing frame plus its padding, as Windows
/// itself sizes it, or 8 px scaled when the system has no answer.
fn border_px(hwnd: HWND) -> f32 {
    // SAFETY: plain queries.
    let (dpi, metric) = unsafe {
        let dpi = GetDpiForWindow(hwnd);
        (dpi, GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi))
    };
    let scale = if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 };
    if metric > 0 { metric as f32 } else { titlebar::resize_border_px(scale) }
}

/// The `HT*` resize code at screen point (`x`, `y`) on the band along the top edge, Windows'
/// resize border high. The frame has no top, so the band is client area; its ends are corners.
fn top_border_hit(hwnd: HWND, x: i32, y: i32) -> Option<u32> {
    if !resizable(hwnd) {
        return None;
    }
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: `r` is a valid RECT to fill.
    if unsafe { GetWindowRect(hwnd, &mut r) } == 0 {
        return None;
    }
    top_border_code(r, x, y, border_px(hwnd))
}

/// The resize code at screen point (`x`, `y`) of window rect `r` on the `edge`-high band along
/// its top, by the shared rule (`titlebar::resize_hit`: corners win); `None` below the band.
fn top_border_code(r: RECT, x: i32, y: i32, edge: f32) -> Option<u32> {
    if (y - r.top) as f32 >= edge {
        return None;
    }
    let w = egui::Rect::from_min_max(egui::pos2(r.left as f32, r.top as f32), egui::pos2(r.right as f32, r.bottom as f32));
    titlebar::resize_hit(w, egui::pos2(x as f32, y as f32), edge).map(hit_code)
}

fn hit_code(d: egui::ResizeDirection) -> u32 {
    use egui::ResizeDirection::*;
    match d {
        North => HTTOP,
        South => HTBOTTOM,
        West => HTLEFT,
        East => HTRIGHT,
        NorthWest => HTTOPLEFT,
        NorthEast => HTTOPRIGHT,
        SouthWest => HTBOTTOMLEFT,
        SouthEast => HTBOTTOMRIGHT,
    }
}

/// Passes Preferences › Layout's `windows_smooth_window_drag` to the vendored winit, at start and
/// whenever it changes; winit reads it at each message, so it applies at once.
#[derive(Default)]
pub struct WindowMove {
    /// The value winit was given last (`set_quiet_window_move`); `None` before the first.
    quiet: Option<bool>,
}

impl WindowMove {
    /// Give winit `smooth_window_drag` if it differs from what it was given last, or nothing was.
    pub fn follow(&mut self, smooth_window_drag: bool) {
        if let Some(quiet) = self.change(smooth_window_drag) {
            winit::platform::windows::set_quiet_window_move(quiet);
        }
    }

    /// The value to give winit for `smooth_window_drag` (remembered as given), `None` when winit
    /// has it already.
    fn change(&mut self, smooth_window_drag: bool) -> Option<bool> {
        let quiet = quiet_window_move(smooth_window_drag);
        (self.quiet != Some(quiet)).then(|| {
            self.quiet = Some(quiet);
            quiet
        })
    }
}

/// winit's switch for the preference: a quiet move (nothing drawn while the title bar is dragged,
/// the picture catches up when the drag ends) only when the preference is on. Off, winit keeps its
/// own behaviour: the app draws during the drag, as before.
fn quiet_window_move(smooth_window_drag: bool) -> bool {
    smooth_window_drag
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Off (the default) leaves winit as it is: the app draws during the drag, as before. On
    /// gives winit a quiet move. Each value is given once, at start and whenever it changes. The
    /// pure part only: the switch itself is one per process, which the tests running in it share.
    #[test]
    fn window_move_gives_winit_the_preference_once_per_change() {
        assert!(!quiet_window_move(false), "the default: drawing goes on during the drag, as before");
        assert!(quiet_window_move(true), "the preference on: nothing drawn while the window is dragged");
        let mut m = WindowMove::default();
        assert_eq!(m.change(false), Some(false), "at start, whatever the value");
        assert_eq!(m.change(false), None, "unchanged: winit has it");
        assert_eq!(m.change(true), Some(true), "turned on");
        assert_eq!(m.change(true), None);
        assert_eq!(m.change(false), Some(false), "and off again");
        let mut on = WindowMove::default();
        assert_eq!(on.change(true), Some(true), "a saved `true` is given at start too");
    }

    /// #18473: a maximized window over an auto-hide taskbar leaves 1 px at that edge only.
    #[test]
    fn maximized_window_leaves_one_pixel_at_an_auto_hide_taskbar() {
        let full = (0, 0, 1920, 1080);
        assert_eq!(shrink_for_auto_hide(full, AutoHideEdges::default()), full, "no auto-hide taskbar: untouched");
        let bottom = AutoHideEdges { bottom: true, ..Default::default() };
        assert_eq!(shrink_for_auto_hide(full, bottom), (0, 0, 1920, 1079));
        let left = AutoHideEdges { left: true, ..Default::default() };
        assert_eq!(shrink_for_auto_hide(full, left), (1, 0, 1920, 1080));
        let top = AutoHideEdges { top: true, ..Default::default() };
        assert_eq!(shrink_for_auto_hide((-1920, 0, 0, 1080), top), (-1920, 1, 0, 1080), "a monitor left of the primary");
        let right = AutoHideEdges { right: true, ..Default::default() };
        assert_eq!(shrink_for_auto_hide(full, right), (0, 0, 1919, 1080));
        let all = AutoHideEdges { left: true, top: true, right: true, bottom: true };
        assert_eq!(shrink_for_auto_hide(full, all), (1, 1, 1919, 1079));
        assert_eq!(shrink_for_auto_hide((0, 0, 1, 1), all), (0, 0, 1, 1), "too small to lose the pixel");
    }

    #[test]
    fn every_direction_has_its_own_hit_code() {
        use egui::ResizeDirection::*;
        let codes: Vec<u32> = [North, South, West, East, NorthWest, NorthEast, SouthWest, SouthEast].into_iter().map(hit_code).collect();
        assert_eq!(codes, vec![HTTOP, HTBOTTOM, HTLEFT, HTRIGHT, HTTOPLEFT, HTTOPRIGHT, HTBOTTOMLEFT, HTBOTTOMRIGHT]);
    }

    #[test]
    fn only_a_different_size_counts_as_snapped() {
        let rect = |left, top, w, h| RECT { left, top, right: left + w, bottom: top + h };
        // The restored rect is in work-area coordinates, so its position may differ when restored.
        assert!(!differs_in_size(rect(2187, 133, 1500, 1000), rect(267, 133, 1500, 1000)));
        assert!(differs_in_size(rect(1920, 0, 1280, 1380), rect(267, 133, 1500, 1000)));
        assert!(differs_in_size(rect(0, 0, 1500, 1380), rect(0, 0, 1500, 1000)));
    }

    /// A restored window at (100, 50), 1000 × 700 pixels, with Windows' 8 px resize border.
    const WINDOW: RECT = RECT { left: 100, top: 50, right: 1100, bottom: 750 };
    const EDGE: f32 = 8.0;

    #[test]
    fn only_the_band_along_the_top_is_answered_as_a_resize_border() {
        // Points along the window's edges, at its corners and inside it. The system frame answers
        // for the left, right and bottom, outside the client area.
        let probes = [(101, 400), (1098, 400), (600, 51), (600, 748), (101, 51), (1098, 52), (102, 747), (1099, 749), (600, 400), (600, 70)];
        let codes: Vec<Option<u32>> = probes.into_iter().map(|(x, y)| top_border_code(WINDOW, x, y, EDGE)).collect();
        assert_eq!(codes, vec![None, None, Some(HTTOP), None, Some(HTTOPLEFT), Some(HTTOPRIGHT), None, None, None, None]);
        // The band is as high as the border at the window's scale: 12 px at 150 %.
        assert_eq!(top_border_code(WINDOW, 600, 60, EDGE), None);
        assert_eq!(top_border_code(WINDOW, 600, 60, 12.0), Some(HTTOP));
        // The same answer as the shared rule gives there, point for point.
        let w = egui::Rect::from_min_max(egui::pos2(100.0, 50.0), egui::pos2(1100.0, 750.0));
        for x in [100, 107, 115, 116, 600, 1083, 1084, 1099] {
            assert_eq!(top_border_code(WINDOW, x, 53, EDGE), titlebar::resize_hit(w, egui::pos2(x as f32, 53.0), EDGE).map(hit_code), "x = {x}");
        }
    }

    #[test]
    fn the_bare_bar_is_the_caption_and_the_maximize_button_is_windows_own() {
        assert_eq!((caption_code(CaptionHit::Drag), caption_code(CaptionHit::MaximizeButton)), (Some(HTCAPTION), Some(HTMAXBUTTON)));
        // The bar's controls and everything off the bar keep the default answer.
        assert_eq!((caption_code(CaptionHit::Client), caption_code(CaptionHit::Outside)), (None, None));
    }

    #[test]
    fn a_dpi_scaled_size_has_the_system_frame_around_the_client_area() {
        assert_eq!(framed_size(Some((1500, 1000)), Some((8, 8, 8))).map(|s| (s.cx, s.cy)), Some((1516, 1008)), "no frame at the top");
        assert_eq!(framed_size(None, Some((8, 8, 8))).map(|s| (s.cx, s.cy)), None);
        assert_eq!(framed_size(Some((1500, 1000)), None).map(|s| (s.cx, s.cy)), None);
    }

    #[test]
    fn a_mouse_message_s_point_is_signed() {
        let lparam = |x: i16, y: i16| ((y as u16 as u32) << 16 | x as u16 as u32) as LPARAM;
        assert_eq!(screen_point(lparam(640, 20)), (640, 20));
        assert_eq!(screen_point(lparam(-1280, -40)), (-1280, -40), "a monitor left of and above the primary one");
    }
}
