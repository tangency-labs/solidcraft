//! More interactive sketch tools: projection and other model-pick tools, curve tools (trim,
//! extend, break, offset, fillet, chamfer), multi-pick tools (mirror, patterns, polygon), point
//! tools for free-form curves and slots, text placement, and constraint glyphs. Like `tools`,
//! these are UI state only: every action runs a command.

use crate::icons::markers::glyph_icon;
use crate::icons::markers::glyph_letter;
use std::cell::{Cell, RefCell};

use egui::{Pos2, Stroke};
use serde_json::{Value, json};
use solidcraft_engine::geom::{Plane, Vec2, Vec3};

use crate::SolveApp;
use crate::tool_modifiers::{Audit, Binding, Match, ModAction, ModifierCombo, Scope, ToolModifiers, Trigger};
use crate::tools::{Kind, Tool};
use crate::viewport::{Hit, Proj, pick_for_tool as pick, sketch_point_at};

const DRAW_SCOPES: &[Scope] = &[Scope::SketchDraw, Scope::SketchDrag];
const NONE: &[Scope] = &[];

const ISOCURVE_ALT: &[Binding] = &[Binding {
    action: ModAction::IsoCurveAlternate,
    defaults: &[ModifierCombo::SHIFT],
    matching: Match::AtLeast,
    trigger: Trigger::Press,
    rebindable: true,
}];

/// Extend: Shift picks the second curve of the two-curve form (`ExtendPair`). Shift does not snap
/// the clicked point to a midpoint here (an empty binding overrides the sketch scopes' one), so the
/// end nearest the click stays where it was clicked.
const EXTEND_BINDINGS: &[Binding] = &[
    Binding { action: ModAction::ExtendPair, defaults: &[ModifierCombo::SHIFT], matching: Match::AtLeast, trigger: Trigger::Press, rebindable: true },
    Binding { action: ModAction::TargetMidpoint, defaults: &[], matching: Match::AtLeast, trigger: Trigger::Live, rebindable: true },
];

