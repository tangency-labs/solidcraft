//! Windows: the frame and title bar of the window without OS decorations (#1381).
//!
//! The window keeps its thick frame (`WS_THICKFRAME`), so Aero Snap, Win+arrows and the OS's own
//! move and size loops work. Restored, winit's `WM_NCCALCSIZE` (vendored) keeps the system frame
//! on the left, right and bottom and makes the top client area, where the UI draws its title bar.
//! This module answers `WM_NCHITTEST` there from the first message on, before any frame: the band
//! along the top edge resizes, the bare title bar is `HTCAPTION` and the maximize button
//! `HTMAXBUTTON` (Windows 11's snap layouts). egui never sees the presses Windows takes there or
//! on the frame, so they are reported to the UI, whose open menus close as for a press outside.
//!
//! It also answers `WM_GETDPISCALEDSIZE` with the size the window will really have at the new DPI,
//! so a window dragged onto a monitor with another scale rescales once and stays under the pointer.
//!
//! Replacing a window procedure and setting a window class's icon have no safe API, hence the
//! `unsafe_code` allowance on this module (the app crate denies it; see `native_crash.rs` and
//! `open_documents.rs` for the others).
//! The procedure is installed once, on the thread that owns the window, for the main window only,
//! and calls the previous one (winit's) for everything it doesn't answer.
//!
//! [`set_class_icon`] gives the window's class the exe's icon, so the taskbar button shows the
//! mark even when the window is too busy to answer the taskbar's request for its icon.
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use solidcraft_frame::CaptionHit;
use solidcraft_ui_egui::titlebar;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, ScreenToClient};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GCLP_HICON, GCLP_HICONSM, GWL_EXSTYLE, GWL_STYLE, GWLP_WNDPROC, GetClientRect, GetWindowLongPtrW, GetWindowRect, HTBOTTOM,
    HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTMAXBUTTON, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IMAGE_ICON, IsIconic, IsZoomed,
    LR_DEFAULTCOLOR, LoadImageW, PostMessageW, SC_MOVE, SC_SIZE, SM_CXICON, SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SM_CXSMICON, SW_MAXIMIZE, SW_RESTORE,
    SetWindowLongPtrW, ShowWindow, TITLEBARINFOEX, WM_GETDPISCALEDSIZE, WM_GETTITLEBARINFOEX, WM_MOUSEMOVE, WM_NCDESTROY, WM_NCHITTEST,
    WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMBUTTONDBLCLK, WM_NCMBUTTONDOWN, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_NCRBUTTONDBLCLK,
    WM_NCRBUTTONDOWN, WM_NCRBUTTONUP, WM_NCXBUTTONDBLCLK, WM_NCXBUTTONDOWN, WM_SYSCOMMAND, WNDPROC, WS_THICKFRAME,
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

/// The window procedure ours replaced (winit's), as an address; 0 before [`install`].
static PREVIOUS: AtomicIsize = AtomicIsize::new(0);

/// Answer the main window's hit test and DPI-size questions from now on. Call it once, on the
/// thread that created the window (eframe's app creator). `false` if it isn't a Win32 window.
pub fn install(window: &impl HasWindowHandle) -> bool {
    let Ok(handle) = window.window_handle() else { return false };
    let RawWindowHandle::Win32(w) = handle.as_raw() else { return false };
    let hwnd = w.hwnd.get() as HWND;
    if PREVIOUS.load(Ordering::SeqCst) != 0 {
        return false;
    }
    // SAFETY: `hwnd` is the live window eframe just created on this thread; `frame_proc` has the
    // WNDPROC signature and stays valid for the life of the process. SetWindowLongPtrW returns
    // the previous procedure, which `frame_proc` calls for every other message.
    let previous = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, frame_proc as *const () as isize as _) };
    // `as _`: on 32-bit Windows SetWindowLongPtrW is SetWindowLongW, which takes and
    // returns i32.
    if previous == 0 {
        return false;
    }
    PREVIOUS.store(previous as _, Ordering::SeqCst);
    WINDOW.store(hwnd as isize, Ordering::SeqCst);
    titlebar::set_system_commands(system_command);
    true
}

