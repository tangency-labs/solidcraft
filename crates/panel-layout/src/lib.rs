//! Panel placement core, app-neutral: panels registered by id with their title, preferred zone
//! and sizes; the dock, floating and window placement of each open panel; stacking on a side;
//! allocation into the rects a host draws into; and the saved layout as a value the host stores.
//! No Session, no egui and no application command: a host (SolidCraft's engine among them)
//! commits changes through `LayoutService`, reads rects through `resolve_geometry`, and keeps
//! the encoded envelope where it keeps its preferences.
mod geometry;
pub mod interaction;
pub mod layout;
mod model;
mod persistence;
mod reducer;
mod scope;

pub use geometry::*;
pub use interaction::*;
pub use layout::*;
pub use model::*;
pub use persistence::*;
pub use reducer::*;
pub use scope::*;