/// Per-tool modifier descriptors owned by this host file.
pub const MODIFIERS: &[ToolModifiers] = &[
    ToolModifiers { tool: "sketch.construction_line", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.construction_line.horizontal", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.construction_line.vertical", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.construction_line.angle", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.construction_line.parallel", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.construction_line.perpendicular", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.construction_line.tangent_external", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.construction_line.tangent_at", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.construction_line.tangent_two", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.construction_line.bisector", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.project", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.intersect", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.include_3d", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.fit_curves_to_section", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.isoparametric_curve", scopes: NONE, bindings: ISOCURVE_ALT, audit: Audit::Implemented },
    ToolModifiers { tool: "inspect.curvature_comb", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "inspect.minimum_radius", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "inspect.center_of_mass", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "inspect.isocurve", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "canvas.insert", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "canvas.decal", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.trim", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.extend", scopes: NONE, bindings: EXTEND_BINDINGS, audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.break", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.line.midpoint", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.arc.tangent", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.ellipse", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.conic", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.slot.center_point", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.slot.arc_three_point", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.slot.arc_center", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.spline.fit_point", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.spline.control_point", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.spline.control_point_5", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.text", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.edit_text", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.blend_curve", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.constraint.curvature", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.circle.two_tangent", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.circle.three_tangent", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constrainer", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.polygon", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.centerline", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.project_to_surface", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.intersection_curve", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.spun_profile", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
];

thread_local! {
    /// Shift held this frame.
    static SHIFT: Cell<bool> = const { Cell::new(false) };
    /// Extend's two-curve chord (`ModAction::ExtendPair`) held this frame.
    static EXTEND_PAIR: Cell<bool> = const { Cell::new(false) };
    /// Glyphs drawn this frame (constraint id, screen centre) and the selected one.
    static GLYPHS: RefCell<Vec<(String, Pos2)>> = const { RefCell::new(Vec::new()) };
    static PICKED: RefCell<Option<String>> = const { RefCell::new(None) };
    /// The inference the cursor snapped to last (label, sketch point), for the hint.
    static SNAP: RefCell<Option<(&'static str, Vec2)>> = const { RefCell::new(None) };
    /// The points that snap lined the cursor up with, and where it went (#7828).
    static LINED: RefCell<Option<(Vec2, Vec<Lined>)>> = const { RefCell::new(None) };
    /// The drawing tool's points so far and whether it is the Line tool, from the last hover: a
    /// click takes the tool out of the app while it runs, and must place what the hover showed.
    static TOOL: RefCell<(Vec<Vec2>, bool)> = const { RefCell::new((Vec::new(), true)) };
    /// Curvature comb teeth (foot, tip) shown while the comb tool is active.
    static COMBS: RefCell<Vec<Vec<[Vec3; 2]>>> = const { RefCell::new(Vec::new()) };
    /// The image file a Canvas or Decal tool places.
    static IMAGE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Analysis markers shown while an analysis tool is active: (where, label).
    static MARKS: RefCell<Vec<(Vec3, String)>> = const { RefCell::new(Vec::new()) };
    /// Isocurve analysis polylines (as segments) while the tool is active.
    static ISO: RefCell<Vec<Vec<[Vec3; 2]>>> = const { RefCell::new(Vec::new()) };
    /// Center of Mass has just started: measure everything on the next frame.
    static DEFERRED: Cell<bool> = const { Cell::new(false) };
    /// A Trim drag: the pieces its path would remove (sketch polylines), and where the path's
    /// last point is on screen.
    static TRIM_REMOVED: RefCell<Vec<Vec<Vec2>>> = const { RefCell::new(Vec::new()) };
    static TRIM_LAST: Cell<Option<Pos2>> = const { Cell::new(None) };
}

/// Screen distance the pointer moves before a Trim drag's path takes another point.
const TRIM_STEP_PX: f32 = 4.0;

/// The sketch-plane position under screen position `pos`.
fn on_sketch_plane(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<Vec2> {
    let st = app.session.world_state();
    let ss = st.sketch(active_sketch(app)?)?;
    let (o, d) = proj.ray(pos);
    ss.plane.intersect_ray(o, d).map(|w| ss.plane.to_local(w))
}

/// A press and drag with Trim draws a path from `press`; true when it started one. Each piece
/// the path crosses is trimmed when it is let go (`sketch.trim.path`).
pub fn trim_drag_start(app: &mut SolveApp, proj: &Proj, press: Pos2) -> bool {
    let Some(q) = on_sketch_plane(app, proj, press) else { return false };
    let Some(tool) = app.tool.as_mut().filter(|t| t.cmd == "sketch.trim") else { return false };
    tool.pts = vec![(q, None)];
    TRIM_LAST.with(|c| c.set(Some(press)));
    TRIM_REMOVED.with(|r| r.borrow_mut().clear());
    true
}

/// The Trim drag's path reaches `pos`: the pieces it would remove light up (a dry run of the
/// command the release runs).
pub fn trim_drag_to(app: &mut SolveApp, proj: &Proj, pos: Pos2) {
    if TRIM_LAST.with(Cell::get).is_some_and(|last| last.distance(pos) < TRIM_STEP_PX) {
        return;
    }
    let Some(q) = on_sketch_plane(app, proj, pos) else { return };
    let Some(tool) = app.tool.as_mut().filter(|t| t.cmd == "sketch.trim" && !t.pts.is_empty()) else { return };
    if tool.pts.len() >= TRIM_PATH_MAX {
        return;
    }
    tool.pts.push((q, None));
    TRIM_LAST.with(|c| c.set(Some(pos)));
    let points: Vec<[f64; 2]> = tool.pts.iter().map(|p| [p.0.x, p.0.y]).collect();
    let removed = app
        .session
        .execute("sketch.trim.path", &json!({"points": points, "dry_run": true}))
        .map(|v| {
            v["removed"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|piece| piece.as_array().into_iter().flatten().filter_map(solidcraft_engine::params::vec2).collect())
                .collect()
        })
        .unwrap_or_default();
    TRIM_REMOVED.with(|r| *r.borrow_mut() = removed);
}

/// Most points a Trim drag's path keeps (the command's limit).
const TRIM_PATH_MAX: usize = 2_000;

/// The Trim drag is let go: trim every piece its path crossed, in one undo step. False when the
/// path crossed nothing (the caller takes the release as a click).
pub fn trim_drag_end(app: &mut SolveApp) -> bool {
    TRIM_LAST.with(|c| c.set(None));
    let crossed = TRIM_REMOVED.with(|r| !std::mem::take(&mut *r.borrow_mut()).is_empty());
    let Some(tool) = app.tool.as_mut().filter(|t| t.cmd == "sketch.trim") else { return false };
    let points: Vec<[f64; 2]> = std::mem::take(&mut tool.pts).iter().map(|p| [p.0.x, p.0.y]).collect();
    if !crossed || points.len() < 2 {
        return false;
    }
    let _ = app.run("sketch.trim.path", json!({"points": points}));
    true
}

/// Run an analysis and keep its result as a marker.
fn analysis(app: &mut SolveApp, cmd: &str, p: Value) {
    let Ok(v) = app.run(cmd, p) else { return };
    let mark = match cmd {
        "inspect.center_of_mass" => v3(&v["center"]).map(|c| (c, "Center of mass".to_string())),
        _ => v3(&v["at"]).zip(v["min_radius"].as_f64()).map(|(c, r)| (c, format!("R min {}", crate::prefs::show_mm(app, r, 1)))),
    };
    if let Some(m) = mark {
        MARKS.with(|k| k.borrow_mut().push(m));
    } else if cmd == "inspect.minimum_radius" {
        app.set_status("Minimum radius: it is straight (no curvature)", false);
    }
}

/// A seam from an `inspect.continuity` reply as a marker label: its class and the measure that
/// decided it (the gap, the angle between the faces' normals, or the curvature jump).
fn continuity_label(app: &SolveApp, seam: &Value) -> String {
    let deg = |key: &str| seam[key]["value"].as_f64().map(|x| format!("{x:.3}\u{b0}")).unwrap_or_default();
    let jump = seam["curvature_jump_per_mm"]["value"].as_f64().map(|k| format!("\u{394}k {k:.4}/mm")).unwrap_or_default();
    match seam["verdict"].as_str().unwrap_or_default() {
        "gap" => format!("Gap {}", seam["gap_mm"]["value"].as_f64().map(|g| crate::prefs::show_mm(app, g, 1)).unwrap_or_default()),
        "sharp" => format!("G0 {}", deg("normal_angle_deg")),
        "curvature_jump" => format!("G1 {} {jump}", deg("normal_angle_deg")),
        "smooth" => format!("G2 {} {jump}", deg("normal_angle_deg")),
        other => other.to_string(),
    }
}

/// The Continuity Analysis legend in the viewport's lower left corner: each class in its
/// colour, while seams are shaded.
/// Isocurve analysis lines.
fn iso_lines(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if app.tool.as_ref().map(|t| t.cmd) != Some("inspect.isocurve") {
        ISO.with(|i| i.borrow_mut().clear());
        return;
    }
    let tk = crate::theme::Tokens::get();
    ISO.with(|i| {
        for c in i.borrow().iter() {
            for [a, b] in c {
                if let (Some(p), Some(q)) = (proj.to_screen(*a), proj.to_screen(*b)) {
                    painter.line_segment([p, q], Stroke::new(1.2, tk.accent));
                }
            }
        }
    });
}

/// Analysis markers: a target with its label.
fn marks(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if !app.tool.as_ref().is_some_and(|t| matches!(t.cmd, "inspect.minimum_radius" | "inspect.center_of_mass" | "inspect.continuity")) {
        MARKS.with(|m| m.borrow_mut().clear());
        return;
    }
    let tk = crate::theme::Tokens::get();
    MARKS.with(|m| {
        for (at, label) in m.borrow().iter() {
            let Some(q) = proj.to_screen(*at) else { continue };
            painter.circle(q, 6.0, tk.panel.gamma_multiply(0.8), Stroke::new(1.6, tk.accent));
            painter.line_segment([q - egui::vec2(10.0, 0.0), q + egui::vec2(10.0, 0.0)], Stroke::new(1.2, tk.accent));
            painter.line_segment([q - egui::vec2(0.0, 10.0), q + egui::vec2(0.0, 10.0)], Stroke::new(1.2, tk.accent));
            let r = painter.text(
                q + egui::vec2(12.0, -12.0),
                egui::Align2::LEFT_BOTTOM,
                label,
                egui::FontId::proportional(crate::theme::Tokens::get().font_body),
                tk.text,
            );
            painter.rect_filled(r.expand(3.0), tk.radius_sm.saturating_sub(1), tk.panel.gamma_multiply(0.85));
            painter.text(
                q + egui::vec2(12.0, -12.0),
                egui::Align2::LEFT_BOTTOM,
                label,
                egui::FontId::proportional(crate::theme::Tokens::get().font_body),
                tk.text,
            );
        }
    });
}

/// Commands that start with a step of their own: Canvas and Decal ask for the image first,
/// Edit Canvas opens the canvas panel. False when the command should not go on.
pub fn start_hook(app: &mut SolveApp, id: &str) -> bool {
    if id == "inspect.section" && app.preferences.viewport_section_status && app.session.section.is_some() {
        let _ = app.run(id, json!({"clear":true}));
        return false;
    }
    if id == "sketch.intersect"
        && let Some(dialog) = crate::dialogs::Dialog::for_command(app, id)
    {
        if dialog.wants_picks() && !app.session.selection.is_empty() {
            let _ = app.session.execute("select.clear", &json!({}));
        }
        app.dialog = Some(dialog);
        return false;
    }
    if id == "sketch.edit_text"
        && let Some(dialog) = crate::dialogs::Dialog::for_command(app, id)
    {
        app.dialog = Some(dialog);
        return false;
    }
    // Surface analyses toggle: a second click turns the same one off.
    use solidcraft_engine::SurfaceAnalysis as A;
    let same = match (id, app.session.analysis) {
        ("inspect.zebra", Some(A::Zebra { .. })) => Some(true),
        ("inspect.draft", Some(A::Draft { .. })) => Some(true),
        ("inspect.curvature_map", Some(A::Curvature { .. })) => Some(true),
        ("inspect.environment_map", Some(A::Environment)) => Some(true),
        ("inspect.accessibility", Some(A::Access { .. })) => Some(true),
        ("inspect.zebra" | "inspect.draft" | "inspect.curvature_map" | "inspect.environment_map" | "inspect.accessibility", _) => Some(false),
        _ => None,
    };
    if let Some(on) = same {
        let p = if on { json!({"clear": true}) } else { json!({}) };
        if app.run(id, p).is_ok() {
            app.set_status(if on { "Analysis off" } else { "Analysis on (click it again to turn it off)" }, false);
        }
        return false;
    }
    if id == "inspect.continuity" {
        // Every edge of the shown bodies at once; the tool stays for clicking single edges. With
        // seams shaded, the button turns the analysis off.
        MARKS.with(|m| m.borrow_mut().clear());
        if !app.session.continuity.is_empty() {
            if app.run(id, json!({"clear": true})).is_ok() {
                app.set_status("Analysis off", false);
            }
            return false;
        }
        let Ok(v) = app.run(id, json!({})) else { return false };
        let summary = &v["summary"];
        let counts: Vec<String> = [("smooth", "G2"), ("curvature_jump", "G1"), ("sharp", "G0"), ("gap", "gap")]
            .iter()
            .filter_map(|(key, class)| summary[*key].as_u64().filter(|n| *n > 0).map(|n| format!("{n} {class}")))
            .collect();
        app.set_status(format!("Continuity: {} (click the button again to turn it off)", counts.join(", ")), false);
        return true;
    }
    if id == "inspect.center_of_mass" {
        // The whole model at once; the tool stays for picking single bodies.
        MARKS.with(|m| m.borrow_mut().clear());
        DEFERRED.with(|d| d.set(true));
        return true;
    }
    if id == "canvas.edit" {
        // The browser's canvas panel (one panel for canvases).
        if let Some(first) = app.session.doc.canvases.first().map(|c| c.id) {
            app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(app, first, false));
        }
        return false;
    }
    if !matches!(id, "canvas.insert" | "canvas.decal") {
        return true;
    }
    let command = id.to_string();
    // Offer the image types first: the default Open filter lists only designs and meshes.
    app.pick_open_for(&["png", "jpg", "jpeg"], move |app, path| {
        if let Some(path) = path {
            IMAGE.with(|i| *i.borrow_mut() = Some(path));
            app.last_command = solidcraft_engine::find_command(&command).map(|spec| (spec.id.into(), spec.label.into()));
            app.tool = crate::tools::Tool::for_command(&command);
        }
    });
    false
}

fn v3(v: &Value) -> Option<Vec3> {
    Some(Vec3::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?))
}

/// Curvature combs: teeth and the envelope through their tips.
fn combs(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if app.tool.as_ref().map(|t| t.cmd) != Some("inspect.curvature_comb") {
        COMBS.with(|c| c.borrow_mut().clear());
        return;
    }
    let tk = crate::theme::Tokens::get();
    COMBS.with(|c| {
        for comb in c.borrow().iter() {
            let mut tips: Vec<Pos2> = Vec::new();
            for [a, b] in comb {
                let (Some(p), Some(q)) = (proj.to_screen(*a), proj.to_screen(*b)) else { continue };
                painter.line_segment([p, q], Stroke::new(1.0, tk.warning.gamma_multiply(0.8)));
                tips.push(q);
            }
            painter.add(egui::Shape::line(tips, Stroke::new(1.4, tk.warning)));
        }
    });
}

/// A point the cursor lined up with while drawing (#7828): its sketch point id (none for a point
/// of the shape being drawn, not in the sketch yet), where it is, and the axis the cursor
/// shares with it: level (`horizontal`, the same y) or plumb (the same x).
#[derive(Clone, Debug, PartialEq)]
pub struct Lined {
    pub id: Option<String>,
    pub at: Vec2,
    pub horizontal: bool,
}

/// The points the cursor can line up with: the sketch's visible points on screen (line ends,
/// centres, standalone points, the origin) and the shape's own points so far.
fn align_targets(
    proj: &Proj,
    sk: &solidcraft_engine::sketch::Sketch,
    plane: &solidcraft_engine::geom::Plane,
    own: &[Vec2],
) -> Vec<(Option<String>, Vec2)> {
    let visible = solidcraft_engine::view::SketchPointVisibility::new(sk);
    let on_screen = |p: Vec2| proj.to_screen(plane.to_world(p)).is_some_and(|q| proj.rect.contains(q));
    let mut out: Vec<(Option<String>, Vec2)> =
        sk.points.iter().filter(|p| visible.takes(p) && on_screen(p.pos)).map(|p| (Some(p.id.clone()), p.pos)).collect();
    out.extend(own.iter().map(|p| (None, *p)));
    out
}

/// The nearest point level with `lp` (within `tol` across, more than `tol` along) and the
/// nearest plumb with it, each on its own axis, as a dragged point lines up (`drag_snap::find`).
pub(crate) fn line_up(points: &[(Option<String>, Vec2)], lp: Vec2, tol: f64) -> (Option<Lined>, Option<Lined>) {
    let (mut level, mut plumb): (Option<(f64, Lined)>, Option<(f64, Lined)>) = (None, None);
    for (id, p) in points {
        let (dx, dy) = ((lp.x - p.x).abs(), (lp.y - p.y).abs());
        if dy < tol && dx > tol && level.as_ref().is_none_or(|l| dy < l.0) {
            level = Some((dy, Lined { id: id.clone(), at: *p, horizontal: true }));
        }
        if dx < tol && dy > tol && plumb.as_ref().is_none_or(|l| dx < l.0) {
            plumb = Some((dx, Lined { id: id.clone(), at: *p, horizontal: false }));
        }
    }
    (level.map(|l| l.1), plumb.map(|l| l.1))
}

/// What the last snap lined a point placed at `at` up with (#7828).
pub(crate) fn lined_at(at: Vec2) -> Vec<Lined> {
    LINED.with(|l| l.borrow().as_ref().filter(|(p, _)| p.dist(at) < 1e-9).map(|(_, lined)| lined.clone()).unwrap_or_default())
}

/// The active sketch's point ids, to tell the points a command makes from those it had.
pub(crate) fn point_ids(app: &SolveApp) -> Vec<String> {
    let st = app.session.world_state();
    app.session.active_sketch.and_then(|id| st.sketch(id)).map(|ss| ss.sketch.points.iter().map(|p| p.id.clone()).collect()).unwrap_or_default()
}

/// Once a shape is made (#7828): each new point that was placed lined up with a sketch point
/// (`placed`, the shape's points) stays level or plumb with it. Only what the sketch doesn't
/// already have is added, in the shape's undo step.
pub(crate) fn constrain_lined(app: &mut SolveApp, tool: &mut Tool, placed: &[Vec2], before: &[String]) {
    let lined = std::mem::take(&mut tool.lined);
    let new: Vec<(String, Vec2)> = {
        let st = app.session.world_state();
        let Some(ss) = app.session.active_sketch.and_then(|id| st.sketch(id)) else { return };
        ss.sketch.points.iter().filter(|p| !before.contains(&p.id)).map(|p| (p.id.clone(), p.pos)).collect()
    };
    let history = std::mem::take(&mut app.session.undo);
    for (at, targets) in lined.iter().filter(|(at, _)| placed.iter().any(|p| p.dist(*at) < 1e-9)) {
        let Some((point, _)) = new.iter().find(|(_, p)| p.dist(*at) < 1e-6) else { continue };
        for target in targets {
            let Some(id) = &target.id else { continue };
            let mode = if target.horizontal { "horizontal" } else { "vertical" };
            let params = json!({"points": [point, id], "mode": mode, "if_redundant": "refuse"});
            let _ = app.session.execute("sketch.constraint.horizontal_vertical", &params);
        }
    }
    app.session.undo = history;
}

/// The point of `poly` nearest `p`; None for fewer than two points.
fn nearest_on_polyline(poly: &[Vec2], p: Vec2) -> Option<Vec2> {
    poly.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let ab = b - a;
            let t = if ab.len2() > 0.0 { ((p - a).dot(ab) / ab.len2()).clamp(0.0, 1.0) } else { 0.0 };
            a + ab * t
        })
        .min_by(|x, y| x.dist(p).total_cmp(&y.dist(p)))
}