/// The procedure ours replaced.
fn previous() -> WNDPROC {
    let p = PREVIOUS.load(Ordering::SeqCst);
    // SAFETY: a nonzero value is the WNDPROC that SetWindowLongPtrW returned in `install`.
    (p != 0).then(|| unsafe { std::mem::transmute::<isize, unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>(p) })
}

unsafe extern "system" fn frame_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let prev = previous();
    // SAFETY: the arguments are the message as Windows delivered it to this window.
    let call_previous = || unsafe { CallWindowProcW(prev, hwnd, msg, wparam, lparam) };
    match msg {
        WM_NCHITTEST => {
            let default = call_previous();
            // The system frame (left, right, bottom) answers for itself.
            if default != HTCLIENT as LRESULT {
                return default;
            }
            // Screen coordinates, signed (monitors left of or above the primary one).
            let (x, y) = (i32::from(lparam as u16 as i16), i32::from((lparam >> 16) as u16 as i16));
            // The client area: the top resize band, then the title bar the UI published.
            if let Some(hit) = top_border_hit(hwnd, x, y) {
                return hit as LRESULT;
            }
            match caption_hit(hwnd, x, y) {
                CaptionHit::Drag => HTCAPTION as LRESULT,
                CaptionHit::MaximizeButton => HTMAXBUTTON as LRESULT,
                _ => default,
            }
        }
        // The maximize button is HTMAXBUTTON, so its pointer arrives as non-client messages; the
        // UI draws the button from the state reported here. Presses stop here: the default
        // handling would run its own loop and draw a classic button.
        WM_NCMOUSEMOVE if wparam == HTMAXBUTTON as WPARAM => {
            // The default handling arms the hover that opens the snap layouts.
            let result = call_previous();
            titlebar::set_native_maximize(true, MAX_DOWN.load(Ordering::Relaxed));
            result
        }
        // Off the button: no hover, and a press dragged off it is no longer a click.
        WM_NCMOUSEMOVE | WM_MOUSEMOVE => {
            MAX_DOWN.store(false, Ordering::Relaxed);
            titlebar::set_native_maximize(false, false);
            call_previous()
        }
        WM_NCMOUSELEAVE => {
            MAX_DOWN.store(false, Ordering::Relaxed);
            titlebar::set_native_maximize(false, false);
            call_previous()
        }
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if wparam == HTMAXBUTTON as WPARAM => {
            titlebar::window_frame_pressed();
            MAX_DOWN.store(true, Ordering::Relaxed);
            titlebar::set_native_maximize(true, true);
            0
        }
        // egui sees no press on the caption or the frame: the UI is told, so its open menus close
        // (as a system menu does), and the press does what it does.
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK | WM_NCMBUTTONDOWN | WM_NCMBUTTONDBLCLK | WM_NCXBUTTONDOWN | WM_NCXBUTTONDBLCLK => {
            titlebar::window_frame_pressed();
            call_previous()
        }
        WM_NCLBUTTONUP if wparam == HTMAXBUTTON as WPARAM => {
            if MAX_DOWN.swap(false, Ordering::Relaxed) {
                // SAFETY: plain calls on a window handle the system passed in.
                unsafe { ShowWindow(hwnd, if IsZoomed(hwnd) != 0 { SW_RESTORE } else { SW_MAXIMIZE }) };
            }
            titlebar::set_native_maximize(true, false);
            0
        }
        // Windows opens a window menu on a right-click only over a caption it draws itself. This
        // caption is the UI's, so the UI's window menu opens, on release, at the pointer.
        WM_NCRBUTTONDOWN | WM_NCRBUTTONDBLCLK if wparam == HTCAPTION as WPARAM => {
            titlebar::window_frame_pressed();
            0
        }
        WM_NCRBUTTONDOWN | WM_NCRBUTTONDBLCLK => {
            titlebar::window_frame_pressed();
            call_previous()
        }
        WM_NCRBUTTONUP if wparam == HTCAPTION as WPARAM => {
            let mut p = POINT { x: i32::from(lparam as u16 as i16), y: i32::from((lparam >> 16) as u16 as i16) };
            // SAFETY: `p` is a valid POINT to convert; the handle is the system's.
            if unsafe { ScreenToClient(hwnd, &mut p) } != 0 {
                titlebar::open_window_menu(p.x, p.y);
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
            match (resizable(hwnd), client_size(hwnd), unsafe { (lparam as *mut SIZE).as_mut() }) {
                (true, Some(size), Some(out)) => {
                    // SAFETY: a window handle the system passed in.
                    let old_dpi = unsafe { GetDpiForWindow(hwnd) };
                    match (titlebar::dpi_scaled_size(size, old_dpi, new_dpi), side_frame(hwnd, new_dpi)) {
                        (Some((cx, cy)), Some((left, right, bottom))) => {
                            *out = SIZE { cx: cx + left + right, cy: cy + bottom };
                            1
                        }
                        _ => call_previous(),
                    }
                }
                _ => call_previous(),
            }
        }
        WM_NCDESTROY => {
            WINDOW.store(0, Ordering::SeqCst);
            // Hand the window back to winit's procedure for its last message.
            if prev.is_some() {
                // SAFETY: restores the procedure this window had before `install`.
                unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, PREVIOUS.load(Ordering::SeqCst) as _) };
            }
            call_previous()
        }
        _ => call_previous(),
    }
}

