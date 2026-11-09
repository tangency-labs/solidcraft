//! Handles on the drawing being placed by Insert SVG / DXF: its box with a corner at each end
//! to scale (the opposite corner stays put, the aspect is kept), the box itself to move, and a
//! knob above it to turn (Shift snaps to 15 degrees). They change the same X, Y, Scale and Angle
//! the window shows.

use egui::{Pos2, Sense, Stroke, Vec2 as Screen, vec2};
use solidcore_record::{Plane, Vec2};
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::dialogs_insert_drawing::{MAX_SCALE, Place};
use crate::viewport::Proj;

/// Half the side of a corner handle, in screen points.
const HANDLE: f32 = 6.0;
/// How far above the box the turning knob sits, in screen points.
const KNOB_GAP: f32 = 28.0;
/// Smallest scale a drag makes.
const MIN_SCALE: f64 = 0.0001;

/// What a drag took hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Move,
    /// A corner, by its signs in the box's own frame.
    Corner(f64, f64),
    Turn,
}

/// A drag in progress.
#[derive(Clone, Debug)]
pub struct Drag {
    part: Part,
    /// The pointer on the sketch where it began, and the placement then.
    from: Vec2,
    at: (f64, f64, f64, f64),
}

/// The box's half sizes (mm), unturned.
fn half(p: &Place) -> Option<Vec2> {
    let (w, h) = p.natural?;
    Some(Vec2::new(w * p.scale / 2.0, h * p.scale / 2.0))
}

fn turn(v: Vec2, degrees: f64) -> Vec2 {
    let (s, c) = degrees.to_radians().sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// A spot of the box in its own frame (`u`, `v` in half sizes) on the sketch.
fn spot(p: &Place, u: f64, v: f64) -> Option<Vec2> {
    let h = half(p)?;
    Some(Vec2::new(p.x, p.y) + turn(Vec2::new(u * h.x, v * h.y), p.angle))
}

fn on_screen(plane: &Plane, proj: &Proj, q: Vec2) -> Option<Pos2> {
    proj.to_screen(plane.to_world(q))
}

fn pointer_on_sketch(plane: &Plane, proj: &Proj, at: Pos2) -> Option<Vec2> {
    let (o, d) = proj.ray(at);
    plane.intersect_ray(o, d).map(|w| plane.to_local(w))
}

fn place_of(app: &SolveApp) -> Option<Place> {
    match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::InsertDrawing(p)) => Some(p.clone()),
        _ => None,
    }
}