/// Snap a cursor point on the sketch plane to what is inferred while drawing: a crossing of
/// two curves, a line's midpoint, or alignment (horizontal/vertical) with the previous point
/// of the tool. None: nothing near, use the grid.
pub fn refine_snap(
    app: &SolveApp,
    proj: &Proj,
    sk: &solidcraft_engine::sketch::Sketch,
    plane: &solidcraft_engine::geom::Plane,
    lp: Vec2,
) -> Option<(Vec2, Option<String>)> {
    crate::inference::clear();
    LINED.with(|l| *l.borrow_mut() = None);
    // Millimetres per pixel here.
    let (a, b) = (proj.to_screen(plane.to_world(lp))?, proj.to_screen(plane.to_world(lp + Vec2::X))?);
    let px = 1.0 / (a.distance(b) as f64).max(1e-9);
    let tol = 7.0 * px;
    // A model vertex under the cursor: drawing on it projects it into the sketch.
    for h in pick(app, proj, a) {
        if let Hit::Vertex { point, .. } = h {
            SNAP.with(|s| *s.borrow_mut() = Some(("Pt", plane.to_local(point))));
            return Some((plane.to_local(point), Some(format!("vertex:{},{},{}", point.x, point.y, point.z))));
        }
    }
    let mid_only = crate::tool_modifiers::target_midpoint();
    // Shift midpoint mode: widen the midpoint capture a little and skip other inferences.
    let mid_tol = if mid_only { tol * 1.5 } else { tol };
    let mut best: Option<(f64, &'static str, Vec2)> = None;
    let mut mid_of: Option<String> = None;
    // A crossing of two curves, before a midpoint or a point on one curve.
    if !mid_only
        && let Some((q, i, j)) = solidcraft_engine::sketch::nearest_curve_intersection(sk, lp, tol)
        && let (Some(a), Some(b)) = (sk.curves.get(i), sk.curves.get(j))
    {
        best = Some((q.dist(lp), "X", q));
        mid_of = Some(format!("{}+{}:{},{}", a.id, b.id, q.x, q.y));
    }
    if best.is_none() {
        for (i, c) in sk.curves.iter().enumerate() {
            if let solidcraft_engine::sketch::CurveKind::Line { .. } = c.kind
                && let Some(solidcraft_engine::sketch::Shape::Line { a, b }) = sk.shape(i)
            {
                let m = (a + b) * 0.5;
                let d = m.dist(lp);
                if d < mid_tol && best.is_none_or(|x| d < x.0) {
                    best = Some((d, "Mid", m));
                    mid_of = Some(c.id.clone());
                }
            }
        }
    }
    // The two most recent midpoints or centres: snap where their axes meet. A free coordinate,
    // so placing it adds no constraint. Off unless Sketch preferences ask for it. One reference
    // stays a visual guide. Shift skips this.
    if !mid_only
        && app.preferences.sketch_recent_snap
        && best.is_none()
        && let Some(at) = app.viewport.tracking.meeting(sk, plane, proj, a)
    {
        best = Some((at.dist(lp), "+", at));
    }
    // The tool's points and kind (a click takes the tool out of the app while it runs, so the
    // last hover's are kept: a rectangle's click takes no Line tool inference its hover didn't).
    let (own, line_tool) = match app.tool.as_ref() {
        Some(t) => {
            let own: (Vec<Vec2>, bool) = (t.pts.iter().map(|p| p.0).collect(), t.cmd == "sketch.line");
            TOOL.with(|c| *c.borrow_mut() = own.clone());
            own
        }
        None => TOOL.with(|c| c.borrow().clone()),
    };
    // With `sketch_align_points` off, a click takes the Line tool's inferences, as before.
    let line_tool = if app.preferences.sketch_align_points { line_tool } else { app.tool.as_ref().is_none_or(|t| t.cmd == "sketch.line") };
    let prev = own.last().copied();
    // Line tool: the tangent point of a circle or arc, then a line through its centre
    // (before plain "on the curve", which would leave a chord free to miss the centre).
    if !mid_only
        && best.is_none()
        && line_tool
        && let Some(prev) = prev
        && let Some(i) = crate::inference::tangent(sk, prev, lp, tol)
    {
        best = Some((i.dist, i.label(), i.at));
    }
    if !mid_only
        && best.is_none()
        && line_tool
        && let Some(prev) = prev
        && let Some(i) = crate::inference::through_center(sk, prev, lp, tol)
    {
        best = Some((i.dist, i.label(), i.at));
        mid_of = i.arg;
    }
    // On a curve (coincident with it). A spline, ellipse or conic has no exact shape here: the
    // nearest point of the polyline it is drawn with, which the click's point-on-curve
    // constraint then puts on the curve itself (#9321).
    if !mid_only && best.is_none() {
        for (i, c) in sk.curves.iter().enumerate() {
            let (d, q) = match sk.shape(i) {
                Some(sh) => (sh.dist(lp), sh.project(lp)),
                None if c.kind.is_freeform() => {
                    let Some(q) = nearest_on_polyline(&sk.polyline(i), lp) else { continue };
                    (q.dist(lp), q)
                }
                None => continue,
            };
            if d < tol * 0.8 && best.is_none_or(|x| d < x.0) {
                best = Some((d, "On", q));
                mid_of = Some(format!("{}:{},{}", c.id, q.x, q.y));
            }
        }
    }
    // The free coordinate still follows the grid.
    let step = solidcraft_engine::view::grid_step(app.cam.half_height()).0 / 10.0;
    let g = |v: f64| (v / step).round() * step;
    // Points to line up with as well (#7828): with `sketch_align_points` off, none, so only the
    // previous point, one axis at a time, as before.
    let targets = if app.preferences.sketch_align_points { align_targets(proj, sk, plane, &own) } else { Vec::new() };
    let mut lined = Vec::new();
    if !mid_only
        && best.is_none()
        && let Some(prev) = prev
    {
        let (dx, dy) = ((lp.x - prev.x).abs(), (lp.y - prev.y).abs());
        // Level with (or plumb with) the previous point, and lined up across with another.
        let (level, plumb) = line_up(&targets, lp, tol);
        if dy < tol && dx > tol {
            best = Some((dy, if plumb.is_some() { "H+V" } else { "H" }, Vec2::new(plumb.as_ref().map_or(g(lp.x), |p| p.at.x), prev.y)));
            lined.extend(plumb);
        } else if dx < tol && dy > tol {
            best = Some((dx, if level.is_some() { "H+V" } else { "V" }, Vec2::new(prev.x, level.as_ref().map_or(g(lp.y), |l| l.at.y))));
            lined.extend(level);
        }
    }
    // Line tool: parallel or perpendicular to a line, or on a line's extension.
    if !mid_only
        && best.is_none()
        && line_tool
        && let Some(prev) = prev
        && let Some(i) = crate::inference::lines(sk, prev, lp, tol)
    {
        best = Some((i.dist, i.label(), i.at));
    }
    // Lined up with other points, on one axis or on both at once (#7828).
    if !mid_only && best.is_none() {
        let (level, plumb) = line_up(&targets, lp, tol);
        let at = Vec2::new(plumb.as_ref().map_or(g(lp.x), |p| p.at.x), level.as_ref().map_or(g(lp.y), |l| l.at.y));
        let off = |l: &Lined| if l.horizontal { (lp.y - l.at.y).abs() } else { (lp.x - l.at.x).abs() };
        let label = match (&level, &plumb) {
            (Some(_), Some(_)) => Some("H+V"),
            (Some(_), None) => Some("H"),
            (None, Some(_)) => Some("V"),
            (None, None) => None,
        };
        if let Some(label) = label {
            best = Some((level.iter().chain(&plumb).map(off).fold(f64::INFINITY, f64::min), label, at));
            lined.extend(level);
            lined.extend(plumb);
        }
    }
    SNAP.with(|s| *s.borrow_mut() = best.map(|(_, l, p)| (l, p)));
    LINED.with(|l| *l.borrow_mut() = best.filter(|_| !lined.is_empty()).map(|(_, _, at)| (at, lined)));
    // A midpoint snap is a point argument the commands understand ("mid:<line>").
    // "x:a+b:x,y" is a crossing of two curves; "on:<curve>:x,y" is a point on one curve.
    best.map(|(_, l, p)| {
        let arg = match l {
            "X" => mid_of.map(|m| format!("x:{m}")),
            "Mid" => mid_of.map(|m| format!("mid:{m}")),
            "On" => mid_of.map(|m| format!("on:{m}")),
            // Already a point argument (`<id>.center` or `on:<id>:x,y`), or none for a free point.
            "Ctr" => mid_of,
            _ => None,
        };
        (p, arg)
    })
}

/// The constraints one glyph stands for. A point that is the midpoint of several lines (their
/// midpoints meet) shows a single MidPoint glyph, the first constraint's, for all of them;
/// any other constraint stands for itself.
pub fn glyph_group(sk: &solidcraft_engine::sketch::Sketch, id: &str) -> Vec<String> {
    use solidcraft_engine::sketch::ConstraintKind::Midpoint;
    match sk.constraints.iter().find(|c| c.id == id).map(|c| (c.kind.clone(), c.driven)) {
        Some((Midpoint { p, .. }, false)) => {
            sk.constraints.iter().filter(|c| !c.driven && matches!(c.kind, Midpoint { p: q, .. } if q == p)).map(|c| c.id.clone()).collect()
        }
        _ => vec![id.to_string()],
    }
}

/// What deleting glyph `id` removes: the constraints it stands for, and, for one MidPoint glyph
/// shared by several lines, their common point once nothing else holds it (it only tied the
/// midpoints together, so it would be left floating).
pub fn glyph_deletion(sk: &solidcraft_engine::sketch::Sketch, id: &str) -> Vec<solidcraft_engine::Sel> {
    use solidcraft_engine::{Sel, sketch::ConstraintKind::Midpoint};
    let group = glyph_group(sk, id);
    let shared = match sk.constraints.iter().find(|c| c.id == id).map(|c| &c.kind) {
        Some(Midpoint { p, .. }) if group.len() > 1 => Some(*p),
        _ => None,
    };
    let orphan = shared.filter(|&p| {
        !sk.curves.iter().any(|c| c.kind.uses(p))
            && !sk.constraints.iter().any(|c| !group.contains(&c.id) && c.kind.indices().0.contains(&p))
            && sk.points.get(p).is_some_and(|q| !q.fixed && q.link.is_none() && q.exprs.is_none())
    });
    let mut out: Vec<Sel> = group.into_iter().map(|id| Sel::SketchConstraint { id }).collect();
    if let Some(q) = orphan.and_then(|p| sk.points.get(p)) {
        out.push(Sel::SketchPoint { id: q.id.clone() });
    }
    out
}

/// [`glyph_deletion`] in the active sketch.
pub fn active_glyph_deletion(app: &SolveApp, id: &str) -> Vec<solidcraft_engine::Sel> {
    let st = app.session.world_state();
    match active_sketch(app).and_then(|s| st.sketch(s)) {
        Some(ss) => glyph_deletion(&ss.sketch, id),
        None => vec![solidcraft_engine::Sel::SketchConstraint { id: id.to_string() }],
    }
}

/// The constraint glyph under a screen position (its constraint id).
pub fn glyph_at(pos: Pos2) -> Option<String> {
    GLYPHS.with(|g| g.borrow().iter().find(|(_, c)| c.distance(pos) <= GLYPH * 0.6).map(|(id, _)| id.clone()))
}

/// Constraint glyphs drawn last frame: (constraint id, screen centre).
pub fn glyph_positions() -> Vec<(String, Pos2)> {
    GLYPHS.with(|g| g.borrow().clone())
}

/// Delete the selected constraint glyph (true when one was selected).
pub fn delete_glyph(app: &mut SolveApp) -> bool {
    let Some(id) = PICKED.with(|p| p.borrow_mut().take()) else { return false };
    let entities: Vec<String> = active_glyph_deletion(app, &id)
        .into_iter()
        .filter_map(|s| match s {
            solidcraft_engine::Sel::SketchConstraint { id } | solidcraft_engine::Sel::SketchPoint { id } => Some(id),
            _ => None,
        })
        .collect();
    let _ = app.run("sketch.delete", json!({ "entities": entities }));
    true
}

/// What the extra tools do with clicks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Construction,
    /// Each click on model geometry runs the command with that reference.
    ModelRef,
    /// Click a curve of the active sketch where it should change.
    CurveAt,
    /// Click points; `n` complete the command (0: until right-click).
    Points(usize),
    /// Pick sketch entities; `n` run the command (0: until right-click).
    Entities(usize),
    /// Model faces / sketch curves in steps (see the command).
    Steps,
}

