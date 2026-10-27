//! Keep AppKit's displayed cursor in sync with internal resize hit targets.
//! The regular winit cursor cache skips identical icon requests. AppKit can reset
//! the displayed cursor while moving within the same view, so internal controls
//! explicitly refresh it. No main-window edge hit tests or geometry change here.
use egui::{Context, CursorIcon};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSCursor, NSCursorFrameResizeDirections, NSCursorFrameResizePosition, NSEvent, NSScreen};

#[allow(deprecated)] // Older macOS versions need the pre-15 horizontal/vertical cursors.
pub fn refresh(ctx: &Context, icon: CursorIcon) {
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    // Hover should work while another app has keyboard focus too. Only apply
    // when the physical pointer matches this viewport's reported pointer: test
    // and control-channel pointer injection must not change another app's cursor.
    let Some((inner, pointer)) = ctx.input(|i| i.viewport().inner_rect.zip(i.pointer.hover_pos())) else {
        return;
    };
    let Some(screen) = NSScreen::screens(main).firstObject() else {
        return;
    };
    let mouse = NSEvent::mouseLocation();
    let zoom = ctx.zoom_factor();
    let physical = egui::pos2(mouse.x as f32 / zoom, (screen.frame().size.height - mouse.y) as f32 / zoom);
    if (physical - (inner.min + pointer.to_vec2())).length() > 2.0 {
        return;
    }
    let cursor = if objc2::available!(macos = 15.0) {
        let position = match icon {
            CursorIcon::ResizeHorizontal => NSCursorFrameResizePosition::Left,
            CursorIcon::ResizeVertical => NSCursorFrameResizePosition::Bottom,
            CursorIcon::ResizeNwSe => NSCursorFrameResizePosition::TopLeft,
            CursorIcon::ResizeNeSw => NSCursorFrameResizePosition::TopRight,
            _ => return,
        };
        NSCursor::frameResizeCursorFromPosition_inDirections(position, NSCursorFrameResizeDirections::All)
    } else {
        match icon {
            CursorIcon::ResizeHorizontal => NSCursor::resizeLeftRightCursor(),
            CursorIcon::ResizeVertical => NSCursor::resizeUpDownCursor(),
            _ => return,
        }
    };
    cursor.set();
    // A reset can arrive after the frame was handled. Refresh while the pointer
    // remains on this internal control, without changing any native resize region.
    ctx.request_repaint_after(std::time::Duration::from_millis(50));
}
