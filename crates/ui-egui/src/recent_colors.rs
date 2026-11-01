//! The colours picked last in any colour field, newest first (#9234). The app keeps the list in
//! its saved settings (`UiState::recent_colors`) and hands it to the colour pickers every frame
//! through egui memory; the shared `ui_kit` colour popover shows it and adds finished picks.

/// How many colours the recent list keeps.
pub(crate) const MAX_RECENT_COLORS: usize = 12;

/// The colours picked last, newest first, shared by every colour picker of the window.
#[derive(Clone, Default)]
struct RecentColors {
    colors: Vec<[u8; 3]>,
    /// Picked since the app last took the list for its settings.
    changed: bool,
}

fn recent_id() -> egui::Id {
    egui::Id::new("color_recent_colors")
}

/// The recently picked colours, newest first.
pub(crate) fn recent_colors(ctx: &egui::Context) -> Vec<[u8; 3]> {
    ctx.data(|d| d.get_temp::<RecentColors>(recent_id())).map(|r| r.colors).unwrap_or_default()
}

/// Put a finished pick at the front of the recent list (once, capped).
/// `replacing` is the colour the same popover added earlier while it stayed open: it goes if it
/// is still the newest entry.
pub(crate) fn remember_color(ctx: &egui::Context, rgb: [u8; 3], replacing: Option<[u8; 3]>) {
    ctx.data_mut(|d| {
        let recent = d.get_temp_mut_or_default::<RecentColors>(recent_id());
        if replacing.is_some() && recent.colors.first().copied() == replacing {
            recent.colors.remove(0);
        }
        recent.colors.retain(|c| *c != rgb);
        recent.colors.insert(0, rgb);
        recent.colors.truncate(MAX_RECENT_COLORS);
        recent.changed = true;
    });
}

/// Keep the saved list and the pickers' list the same; the app calls this every frame. Picks made
/// since the last call are written to `saved`; otherwise `saved` (loaded, reset) is what the
/// pickers show.
pub(crate) fn share(ctx: &egui::Context, saved: &mut Vec<[u8; 3]>) {
    saved.truncate(MAX_RECENT_COLORS);
    ctx.data_mut(|d| {
        let recent = d.get_temp_mut_or_default::<RecentColors>(recent_id());
        if recent.changed {
            saved.clone_from(&recent.colors);
            recent.changed = false;
        } else if recent.colors != *saved {
            recent.colors.clone_from(saved);
        }
    });
}