/// The placement window's drawing, with its handles. A drag on them edits the window's values.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui, proj: &Proj) {
    let Some(place) = place_of(app) else {
        app.viewport.insert_drag = None;
        return;
    };
    let plane = placement_plane(app);
    let painter = ui.painter().clone();
    show_svg_diagnostics(&place, &plane, proj, &painter);
    if place.command.ends_with("svg") && !crate::dialogs_insert_drawing::placement_visible(&place) {
        app.viewport.insert_drag = None;
        return;
    }

    let Some(corners) = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .iter()
        .map(|(u, v)| spot(&place, *u, *v).and_then(|q| on_screen(&plane, proj, q)))
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let (Some(top), Some(knob_from)) =
        (spot(&place, 0.0, 1.0).and_then(|q| on_screen(&plane, proj, q)), spot(&place, 0.0, 0.0).and_then(|q| on_screen(&plane, proj, q)))
    else {
        return;
    };
    let away = (top - knob_from).normalized();
    let knob = top + if away.length() > 0.0 { away * KNOB_GAP } else { vec2(0.0, -KNOB_GAP) };
    let t = solidcraft_theme::Tokens::get();
    let painter = ui.painter().clone();
    let stroke = Stroke::new(t.space_2, if place.command.ends_with("svg") { t.svg_repair_segment } else { t.accent });
    painter.add(egui::Shape::closed_line(corners.clone(), stroke));
    painter.line_segment([top, knob], stroke);
    painter.circle(knob, HANDLE, t.overlay, stroke);

    // Whole box first, so the corners and the knob above it win where they overlap.
    let mut parts: Vec<(egui::Rect, Part, egui::CursorIcon)> = Vec::new();
    let bounds = egui::Rect::from_points(&corners);
    parts.push((bounds, Part::Move, egui::CursorIcon::Grab));
    for (c, (u, v)) in corners.iter().zip([(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]) {
        painter.rect(egui::Rect::from_center_size(*c, Screen::splat(HANDLE * 2.0)), 1.0, t.overlay, stroke, egui::StrokeKind::Inside);
        parts.push((egui::Rect::from_center_size(*c, Screen::splat(HANDLE * 3.0)), Part::Corner(u, v), egui::CursorIcon::ResizeNwSe));
    }
    parts.push((egui::Rect::from_center_size(knob, Screen::splat(HANDLE * 3.0)), Part::Turn, egui::CursorIcon::Crosshair));
    for (i, (rect, part, cursor)) in parts.into_iter().enumerate() {
        let response = ui.interact(rect, egui::Id::unique(("sc_insert_handle", i)), Sense::drag()).on_hover_cursor(cursor);
        solidcraft_ui_registry::publish_handle(&format!("insert:handle:{i}"), rect.center());
        if response.drag_started()
            && let Some(from) = response.interact_pointer_pos().and_then(|p| pointer_on_sketch(&plane, proj, p))
        {
            app.viewport.insert_drag = Some(Drag { part, from, at: (place.x, place.y, place.scale, place.angle) });
        }
        // The release frame carries the last move too: it counts, or a quick drag stops short.
        if (response.dragged() || response.drag_stopped())
            && let Some(drag) = app.viewport.insert_drag.clone()
            && drag.part == part
            && let Some(p) =
                response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.latest_pos())).and_then(|p| pointer_on_sketch(&plane, proj, p))
        {
            let snap = ui.input(|i| i.modifiers.shift);
            if let Some(Kind::InsertDrawing(target)) = app.dialog.as_mut().map(|d| &mut d.kind) {
                apply(target, &drag, p, snap);
            }
        }
        if response.drag_stopped() {
            app.viewport.insert_drag = None;
        }
    }
}

/// Insert creates an XY sketch on OK without an active sketch; its preview carries that plane.
pub(crate) fn placement_plane(app: &SolveApp) -> Plane {
    app.session
        .active_sketch
        .and_then(|id| crate::canvas::profile_plane(app, id))
        .or_else(|| app.preview.sketches.first().map(|preview| preview.sketch.plane))
        .unwrap_or(Plane::XY)
}

pub(crate) fn diagnostic_world(place: &Place, plane: &Plane, q: [f64; 2]) -> Option<solidcore_record::Vec3> {
    let q = Vec2::new(q[0], q[1]);
    let point = turn(q, place.angle) + Vec2::new(place.x, place.y);
    point.is_finite().then(|| plane.to_world(point))
}

/// Read-only viewport captions use the same semantic backing as the other HUDs.
fn svg_caption(painter: &egui::Painter, at: Pos2, text: String, pill: bool) -> egui::Rect {
    let t = solidcraft_theme::Tokens::get();
    let galley = painter.layout(text, egui::FontId::proportional(t.font_small), t.text, t.form_card_width);
    let container =
        crate::ui_kit::Container::new(painter.ctx(), if pill { crate::ui_kit::Surface::ViewportStatus } else { crate::ui_kit::Surface::Panel });
    let frame = container.frame(painter.ctx(), t.overlay, crate::frame::Side::Left).inner_margin(t.space_8);
    let clip = painter.clip_rect().shrink(t.space_12);
    let size = galley.size();
    let pos = egui::pos2(
        at.x.clamp(clip.left(), (clip.right() - size.x).max(clip.left())),
        at.y.clamp(clip.top(), (clip.bottom() - size.y).max(clip.top())),
    );
    let content = egui::Rect::from_min_size(pos, size);
    container.paint_background(painter, frame, content);
    painter.galley(content.min, galley, t.text);
    frame.widget_rect(content)
}