/// Whether a click with tool `id` acts on `hit`: what is under the pointer lights up only when
/// the tool takes it, so the hover shows what a click does and lights nothing over a view the
/// tool draws (Continuity Analysis's seam colours on a face the tool ignores).
pub fn takes(id: &str, hit: &Hit) -> bool {
    match id {
        "inspect.continuity" => matches!(hit, Hit::Edge { .. }),
        _ => true,
    }
}

fn mode(id: &str) -> Option<Mode> {
    if solidcraft_engine::cmd::construction_line_mode(id).is_some() {
        return Some(Mode::Construction);
    }
    Some(match id {
        "sketch.project" | "sketch.intersect" | "sketch.include_3d" | "sketch.fit_curves_to_section" | "sketch.isoparametric_curve" => Mode::ModelRef,
        "inspect.curvature_comb" | "canvas.insert" | "canvas.decal" => Mode::ModelRef,
        "inspect.minimum_radius" | "inspect.center_of_mass" | "inspect.isocurve" | "inspect.continuity" => Mode::ModelRef,
        "sketch.trim" | "sketch.extend" | "sketch.break" => Mode::CurveAt,
        "sketch.line.midpoint" => Mode::Points(2),
        "sketch.arc.tangent" => Mode::Points(2),
        "sketch.ellipse" | "sketch.conic" | "sketch.slot.center_point" => Mode::Points(3),
        "sketch.slot.arc_three_point" | "sketch.slot.arc_center" => Mode::Points(4),
        "sketch.spline.fit_point" | "sketch.spline.control_point" | "sketch.spline.control_point_5" => Mode::Points(0),
        "sketch.text" => Mode::Points(1),
        "sketch.edit_text" => Mode::Entities(1),
        "sketch.blend_curve" => Mode::Points(2),
        "sketch.constraint.curvature" | "sketch.circle.two_tangent" => Mode::Entities(2),
        "sketch.circle.three_tangent" => Mode::Entities(3),
        "sketch.constrainer" | "sketch.constraint.polygon" => Mode::Entities(0),
        "sketch.centerline" => Mode::Entities(1),
        "sketch.project_to_surface" | "sketch.intersection_curve" | "sketch.spun_profile" => Mode::Steps,
        _ => return None,
    })
}

/// A tool for one of the extra sketch commands.
pub fn tool_for(id: &str) -> Option<Tool> {
    mode(id)?;
    let spec = solidcraft_engine::find_command(id)?;
    Some(Tool {
        cmd: spec.id,
        kind: Kind::Ext,
        pts: Vec::new(),
        hover: None,
        picks: Vec::new(),
        point_picks: Vec::new(),
        dims: Vec::new(),
        dim_history: Vec::new(),
        dims_stage: usize::MAX,
        dims_focus: false,
        arc_turn: 0.0,
        lined: Vec::new(),
        chain: Default::default(),
    })
}

/// A successful image placement consumes the selected file and ends its tool.
pub fn image_placement_finished(tool: &Tool) -> bool {
    matches!(tool.cmd, "canvas.insert" | "canvas.decal") && IMAGE.with(|image| image.borrow().is_none())
}

pub fn hint(id: &str) -> Option<String> {
    if solidcraft_engine::cmd::construction_line_mode(id).is_some() {
        return Some(crate::sketch_construction::hint(id));
    }
    Some(
        match id {
            "sketch.project" => "Project: click edges, faces, vertices, other sketches' curves, axes or planes",
            "sketch.intersect" => "Intersect: click faces or edges to cut with the sketch plane",
            "sketch.include_3d" => "Include 3D Geometry: click edges or vertices",
            "sketch.fit_curves_to_section" => "Fit Curves to Mesh Section: click a body",
            "sketch.isoparametric_curve" => "Isoparametric Curve: click a point on a face (Shift: along)",
            "inspect.curvature_comb" => "Curvature comb: click sketch curves or model edges",
            "inspect.minimum_radius" => "Minimum radius: click sketch curves, edges or faces",
            "inspect.center_of_mass" => "Center of mass of all bodies; click a body for its own",
            "inspect.isocurve" => "Isocurves: click faces",
            "inspect.continuity" => "Continuity: click an edge for its numbers (the button again turns the colours off)",
            "canvas.insert" => crate::i18n::tr("canvas.place_hint"),
            "canvas.decal" => crate::i18n::tr("canvas.decal_place_hint"),
            "sketch.trim" => "Trim: click the piece of a curve to remove, or drag across the pieces to remove",
            "sketch.extend" => "Extend: click a curve near the end to extend (Shift: click two curves to extend both to where they meet)",
            "sketch.break" => "Break: click a curve where it should split",
            "sketch.line.midpoint" => "Midpoint line: click the middle, then an end",
            "sketch.arc.tangent" => "Tangent arc: click the end of a line or arc, then the arc's end",
            "sketch.ellipse" => "Ellipse: click the centre, the end of the major axis, then a point on it",
            "sketch.conic" => "Conic: click the start, the end, then the apex; type Rho to shape it (0.5 is a parabola)",
            "sketch.slot.center_point" => "Slot: click the centre, an arc centre, then the width",
            "sketch.slot.arc_three_point" => "Arc slot: click start, a point on the arc, the end, then the width",
            "sketch.slot.arc_center" => "Arc slot: click the centre, the start, the end, then the width",
            "sketch.spline.fit_point" => "Spline: click fit points; click the start to close smoothly, or right-click to finish",
            "sketch.spline.control_point" | "sketch.spline.control_point_5" => "Control point spline: click control points; right-click to finish",
            "sketch.text" => "Text: click where the text starts",
            "sketch.edit_text" => "Edit Text: click a letter interior or outline",
            "sketch.blend_curve" => "Blend curve: click the ends of two curves",
            "sketch.constraint.curvature" => "Curvature: click two curves that share an end",
            "sketch.circle.two_tangent" => "2-tangent circle: click two curves, then enter the radius (the circle goes between the two clicks)",
            "sketch.circle.three_tangent" => "3-tangent circle: click three curves",
            "sketch.constrainer" => "Constrain: click one or two entities; right-click to apply",
            "sketch.constraint.polygon" => "Polygon: click the lines of a closed chain; right-click to apply",
            "sketch.centerline" => "Centerline: click lines",
            "sketch.project_to_surface" => "Project to surface: click curves of other sketches, right-click, then click the face",
            "sketch.intersection_curve" => "Intersection curve: click two faces",
            "sketch.spun_profile" => "Spun profile: click the body, then the axis line",
            _ => return None,
        }
        .to_string(),
    )
}

fn active_sketch(app: &SolveApp) -> Option<u64> {
    app.session.active_sketch
}

/// A model reference for a hit (for Project and friends).
pub(crate) fn model_ref(app: &SolveApp, h: &Hit) -> Option<Value> {
    let active = active_sketch(app);
    Some(match h {
        Hit::Vertex { body, point } => json!({"vertex": [point.x, point.y, point.z], "body": body}),
        Hit::Edge { body, mid, .. } => json!({"edge": [mid.x, mid.y, mid.z], "body": body}),
        Hit::Face { body, point, .. } => json!({"face": [point.x, point.y, point.z], "body": body}),
        Hit::SketchCurve { sketch, id, .. } if Some(*sketch) != active => json!({"sketch": sketch, "curve": id}),
        Hit::SketchPoint { sketch, id, .. } if Some(*sketch) != active => json!({"sketch": sketch, "point": id}),
        Hit::Axis { name } => json!({"axis": name}),
        Hit::Plane { name, .. } => json!({"plane": name}),
        _ => return None,
    })
}

/// The active sketch's curve or point under the cursor.
fn sketch_entity(app: &SolveApp, hits: &[Hit]) -> Option<(String, bool)> {
    let active = active_sketch(app)?;
    hits.iter().find_map(|h| match h {
        Hit::SketchPoint { sketch, id, .. } if *sketch == active => Some((id.clone(), true)),
        Hit::SketchCurve { sketch, id, .. } if *sketch == active => Some((id.clone(), false)),
        _ => None,
    })
}

/// The two editable curves that meet at sketch point `pi`, in curve order, which is the order
/// the corner commands resolve them in from a `point` parameter. Construction curves are
/// ignored when more than two meet. Each side must be a line, or an arc for a fillet.
pub(crate) fn corner_curves(sk: &solidcraft_engine::sketch::Sketch, pi: usize, arcs_ok: bool) -> Option<(usize, usize)> {
    use solidcraft_engine::sketch::CurveKind;
    let candidates =
        sk.curves.iter().enumerate().filter(|(ci, c)| !sk.is_linked_curve(*ci) && c.kind.ends().is_some_and(|(a, b)| a == pi || b == pi));
    let mut curves: Vec<_> = candidates.clone().take(3).collect();
    // Center rectangles have construction diagonals at their corners. Match the engine's
    // corner selection: when more than two curves meet, use the profile sides.
    if curves.len() > 2 {
        curves = candidates.filter(|(_, c)| !c.construction).take(3).collect();
    }
    if curves.iter().any(|(_, c)| !matches!(c.kind, CurveKind::Line { .. }) && !(arcs_ok && matches!(c.kind, CurveKind::Arc { .. }))) {
        return None;
    }
    match curves[..] {
        [(a, _), (b, _)] => Some((a, b)),
        _ => None,
    }
}

/// Corner tools accept endpoints shared by two editable curves. Pick among those endpoints
/// directly so the origin, a circle centre or an overlapping loose point cannot hide a corner.
pub(crate) fn corner_point(app: &SolveApp, proj: &Proj, pos: Pos2, arcs_ok: bool) -> Option<(Vec2, String)> {
    let st = app.session.world_state();
    let ss = st.sketch(active_sketch(app)?)?;
    let sk = &ss.sketch;
    sk.points
        .iter()
        .enumerate()
        .filter_map(|(pi, point)| {
            let distance = proj.to_screen(ss.plane.to_world(point.pos))?.distance(pos);
            if distance >= 8.0 {
                return None;
            }
            corner_curves(sk, pi, arcs_ok)?;
            Some((distance, point.pos, point.id.clone()))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, at, id)| (at, id))
}

fn xy(p: Vec2) -> Value {
    json!([p.x, p.y])
}

fn arg(p: &(Vec2, Option<String>)) -> Value {
    match &p.1 {
        Some(id) => json!(id),
        None => xy(p.0),
    }
}

