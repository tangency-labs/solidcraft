//! A drawing's views (W17a, #4816): each a view of one of the drawing's designs, on one of its
//! sheets. A **base view** draws a source from a direction ([`ViewDirection`]). A **projected
//! view** sits on one side of its parent and looks from that side, or from the side away from it
//! in first angle ([`Drawing::projection`], [`projected_basis`]); lined up with its parent, it saves
//! only its place along the free axis, as the other comes from the parent ([`Drawing::view_centre`]).
//!
//! What a view sets itself is in [`ViewOverrides`]: each setting it leaves out follows its parent,
//! then the default, and [`Drawing::resolve_view`] is that one rule for every reader. A projected
//! view's source and basis come from its chain of parents, so neither is saved.
//!
//! The rules, beside the record's: a view is on one of the drawing's sheets; a base view draws one
//! of its sources, has a scale and looks from a direction that makes a view; every parent is a view
//! of the drawing, no chain of parents loops, and a chain is at most [`MAX_VIEW_DEPTH`] views; a
//! view lined up with its parent is on the parent's sheet; a view's place is on paper
//! ([`is_paper_coordinate`]); at most [`MAX_VIEWS`] views, listed by id ([`Drawing::add_views`]).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use solidcraft_geom::drawing_view::{ProjectedSide, Scale, is_paper_coordinate, projected_basis};
use solidcraft_geom::paper::MAX_SIDE_MM;
use solidcraft_geom::{Plane, Vec2};
use solidcraft_render::VisualStyle;

use super::{Drawing, add_by_id, in_id_order, require};
use crate::drawing_view::{MAX_REFERENCE_SHOWN_PERCENT, TangentEdges, ViewDirection, ViewOverrides, ViewSettings};
use crate::{DocError, Result, ids};

/// The most views a drawing may have.
pub const MAX_VIEWS: usize = 4096;
/// The most views in a chain, from a base view down through the views projected from it.
pub const MAX_VIEW_DEPTH: usize = 32;

/// One view of a drawing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    #[serde(with = "crate::ids")]
    pub id: u64,
    /// The sheet it is on.
    #[serde(with = "crate::ids")]
    pub sheet: u64,
    pub kind: ViewKind,
    /// Where its centre sits on its sheet.
    pub place: ViewPlace,
    /// What it sets itself; each setting left out follows its parent, then the default.
    #[serde(default, skip_serializing_if = "ViewOverrides::is_empty")]
    pub settings: ViewOverrides,
    /// How far its boundary grows from the model's box towards the box that also holds its reference
    /// parts, each side on its own: 0 to [`MAX_REFERENCE_SHOWN_PERCENT`], each view its own.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub reference_shown_percent: f64,
}

/// What a view draws, and from where.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewKind {
    /// The design of `source`, seen from `view`.
    Base {
        #[serde(with = "crate::ids")]
        source: u64,
        view: ViewDirection,
    },
    /// Projected from `parent`, on `side` of it.
    Projected {
        #[serde(with = "crate::ids")]
        parent: u64,
        side: ProjectedSide,
    },
}

/// Where a view's centre sits on its sheet, in paper millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewPlace {
    /// At this point of the sheet.
    At([f64; 2]),
    /// Lined up with its parent, this far along the free axis: across for a view to the left or
    /// right of it, up for one above or below. A projected view only.
    Aligned(f64),
}

/// Who gave a resolved setting its value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetBy {
    /// The view itself.
    This,
    /// The parent of this id, the nearest that sets it.
    Parent(u64),
    /// No view of the chain: the default.
    Default,
}

/// A setting's value and who gave it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setting<T> {
    pub value: T,
    pub set_by: SetBy,
}

/// A view as it draws: the source it draws, which way it looks, and each setting with who gave it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedView {
    pub source: u64,
    pub basis: Plane,
    pub scale: Setting<Scale>,
    pub display_style: Setting<VisualStyle>,
    pub reference_style: Setting<VisualStyle>,
    pub reference_hides_model: Setting<bool>,
    pub tangent_edges: Setting<TangentEdges>,
    pub reference_shown_percent: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// The views from `id` up to its base view, nearest first, each found by `find`: refused when a
