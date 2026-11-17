//! The Layout page's "Design tabs" choices, read from one place by the tab strip and the views code.

use crate::design_views::ViewKind;
use crate::prefs::Prefs;

/// How a design's views are shown and opened, copied from the preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesignTabPrefs {
    /// Views are sub-tabs inside their design's tab (off: one tab with a view switcher).
    pub views_as_tabs: bool,
    /// With `views_as_tabs` off, the tab shows its view switcher even for a design with one view.
    pub show_switcher: bool,
    /// A drawing sheet opens as a view of its design.
    pub sub_tab_sheets: bool,
    /// Editing a sketch opens it as a view of its design.
    pub sub_tab_sketches: bool,
    /// Render and Animate open as views of their design.
    pub sub_tab_render: bool,
    /// Opening a sheet, sketch or workspace a view already shows activates that view.
    pub reuse_views: bool,
    /// A design opens with the views it had when it was last closed.
    pub reopen_views: bool,
}

impl DesignTabPrefs {
    /// Whether the app opens views by itself (a sketch being edited, the Render workspace) so the
    /// strip can show them: only while the strip shows views, as sub-tabs or a switcher. Off, a
    /// design keeps the views the user opened and its tab stays as it was before design views.
    pub fn opens_views_by_itself(&self) -> bool {
        self.views_as_tabs || self.show_switcher
    }

    /// Whether the selected design's tab shows a switcher even with its one view.
    pub fn switcher_always(&self) -> bool {
        self.show_switcher && !self.views_as_tabs
    }

    /// Whether a view of `kind` may be added as a sub-tab: Model always, the others when their
    /// toggle is on (off: the design enters that state in the view already shown).
    pub fn opens_sub_tab(&self, kind: ViewKind) -> bool {
        match kind {
            ViewKind::Model => true,
            ViewKind::Sketch => self.sub_tab_sketches,
            ViewKind::Sheet => self.sub_tab_sheets,
            ViewKind::Render | ViewKind::Animation => self.sub_tab_render,
        }
    }
}

impl From<&Prefs> for DesignTabPrefs {
    fn from(p: &Prefs) -> Self {
        Self {
            views_as_tabs: p.design_views_as_tabs,
            show_switcher: p.show_design_view_switcher,
            sub_tab_sheets: p.design_sub_tab_sheets,
            sub_tab_sketches: p.design_sub_tab_sketches,
            sub_tab_render: p.design_sub_tab_render,
            reuse_views: p.design_reuse_views,
            reopen_views: p.design_reopen_views,
        }
    }
}