/// Length of a curve (for default sizes).
fn curve_len(app: &SolveApp, id: &str) -> f64 {
    let st = app.session.world_state();
    let Some(ss) = active_sketch(app).and_then(|s| st.sketch(s)) else { return 10.0 };
    ss.sketch.curve_index(id).map(|c| ss.sketch.polyline(c).windows(2).map(|w| w[0].dist(w[1])).sum::<f64>()).unwrap_or(10.0)
}

fn round_nice(x: f64) -> f64 {
    if !(x.is_finite() && x > 0.0) {
        return 1.0;
    }
    let p = 10f64.powf(x.log10().floor());
    let m = x / p;
    let n = if m < 1.5 {
        1.0
    } else if m < 3.5 {
        2.0
    } else if m < 7.5 {
        5.0
    } else {
        10.0
    };
    n * p
}

/// The radius offered after the two curves of a tangent circle have been picked.
pub(crate) fn two_tangent_default_radius(app: &SolveApp, tool: &Tool) -> f64 {
    match tool.picks.as_slice() {
        [a, b] => round_nice(curve_len(app, a).min(curve_len(app, b)) * 0.1),
        _ => 1.0,
    }
}

/// A click with an extra tool.
pub fn periodic_start_snap(app: &SolveApp, tool: &Tool, proj: &Proj, pos: Pos2) -> Option<Vec2> {
    if tool.cmd != "sketch.spline.fit_point" || tool.pts.len() < 3 || crate::drag_snap::suppressed() {
        return None;
    }
    let first = tool.pts.first()?.0;
    let mut distinct = Vec::new();
    for p in &tool.pts {
        if !distinct.contains(&p.0) {
            distinct.push(p.0);
        }
        if distinct.len() == 3 {
            break;
        }
    }
    if distinct.len() < 3 {
        return None;
    }
    let state = app.session.world_state();
    let plane = &state.sketch(app.session.active_sketch?)?.plane;
    let screen = proj.to_screen(plane.to_world(first))?;
    (screen.distance(pos) <= 8.0).then_some(first)
}

pub fn on_click(app: &mut SolveApp, tool: &mut Tool, proj: &Proj, pos: Pos2) {
    let Some(m) = mode(tool.cmd) else { return };
    let hits = pick(app, proj, pos);
    let at = sketch_point_at(app, proj, pos);
    let cmd = tool.cmd;
    match m {
        Mode::Construction => crate::sketch_construction::on_click(app, tool, &hits, at),
        Mode::ModelRef if cmd == "inspect.curvature_comb" => {
            let params = hits.iter().find_map(|h| match h {
                Hit::SketchCurve { sketch, id, .. } => Some(json!({"sketch": sketch, "curves": [id]})),
                Hit::Edge { mid, .. } => Some(json!({"edges": [[mid.x, mid.y, mid.z]]})),
                _ => None,
            });
            if let Some(p) = params
                && let Ok(v) = app.run(cmd, p)
            {
                for comb in v["combs"].as_array().into_iter().flatten() {
                    let teeth: Vec<[Vec3; 2]> =
                        comb["teeth"].as_array().into_iter().flatten().filter_map(|t| Some([v3(t.get(0)?)?, v3(t.get(1)?)?])).collect();
                    COMBS.with(|c| c.borrow_mut().push(teeth));
                }
            }
        }
        Mode::ModelRef if cmd == "inspect.isocurve" => {
            let face = hits.iter().find_map(|h| if let Hit::Face { point, .. } = h { Some(*point) } else { None });
            if let Some(at) = face
                && let Ok(v) = app.run(cmd, json!({"faces": [[at.x, at.y, at.z]]}))
            {
                for c in v["curves"].as_array().into_iter().flatten() {
                    let pts: Vec<Vec3> = c.as_array().into_iter().flatten().filter_map(v3).collect();
                    let teeth: Vec<[Vec3; 2]> = pts.windows(2).map(|w| [w[0], w[1]]).collect();
                    ISO.with(|i| i.borrow_mut().push(teeth));
                }
            }
        }
        Mode::ModelRef if cmd == "inspect.continuity" => {
            // The edge measured again, added to the seams shown, and its numbers pinned there.
            let edge = hits.iter().filter(|h| takes(cmd, h)).find_map(|h| if let Hit::Edge { mid, .. } = h { Some(*mid) } else { None });
            if let Some(mid) = edge
                && let Ok(v) = app.run(cmd, json!({"edges": [[mid.x, mid.y, mid.z]], "add": true}))
                && let Some(seam) = v["bodies"][0]["edges"].get(0)
            {
                let label = continuity_label(app, seam);
                MARKS.with(|m| m.borrow_mut().push((mid, label)));
            }
        }
        Mode::ModelRef if matches!(cmd, "inspect.minimum_radius" | "inspect.center_of_mass") => {
            let params = hits.iter().find_map(|h| match (cmd, h) {
                ("inspect.minimum_radius", Hit::SketchCurve { sketch, id, .. }) => Some(json!({"sketch": sketch, "curves": [id]})),
                ("inspect.minimum_radius", Hit::Edge { mid, .. }) => Some(json!({"edges": [[mid.x, mid.y, mid.z]]})),
                ("inspect.minimum_radius", Hit::Face { point, .. }) => Some(json!({"faces": [[point.x, point.y, point.z]]})),
                ("inspect.center_of_mass", Hit::Face { body, .. }) => Some(json!({"bodies": [body]})),
                _ => None,
            });
            if let Some(p) = params {
                analysis(app, cmd, p);
            }
        }
        Mode::ModelRef if matches!(cmd, "canvas.insert" | "canvas.decal") => {
            let Some(path) = IMAGE.with(|i| i.borrow().clone()) else { return };
            let decal = cmd == "canvas.decal";
            // A plane the view shows edge-on can't show the image: at the origin its square is
            // under the cursor too, and the image went onto it out of sight (#4717).
            let (_, ray) = proj.ray(pos);
            let seen = |name: &str| crate::dialogs::plane_by_name(&app.session, name).is_some_and(|pl| pl.normal().dot(ray).abs() > 0.2 * ray.len());
            let target = hits.iter().find_map(|h| match h {
                // The picked body and face, as Create Sketch sends them: a point alone makes the
                // engine search every face of every body, which gives up on a large design.
                Hit::Face { body, index, point } => {
                    let face = solidcraft_engine::Sel::Face { body: body.clone(), index: *index, point: *point };
                    Some((
                        crate::sketch_attachment_ui::face_plane(app, &face).unwrap_or_else(|| json!({"face": [point.x, point.y, point.z]})),
                        *point,
                    ))
                }
                Hit::Plane { name, point } if !decal && seen(name) => Some((json!(name), *point)),
                _ => None,
            });
            // Empty space while sketching: the sketch plane.
            let target = target.or_else(|| {
                let st = app.session.world_state();
                let ss = st.sketch(app.session.active_sketch?)?;
                let (o, d) = proj.ray(pos);
                let w = ss.plane.intersect_ray(o, d)?;
                let pl = &ss.plane;
                Some((
                    json!({"origin": [pl.origin.x, pl.origin.y, pl.origin.z], "x_dir": [pl.x.x, pl.x.y, pl.x.z], "y_dir": [pl.y.x, pl.y.y, pl.y.z]}),
                    w,
                ))
            });
            let Some((plane, at)) = target else { return };
            let p = if decal {
                json!({"path": path, "face": if plane.get("face").is_some() { plane } else { Value::Null }})
            } else {
                json!({"path": path, "plane": plane, "at": [at.x, at.y, at.z]})
            };
            if let Ok(v) = app.run(cmd, p) {
                IMAGE.with(|i| *i.borrow_mut() = None);
                app.tool = None;
                if let Some(id) = v["canvas"].as_u64() {
                    app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(app, id, false));
                }
            }
        }
        Mode::ModelRef => {
            let r = hits.iter().find_map(|h| model_ref(app, h));
            let Some(r) = r else { return };
            match cmd {
                "sketch.fit_curves_to_section" => {
                    if let Some(b) = r.get("body").and_then(Value::as_str) {
                        let _ = app.run(cmd, json!({"body": b}));
                    }
                }
                "sketch.isoparametric_curve" => {
                    if r.get("face").is_some() {
                        let along = SHIFT.with(Cell::get);
                        let _ = app.run(cmd, json!({"face": r["face"], "body": r["body"], "direction": if along { "v" } else { "u" }}));
                    }
                }
                _ => {
                    let _ = app.run(cmd, json!({"refs": [r]}));
                }
            }
        }
        Mode::CurveAt => {
            let curve = active_sketch(app).and_then(|active| {
                hits.iter().find_map(|hit| match hit {
                    Hit::SketchCurve { sketch, id, .. } if *sketch == active => Some(id.clone()),
                    _ => None,
                })
            });
            if let (Some(id), Some((p, _))) = (curve, at) {
                if cmd == "sketch.extend" && EXTEND_PAIR.with(Cell::get) {
                    // The pair chord picks the two curves to extend to where they meet: the first
                    // click remembers its curve and point (in point_picks, which nothing draws for
                    // Extend), the second runs the command with both, each growing from the end
                    // nearest the point clicked on it.
                    match tool.picks.first().cloned() {
                        None => {
                            tool.picks.push(id);
                            tool.point_picks.push(xy(p));
                            app.set_status("Extend: Shift-click the second curve", false);
                        }
                        Some(first) if first != id => {
                            let first_pt = tool.point_picks.first().cloned();
                            tool.picks.clear();
                            tool.point_picks.clear();
                            let mut params = json!({"curves": [first, id]});
                            if let Some(first_pt) = first_pt {
                                params["at"] = json!([first_pt, xy(p)]);
                            }
                            let _ = app.run(cmd, params);
                        }
                        Some(_) => {}
                    }
                } else {
                    tool.picks.clear();
                    tool.point_picks.clear();
                    let _ = app.run(cmd, json!({"curve": id, "at": xy(p)}));
                }
            }
        }
        Mode::Points(_) => {
            if periodic_start_snap(app, tool, proj, pos).is_some() {
                // Only this deliberate click selects periodic interpolation. A
                // right-click finish and all previously saved splines stay natural.
                let params = json!({"points":tool.pts.iter().map(arg).collect::<Vec<_>>(),"periodic":true});
                if app.run(cmd, params).is_ok() {
                    tool.pts.clear();
                    crate::sketch_dims::reset(tool);
                }
                return;
            }
            let Some(p) = at else { return };
            place_point(app, tool, p);
        }
        Mode::Entities(n) => {
            if cmd == "sketch.edit_text" {
                let selection = hits.first().and_then(|hit| match hit {
                    Hit::SketchCurve { sketch, id, .. } if Some(*sketch) == active_sketch(app) => {
                        Some(solidcraft_engine::Sel::SketchCurve { id: id.clone(), sketch: None })
                    }
                    Hit::Profile { sketch, index } => Some(solidcraft_engine::Sel::Profile { sketch: *sketch, index: *index }),
                    _ => None,
                });
                if let Some(id) = selection.as_ref().and_then(|sel| crate::sketch_text::entity_for_selection(app, sel)) {
                    crate::sketch_text::open(app, &id);
                }
                return;
            }
            let Some((id, _)) = sketch_entity(app, &hits) else { return };
            if cmd == "sketch.centerline" {
                let _ = app.run(cmd, json!({"curves": [id]}));
                return;
            }
            // Once both tangent curves are picked, the Radius box owns the interaction until
            // Enter commits it or Escape cancels it. A click behind the box must not replace or
            // append another curve.
            if cmd == "sketch.circle.two_tangent" && tool.picks.len() >= 2 {
                return;
            }
            if !tool.picks.contains(&id) {
                tool.picks.push(id);
                if let Some((p, _)) = at {
                    tool.pts.push((p, None));
                }
            }
            if n > 0 && tool.picks.len() >= n && cmd != "sketch.circle.two_tangent" {
                run_entities(app, tool);
                tool.picks.clear();
                tool.pts.clear();
            }
        }
        Mode::Steps => match cmd {
            "sketch.intersection_curve" => {
                if let Some(r) = hits.iter().find_map(|h| model_ref(app, h)).filter(|r| r.get("face").is_some()) {
                    tool.picks.push(r.to_string());
                    if tool.picks.len() >= 2 {
                        let a: Value = serde_json::from_str(&tool.picks[0]).unwrap_or(Value::Null);
                        let b: Value = serde_json::from_str(&tool.picks[1]).unwrap_or(Value::Null);
                        let _ = app.run(cmd, json!({"a": a, "b": b}));
                        tool.picks.clear();
                    }
                }
            }
            "sketch.spun_profile" => {
                if tool.picks.is_empty() {
                    if let Some(b) =
                        hits.iter().find_map(|h| model_ref(app, h)).and_then(|r| r.get("body").and_then(Value::as_str).map(str::to_string))
                    {
                        tool.picks.push(b);
                    }
                } else if let Some((line, false)) = sketch_entity(app, &hits) {
                    let _ = app.run(cmd, json!({"body": tool.picks[0], "axis": line}));
                    tool.picks.clear();
                }
            }
            _ => {
                // sketch.project_to_surface: curves of other sketches, then (after a right-click) a face.
                let staged = tool.picks.last().is_some_and(|x| x == "|");
                if staged {
                    if let Some(r) = hits.iter().find_map(|h| model_ref(app, h)).filter(|r| r.get("face").is_some()) {
                        let curves: Vec<Value> = tool.picks.iter().filter(|x| *x != "|").filter_map(|x| serde_json::from_str(x).ok()).collect();
                        let _ = app.run(cmd, json!({"curves": curves, "face": r["face"], "body": r["body"]}));
                        tool.picks.clear();
                    }
                } else if let Some(r) = hits.iter().find_map(|h| model_ref(app, h)).filter(|r| r.get("curve").is_some()) {
                    tool.picks.push(r.to_string());
                }
            }
        },
    }
}