/// The maximize button was pressed and not yet released (non-client press).
static MAX_DOWN: AtomicBool = AtomicBool::new(false);

/// Write the published title bar and its buttons into `info`, in screen coordinates: minimize,
/// maximize and close (indexes 2, 3 and 5) sit side by side, each as wide as the maximize button.
fn report_title_bar(hwnd: HWND, info: &mut TITLEBARINFOEX) {
    let Some((bar, Some(max))) = titlebar::caption_map().bar_and_maximize() else { return };
    let screen = |r: solidcraft_frame::PxRect| -> RECT {
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
    let beside = |dx: i32| solidcraft_frame::PxRect::new(max.min_x + dx, max.min_y, max.max_x + dx, max.max_y);
    info.rcTitleBar = screen(bar);
    info.rgrect[2] = screen(beside(-width));
    info.rgrect[3] = screen(max);
    info.rgrect[5] = screen(beside(width));
}

/// The main window, for the window menu's Move and Size; 0 before [`install`] and once it is gone.
static WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Start Windows' keyboard move or size of the main window (the window menu's Move and Size).
fn system_command(command: titlebar::SystemCommand) {
    let hwnd = WINDOW.load(Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    let sc = match command {
        titlebar::SystemCommand::Move => SC_MOVE,
        titlebar::SystemCommand::Size => SC_SIZE,
    };
    // SAFETY: a message posted to the main window, which `WINDOW` holds only while it exists.
    unsafe { PostMessageW(hwnd as HWND, WM_SYSCOMMAND, sc as WPARAM, 0) };
}

/// What the published title bar has at screen point (`x`, `y`) (the UI's layout, in client
/// pixels).
fn caption_hit(hwnd: HWND, x: i32, y: i32) -> CaptionHit {
    let mut p = POINT { x, y };
    // SAFETY: `p` is a valid POINT to convert; the handle is the system's.
    if unsafe { ScreenToClient(hwnd, &mut p) } == 0 {
        return CaptionHit::Outside;
    }
    titlebar::caption_map().hit(p.x, p.y)
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

/// The window can be resized by its border now: it has the thick frame (not fullscreen, which
/// drops it) and is neither maximized nor minimized.
fn resizable(hwnd: HWND) -> bool {
    // SAFETY: plain queries on a window handle the system passed in.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        style & WS_THICKFRAME != 0 && IsZoomed(hwnd) == 0 && IsIconic(hwnd) == 0
    }
}

/// The client area's size in pixels.
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
/// resize border wide. The frame has no top, so the band is client area; its ends are corners.
fn top_border_hit(hwnd: HWND, x: i32, y: i32) -> Option<u32> {
    if !resizable(hwnd) {
        return None;
    }
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: `r` is a valid RECT to fill.
    if unsafe { GetWindowRect(hwnd, &mut r) } == 0 {
        return None;
    }
    let edge = border_px(hwnd);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_direction_has_its_own_hit_code() {
        use egui::ResizeDirection::*;
        let codes: Vec<u32> = [North, South, West, East, NorthWest, NorthEast, SouthWest, SouthEast].into_iter().map(hit_code).collect();
        assert_eq!(codes, vec![HTTOP, HTBOTTOM, HTLEFT, HTRIGHT, HTTOPLEFT, HTTOPRIGHT, HTBOTTOMLEFT, HTBOTTOMRIGHT]);
    }
}
