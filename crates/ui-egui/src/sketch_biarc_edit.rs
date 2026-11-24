//! Drag the join ratio of the selected Biarc span without moving its nodes or tangent guides.

use egui::{CursorIcon, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2 as ScreenVec, vec2};
use serde_json::json;
use solidcore_ops::prism::{self, chord_point, ray_chord_parameter};
use solidcore_ops::{affine, measure};
use solidcore_record::{Plane, Seg2, Vec2};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::render::{SamplingBudget, sample_segment};
use solidcraft_engine::sketch::{BiarcSpanMode, BiarcSpanPreview, Profile};
use solidcraft_theme::Tokens;

use crate::SolveApp;
use crate::ui_kit::BackingTooltipExt as _;
use crate::viewport::Proj;

const GRAB: f32 = 8.0;
const DIAMOND: f32 = 5.5;
const END_MARGIN: f64 = 0.0001;

fn screen_record(point: Pos2) -> Vec2 {
    Vec2::new(f64::from(point.x), f64::from(point.y))
}

fn screen_point(point: [f64; 2]) -> Option<Pos2> {
    let [x, y] = point;
    let point = Pos2::new(x as f32, y as f32);
    point.is_finite().then_some(point)
}

fn screen_distance(a: Pos2, b: Pos2) -> f32 {
    prism::chord(screen_record(a), screen_record(b)).1 as f32
}

struct Handle {
    sketch: u64,
    spline: String,
    span: usize,
    members: [String; 2],
    plane: Plane,
    ends: [Vec2; 2],
    join: Vec2,
    ratio: f64,
    construction: bool,
    geometry: BiarcSpanPreview,
}

impl Handle {
    fn selected(app: &SolveApp) -> Option<Self> {
        let sketch = enabled_sketch(app).filter(|sketch| crate::sketch_view::active_sketch(app) == Some(*sketch))?;
        let world = crate::open_meshing::shown_world(app);
        let solved = world.sketch(sketch)?;
        let sk = &solved.sketch;
        let (spline, index) = crate::sketch_biarc_ui::selected_span(sk, sketch, &app.session.selection)?;
        let span = spline.spans.get(index)?;
        if span.mode != BiarcSpanMode::Auto {
            return None;
        }
        let [first, last] = spline.nodes.get(index..)?.first_chunk::<2>()?;
        let point = |id: &str| sk.point(sk.resolve_point(id)?);
        let join = point(&format!("{}.end", span.curves[0]))?;
        let construction = sk.curves.get(sk.curve_index(&span.curves[0])?)?.construction;
        Some(Self {
            sketch,
            spline: spline.id.clone(),
            span: index,
            members: span.curves.clone(),
            plane: solved.plane,
            ends: [point(&first.point)?, point(&last.point)?],
            join,
            ratio: span.ratio,
            construction,
            geometry: sk.biarc_span_preview(&spline.id, index).ok()?,
        })
    }

    fn screen(&self, proj: &Proj, ratio: f64) -> Option<([Pos2; 2], Pos2)> {
        let [a, b] = self.ends;
        let to_screen = |p| proj.to_screen(self.plane.to_world(p)).filter(|p| p.is_finite());
        let ends = [to_screen(a)?, to_screen(b)?];
        let length = screen_distance(ends[0], ends[1]);
        (length.is_finite() && length > GRAB * 2.0).then_some((ends, to_screen(chord_point(a, b, ratio))?))
    }

    fn hot(&self, proj: &Proj, cursor: Pos2) -> Option<Pos2> {
        let (ends, at) = self.screen(proj, self.ratio)?;
        // Keep both authored points pickable when the ratio lies close to an endpoint.
        (screen_distance(at, cursor) <= GRAB && ends.iter().all(|end| screen_distance(*end, cursor) > DIAMOND)).then_some(at)
    }

    fn ratio_at(&self, proj: &Proj, cursor: Pos2) -> Option<f64> {
        let ([left, right], _) = self.screen(proj, self.ratio)?;
        // Project onto the screen chord before mapping that point back to the sketch plane.
        let (_, closest) =
            measure::polyline_2d_closest_point(screen_record(cursor).into(), &[screen_record(left).into(), screen_record(right).into()]).ok()??;
        let (origin, direction) = proj.ray(screen_point(closest)?);
        let ratio = ray_chord_parameter(&self.plane, origin, direction, self.ends)?;
        Some(ratio.clamp(END_MARGIN, 1.0 - END_MARGIN))
    }
}