/// Repair overlays describe actual changes. Detailed originals and endpoint dots appear only
/// during explicit review; source warnings never appear automatically on initial placement.
fn show_svg_diagnostics(place: &Place, plane: &Plane, proj: &Proj, painter: &egui::Painter) {
    if !place.command.ends_with("svg") {
        return;
    }
    let t = solidcraft_theme::Tokens::get();
    let screen = |q: [f64; 2]| diagnostic_world(place, plane, q).and_then(|q| proj.to_screen(q));
    let selected = place.svg.review.then_some(place.svg.selected).flatten();
    let stale = crate::dialogs_insert_drawing::is_stale(place);
    let mut highlighted = 0;
    let mut changed = 0;
    let mut before_lines = 0;
    let mut after_lines = 0;
    let mut overlay_vertices = 65_536usize;
    let mut draw = |chain: &[[f64; 2]], stroke: Stroke, dashed: bool| -> bool {
        let limit = overlay_vertices.min(4096).min(chain.len());
        overlay_vertices = overlay_vertices.saturating_sub(limit);
        let points: Vec<Pos2> = chain.iter().take(limit).filter_map(|q| screen(*q)).collect();
        if points.len() < 2 {
            return false;
        }
        if dashed {
            painter.extend(crate::ui_kit::dashed_line(&points, painter.clip_rect(), stroke, t.space_6, t.space_4));
        } else {
            painter.add(egui::Shape::line(points, stroke));
        }
        true
    };
    // The selected issue gets its budget first, even in an outline with thousands of changes.
    if let Some((index, issue)) = selected.and_then(|i| place.svg.diagnostics.get(i).map(|d| (i, d))) {
        for chain in &issue.replacement_highlight {
            after_lines += usize::from(draw(chain, Stroke::new(t.space_3, t.svg_repair_segment), false));
        }
        before_lines += usize::from(draw(&issue.highlight, Stroke::new(t.space_2, t.svg_original_segment), true));
        highlighted = 1;
        if let Some(at) = screen(issue.at) {
            painter.circle(at, t.space_6, t.overlay, Stroke::new(t.space_2, if issue.repaired { t.svg_repair_segment } else { t.warning }));
            solidcraft_ui_registry::publish_handle(&format!("insert:svg:highlight:{index}"), at);
            if issue.repaired {
                changed += 1;
                solidcraft_ui_registry::publish_handle(&format!("insert:svg:changed:{index}"), at);
                if issue.replacement_highlight.is_empty() {
                    let d = vec2(t.space_4, t.space_4);
                    let e = vec2(t.space_4, -t.space_4);
                    painter.line_segment([at - d, at + d], Stroke::new(t.space_2, t.preview_cut));
                    painter.line_segment([at - e, at + e], Stroke::new(t.space_2, t.preview_cut));
                }
            }
            for point in issue
                .replacement_highlight
                .first()
                .and_then(|p| p.first())
                .into_iter()
                .chain(issue.replacement_highlight.last().and_then(|p| p.last()))
            {
                if let Some(point) = screen(*point) {
                    painter.circle(point, t.space_3, t.overlay, Stroke::new(t.space_2, t.svg_repair_segment));
                }
            }
        }
        // Other changes in the selected source outline remain legible above the dimmed drawing.
        for (other, part) in place.svg.diagnostics.iter().enumerate() {
            if other == index || part.path != issue.path || part.subpath != issue.subpath {
                continue;
            }
            if part.repaired && !place.svg.original_view {
                for chain in &part.replacement_highlight {
                    draw(chain, Stroke::new(t.space_2, t.svg_curve), false);
                }
            } else {
                draw(&part.highlight, Stroke::new(t.space_2, t.svg_curve), false);
            }
        }
        let reason = solidcraft_ui_text::tr(crate::dialogs_insert_drawing::diagnostic_kind(&issue.kind));
        let text = if let Some(bound) = issue.max_deviation_mm {
            let value = if bound > 0.0 && bound < 1e-4 { format!("{bound:.3e}") } else { format!("{bound:.6}") };
            format!("{}\n{}", reason, solidcraft_ui_text::format("ui.svg_viewport_largest_move", &[("deviation", &format!("≤ {value}"))]))
        } else {
            reason.to_string()
        };
        if let Some(at) = screen(issue.at) {
            svg_caption(painter, at + vec2(t.space_16, -t.space_24 * 2.0), text, false);
        }
    } else if !place.svg.review && place.svg.repair && !place.svg.original_view && place.svg.show_curves {
        let color = if stale { t.svg_original_segment.gamma_multiply(0.35) } else { t.svg_repair_segment };
        for issue in place.svg.diagnostics.iter().filter(|d| d.repaired) {
            for chain in &issue.replacement_highlight {
                after_lines += usize::from(draw(chain, Stroke::new(t.space_3, color), false));
            }
        }
    }
    if !place.svg.review && place.svg.show_changes {
        for (index, issue) in place.svg.diagnostics.iter().enumerate().filter(|(_, d)| !d.repaired).take(512) {
            if let Some(at) = screen(issue.at) {
                painter.circle_filled(at, t.space_3, t.warning);
                solidcraft_ui_registry::publish_handle(&format!("insert:svg:highlight:{index}"), at);
                highlighted += 1;
            }
        }
    }
    if place.svg.review {
        let legend = format!(
            "{}\n{}\n{}",
            solidcraft_ui_text::tr("ui.svg_viewport_original_legend"),
            solidcraft_ui_text::tr("ui.svg_viewport_replacement_legend"),
            solidcraft_ui_text::tr("ui.svg_viewport_removed_legend")
        );
        svg_caption(painter, egui::pos2(proj.rect.left() + t.space_16, proj.rect.bottom() - t.control_height * 4.0), legend, false);
    }
    // The closed-region and change summaries live in the single SVG integrity status item.
    solidcraft_ui_registry::publish_count("svg_highlighted_segments", highlighted as f64);
    solidcraft_ui_registry::publish_count("svg_changed_locations", changed as f64);
    solidcraft_ui_registry::publish_count("svg_original_overlays", before_lines as f64);
    solidcraft_ui_registry::publish_count("svg_repaired_overlays", after_lines as f64);
}

