//! Windows: the resize border of the window without OS decorations (#1381).
//!
//! The window keeps its native thick frame (`WS_THICKFRAME`, so Aero Snap, Win+arrows and the
//! OS's own move and size loops work) but winit draws no frame: its `WM_NCCALCSIZE` makes the
//! whole window client area, and every point then hit-tests as `HTCLIENT`. This module puts the
//! resize border back by answering `WM_NCHITTEST` itself: within Windows' resize-border width
//! for the window's current DPI from each edge (`solidcraft_ui_egui::titlebar::resize_hit`;
//! 8 px at 100 %, corners win), it returns `HTLEFT`, `HTTOPRIGHT`, … So from the first message
//! the OS shows its resize cursors and runs its own resize, whatever egui has laid out, whatever
//! the scale; nothing waits for a first frame or a first size change.
//!
//! It also answers `WM_GETDPISCALEDSIZE` with the size the window will really have at the new
//! DPI (the same logical size, as winit computes it), so the rect Windows suggests while the
//! title bar is dragged onto a monitor with another scale is the rect winit then applies: the
//! window rescales once at the crossing and stays under the pointer, instead of the OS's move
//! loop and winit disagreeing about a frame the window doesn't have.
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

use std::sync::atomic::{AtomicIsize, Ordering};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use solidcraft_ui_egui::titlebar;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GCLP_HICON, GCLP_HICONSM, GWL_STYLE, GWLP_WNDPROC, GetClientRect, GetWindowLongPtrW, GetWindowRect, HTBOTTOM, HTBOTTOMLEFT,
    HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IMAGE_ICON, IsIconic, IsZoomed, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON,
    SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SM_CXSMICON, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW,
    SetWindowPos, WM_GETDPISCALEDSIZE, WM_NCCALCSIZE, WM_NCDESTROY, WM_NCHITTEST, WNDPROC, WS_THICKFRAME,
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
    // A 1 px DWM frame keeps the shadow now that there is no non-client area.
    let margins = MARGINS { cxLeftWidth: 1, cxRightWidth: 1, cyTopHeight: 1, cyBottomHeight: 1 };
    let corners = DWMWCP_ROUND;
    // SAFETY: `hwnd` is the live window; the pointers are to locals that outlive the calls.
    unsafe {
        DwmExtendFrameIntoClientArea(hwnd, &margins);
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as u32, (&raw const corners).cast(), std::mem::size_of_val(&corners) as u32);
        SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
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
            // Screen coordinates, signed (monitors left of or above the primary one).
            let (x, y) = (i32::from(lparam as u16 as i16), i32::from((lparam >> 16) as u16 as i16));
            // The border, else what winit and the system say (`HTCLIENT` everywhere else).
            border_hit(hwnd, x, y).map_or(default, |hit| hit as LRESULT)
        }
        // Client area = window. winit's 1 px shift makes Windows squash each frame, blurring text (#3927).
        WM_NCCALCSIZE if wparam != 0 && !zoomed(hwnd) => 0,
        WM_GETDPISCALEDSIZE => {
            let new_dpi = wparam as u32;
            // SAFETY: for this message lParam points to a SIZE the system reads back.
            match (resizable(hwnd), client_size(hwnd), unsafe { (lparam as *mut SIZE).as_mut() }) {
                (true, Some(size), Some(out)) => {
                    // SAFETY: a window handle the system passed in.
                    let old_dpi = unsafe { GetDpiForWindow(hwnd) };
                    match titlebar::dpi_scaled_size(size, old_dpi, new_dpi) {
                        Some((cx, cy)) => {
                            *out = SIZE { cx, cy };
                            1
                        }
                        None => call_previous(),
                    }
                }
                _ => call_previous(),
            }
        }
        WM_NCDESTROY => {
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

/// The window can be resized by its border now: it has the thick frame (not fullscreen, which
/// drops it) and is neither maximized nor minimized.
fn resizable(hwnd: HWND) -> bool {
    // SAFETY: plain queries on a window handle the system passed in.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        style & WS_THICKFRAME != 0 && !zoomed(hwnd) && IsIconic(hwnd) == 0
    }
}

fn zoomed(hwnd: HWND) -> bool {
    // SAFETY: a plain query on a window handle the system passed in.
    unsafe { IsZoomed(hwnd) != 0 }
}

/// The client area's size in pixels (the whole window: winit gives it no frame).
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

/// The `HT*` resize code at screen point (`x`, `y`), if it is on the border.
fn border_hit(hwnd: HWND, x: i32, y: i32) -> Option<u32> {
    if !resizable(hwnd) {
        return None;
    }
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: `r` is a valid RECT to fill.
    if unsafe { GetWindowRect(hwnd, &mut r) } == 0 {
        return None;
    }
    let w = egui::Rect::from_min_max(egui::pos2(r.left as f32, r.top as f32), egui::pos2(r.right as f32, r.bottom as f32));
    titlebar::resize_hit(w, egui::pos2(x as f32, y as f32), border_px(hwnd)).map(hit_code)
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