/// parent isn't one, the chain loops, or it is longer than [`MAX_VIEW_DEPTH`].
fn chain<'a>(id: u64, find: impl Fn(u64) -> Option<&'a View>) -> Result<Vec<&'a View>> {
    let mut chain: Vec<&View> = Vec::new();
    let mut at = id;
    loop {
        let view = find(at).ok_or_else(|| match chain.as_slice() {
            [] => DocError::Invalid(format!("the drawing has no view {}", ids::encode(at))),
            [_] => DocError::Invalid(format!("its parent {} isn't one of the drawing's views", ids::encode(at))),
            // Further up, the view whose parent is missing is named: it is that view's parent.
            [.., child] => DocError::Invalid(format!(
                "its chain of parents reaches view {}, whose parent {} isn't one of the drawing's views",
                ids::encode(child.id),
                ids::encode(at)
            )),
        })?;
        require(chain.iter().all(|v| v.id != at), || "its chain of parents loops".into())?;
        chain.push(view);
        match view.kind {
            ViewKind::Base { .. } => return Ok(chain),
            ViewKind::Projected { parent, .. } => at = parent,
        }
        require(chain.len() < MAX_VIEW_DEPTH, || format!("a chain of views is at most {MAX_VIEW_DEPTH} views"))?;
    }
}

/// The nearest view of `chain` (the view first) that sets a setting `field` reads, with its value.
fn inherited<T: Copy>(chain: &[&View], field: impl Fn(&ViewOverrides) -> Option<T>) -> Option<Setting<T>> {
    chain.iter().enumerate().find_map(|(i, view)| {
        let set_by = if i == 0 { SetBy::This } else { SetBy::Parent(view.id) };
        field(&view.settings).map(|value| Setting { value, set_by })
    })
}

/// [`inherited`], or the default `value` when no view of the chain sets it.
fn or_default<T: Copy>(chain: &[&View], field: impl Fn(&ViewOverrides) -> Option<T>, value: T) -> Setting<T> {
    inherited(chain, field).unwrap_or(Setting { value, set_by: SetBy::Default })
}

impl Drawing {
    /// Adds `views`, each where its id puts it: a drawing lists its views by id, never in the order
    /// they were added in, so the same views always save as the same bytes. Every edit keeps the
    /// order ([`Drawing::validate`]), and reading puts a file's views in it.
    pub fn add_views(&mut self, views: impl IntoIterator<Item = Arc<View>>) {
        add_by_id(&mut self.views, views, |v| v.id);
    }

    /// View `id`, found by halves in the views' id order.
    pub fn view(&self, id: u64) -> Option<&View> {
        self.view_index(id).and_then(|i| self.views.get(i)).map(|v| &**v)
    }

    /// Where view `id` is in [`Drawing::views`].
    pub fn view_index(&self, id: u64) -> Option<usize> {
        self.views.binary_search_by_key(&id, |v| v.id).ok()
    }

    /// The views' rules (the module's notes).
    pub(super) fn check_views(&self) -> Result<()> {
        require(self.views.len() <= MAX_VIEWS, || format!("a drawing has at most {MAX_VIEWS} views"))?;
        // In id order, so each view after this is found by halves.
        require(in_id_order(&self.views, |v| v.id), || "a drawing lists its views by id".into())?;
        for view in &self.views {
            self.check_view(view).map_err(|e| DocError::Invalid(format!("view {}: {e}", ids::encode(view.id))))?;
        }
        Ok(())
    }