fn enabled_sketch(app: &SolveApp) -> Option<u64> {
    app.session.active_sketch.filter(|_| {
        app.deferred.is_none() && app.tool.is_none() && app.dialog.is_none() && app.viewport.context_menu.is_none() && !crate::dim_view::editing()
    })
}

struct Preview {
    ratio: f64,
    join: Vec2,
    pieces: [Seg2; 2],
}

struct Drag {
    document: u64,
    revision: u64,
    selected: Sel,
    handle: Handle,
    offset: ScreenVec,
    preview: Option<Preview>,
}

impl Drag {
    fn valid(&self, app: &SolveApp) -> bool {
        self.document == app.session.uid
            && self.revision == app.session.revision
            && enabled_sketch(app) == Some(self.handle.sketch)
            && app.session.selection.as_slice() == std::slice::from_ref(&self.selected)
    }

    fn follow(&mut self, proj: &Proj, cursor: Pos2) {
        let Some(ratio) = self.handle.ratio_at(proj, cursor + self.offset) else { return };
        if (ratio - self.handle.ratio).abs() <= 1e-7 {
            self.preview = None;
        } else if self.preview.as_ref().is_none_or(|p| p.ratio != ratio)
            && let Ok((join, pieces)) = self.handle.geometry.geometry(ratio, BiarcSpanMode::Auto)
        {
            // If a split is singular at this scale, keep the last drawable split.
            self.preview = Some(Preview { ratio, join, pieces });
        }
    }
}

#[derive(Default)]
pub(crate) struct State {
    // Keep the drag payload off idle view stacks.
    drag: Option<Box<Drag>>,
    /// A cancelled press still owns the pointer until release.
    cancelled: bool,
}

pub(crate) fn dragging(app: &SolveApp) -> bool {
    app.viewport.biarc_edit.drag.as_ref().is_some_and(|d| d.valid(app))
}

pub(crate) fn cancel(app: &mut SolveApp) -> bool {
    if app.viewport.biarc_edit.drag.take().is_none() {
        return false;
    }
    app.viewport.biarc_edit.cancelled = true;
    app.last_escape = None;
    true
}

pub(crate) fn owns_pointer(app: &SolveApp, proj: &Proj, cursor: Pos2) -> bool {
    crate::sketch_view::active_sketch(app).is_some() && (dragging(app) || Handle::selected(app).is_some_and(|h| h.hot(proj, cursor).is_some()))
}

/// Handle the primary press before sketch dragging, selection, and hover picking see it.
pub(crate) fn interact(app: &mut SolveApp, ui: &Ui, proj: &Proj, response: &Response, hover: Option<Pos2>, navigating: bool) -> bool {
    // Passive sibling tiles leave the owner's gesture and cancelled press alone.
    if crate::sketch_view::active_sketch(app).is_none() {
        return false;
    }
    let (pressed, down, released, cursor, origin, focus_lost) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.primary_released(),
            i.pointer.interact_pos(),
            i.pointer.press_origin(),
            i.events.iter().any(|event| matches!(event, egui::Event::WindowFocused(false))),
        )
    });
    if app.viewport.biarc_edit.cancelled && !pressed {
        app.viewport.biarc_edit.cancelled = down;
        return true;
    }
    app.viewport.biarc_edit.cancelled = false;
    if let Some(mut drag) = app.viewport.biarc_edit.drag.take() {
        if !drag.valid(app) || !ui.is_enabled() || focus_lost || navigating {
            app.viewport.biarc_edit.cancelled = down;
            return true;
        }
        if (response.dragged_by(egui::PointerButton::Primary) || response.drag_stopped_by(egui::PointerButton::Primary))
            && let Some(cursor) = cursor
        {
            drag.follow(proj, cursor);
        }
        if released {
            if let Some(preview) = drag.preview {
                let _ = app.run(crate::sketch_biarc_ui::EDIT, json!({"curve": drag.handle.spline, "span": drag.handle.span, "ratio": preview.ratio}));
            }
        } else if down {
            app.viewport.biarc_edit.drag = Some(drag);
        }
        return true;
    }

    if navigating || focus_lost || !ui.is_enabled() || app.viewport.point_drag.is_some() || app.viewport.boxsel.is_some() {
        return false;
    }
    // The canvas can recognise a drag after its press frame. Take the same press origin that
    // ordinary sketch drags use, before they can take a grip that coincides with a profile curve.
    let started = response.drag_started_by(egui::PointerButton::Primary);
    let press = origin.filter(|p| (pressed && hover.is_some() || started) && proj.rect.contains(*p) && !crate::shell::covered(app, *p));
    if let Some(press) = press.filter(|_| down) {
        let Some(handle) = Handle::selected(app) else { return false };
        let Some(at) = handle.hot(proj, press) else { return false };
        let [selected] = app.session.selection.as_slice() else { return false };
        let mut drag =
            Drag { document: app.session.uid, revision: app.session.revision, selected: selected.clone(), handle, offset: at - press, preview: None };
        if started && let Some(cursor) = cursor {
            drag.follow(proj, cursor);
        }
        app.viewport.biarc_edit.drag = Some(Box::new(drag));
        return true;
    }
    !down && hover.is_some_and(|p| owns_pointer(app, proj, p))
}

