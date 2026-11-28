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
    WindowAction as NativeWindowAction, paint as paint_native_window, paint_drop as paint_native_drop, window_chrome as native_window_chrome,
};
pub use runtime::*;
pub use surfaces::*;
pub use workspace::*;
