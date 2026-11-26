//! App-neutral panel placement runtime: layout fitting, drag-to-dock/undock/stack/tab
//! interaction, floating bounds, desktop pop-out windows (`Floating` fallback on `wasm32`;
//! `Rule::Capability` refusal when desktop `native_windows` is `false`), and persistence via
//! `PanelLayoutStore`.

mod drag_paint;
mod host;
mod interaction;
mod layout;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod native_ui;
mod runtime;
mod surfaces;
mod workspace;

#[cfg(not(target_arch = "wasm32"))]
pub use drag_paint::WindowCard;
pub use host::*;
pub use interaction::*;
pub use layout::*;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{DesktopNativeAdapter, NativeMonitor};
#[cfg(not(target_arch = "wasm32"))]
pub use native_ui::{
    ActionPlace as NativeActionPlace, CardWindow as NativeCardWindow, WindowAction as NativeWindowAction, WindowControl as NativeWindowControl,
    action_place as native_action_place, bar_holds_actions as native_bar_holds_actions, bar_title_extent as native_bar_title_extent,
    bar_width_for_actions as native_bar_width_for_actions, card_action_row as native_card_action_row, drop_chrome as native_drop_chrome,
    paint as paint_native_window, paint_drop as paint_native_drop, width_for_actions as native_width_for_actions,
    window_actions as native_window_actions, window_chrome as native_window_chrome,
};
pub use runtime::*;
pub use surfaces::*;
pub use workspace::*;