    fn check_view(&self, view: &View) -> Result<()> {
        let on = |sheet: u64| format!("it is on sheet {}, which the drawing doesn't have", ids::encode(sheet));
        require(self.sheets.iter().any(|s| s.id == view.sheet), || on(view.sheet))?;
        let percent = view.reference_shown_percent;
        require(percent.is_finite() && (0.0..=MAX_REFERENCE_SHOWN_PERCENT).contains(&percent), || {
            format!("its reference_shown_percent is 0 to {MAX_REFERENCE_SHOWN_PERCENT}")
        })?;
        let place = match view.place {
            ViewPlace::At([x, y]) => is_paper_coordinate(x) && is_paper_coordinate(y),
            ViewPlace::Aligned(along) => is_paper_coordinate(along),
        };
        require(place, || format!("its place is at most {MAX_SIDE_MM} mm from the sheet's origin"))?;
        match view.kind {
            ViewKind::Base { source, view: direction } => {
                let drawn = || format!("it draws source {}, which the drawing doesn't have", ids::encode(source));
                require(self.sources.iter().any(|s| s.id == source), drawn)?;
                require(view.settings.scale.is_some(), || "a base view has a scale".into())?;
                require(matches!(view.place, ViewPlace::At(_)), || "a base view is placed at a point: it has no parent to line up with".into())?;
                direction.basis().map(|_| ()).map_err(|e| DocError::Invalid(e.to_string()))
            }
            ViewKind::Projected { parent, .. } => {
                self.chain_of(view.id)?;
                let parent_sheet = self.view(parent).map(|p| p.sheet);
                let lined_up = matches!(view.place, ViewPlace::Aligned(_));
                require(!lined_up || parent_sheet == Some(view.sheet), || "a view lined up with its parent is on its parent's sheet".into())
            }
        }
    }

    /// View `id` as it draws: its base view's source, its basis turned down its chain of parents
    /// ([`projected_basis`], in the drawing's projection), and each setting from the nearest view
    /// of the chain that sets it, then the default. One rule for every reader of a view.
    pub fn resolve_view(&self, id: u64) -> Result<ResolvedView> {
        let chain = self.chain_of(id)?;
        let Some(ViewKind::Base { source, view: direction }) = chain.last().map(|base| base.kind) else {
            return Err(DocError::Invalid(format!("view {} has no base view", ids::encode(id))));
        };
        let mut basis = direction.basis().map_err(|e| DocError::Invalid(e.to_string()))?;
        for view in chain.iter().rev() {
            if let ViewKind::Projected { side, .. } = view.kind {
                basis = projected_basis(&basis, side, self.projection).map_err(|e| DocError::Invalid(e.to_string()))?;
            }
        }
        let scale = inherited(&chain, |s| s.scale).ok_or_else(|| DocError::Invalid(format!("view {} has no scale", ids::encode(id))))?;
        let defaults = ViewSettings::default();
        Ok(ResolvedView {
            source,
            basis,
            scale,
            display_style: or_default(&chain, |s| s.display_style, defaults.display_style),
            reference_style: or_default(&chain, |s| s.reference_style, defaults.reference_style),
            reference_hides_model: or_default(&chain, |s| s.reference_hides_model, defaults.reference_hides_model),
            tangent_edges: or_default(&chain, |s| s.tangent_edges, defaults.tangent_edges),
            reference_shown_percent: chain.first().map_or(0.0, |view| view.reference_shown_percent),
        })
    }

    /// Where view `id`'s centre sits on its sheet, in paper millimetres. A view placed at a point
    /// is there. One lined up with its parent takes its own place along the free axis and its
    /// parent's centre across it ([`ProjectedSide::on_free_axis`]): both draw the same model, so
    /// their extents share that axis's middle, and they line up whatever their scales.
    pub fn view_centre(&self, id: u64) -> Result<Vec2> {
        let mut centre = Vec2::ZERO;
        // From the base view down to this one, each lined up with the one before or placed apart.
        for view in self.chain_of(id)?.iter().rev() {
            centre = match (view.place, view.kind) {
                (ViewPlace::At([x, y]), _) => Vec2::new(x, y),
                (ViewPlace::Aligned(along), ViewKind::Projected { side, .. }) => side.on_free_axis(centre, along),
                (ViewPlace::Aligned(_), ViewKind::Base { .. }) => {
                    return Err(DocError::Invalid(format!("base view {} isn't placed at a point", ids::encode(view.id))));
                }
            };
        }
        Ok(centre)
    }

    /// The views from `id` up to its base view, nearest first ([`chain`]).
    fn chain_of(&self, id: u64) -> Result<Vec<&View>> {
        chain(id, |id| self.view(id))
    }
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