/// Put down the next point of a point tool, or confirm an extra tool's value box with Enter.
/// Typed values are checked first, so a rejected one stays in its box to be corrected rather
/// than reaching the command.
pub(crate) fn place_point(app: &mut SolveApp, tool: &mut Tool, p: (Vec2, Option<String>)) {
    if solidcraft_engine::cmd::construction_line_mode(tool.cmd).is_some() {
        crate::sketch_construction::place_point(app, tool, p);
        return;
    }
    if tool.cmd == "sketch.circle.two_tangent" && tool.picks.len() == 2 {
        if let Err(error) = crate::sketch_dims::validate(app, tool) {
            app.set_status(error, true);
            return;
        }
        if run_entities(app, tool) {
            tool.picks.clear();
            tool.pts.clear();
            crate::sketch_dims::reset(tool);
        }
        return;
    }
    if crate::sketch_line_arc::in_arc_mode(tool) {
        crate::sketch_line_arc::place_arc(app, tool, &p);
        return;
    }
    let Some(Mode::Points(n)) = mode(tool.cmd) else { return };
    if let Err(error) = crate::sketch_dims::validate(app, tool) {
        app.set_status(error, true);
        return;
    }
    let p = crate::sketch_dims::placed(app, tool, &p);
    tool.pts.push(p);
    if n > 0 && tool.pts.len() >= n {
        if run_points(app, tool) {
            tool.pts.clear();
            crate::sketch_dims::reset(tool);
        } else {
            tool.pts.pop();
        }
    }
}

fn run_points(app: &mut SolveApp, t: &Tool) -> bool {
    let p = &t.pts;
    let width = |a: Vec2, b: Vec2, c: Vec2| -> f64 { 2.0 * (b - a).normalized().map(|d| d.cross(c - a).abs()).unwrap_or(1.0) };
    let slot_width = |fallback: f64| crate::sketch_dims::locked_value(app, t, "Width").unwrap_or_else(|| fallback.max(1e-3));
    let params = match (t.cmd, p.as_slice()) {
        ("sketch.line.midpoint", [m, e]) => json!({"mid": arg(m), "end": xy(e.0)}),
        ("sketch.arc.tangent", [s, e]) => json!({"start": arg(s), "end": xy(e.0)}),
        ("sketch.ellipse", [c, m, q]) => json!({"center": xy(c.0), "major": xy(m.0), "minor": xy(q.0), "axes": app.preferences.ellipse_axes}),
        ("sketch.conic", [a, b, x]) => {
            // The typed Rho box, or the parabola the command defaults to.
            let rho = crate::sketch_dims::locked_value(app, t, "Rho").unwrap_or(0.5);
            json!({"start": arg(a), "end": arg(b), "apex": arg(x), "rho": rho})
        }
        ("sketch.slot.center_point", [c, e, w]) => json!({"center": xy(c.0), "end": xy(e.0), "width": slot_width(width(c.0, e.0, w.0))}),
        ("sketch.slot.arc_three_point", [a, th, b, w]) => {
            json!({"start": xy(a.0), "through": xy(th.0), "end": xy(b.0), "width": slot_width(2.0 * w.0.dist(b.0))})
        }
        ("sketch.slot.arc_center", [c, a, b, w]) => {
            let r = c.0.dist(a.0);
            json!({"center": xy(c.0), "start": xy(a.0), "end": xy(b.0), "width": slot_width(2.0 * (w.0.dist(c.0) - r).abs())})
        }
        ("sketch.spline.fit_point" | "sketch.spline.control_point" | "sketch.spline.control_point_5", pts) if pts.len() >= 2 => {
            json!({"points": pts.iter().map(arg).collect::<Vec<_>>()})
        }
        ("sketch.blend_curve", [a, b]) => match (&a.1, &b.1) {
            (Some(x), Some(y)) => json!({"a": x, "b": y}),
            _ => return false,
        },
        ("sketch.text", [at]) => {
            crate::sketch_text::place(app, at.0, round_nice(app.cam.half_height() * 0.1));
            return true;
        }
        _ => return false,
    };
    crate::tools::draw(app, t, params).is_ok()
}

fn run_entities(app: &mut SolveApp, t: &Tool) -> bool {
    let k = &t.picks;
    let params = match t.cmd {
        "sketch.constraint.curvature" => json!({"a": k[0], "b": k[1]}),
        "sketch.circle.two_tangent" => {
            let r = crate::sketch_dims::locked_expression(t, "Radius")
                .map_or_else(|| json!(two_tangent_default_radius(app, t)), |expression| json!(expression));
            // Between the two clicks: each lies on its curve, so the circle in the corner they
            // span is the one nearest their midpoint (#3141).
            let near = xy(t.pts.iter().fold(Vec2::ZERO, |a, p| a + p.0) / t.pts.len().max(1) as f64);
            json!({"curves": k, "radius": r, "near": near})
        }
        "sketch.circle.three_tangent" => {
            // Start inside the three picks.
            let c = t.pts.iter().fold(Vec2::ZERO, |a, p| a + p.0) / t.pts.len().max(1) as f64;
            json!({"curves": k, "near": xy(c)})
        }
        "sketch.constrainer" => json!({"entities": k}),
        "sketch.constraint.polygon" => json!({"lines": k}),
        _ => return false,
    };
    app.run(t.cmd, params).is_ok()
}

/// Right-click with an extra tool: finish a chain or move to the next stage. Returns false
/// when the tool should be dropped.
pub fn finish(app: &mut SolveApp, tool: &mut Tool) -> bool {
    let Some(m) = mode(tool.cmd) else { return false };
    match m {
        Mode::Points(0) if tool.pts.len() >= 2 => {
            run_points(app, tool);
            tool.pts.clear();
            true
        }
        Mode::Entities(0) if !tool.picks.is_empty() => {
            run_entities(app, tool);
            tool.picks.clear();
            tool.pts.clear();
            true
        }
        Mode::Steps if !tool.picks.is_empty() && tool.picks.last().is_some_and(|x| x != "|") => {
            tool.picks.push("|".into());
            true
        }
        _ if !tool.pts.is_empty() || !tool.picks.is_empty() => {
            tool.pts.clear();
            tool.picks.clear();
            true
        }
        _ => false,
    }
}

/// The cursor's Trim piece, keyed by design revision, sketch, curve and cursor: the viewport's
/// hover and the preview both ask for it each frame, and it intersects the curve with every other.
type TrimPieceKey = (u64, u64, String, [u64; 2]);

thread_local! {
    static TRIM_PIECE: std::cell::RefCell<Option<(TrimPieceKey, Option<Vec<Vec2>>)>> = const { std::cell::RefCell::new(None) };
}

/// With Trim active and the cursor over curve `id` of `sketch` (the active sketch): the piece a
/// click takes away (#3052), which the hover shows in place of the whole curve. `None` with
/// another tool, or for a curve trim cuts otherwise (`solidcraft_engine::cmd::trim_piece`).
pub(crate) fn trim_hover_piece(app: &SolveApp, sketch: u64, id: &str) -> Option<Vec<Vec2>> {
    let tool = app.tool.as_ref().filter(|t| t.cmd == "sketch.trim")?;
    let (at, _) = tool.hover.as_ref()?;
    if active_sketch(app) != Some(sketch) {
        return None;
    }
    let key = (app.session.revision, sketch, id.to_string(), [at.x.to_bits(), at.y.to_bits()]);
    if let Some(piece) = TRIM_PIECE.with(|c| c.borrow().as_ref().filter(|(k, _)| *k == key).map(|(_, p)| p.clone())) {
        return piece;
    }
    let st = app.session.world_state();
    let piece = solidcraft_engine::cmd::trim_piece(&st.sketch(sketch)?.sketch, id, *at);
    TRIM_PIECE.with(|c| *c.borrow_mut() = Some((key, piece.clone())));
    piece
}

/// Extend preview: the geometry that would be added, based on the hover position over curve `id`
/// of `sketch`. Returns None when extend is not active or the geometry does not support a
/// preview. An arc grows along its own circle, in either direction, as a new arc.
fn extend_preview_piece(app: &SolveApp, sketch: u64, id: &str) -> Option<Vec<Vec2>> {
    // A pending pair pick (the first curve of the two-curve form) hides the single-curve preview.
    let tool = app.tool.as_ref().filter(|t| t.cmd == "sketch.extend" && t.picks.is_empty())?;
    let hover = tool.hover.as_ref()?;
    if active_sketch(app) != Some(sketch) {
        return None;
    }
    let st = app.session.world_state();
    let sk = st.sketch(sketch)?;
    let hover_pos = hover.0;
    let ci = sk.sketch.curve_index(id)?;
    let kind = sk.sketch.curves.get(ci)?.kind.clone();
    // Arcs extend along their circle either way round, up to the first geometry in the way.
    if let solidcraft_engine::sketch::CurveKind::Arc { .. } = kind {
        return solidcraft_engine::cmd::extend_arc_preview(&sk.sketch, ci, hover_pos);
    }
    let solidcraft_engine::sketch::CurveKind::Line { a, b } = kind else { return None };
    let at_b = hover_pos.dist(sk.sketch.point(b)?) <= hover_pos.dist(sk.sketch.point(a)?);
    let end = sk.sketch.point(if at_b { b } else { a })?;
    let hit = solidcraft_engine::cmd::extend_preview(&sk.sketch, ci, hover_pos)?;
    Some(vec![end, hit])
}