pub(crate) fn replaced_members(app: &SolveApp, sketch: u64) -> Option<&[String; 2]> {
    let drag = app.viewport.biarc_edit.drag.as_ref()?;
    (crate::sketch_view::active_sketch(app) == Some(sketch) && drag.handle.sketch == sketch && drag.valid(app) && drag.preview.is_some())
        .then_some(&drag.handle.members)
}

pub(crate) fn replaced_profile(members: Option<&[String; 2]>, profile: &Profile) -> bool {
    members.is_some_and(|members| profile.outer_curves.iter().chain(profile.hole_curves.iter().flatten()).any(|id| members.contains(id)))
}

pub(crate) fn show(app: &SolveApp, ui: &Ui, painter: &Painter, proj: &Proj, hover: Option<Pos2>) {
    if crate::sketch_view::active_sketch(app).is_none() {
        return;
    }
    let current;
    let drag = app.viewport.biarc_edit.drag.as_ref().filter(|d| d.valid(app));
    let handle = if let Some(drag) = drag {
        &drag.handle
    } else {
        let Some(selected) = Handle::selected(app) else { return };
        current = selected;
        &current
    };
    let preview = drag.and_then(|d| d.preview.as_ref());
    let ratio = preview.map_or(handle.ratio, |p| p.ratio);
    let Some(([a, b], at)) = handle.screen(proj, ratio) else { return };
    let tk = Tokens::view();
    if let Some(preview) = preview {
        let view = crate::sketch_display::pixel_view(proj, ui.ctx().pixels_per_point());
        let mut budget = SamplingBudget::new(4096, 65_536);
        let mode = u8::from(handle.construction);
        for piece in &preview.pieces {
            for part in sample_segment(piece, &handle.plane, &view, &mut budget).parts {
                crate::tools::paint_sketch_path(&part, mode, painter, proj, Stroke::new(2.5, tk.sketch_accent));
            }
        }
    }
    let chord = Stroke::new(1.0, tk.sketch_construction.gamma_multiply(0.7));
    painter.line_segment([a, b], chord);
    let (direction, _) = prism::chord(screen_record(a), screen_record(b));
    let Ok([[dx, dy, _], _]) = affine::orthonormal_axes([direction.x, direction.y, 0.0], [0.0, 0.0, 1.0]) else { return };
    let d = vec2(dx as f32, dy as f32);
    let arrow = b - d * 8.0;
    if screen_distance(at, arrow) > GRAB + 4.0 {
        let Some(normal) = prism::left_unit_normal(screen_record(b), screen_record(a), 0.0) else { return };
        let n = vec2(normal.x as f32, normal.y as f32) * 2.5;
        painter.add(Shape::line(vec![arrow - d * 4.0 + n, arrow, arrow - d * 4.0 - n], chord));
    }
    if let Some(join) = proj.to_screen(handle.plane.to_world(preview.map_or(handle.join, |p| p.join))).filter(|p| p.is_finite())
        && screen_distance(join, at) > GRAB + 2.0
    {
        painter.line_segment([at, join], chord);
        painter.circle_filled(join, 2.0, tk.sketch_construction);
    }
    let hot = hover.is_some_and(|p| handle.hot(proj, p).is_some());
    let fill = if hot || drag.is_some() { tk.sketch_accent } else { tk.panel };
    let diamond = vec![at - vec2(0.0, DIAMOND), at + vec2(DIAMOND, 0.0), at + vec2(0.0, DIAMOND), at - vec2(DIAMOND, 0.0)];
    painter.add(Shape::convex_polygon(diamond, fill, Stroke::new(1.5, tk.sketch_accent)));
    crate::scenario::publish_handle("biarc-ratio", at);
    if drag.is_some() {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    } else if hot {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
        ui.interact(Rect::from_center_size(at, ScreenVec::splat(GRAB * 2.0)), ui.scope_id().with("biarc-ratio"), Sense::hover())
            .backing_on_hover_text(app.tr("sketch.biarc.join_handle"));
    }
}
