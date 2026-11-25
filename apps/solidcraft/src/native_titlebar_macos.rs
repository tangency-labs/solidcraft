//! macOS: the window's content view never starts a native window drag.
//!
//! The window has a transparent title bar over a full-size content view
//! (`app_viewport`), so the application bar sits where AppKit's title bar is. AppKit moves the
//! window when a press in that strip lands on a view whose `mouseDownCanMoveWindow` answers
//! `YES`, and an `NSView` answers `YES` unless it is opaque. winit's view is not overridden, so a
//! drag that started on a tab or an application bar button moved the whole window before egui
//! could use it. Answering `NO` leaves the moves to egui: empty bar space starts one with
//! `ViewportCommand::StartDrag` (`solidcraft_ui_egui::titlebar::drag_area`), tabs and buttons
//! keep their drags.
//!
//! Adding a method to another crate's Objective-C class has no safe API, hence the `unsafe_code`
//! allowance on this module (see `open_documents.rs`).
#![allow(unsafe_code)]

use objc2::ffi::class_replaceMethod;
use objc2::runtime::{AnyObject, Bool, Imp, Sel};
use objc2::sel;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// `-[NSView mouseDownCanMoveWindow]` of the window's view: always `NO`.
extern "C-unwind" fn never_moves_window(_view: &AnyObject, _cmd: Sel) -> Bool {
    Bool::NO
}

/// Stop the content view of `window` from starting native window drags. Returns whether the
/// method was installed.
#[allow(unsafe_code)]
pub fn install(window: &impl HasWindowHandle) -> bool {
    let Ok(handle) = window.window_handle() else { return false };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return false };
    // SAFETY: an AppKit window handle's `ns_view` is the live main window's `NSView` (winit's
    // view), valid for the duration of this call on the main thread.
    let view: &AnyObject = unsafe { h.ns_view.cast::<AnyObject>().as_ref() };
    let class = view.class();
    let handler: extern "C-unwind" fn(&AnyObject, Sel) -> Bool = never_moves_window;
    // SAFETY: `Imp` is the untyped form of a method implementation; the runtime calls it with the
    // arguments the type encoding declares, which match `never_moves_window`: a `BOOL` result, the
    // receiver (`@`) and the selector (`:`). Replacing adds the method to this view's own class
    // (winit's), overriding `NSView`'s without touching other views.
    unsafe {
        let imp = std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel) -> Bool, Imp>(handler);
        let encoding = [<Bool as objc2::Encode>::ENCODING.to_string().as_str(), "@:"].concat();
        let Ok(types) = std::ffi::CString::new(encoding) else { return false };
        class_replaceMethod(std::ptr::from_ref(class).cast_mut(), sel!(mouseDownCanMoveWindow), imp, types.as_ptr());
    }
    true
}