/// Rubber band for point tools and pick markers.
pub fn preview(app: &SolveApp, t: &Tool, painter: &egui::Painter, proj: &Proj) {
    if solidcraft_engine::cmd::construction_line_mode(t.cmd).is_some() {
        crate::sketch_construction::preview(app, t, painter, proj);
        return;
    }
    let Some(sid) = active_sketch(app) else { return };
    let st = app.session.world_state();
    let Some(ss) = st.sketch(sid) else { return };
    let to = |p: Vec2| proj.to_screen(ss.plane.to_world(p));
    let tk = crate::theme::Tokens::get();
    if let Some(crate::viewport::Hit::SketchCurve { sketch, id, .. }) = &app.viewport.hover
        && let Some(piece) = trim_hover_piece(app, *sketch, id)
    {
        let world: Vec<_> = piece.iter().map(|p| ss.plane.to_world(*p)).collect();
        // In the error colour, thicker than a hover: it is what the click deletes.
        crate::tools::paint_sketch_path(&world, 0, painter, proj, Stroke::new(4.5, tk.error));
    }
    // Extend preview: the line or arc that would be added (or the end that would move).
    if let Some(crate::viewport::Hit::SketchCurve { sketch, id, .. }) = &app.viewport.hover
        && let Some(preview_line) = extend_preview_piece(app, *sketch, id)
    {
        let world: Vec<_> = preview_line.iter().map(|p| ss.plane.to_world(*p)).collect();
        // Draw the preview extension line using preview_new color (same as fillet/chamfer preview)
        crate::tools::paint_sketch_path(&world, 0, painter, proj, Stroke::new(2.0, tk.preview_new.gamma_multiply(0.9)));
    }
    let stroke = Stroke::new(1.5, tk.sketch_accent);
    // A Trim drag: the pieces it will remove, then the path drawn so far.
    if t.cmd == "sketch.trim" && !t.pts.is_empty() {
        TRIM_REMOVED.with(|r| {
            for piece in r.borrow().iter() {
                let line: Vec<Pos2> = piece.iter().filter_map(|p| to(*p)).collect();
                painter.add(egui::Shape::line(line, Stroke::new(4.0, tk.error.gamma_multiply(0.8))));
            }
        });
        let path: Vec<Pos2> = t.pts.iter().filter_map(|p| to(p.0)).collect();
        painter.add(egui::Shape::line(path, stroke));
        return;
    }
    if let Some((hp, snap)) = &t.hover
        && let Some(sp) = to(*hp)
    {
        painter.circle_stroke(sp, if snap.is_some() { 6.0 } else { 3.0 }, stroke);
    }
    let faint = Stroke::new(1.0, tk.sketch_accent.gamma_multiply(0.6));
    let mode = crate::sketch_palette::mode(app);
    for w in t.pts.windows(2) {
        crate::tools::paint_sketch_path(&[ss.plane.to_world(w[0].0), ss.plane.to_world(w[1].0)], mode, painter, proj, faint);
    }
    // Placing a conic's apex: the curve is the preview, and the two tangent legs it is built
    // from are a faint guide. A full-strength cursor rubber band here read as a straight line
    // being drawn rather than as a curve being shaped.
    let legs = t.cmd == "sketch.conic" && t.pts.len() == 2;
    if let (Some(last), Some((h, _))) = (t.pts.last(), t.hover.as_ref()) {
        let hw = ss.plane.to_world(*h);
        if legs {
            if let Some(first) = t.pts.first() {
                crate::tools::paint_sketch_path(&[ss.plane.to_world(first.0), hw], mode, painter, proj, faint);
            }
            crate::tools::paint_sketch_path(&[ss.plane.to_world(last.0), hw], mode, painter, proj, faint);
        } else if crate::sketch_line_arc::in_arc_mode(t) {
            // The Line tool switched to tangent arcs: the arc a click makes, or (straight ahead,
            // where there is none) only a faint guide.
            match crate::sketch_line_arc::preview_arc(&ss.sketch, t, *h) {
                Some(arc) => crate::tools::paint_preview_curve(arc, &ss.plane, painter, proj, stroke, mode),
                None => crate::tools::paint_sketch_path(&[ss.plane.to_world(last.0), hw], mode, painter, proj, faint),
            }
        } else {
            crate::tools::paint_sketch_path(&[ss.plane.to_world(last.0), hw], mode, painter, proj, stroke);
        }
    }
    if let Some(hover) = t.hover.as_ref()
        && let Some(curve) = crate::tools::preview_curve(
            t,
            crate::sketch_dims::shown(app, t, hover),
            None,
            crate::sketch_dims::locked_value(app, t, "Rho"),
            crate::sketch_dims::locked_value(app, t, "Width"),
        )
    {
        crate::tools::paint_preview_curve(curve, &ss.plane, painter, proj, stroke, crate::sketch_palette::mode(app));
    }
    // Picked curves highlighted.
    let mut display_budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
    for id in t.picks.iter().filter(|x| *x != "|") {
        if let Some(ci) = ss.sketch.curve_index(id) {
            crate::tools::paint_curve(&ss.sketch, ci, &ss.plane, painter, proj, Stroke::new(3.0, tk.accent.gamma_multiply(0.7)), &mut display_budget);
        }
    }
}

/// Constraint glyphs of the active sketch: a small badge per constraint at its anchor
/// (failing ones in `sketch_conflict`).
/// Glyph size and spacing (screen pixels).
const GLYPH: f32 = 15.0;

/// Screen pixels per millimetre of the sketch plane below which constraint glyphs hide when the
/// `hide_constraint_glyphs_zoomed_out` preference is on (#8980). About two pixels per millimetre:
/// a 10 mm feature is then smaller than a glyph.
pub const MIN_GLYPH_PX_PER_MM: f32 = 2.0;

/// Whether constraint glyphs should draw at this sketch-plane scale (#8980).
pub fn constraint_glyphs_visible_at_scale(px_per_mm: f32, hide_when_zoomed_out: bool) -> bool {
    !hide_when_zoomed_out || px_per_mm >= MIN_GLYPH_PX_PER_MM
}

/// Pixels on screen for one millimetre along the sketch plane's U axis, near the plane origin.
fn sketch_px_per_mm(plane: &Plane, proj: &Proj) -> Option<f32> {
    let a = proj.to_screen(plane.to_world(Vec2::ZERO))?;
    let b = proj.to_screen(plane.to_world(Vec2::new(1.0, 0.0)))?;
    Some(a.distance(b))
}

fn glyphs(app: &SolveApp, painter: &egui::Painter, proj: &Proj, hover: Option<Pos2>) {
    let Some(sid) = active_sketch(app) else { return };
    let st = app.session.world_state();
    let Some(ss) = st.sketch(sid) else { return };
    if ss.sketch.view.hide_constraints {
        GLYPHS.with(|g| g.borrow_mut().clear());
        return;
    }
    // Zoomed out far enough: fixed-size glyphs would dominate the sketch (#8980).
    if let Some(scale) = sketch_px_per_mm(&ss.plane, proj)
        && !constraint_glyphs_visible_at_scale(scale, app.preferences.hide_constraint_glyphs_zoomed_out)
    {
        GLYPHS.with(|g| g.borrow_mut().clear());
        return;
    }
    let tk = crate::theme::Tokens::get();
    let picked = PICKED.with(|p| p.borrow().clone());
    // Where each glyph goes: beside its entity (to its left, away from the line), every entity
    // of a two-entity constraint getting one; glyphs that would overlap step along. A glyph
    // that would sit on a dimension's value goes to the other side of its line.
    let dim_texts: Vec<egui::Rect> = crate::dim_view::text_rects().into_iter().map(|(_, r)| r.expand(2.0)).collect();
    let on_dim = |p: Pos2| {
        let r = egui::Rect::from_center_size(p, egui::vec2(GLYPH, GLYPH));
        dim_texts.iter().any(|d| d.intersects(r))
    };
    let mut placed: Vec<(String, Pos2, Pos2)> = Vec::new();
    // The point of each MidPoint glyph drawn so far: lines whose midpoints meet share one glyph.
    let mut midpoints: Vec<usize> = Vec::new();
    for c in &ss.sketch.constraints {
        if c.kind.is_dimension() {
            continue;
        }
        if let solidcraft_engine::sketch::ConstraintKind::Midpoint { p, .. } = c.kind
            && !c.driven
        {
            if midpoints.contains(&p) {
                continue;
            }
            midpoints.push(p);
        }
        if matches!(
            c.kind,
            solidcraft_engine::sketch::ConstraintKind::PatternPoint { .. } | solidcraft_engine::sketch::ConstraintKind::PatternRadius { .. }
        ) && !ss.sketch.patterns.iter().any(|p| p.instances.first().and_then(|i| i.constraints.first()) == Some(&c.id))
        {
            continue;
        }
        for (at, dir) in glyph_anchors(&ss.sketch, &c.kind) {
            let Some(base) = proj.to_screen(ss.plane.to_world(at)) else { continue };
            // The entity's direction on screen, and its side.
            let side = match dir.and_then(|d| proj.to_screen(ss.plane.to_world(at + d))) {
                Some(q) => {
                    let t = (q - base).normalized();
                    if t.length() > 0.5 { egui::vec2(-t.y, t.x) } else { egui::vec2(0.7, -0.7) }
                }
                None => egui::vec2(0.7, -0.7),
            };
            let step = if dir.is_some() { egui::vec2(side.y, -side.x) } else { egui::vec2(1.0, 0.0) };
            let free = |sp: Pos2| !placed.iter().any(|(_, q, _)| q.distance(sp) < GLYPH + 1.0);
            let start = |s: egui::Vec2| base + s * (GLYPH * 0.9);
            let side = if on_dim(start(side)) && !on_dim(start(-side)) { -side } else { side };
            // Step along until clear of other glyphs, preferring a spot clear of dimension
            // values too.
            let settle = |avoid_dims: bool| {
                let mut sp = start(side);
                for _ in 0..12 {
                    if free(sp) && !(avoid_dims && on_dim(sp)) {
                        return Some(sp);
                    }
                    sp += step * (GLYPH + 2.0);
                }
                None
            };
            let sp = settle(true).or_else(|| settle(false)).unwrap_or_else(|| start(side) + step * (12.0 * (GLYPH + 2.0)));
            placed.push((c.id.clone(), sp, base));
        }
    }
    let hovered = hover.and_then(|h| placed.iter().find(|(_, q, _)| q.distance(h) <= GLYPH * 0.6).map(|(id, _, _)| id.clone()));
    // The hovered (or selected) constraint's entities lit up.
    if let Some(id) = hovered.as_ref().or(picked.as_ref()) {
        let group = glyph_group(&ss.sketch, id);
        let (cs, ps) = ss.sketch.constraints.iter().filter(|c| group.contains(&c.id)).map(|c| constraint_entities(&c.kind)).fold(
            (Vec::new(), Vec::new()),
            |(mut cs, mut ps), (c, p)| {
                cs.extend(c);
                ps.extend(p);
                (cs, ps)
            },
        );
        let mut display_budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
        for ci in cs {
            crate::tools::paint_curve(&ss.sketch, ci, &ss.plane, painter, proj, Stroke::new(3.0, tk.accent.gamma_multiply(0.7)), &mut display_budget);
        }
        for pi in ps {
            if let Some(q) = ss.sketch.point(pi).and_then(|q| proj.to_screen(ss.plane.to_world(q))) {
                painter.circle_filled(q, 4.5, tk.accent);
            }
        }
    }
    let mut drawn: Vec<(String, Pos2)> = Vec::new();
    for (id, sp, base) in &placed {
        let Some(c) = ss.sketch.constraints.iter().find(|c| &c.id == id) else { continue };
        let group = glyph_group(&ss.sketch, id);
        let failing = group.iter().any(|g| ss.report.failing.contains(g));
        // Redundant (already implied by the others, but holding): dimmed, not an error.
        let redundant = !failing && group.iter().all(|g| ss.report.redundant.contains(g));
        let selected = picked.as_deref() == Some(id.as_str())
            || app.session.selection.iter().any(|s| matches!(s, solidcraft_engine::Sel::SketchConstraint { id: x } if group.contains(x)));
        let lit = selected || hovered.as_deref() == Some(id.as_str());
        let col = if failing {
            crate::viewport::Conflict::glyph_colour(app, &tk)
        } else if lit {
            tk.accent
        } else if redundant {
            tk.glyph_text.gamma_multiply(0.45)
        } else {
            tk.glyph_text
        };
        // A faint leader back to the entity when the glyph was pushed away from it.
        if sp.distance(*base) > GLYPH * 1.6 {
            painter.line_segment([*base, *sp], Stroke::new(0.8, tk.text_dim.gamma_multiply(0.6)));
        }
        let r = egui::Rect::from_center_size(*sp, egui::vec2(GLYPH, GLYPH));
        painter.rect_filled(r, tk.radius_sm.saturating_sub(1), if lit { tk.accent_soft } else { tk.glyph_bg });
        painter.rect_stroke(
            r,
            tk.radius_sm.saturating_sub(1),
            Stroke::new(1.0, if failing || lit { col } else { tk.glyph_edge }),
            egui::StrokeKind::Inside,
        );
        let pattern_icon = ss.sketch.patterns.iter().find(|p| p.instances.first().and_then(|i| i.constraints.first()) == Some(id)).map(|p| {
            if matches!(p.kind, solidcraft_engine::sketch::SketchPatternKind::Circular { .. }) {
                "sketch_circ_pattern"
            } else {
                "sketch_rect_pattern"
            }
        });
        match pattern_icon.or_else(|| glyph_icon(c.kind.name())) {
            Some(icon) => crate::icons::paint_icon(painter, r.shrink(2.0), icon, f32::from(col.a()) / 255.0),
            None => {
                painter.text(
                    *sp,
                    egui::Align2::CENTER_CENTER,
                    glyph_letter(c.kind.name()),
                    egui::FontId::proportional(crate::theme::Tokens::get().font_panel),
                    col,
                );
            }
        }
        drawn.push((id.clone(), *sp));
    }
    if let (Some(id), Some(h)) = (hovered, hover)
        && let Some(c) = ss.sketch.constraints.iter().find(|c| c.id == id)
    {
        let ids = glyph_group(&ss.sketch, &id).join(", ");
        let label = if ss.report.redundant.contains(&id) {
            format!("{} ({ids}), redundant: already implied by other constraints", c.kind.name())
        } else {
            format!("{} ({ids})", c.kind.name())
        };
        let at = h + egui::vec2(12.0, 12.0);
        let g = painter.layout_no_wrap(label, egui::FontId::proportional(crate::theme::Tokens::get().font_small), tk.text);
        painter.rect_filled(egui::Rect::from_min_size(at, g.size()).expand(3.0), tk.radius_sm.saturating_sub(1), tk.panel);
        painter.galley(at, g, tk.text);
    }
    GLYPHS.with(|g| *g.borrow_mut() = drawn);
}