/// The placement after the pointer has moved to `p`, from where `drag` began.
fn apply(target: &mut Place, drag: &Drag, p: Vec2, snap: bool) {
    let (x, y, scale, angle) = drag.at;
    let start = Place { x, y, scale, angle, ..target.clone() };
    match drag.part {
        Part::Move => {
            target.x = x + (p.x - drag.from.x);
            target.y = y + (p.y - drag.from.y);
        }
        Part::Turn => {
            let to = p - Vec2::new(x, y);
            if to.len() > 1e-9 {
                let a = to.angle().to_degrees() - 90.0;
                let a = if snap { (a / 15.0).round() * 15.0 } else { a };
                // Into -180..180, the range the field takes.
                target.angle = (a + 180.0).rem_euclid(360.0) - 180.0;
            }
        }
        Part::Corner(u, v) => {
            let (Some((nw, nh)), Some(fixed)) = (start.natural, spot(&start, -u, -v)) else { return };
            // The pointer in the box's own frame, measured from the corner that stays.
            let local = turn(p - fixed, -angle);
            let k = (local.x.abs() / nw).max(local.y.abs() / nh).clamp(MIN_SCALE, MAX_SCALE);
            if k.is_finite() {
                let reach = turn(Vec2::new(u * k * nw / 2.0, v * k * nh / 2.0), angle);
                target.scale = k;
                target.x = fixed.x + reach.x;
                target.y = fixed.y + reach.y;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place() -> Place {
        Place {
            command: "sketch.insert_svg".into(),
            path: "a.svg".into(),
            x: 10.0,
            y: 5.0,
            scale: 1.0,
            angle: 0.0,
            natural: Some((20.0, 10.0)),
            svg: Default::default(),
        }
    }

    fn drag(part: Part, from: Vec2, p: &Place) -> Drag {
        Drag { part, from, at: (p.x, p.y, p.scale, p.angle) }
    }

    /// A corner drag scales about the opposite corner, which does not move.
    #[test]
    fn a_corner_scales_about_the_opposite_corner() {
        let mut p = place();
        let fixed = spot(&p, -1.0, -1.0).unwrap();
        let d = drag(Part::Corner(1.0, 1.0), spot(&p, 1.0, 1.0).unwrap(), &p);
        // The corner at (20, 10) goes to (40, 20) from the fixed one at (0, 0): twice the size.
        apply(&mut p, &d, Vec2::new(40.0, 20.0), false);
        assert!((p.scale - 2.0).abs() < 1e-9, "{}", p.scale);
        let now = spot(&p, -1.0, -1.0).unwrap();
        assert!(now.dist(fixed) < 1e-9, "the opposite corner stayed: {now:?} vs {fixed:?}");
    }

    #[test]
    fn a_turned_corner_keeps_its_opposite_corner_too() {
        let mut p = Place { angle: 30.0, ..place() };
        let fixed = spot(&p, 1.0, -1.0).unwrap();
        let d = drag(Part::Corner(-1.0, 1.0), spot(&p, -1.0, 1.0).unwrap(), &p);
        let pulled = spot(&p, -1.0, 1.0).unwrap() + turn(Vec2::new(-10.0, 5.0), 30.0);
        apply(&mut p, &d, pulled, false);
        assert!(p.scale > 1.0);
        assert!(spot(&p, 1.0, -1.0).unwrap().dist(fixed) < 1e-9);
    }

    #[test]
    fn dragging_the_box_moves_it_and_the_knob_turns_it() {
        let mut p = place();
        let d = drag(Part::Move, Vec2::new(10.0, 5.0), &p);
        apply(&mut p, &d, Vec2::new(14.0, 1.0), false);
        assert_eq!((p.x, p.y), (14.0, 1.0));
        // The knob is straight above the middle: dragging it to the left turns a quarter.
        let d = drag(Part::Turn, Vec2::new(14.0, 10.0), &p);
        apply(&mut p, &d, Vec2::new(4.0, 1.0), false);
        assert!((p.angle - 90.0).abs() < 1e-9, "{}", p.angle);
        apply(&mut p, &d, Vec2::new(4.0, 3.0), true);
        assert!(p.angle == 90.0 || (p.angle % 15.0).abs() < 1e-9, "snapped: {}", p.angle);
    }
    #[test]
    fn reviewing_a_late_svg_issue_only_marks_the_selected_change() {
        let ctx = crate::fonts::test_context();
        let mut p = place();
        let issue = crate::dialogs_insert_drawing::Diagnostic {
            path: 1,
            subpath: 1,
            segment: 1,
            kind: "backtracking_curve".into(),
            at: [0.0, 0.0],
            highlight: vec![[0.0, 0.0], [1.0, 1.0]],
            replacement_highlight: vec![vec![[0.0, 0.0], [1.0, 0.9]]],
            repaired: true,
            max_deviation_mm: Some(0.1),
        };
        p.svg.diagnostics = std::sync::Arc::new(vec![issue; 601]);
        crate::scenario::clear_handles();
        let mut first = ctx.run_ui(Default::default(), |ui| {
            let cam = solidcraft_viewport::Camera::default();
            let rect = egui::Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
            let proj = Proj { cam, vp: cam.view_proj(800.0 / 600.0, 100.0), rect };
            show_svg_diagnostics(&p, &Plane::XY, &proj, ui.painter());
        });
        first.textures_delta.clear();
        assert!(crate::scenario::handle_at("insert:svg:highlight:0").is_none());
        p.svg.review = true;
        p.svg.selected = Some(600);
        p.natural = None;
        let mut h = crate::scenario::Harness::new();
        h.app.dialog = Some(crate::dialogs::Dialog::new(Kind::InsertDrawing(p.clone()), Vec::new()));
        crate::scenario::clear_handles();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let cam = solidcraft_viewport::Camera::default();
            let rect = egui::Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
            let proj = Proj { cam, vp: cam.view_proj(800.0 / 600.0, 100.0), rect };
            show(&mut h.app, ui, &proj);
        });
        output.textures_delta.clear();
        assert!(crate::scenario::handle_at("insert:svg:highlight:600").is_some());
        assert_eq!(crate::scenario::count_of("svg_original_overlays"), Some(1.0));
        assert_eq!(crate::scenario::count_of("svg_repaired_overlays"), Some(1.0));
        assert_eq!(crate::scenario::count_of("svg_changed_locations"), Some(1.0));
    }
}