/// The curves and points a constraint ties together.
fn constraint_entities(k: &solidcraft_engine::sketch::ConstraintKind) -> (Vec<usize>, Vec<usize>) {
    use solidcraft_engine::sketch::ConstraintKind::*;
    match *k {
        Coincident { p, q } | HorizontalPoints { p, q } | VerticalPoints { p, q } => (vec![], vec![p, q]),
        PatternPoint { p, q, center, directions, .. } => {
            (directions.into_iter().flatten().map(|(l, _)| l).collect(), vec![p, q].into_iter().chain(center).collect())
        }
        PatternRadius { a, b } => (vec![a, b], vec![]),
        Fix { p } => (vec![], vec![p]),
        Midpoint { p, l } => (vec![l], vec![p]),
        PointOnCurve { p, c } => (vec![c], vec![p]),
        Symmetric { p, q, l } => (vec![l], vec![p, q]),
        Horizontal { l } | Vertical { l } => (vec![l], vec![]),
        Parallel { a, b }
        | Perpendicular { a, b }
        | Collinear { a, b }
        | Equal { a, b }
        | Concentric { a, b }
        | Tangent { a, b }
        | Smooth { a, b } => (vec![a, b], vec![]),
        _ => (vec![], vec![]),
    }
}

/// Where a constraint's glyphs go: one per entity (the middle of a curve, with its direction
/// there; a point, without).
fn glyph_anchors(sk: &solidcraft_engine::sketch::Sketch, k: &solidcraft_engine::sketch::ConstraintKind) -> Vec<(Vec2, Option<Vec2>)> {
    use solidcraft_engine::sketch::ConstraintKind::*;
    // Half way along the curve, and the direction there.
    let mid = |c: usize| -> Option<(Vec2, Option<Vec2>)> {
        let poly = sk.polyline(c);
        let total: f64 = poly.windows(2).map(|w| w[0].dist(w[1])).sum();
        let mut acc = 0.0;
        for w in poly.windows(2) {
            let l = w[0].dist(w[1]);
            if acc + l >= total * 0.5 && l > 0.0 {
                return Some((w[0].lerp(w[1], (total * 0.5 - acc) / l), (w[1] - w[0]).normalized()));
            }
            acc += l;
        }
        poly.first().map(|p| (*p, None))
    };
    let pt = |p: usize| sk.point(p).map(|q| (q, None));
    match *k {
        Coincident { p, .. } | Fix { p } | Midpoint { p, .. } | PointOnCurve { p, .. } => pt(p).into_iter().collect(),
        PatternPoint { p, center, .. } => pt(center.unwrap_or(p)).into_iter().collect(),
        PatternRadius { .. } => Vec::new(),
        HorizontalPoints { p, q } | VerticalPoints { p, q } | Symmetric { p, q, .. } => {
            sk.point(p).zip(sk.point(q)).map(|(a, b)| ((a + b) * 0.5, None)).into_iter().collect()
        }
        Horizontal { l } | Vertical { l } => mid(l).into_iter().collect(),
        Parallel { a, b }
        | Perpendicular { a, b }
        | Collinear { a, b }
        | Equal { a, b }
        | Concentric { a, b }
        | Tangent { a, b }
        | Smooth { a, b } => [mid(a), mid(b)].into_iter().flatten().collect(),
        _ => Vec::new(),
    }
}

/// The inference label next to the snapped cursor (while a tool is active).
fn snap_hint(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if app.tool.is_none() {
        return;
    }
    let Some((label, p)) = SNAP.with(|s| *s.borrow()) else { return };
    let st = app.session.world_state();
    let Some(ss) = active_sketch(app).and_then(|s| st.sketch(s)) else { return };
    let Some(sp) = proj.to_screen(ss.plane.to_world(p)) else { return };
    let tk = crate::theme::Tokens::get();
    // A dotted guide from each point the snap lined the cursor up with (#7828).
    let guides = LINED.with(|l| l.borrow().as_ref().filter(|(at, _)| at.dist(p) < 1e-9).map(|(_, lined)| lined.clone()).unwrap_or_default());
    for from in guides.iter().filter_map(|l| proj.to_screen(ss.plane.to_world(l.at))) {
        painter.add(egui::Shape::dashed_line(&[from, sp], Stroke::new(1.0, tk.rubber_band), 5.0, 4.0));
    }
    crate::scenario::publish_count("sketch_align_guides", guides.len() as f64);
    let r = egui::Rect::from_min_size(sp + egui::vec2(12.0, -24.0), egui::vec2(24.0, 14.0));
    painter.rect_filled(r, tk.radius_sm.saturating_sub(1), tk.accent_soft);
    painter.text(r.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(crate::theme::Tokens::get().font_micro), tk.text);
}

/// Per frame, after the viewport is drawn: constraint glyphs, snap hints and analysis marks.
pub fn show(app: &mut SolveApp, ui: &egui::Ui, painter: &egui::Painter, proj: &Proj) {
    // IsoCurveAlternate (Shift default) for sketch.isoparametric_curve - via resolve, not raw shift.
    let held = crate::tool_modifiers::Held::from_modifiers(ui.input(|i| i.modifiers));
    let iso = crate::tool_modifiers::resolve(&[], crate::tool_modifiers::descriptor("sketch.isoparametric_curve"), held, Trigger::Press);
    SHIFT.with(|c| c.set(iso.contains(ModAction::IsoCurveAlternate)));
    let pair = crate::tool_modifiers::resolve(&[], crate::tool_modifiers::descriptor("sketch.extend"), held, Trigger::Press);
    EXTEND_PAIR.with(|c| c.set(pair.contains(ModAction::ExtendPair)));
    glyphs(app, painter, proj, ui.input(|i| i.pointer.hover_pos()).filter(|_| ui.rect_contains_pointer(proj.rect)));
    snap_hint(app, painter, proj);
    combs(app, painter, proj);
    if DEFERRED.with(|d| d.replace(false)) {
        analysis(app, "inspect.center_of_mass", json!({}));
    }
    marks(app, painter, proj);
    iso_lines(app, painter, proj);
}

#[cfg(test)]
mod glyph_zoom_tests {
    use super::{MIN_GLYPH_PX_PER_MM, constraint_glyphs_visible_at_scale};

    #[test]
    fn glyphs_stay_visible_when_zoomed_in() {
        assert!(constraint_glyphs_visible_at_scale(MIN_GLYPH_PX_PER_MM, true));
        assert!(constraint_glyphs_visible_at_scale(10.0, true));
    }

    #[test]
    fn glyphs_hide_below_the_zoom_threshold() {
        assert!(!constraint_glyphs_visible_at_scale(MIN_GLYPH_PX_PER_MM - f32::EPSILON, true));
        assert!(!constraint_glyphs_visible_at_scale(0.5, true));
    }

    #[test]
    fn preference_off_keeps_glyphs_at_every_zoom() {
        assert!(constraint_glyphs_visible_at_scale(0.1, false));
        assert!(constraint_glyphs_visible_at_scale(MIN_GLYPH_PX_PER_MM - 1.0, false));
    }
}

#[cfg(test)]
#[path = "sketch_corner_ui_tests.rs"]
mod corner_ui_tests;

#[cfg(test)]
#[path = "sketch_continuity_tests.rs"]
mod continuity_tests;

#[cfg(test)]
#[path = "sketch_trim_drag_ui_tests.rs"]
mod trim_drag_ui_tests;
