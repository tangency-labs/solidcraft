//! More interactive sketch tools: projection and other model-pick tools, curve tools (trim,
//! extend, break, offset, fillet, chamfer), multi-pick tools (mirror, patterns, polygon), point
//! tools for free-form curves and slots, text placement, and constraint glyphs. Like `tools`,
//! these are UI state only: every action runs a command.

use crate::i18n::tr_engine_error;
use crate::icons::markers::{glyph_icon, snap_icon};
use crate::ui_kit::BackingTooltipExt as _;
use solidcraft_engine::geom::prelude::*;
use solidcraft_ui_text::tr;
use std::cell::{Cell, RefCell};

use egui::{Pos2, Stroke};
use serde_json::{Value, json};
use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_engine::kernel::Continuity;

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
    ToolModifiers { tool: "sketch.repair_link", scopes: NONE, bindings: &[], audit: Audit::VerifiedNoOp },
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
    /// The side of the glyphs drawn this frame (screen pixels), for [`glyph_at`] (#10794).
    static GLYPH_PX: Cell<f32> = const { Cell::new(GLYPH) };
    static PICKED: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Simulated break-off Coincident glyph shown when hovering a shared endpoint of a selected curve.
    static DETACH_PREVIEW: RefCell<Option<DetachPreview>> = const { RefCell::new(None) };
    /// Active automatic constraint/snap badges and target entities at the cursor.
    static SNAP: RefCell<Option<SnapState>> = const { RefCell::new(None) };
    /// The comb tool's options: comb length (a multiplier on the automatic length) and teeth.
    static COMB_OPTIONS: Cell<CombOptions> = const { Cell::new(CombOptions::DEFAULT) };
    /// The points that snap lined the cursor up with, and where it went (#7828).
    static LINED: RefCell<Option<(Vec2, Vec<Lined>)>> = const { RefCell::new(None) };
    /// The drawing tool's points so far, whether it is the Line tool, and whether it is the
    /// 3-Point Arc waiting for its third point, from the last hover: a click takes the tool out of
    /// the app while it runs, and must place what the hover showed.
    static TOOL: RefCell<(Vec<Vec2>, bool, bool)> = const { RefCell::new((Vec::new(), true, false)) };
    /// The image file a Canvas or Decal tool places.
    static IMAGE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// The link the Repair Link tool gives a new source.
    static REPAIR: RefCell<Option<String>> = const { RefCell::new(None) };
    /// The tool finished its job with this click and ends.
    static ENDED: Cell<bool> = const { Cell::new(false) };
    /// Analysis markers shown while an analysis tool is active: (where, label).
    static MARKS: RefCell<Vec<(Vec3, String)>> = const { RefCell::new(Vec::new()) };
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
    let st = crate::open_meshing::session_world(&app.session);
    let ss = st.sketch(active_sketch(app)?)?;
    let (o, d) = proj.ray(pos);
    ss.plane.intersect_ray(o, d).map(|w| ss.plane.to_local(w))
}

/// A piece is selected on the side of its boundary under the cursor. Drawing snaps can move
/// a click across that boundary, so both hover and click use the pointer's plane position.
pub(super) fn tool_point_at(app: &SolveApp, proj: &Proj, pos: Pos2, cmd: Option<&str>) -> Option<(Vec2, Option<String>)> {
    if matches!(cmd, Some("sketch.break" | "sketch.trim")) {
        on_sketch_plane(app, proj, pos).map(|p| (p, None))
    } else {
        sketch_point_at(app, proj, pos)
    }
}

/// Clicking the selected curve's endpoint with Break can detach it from its neighbour.
fn picked_shared_end(app: &SolveApp, curve: &str, hits: &[Hit]) -> Option<Vec2> {
    let sid = active_sketch(app)?;
    let at = hits.iter().find_map(|hit| match hit {
        Hit::SketchPoint { sketch, at, .. } if *sketch == sid => Some(*at),
        _ => None,
    })?;
    let solve = solved_sketch(app, sid)?;
    let sk = &solve.sketch;
    solidcraft_engine::cmd::break_shared_end(sk, sk.curve_index(curve)?, at).map(|_| at)
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
        "inspect.center_of_mass" => v3(&v["center"]).map(|c| (c, center_label(app, c))),
        _ => v3(&v["at"]).zip(v["min_radius"].as_f64()).map(|(c, r)| (c, format!("R min {}", crate::prefs::show_mm(app, r, 1)))),
    };
    if let Some(m) = mark {
        MARKS.with(|k| k.borrow_mut().push(m));
    } else if cmd == "inspect.minimum_radius" {
        app.set_status("Minimum radius: it is straight (no curvature)", false);
    }
}

/// Mass Center's marker label: the centre's coordinates in the design's units, written as
/// Design Properties writes them.
fn center_label(app: &SolveApp, c: Vec3) -> String {
    let at = |v: f64| crate::prefs::show_mm_bare(app, v);
    let unit = crate::prefs::unit_scale(&app.session.doc.units).1;
    format!("Center of mass ({}, {}, {}) {unit}", at(c.x), at(c.y), at(c.z))
}

/// The message id of the class a seam whose highest order met is `order` reads as: a gap, a
/// crease (G0 only), or the word of its order (Tangent … Smooth curvature change).
pub(crate) fn order_class(order: Option<Continuity>) -> &'static str {
    match order {
        None => "ui.continuity_gap",
        Some(Continuity::G0) => "ui.continuity_crease",
        Some(g) => g.label_id(),
    }
}

/// The measure a seam of `order` fails the next order on: its reply key, symbol and unit.
fn next_measure(order: Continuity) -> Option<(&'static str, &'static str, &'static str)> {
    match order {
        Continuity::G1 => Some(("curvature_jump_per_mm", "\u{394}k", "/mm")),
        Continuity::G2 => Some(("curvature_change_jump_per_mm2", "\u{394}k\u{2032}", "/mm\u{b2}")),
        Continuity::G3 => Some(("curvature_change_rate_jump_per_mm3", "\u{394}k\u{2033}", "/mm\u{b3}")),
        Continuity::G0 | Continuity::G4 => None,
    }
}

/// A seam from an `inspect.continuity` reply as a marker label: its class in words and the
/// measure that keeps it from the next order (the gap, the angle between the faces' normals, or
/// the jump of the curvature, of its change, or of that change's rate).
fn continuity_label(app: &SolveApp, seam: &Value) -> String {
    let deg = |key: &str| seam[key]["value"].as_f64().map(|x| format!("{x:.3}\u{b0}")).unwrap_or_default();
    let order = seam["order"].as_str().and_then(Continuity::parse);
    let class = solidcraft_ui_text::tr(order_class(order));
    match order {
        None => format!("{class} {}", seam["gap_mm"]["value"].as_f64().map(|g| crate::prefs::show_mm(app, g, 1)).unwrap_or_default()),
        Some(g) => {
            let next = next_measure(g)
                .and_then(|(key, symbol, unit)| seam[key]["value"].as_f64().map(|x| format!(" {symbol} {x:.4}{unit}")))
                .unwrap_or_default();
            format!("{class} {}{next}", deg("normal_angle_deg"))
        }
    }
}

/// Analysis markers: a target with its label.
fn marks(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if !app.tool.as_ref().is_some_and(|t| matches!(t.cmd, "inspect.minimum_radius" | "inspect.center_of_mass" | "inspect.continuity")) {
        MARKS.with(|m| m.borrow_mut().clear());
        return;
    }
    let tk = solidcraft_theme::Tokens::get();
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
                egui::FontId::proportional(solidcraft_theme::Tokens::get().font_body),
                tk.text,
            );
            painter.rect_filled(r.expand(3.0), tk.radius_sm.saturating_sub(1), tk.panel.gamma_multiply(0.85));
            painter.text(
                q + egui::vec2(12.0, -12.0),
                egui::Align2::LEFT_BOTTOM,
                label,
                egui::FontId::proportional(solidcraft_theme::Tokens::get().font_body),
                tk.text,
            );
        }
    });
}

/// Commands that start with a step of their own: Canvas and Decal ask for the image first,
/// Edit Canvas opens the canvas panel. False when the command should not go on.
pub fn start_hook(app: &mut SolveApp, id: &str) -> bool {
    if id == "inspect.section" && app.preferences.viewport_section_status && !app.session.section.is_empty() {
        let _ = app.run(id, json!({"clear":true}));
        return false;
    }
    if id == "sketch.intersect"
        && let Some(dialog) = crate::dialogs::Dialog::for_command(app, id)
    {
        if dialog.wants_picks() && app.session.has_selection() {
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
        // Each class in the legend's words, best first.
        let gaps = summary["gap"].as_u64().filter(|n| *n > 0).map(|n| format!("{n} {}", solidcraft_ui_text::tr(order_class(None))));
        let counts: Vec<String> = Continuity::ALL
            .into_iter()
            .rev()
            .filter_map(|g| {
                summary["orders"][g.as_str()].as_u64().filter(|n| *n > 0).map(|n| format!("{n} {}", solidcraft_ui_text::tr(order_class(Some(g)))))
            })
            .chain(gaps)
            .collect();
        app.set_status(format!("Continuity: {} (click the button again to turn it off)", counts.join(", ")), false);
        return true;
    }
    if id == "inspect.curvature_comb" {
        // With combs shown the button turns the analysis off; otherwise it turns it on (any other
        // view analysis off) and the tool combs the curves clicked.
        if app.session.comb.is_some() {
            if app.run(id, json!({"clear": true})).is_ok() {
                app.set_status("Analysis off", false);
            }
            return false;
        }
        comb(app, json!({}));
        return app.session.comb.is_some();
    }
    if id == "inspect.isocurve" && !app.session.isocurves.is_empty() {
        // With curves shown, the button turns the analysis off; otherwise the tool picks faces.
        if app.run(id, json!({"clear": true})).is_ok() {
            app.set_status("Analysis off", false);
        }
        return false;
    }
    if id == "inspect.center_of_mass" {
        // The whole model at once; the tool stays for picking single bodies.
        MARKS.with(|m| m.borrow_mut().clear());
        DEFERRED.with(|d| d.set(true));
        return true;
    }
    if id == "canvas.edit" {
        // The browser's canvas panel (one panel for canvases). A canvas picked in the Browser
        // is the one to edit, not just the first one (#16518).
        let picked = app.tree.picked_canvases.last().copied();
        let target = picked.or_else(|| app.session.doc.canvases.first().map(|c| c.id));
        if let Some(c) = target {
            app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(app, c, false));
        }
        return false;
    }
    if !matches!(id, "canvas.insert" | "canvas.decal") {
        return true;
    }
    let command = id.to_string();
    // Offer the image types first: the default Open filter lists only designs and meshes. An SVG
    // is drawn into a PNG as the command inserts it.
    app.pick_open_for(&["png", "jpg", "jpeg", "svg"], move |app, path| {
        if let Some(path) = path {
            start_image_placement(app, &command, path);
        }
    });
    false
}

/// Whether `path` names an image the Canvas tool takes (PNG or JPEG, by its extension).
pub(crate) fn is_canvas_image(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| ["png", "jpg", "jpeg"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Start placing image `path` with `command` (Canvas or Decal), as when it is chosen in the
/// tool's file dialog: the next click on a plane or face (or the sketch plane) places it.
/// Home closes, since the click needs the viewport (a picture can be dropped on Home).
pub(crate) fn start_image_placement(app: &mut SolveApp, command: &str, path: String) {
    app.home.open = false;
    IMAGE.with(|i| *i.borrow_mut() = Some(path));
    app.last_command = solidcraft_engine::find_command(command).map(|spec| (spec.id.into(), spec.label.into()));
    app.tool = crate::tools::Tool::for_command(command);
}

fn v3(v: &Value) -> Option<Vec3> {
    Some(Vec3::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?))
}

/// The Curvature Comb tool's options: how long the teeth are, as a multiple of the automatic
/// length (`inspect.curvature_comb`'s `length`), and how many each comb has (`density`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CombOptions {
    pub length: f64,
    pub teeth: usize,
}

impl CombOptions {
    const DEFAULT: CombOptions = CombOptions { length: 1.0, teeth: 40 };
    const LENGTH: std::ops::RangeInclusive<f64> = 0.25..=8.0;
    const TEETH: std::ops::RangeInclusive<usize> = 4..=200;
}

/// The comb tool's options now.
pub(crate) fn comb_options() -> CombOptions {
    COMB_OPTIONS.with(Cell::get)
}

/// The combs shown, each as its teeth (foot, tip).
#[cfg(test)]
pub(crate) fn shown_combs(app: &SolveApp) -> Vec<Vec<[Vec3; 2]>> {
    app.session.comb.as_ref().map(|v| v.teeth.clone()).unwrap_or_default()
}

/// Comb `picks` (the comb command's parameters for curves or edges; none: draw those shown
/// again) beside the combs already shown, every comb at one scale with the tool's options. A
/// failure leaves those shown as they were (the status line says why).
fn comb(app: &mut SolveApp, picks: Value) {
    let options = comb_options();
    let mut params = picks;
    params["add"] = json!(true);
    params["density"] = json!(options.teeth);
    params["length"] = json!(options.length);
    let _ = app.run("inspect.curvature_comb", params);
}

/// Change the comb tool's options and draw the combs shown again with them.
pub(crate) fn set_comb_options(app: &mut SolveApp, options: CombOptions) {
    let options = CombOptions {
        length: options.length.clamp(*CombOptions::LENGTH.start(), *CombOptions::LENGTH.end()),
        teeth: options.teeth.clamp(*CombOptions::TEETH.start(), *CombOptions::TEETH.end()),
    };
    if COMB_OPTIONS.with(|o| o.replace(options)) == options {
        return;
    }
    if app.session.comb.is_some() {
        comb(app, json!({}));
    }
}

/// The comb tool's options at the top left of the free viewport while it runs (the analysis and
/// continuity legends keep the lower left): Comb length and Teeth. Changing either draws the
/// combs shown again.
fn comb_options_card(app: &mut SolveApp, ctx: &egui::Context, proj: &Proj) {
    if app.tool.as_ref().map(|t| t.cmd) != Some("inspect.curvature_comb") {
        return;
    }
    let tk = solidcraft_theme::Tokens::get();
    let available = crate::shell::workspace_content(app, crate::shell::content_rect(app, proj.rect));
    let width = tk.form_card_width.min(available.width() - 2.0 * tk.space_16).max(0.0);
    let mut options = comb_options();
    let (length_tip, teeth_tip) = (app.tr("ui.comb_length_tip").to_string(), app.tr("ui.comb_teeth_tip").to_string());
    let shown = crate::ui_kit::Container::new(ctx, crate::ui_kit::Surface::Canvas)
        .area(egui::Id::unique("sc_comb_options"))
        .fixed_pos(available.left_top() + egui::vec2(tk.space_16, tk.space_16))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            crate::ui_kit::Container::popup_frame(ui.style()).fill(tk.dialog_body).inner_margin(tk.space_8).show(ui, |ui| {
                crate::ui_kit::scope(ui, |ui| {
                    ui.set_width(width);
                    let length = crate::ui_kit::row(ui, "ui.comb_length", |ui| {
                        let format = |v: f64| format!("{v:.2}\u{d7}");
                        crate::ui_kit::slider(
                            ui,
                            &mut options.length,
                            CombOptions::LENGTH,
                            crate::ui_kit::Slider { step: Some(0.25), format, ..Default::default() },
                        )
                    });
                    length.backing_on_hover_text(&length_tip);
                    let teeth = crate::ui_kit::row(ui, "ui.comb_teeth", |ui| {
                        crate::ui_kit::slider(
                            ui,
                            &mut options.teeth,
                            CombOptions::TEETH,
                            crate::ui_kit::Slider { integer: true, format: |v| format!("{v:.0}"), ..Default::default() },
                        )
                    });
                    teeth.backing_on_hover_text(&teeth_tip);
                });
            });
        });
    crate::scenario::publish_rect("comb_options", shown.response.rect);
    set_comb_options(app, options);
}

/// Curvature combs (`Session::comb`): teeth and the envelope through their tips.
fn combs(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some(shown) = &app.session.comb else { return };
    // The teeth are the design's: not while a value is typed and the sketch is drawn at it.
    if crate::dim_view::preview_world(app).is_some() {
        return;
    }
    let tk = solidcraft_theme::Tokens::get();
    for comb in &shown.teeth {
        let mut tips: Vec<Pos2> = Vec::new();
        for [a, b] in comb {
            let (Some(p), Some(q)) = (proj.to_screen(*a), proj.to_screen(*b)) else { continue };
            painter.line_segment([p, q], Stroke::new(1.0, tk.comb_teeth));
            tips.push(q);
        }
        painter.add(egui::Shape::line(tips, Stroke::new(1.4, tk.comb_envelope)));
    }
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
fn align_targets(proj: &Proj, sk: &solidcraft_engine::sketch::Sketch, plane: &solidcore_record::Plane, own: &[Vec2]) -> Vec<(Option<String>, Vec2)> {
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
    let st = crate::open_meshing::session_world(&app.session);
    app.session.active_sketch.and_then(|id| st.sketch(id)).map(|ss| ss.sketch.points.iter().map(|p| p.id.clone()).collect()).unwrap_or_default()
}

/// Once a shape is made (#7828): each new point that was placed lined up with a sketch point
/// (`placed`, the shape's points) stays level or plumb with it. Only what the sketch doesn't
/// already have is added, in the shape's undo step.
pub(crate) fn constrain_lined(app: &mut SolveApp, tool: &mut Tool, placed: &[Vec2], before: &[String]) {
    let lined = std::mem::take(&mut tool.lined);
    let new: Vec<(String, Vec2)> = {
        let st = crate::open_meshing::session_world(&app.session);
        let Some(ss) = app.session.active_sketch.and_then(|id| st.sketch(id)) else { return };
        ss.sketch.points.iter().filter(|p| !before.contains(&p.id)).map(|p| (p.id.clone(), p.pos)).collect()
    };
    let history = std::mem::take(&mut app.session.undo);
    for (at, targets) in lined.iter().filter(|(at, _)| placed.iter().any(|p| p.dist(*at) < 1e-9)) {
        let Some((point, _)) = new.iter().find(|(_, p)| p.dist(*at) < 1e-6) else { continue };
        for target in targets {
            let Some(id) = &target.id else { continue };
            let mode = if target.horizontal { "horizontal" } else { "vertical" };
            let other = id.strip_prefix("mid:").map_or_else(|| json!(id), |line| json!({"midpoint": line}));
            let params = json!({"points": [point, other], "mode": mode, "if_redundant": "refuse"});
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

#[derive(Clone, Debug, PartialEq)]
enum SnapTarget {
    Curve(String),
    Point(Vec2),
}

#[derive(Clone, Debug)]
struct SnapState {
    at: Vec2,
    labels: Vec<&'static str>,
    targets: Vec<SnapTarget>,
}

impl SnapState {
    fn new(at: Vec2) -> Self {
        Self { at, labels: Vec::new(), targets: Vec::new() }
    }

    fn push_label(&mut self, label: &'static str) {
        if label == "H+V" {
            self.push_label("H");
            self.push_label("V");
            return;
        }
        if !self.labels.contains(&label) {
            self.labels.push(label);
        }
    }

    fn push_curve(&mut self, id: impl Into<String>) {
        let target = SnapTarget::Curve(id.into());
        if !self.targets.contains(&target) {
            self.targets.push(target);
        }
    }

    fn push_point(&mut self, pt: Vec2) {
        if !self.targets.iter().any(|t| matches!(t, SnapTarget::Point(p) if p.dist(pt) < 1e-9)) {
            self.targets.push(SnapTarget::Point(pt));
        }
    }
}

/// Append compatible directional (`H`, `V`, `//`, `⊥`, `T`, `Ext`) and alignment constraints that
/// hold at a locked snap point `p`.
#[allow(clippy::too_many_arguments)]
fn add_constraints_at_point(
    sk: &solidcraft_engine::sketch::Sketch,
    prev: Option<Vec2>,
    targets: &[(Option<String>, Vec2)],
    line_tool: bool,
    mid_only: bool,
    align_points: bool,
    allow_lined: bool,
    p: Vec2,
    state: &mut SnapState,
    lined: &mut Vec<Lined>,
) {
    if mid_only {
        return;
    }
    let mut axis = false;
    if line_tool && let Some(prev) = prev {
        let d = p - prev;
        let len = d.len();
        if len > 1e-6 {
            if d.y.abs() <= len * 1e-9 {
                state.push_label("H");
                axis = true;
            } else if d.x.abs() <= len * 1e-9 {
                state.push_label("V");
                axis = true;
            }
            let saved = crate::inference::all();
            let has_dir = saved.iter().any(|i| !matches!(i.kind, crate::inference::Kind::Extension));
            let has_ext = saved.iter().any(|i| matches!(i.kind, crate::inference::Kind::Extension));
            let dir_cand = if !has_dir {
                crate::inference::tangent(sk, prev, p, 1e-4)
                    .or_else(|| crate::inference::through_center(sk, prev, p, 1e-4))
                    .or_else(|| (!axis).then(|| crate::inference::lines_directions(sk, prev, p, 1e-4)).flatten())
            } else {
                None
            };
            let ext_cand = (!has_ext).then(|| crate::inference::extension(sk, prev, p, 1e-4)).flatten();
            crate::inference::clear();
            for s in saved {
                crate::inference::add_pending(s);
            }
            for mut inf in [dir_cand, ext_cand].into_iter().flatten() {
                if inf.at.dist(p) <= 1e-4 {
                    inf.at = p;
                    inf.guide.1 = p;
                    state.push_label(inf.label());
                    state.push_curve(inf.curve.clone());
                    crate::inference::add_pending(inf);
                }
            }
        }
    }
    if align_points {
        let other: Vec<_> = targets.iter().filter(|(_, q)| q.dist(p) > 1e-6 && prev.is_none_or(|pr| q.dist(pr) > 1e-6)).cloned().collect();
        let (level, plumb) = line_up(&other, p, 1e-4);
        if let Some(l) = level
            && (p.y - l.at.y).abs() <= 1e-6
        {
            state.push_label("H");
            state.push_point(l.at);
            if allow_lined && !lined.iter().any(|existing| existing.horizontal) {
                lined.push(l);
            }
        }
        if let Some(pl) = plumb
            && (p.x - pl.at.x).abs() <= 1e-6
        {
            state.push_label("V");
            state.push_point(pl.at);
            if allow_lined && !lined.iter().any(|existing| !existing.horizontal) {
                lined.push(pl);
            }
        }
    }
}

/// Forget any pending snap badge, alignment guide, and line-tool inference.
pub(crate) fn clear_snap() {
    crate::inference::clear();
    SNAP.with(|s| *s.borrow_mut() = None);
    LINED.with(|l| *l.borrow_mut() = None);
}

/// Whether `cmd` is an extra sketch constraint-picking tool (`Mode::Entities(_)`).
pub(crate) fn is_entity_constraint_tool(cmd: &str) -> bool {
    matches!(cmd, "sketch.constraint.curvature" | "sketch.constrainer" | "sketch.constraint.polygon")
}

/// Snap a cursor point on the sketch plane to what is inferred while drawing: a crossing of
/// two curves, a line's midpoint, or alignment (horizontal/vertical) with the previous point
/// of the tool. None: nothing near, use the grid.
pub fn refine_snap(
    app: &SolveApp,
    proj: &Proj,
    sk: &solidcraft_engine::sketch::Sketch,
    plane: &solidcore_record::Plane,
    lp: Vec2,
) -> Option<(Vec2, Option<String>)> {
    crate::inference::clear();
    LINED.with(|l| *l.borrow_mut() = None);
    // Millimetres per pixel here.
    let (a, b) = (proj.to_screen(plane.to_world(lp))?, proj.to_screen(plane.to_world(lp + Vec2::X))?);
    let px = 1.0 / (a.distance(b) as f64).max(1e-9);
    let tol = 7.0 * px;
    // The tool's points and kind (a click takes the tool out of the app while it runs, so the
    // last hover's are kept: a rectangle's click takes no Line tool inference its hover didn't).
    let (own, line_tool, arc_tool) = match app.tool.as_ref() {
        Some(t) => {
            let own: (Vec<Vec2>, bool, bool) = (t.pts.iter().map(|p| p.0).collect(), t.cmd == "sketch.line", t.cmd == "sketch.arc.three_point");
            TOOL.with(|c| *c.borrow_mut() = own.clone());
            own
        }
        None => TOOL.with(|c| c.borrow().clone()),
    };
    // With `sketch_align_points` off, a click takes the Line tool's inferences, as before.
    // The 3-Point Arc takes none of them, on hover or on click, when its tangent snap is on (#20719).
    let arc_snap = arc_tool && app.preferences.sketch_arc_tangent_snap;
    let line_tool = if app.preferences.sketch_align_points { line_tool } else { app.tool.as_ref().is_none_or(|t| t.cmd == "sketch.line") };
    let line_tool = line_tool && !arc_snap;
    let prev = own.last().copied();
    let mid_only = crate::tool_modifiers::target_midpoint();
    // Shift midpoint mode: widen the midpoint capture a little and skip other inferences.
    let mid_tol = if mid_only { tol * 1.5 } else { tol };
    let align_points = app.preferences.sketch_align_points;
    // Points to line up with as well (#7828): with `sketch_align_points` off, none, so only the
    // previous point, one axis at a time, as before.
    // A hovered line's midpoint guide lines up too, as the "mid:<line>" point the constraint
    // command makes on that line (#13413). It comes first: `line_up` keeps the first of equally
    // near points, so a free point that happens to share the coordinate does not take its place.
    let mut targets: Vec<(Option<String>, Vec2)> = Vec::new();
    if align_points {
        targets.extend(app.viewport.tracking.line_midpoints(sk).into_iter().map(|(line, at)| (Some(format!("mid:{line}")), at)));
        targets.extend(align_targets(proj, sk, plane, &own));
    }
    let mut lined = Vec::new();

    // Helper to finish any snap state + optional alignment lines.
    let finish = |state: SnapState, lined: Vec<Lined>, arg: Option<String>| -> Option<(Vec2, Option<String>)> {
        let p = state.at;
        SNAP.with(|s| *s.borrow_mut() = (!state.labels.is_empty()).then_some(state));
        LINED.with(|l| *l.borrow_mut() = (!lined.is_empty()).then_some((p, lined)));
        Some((p, arg))
    };

    // 1. Locked positional snaps: model/sketch vertices, curve crossings, midpoints, recent tracking
    // crossings, off-circle tangent points, and circle centers/rims.
    let mut locked: Option<(Vec2, &'static str, Option<String>, Vec<SnapTarget>, Option<crate::inference::Inference>)> = None;
    for h in pick(app, proj, a) {
        if let Hit::Vertex { point, body } = h {
            use solidcraft_engine::snap::SnapKind;
            let vertex = Some(format!("vertex:{},{},{}", point.x, point.y, point.z));
            let (label, arg) = match crate::snap_wake::woken_at(app, point).map(|p| p.kind) {
                Some(SnapKind::Centre | SnapKind::FaceCentre) => ("Ctr", None),
                Some(SnapKind::Middle) => ("Mid", None),
                Some(SnapKind::End) if body.is_empty() => ("End", None),
                Some(SnapKind::End) => ("End", vertex),
                Some(SnapKind::Vertex) | None => ("Pt", vertex),
            };
            let local = plane.to_local(point);
            locked = Some((local, label, arg, vec![SnapTarget::Point(local)], None));
            break;
        }
    }
    if locked.is_none()
        && !mid_only
        && let Some((q, i, j)) = solidcraft_engine::sketch::nearest_curve_intersection(sk, lp, tol)
        && let (Some(ca), Some(cb)) = (sk.curves.get(i), sk.curves.get(j))
    {
        let arg = Some(format!("x:{}+{}:{},{}", ca.id, cb.id, q.x, q.y));
        locked = Some((q, "X", arg, vec![SnapTarget::Curve(ca.id.clone()), SnapTarget::Curve(cb.id.clone())], None));
    }
    if locked.is_none() {
        let mut best_mid: Option<(f64, String, Vec2)> = None;
        for (i, c) in sk.curves.iter().enumerate() {
            if let solidcraft_engine::sketch::CurveKind::Line { .. } = c.kind
                && let Some(solidcraft_engine::sketch::Shape::Line { a, b }) = sk.shape(i)
            {
                let m = (a + b) * 0.5;
                let d = m.dist(lp);
                if d < mid_tol && best_mid.as_ref().is_none_or(|x| d < x.0) {
                    best_mid = Some((d, c.id.clone(), m));
                }
            }
        }
        if let Some((_, id, m)) = best_mid {
            locked = Some((m, "Mid", Some(format!("mid:{id}")), vec![SnapTarget::Curve(id)], None));
        }
    }
    // The two most recent midpoints or centres: snap where their axes meet. Off unless Sketch
    // preferences ask for it. One reference stays a visual guide. Shift skips this. With
    // `sketch_align_points` on, the point lines up with both references below, as it would
    // without this snap (#20859); otherwise it is a free coordinate and adds no constraint.
    let mut crossing = None;
    if locked.is_none()
        && !mid_only
        && app.preferences.sketch_recent_snap
        && let Some(meeting) = app.viewport.tracking.meeting_of(sk, plane, proj, a)
    {
        locked = Some((meeting.at, "+", None, Vec::new(), None));
        crossing = Some(meeting);
    }
    if locked.is_none()
        && !mid_only
        && line_tool
        && let Some(pr) = prev
    {
        if let Some(i) = crate::inference::tangent(sk, pr, lp, tol) {
            crate::inference::clear();
            if crate::inference::direction_line(sk, pr, &i).is_none() {
                let target = SnapTarget::Curve(i.curve.clone());
                let arg = Some(format!("on:{}:{},{}", i.curve, i.at.x, i.at.y));
                locked = Some((i.at, i.label(), arg, vec![target], Some(i)));
            }
        }
        if locked.is_none()
            && let Some(i) = crate::inference::through_center(sk, pr, lp, tol)
        {
            crate::inference::clear();
            if i.arg.is_some() {
                let target = SnapTarget::Curve(i.curve.clone());
                let arg = i.arg.clone();
                locked = Some((i.at, i.label(), arg, vec![target], Some(i)));
            }
        }
    }
    // 3-Point Arc: the third point that makes the arc tangent to a line or arc it starts or ends
    // on (#20719), exact: the point of that tangent circle nearest the cursor. It and the centre
    // snap below win over an earlier snap when their point is nearer the cursor.
    let mut arc_locked = false;
    if !mid_only
        && arc_snap
        && let [start, end] = own[..]
        && let Some(i) = crate::inference::arc_tangent(sk, start, end, lp, tol)
        && locked.as_ref().is_none_or(|l| i.at.dist(lp) < l.0.dist(lp))
    {
        locked = Some((i.at, i.label(), None, Vec::new(), None));
        arc_locked = true;
    }
    // 3-Point Arc: the centre lands on a sketch point (#20719); the click keeps it there.
    if !mid_only
        && arc_snap
        && !(arc_locked && locked.as_ref().is_some_and(|l| l.1 == "T"))
        && let [start, end] = own[..]
        && let Some(i) = crate::inference::arc_centre(sk, start, end, lp, tol)
        && locked.as_ref().is_none_or(|l| i.at.dist(lp) < l.0.dist(lp))
    {
        locked = Some((i.at, i.label(), None, Vec::new(), None));
        arc_locked = true;
    }
    if let Some((p, label, arg, lock_targets, inf)) = locked {
        let mut state = SnapState::new(p);
        state.push_label(label);
        for t in lock_targets {
            match t {
                SnapTarget::Curve(id) => state.push_curve(id),
                SnapTarget::Point(pt) => state.push_point(pt),
            }
        }
        if let Some(inf) = inf {
            if matches!(inf.kind, crate::inference::Kind::Tangent) && arg.as_deref().is_some_and(|a| a.starts_with("on:")) {
                state.push_label("On");
            }
            crate::inference::set_pending(inf);
        }
        // Where two guides meet, the point lines up with the two references the guides come from:
        // level with one and plumb with the other (#20859), never with another point that happens
        // to share a coordinate.
        let has_crossing = crossing.is_some();
        if let Some(meeting) = crossing.filter(|_| align_points) {
            if let Some((id, at)) = meeting.level {
                state.push_label("H");
                state.push_point(at);
                lined.push(Lined { id: Some(id), at, horizontal: true });
            }
            if let Some((id, at)) = meeting.plumb {
                state.push_label("V");
                state.push_point(at);
                lined.push(Lined { id: Some(id), at, horizontal: false });
            }
        }
        let allow_lined = !matches!(label, "End" | "Pt");
        add_constraints_at_point(
            sk,
            prev,
            &targets,
            line_tool,
            mid_only,
            align_points && !has_crossing && !arc_locked,
            allow_lined,
            p,
            &mut state,
            &mut lined,
        );
        return finish(state, lined, arg);
    }

    // 2. Non-locked candidates: curve "On", Line-tool H/V or directional inference, and point alignment.
    // The free coordinate still follows the grid: on a crossing of the drawn grid's lines when
    // the point can be on one, and on the finer lattice between the crossings otherwise (#7114).
    let (minor, fine) = solidcraft_engine::view::sketch_grid_step(app.cam.half_height());
    // `fixed` is the coordinate the point lines up with, on the other axis: the crossing has to
    // run through it. With Snap off (Sketch Palette) the grid holds the point as before.
    let g = |free: f64, fixed: f64| {
        if sk.view.no_snap || !app.preferences.sketch_snap_to_visible_grid {
            solidcraft_engine::view::sketch_grid_lattice_axis(free, fine)
        } else {
            solidcraft_engine::view::sketch_grid_free(free, fixed, minor, fine)
        }
    };

    let mut best_on: Option<(f64, usize, String, Vec2)> = None;
    if !mid_only {
        for (i, c) in sk.curves.iter().enumerate() {
            let (d, q) = match sk.shape(i) {
                Some(sh) => (sh.dist(lp), sh.project(lp)),
                None if c.kind.is_freeform() => {
                    let Some(q) = nearest_on_polyline(&sk.polyline(i), lp) else { continue };
                    (q.dist(lp), q)
                }
                None => continue,
            };
            if d < tol * 0.8 && best_on.as_ref().is_none_or(|x| d < x.0) {
                best_on = Some((d, i, c.id.clone(), q));
            }
        }
    }

    let axis_from_prev = (!mid_only && line_tool).then_some(prev).flatten().and_then(|pr| {
        let (dx, dy) = ((lp.x - pr.x).abs(), (lp.y - pr.y).abs());
        if dy < tol && dx > tol {
            Some(("H", pr, Vec2::X))
        } else if dx < tol && dy > tol {
            Some(("V", pr, Vec2::Y))
        } else {
            None
        }
    });

    let (dir_inf, ext_cand) = if !mid_only
        && line_tool
        && let Some(pr) = prev
    {
        let d = crate::inference::tangent(sk, pr, lp, tol)
            .or_else(|| crate::inference::through_center(sk, pr, lp, tol))
            .or_else(|| crate::inference::lines_directions(sk, pr, lp, tol));
        let e = crate::inference::extension(sk, pr, lp, tol);
        crate::inference::clear();
        (d, e)
    } else {
        (None, None)
    };
    let dir_line = dir_inf.as_ref().and_then(|i| prev.and_then(|pr| crate::inference::direction_line(sk, pr, i)));
    let ext_line = ext_cand.as_ref().and_then(|i| prev.and_then(|pr| crate::inference::direction_line(sk, pr, i)));
    let (level, plumb) = if !mid_only && align_points { line_up(&targets, lp, tol) } else { (None, None) };
    let align_with_line = |origin: Vec2, dir: Vec2, require_forward: bool| -> Option<(Vec2, Lined, &'static str)> {
        [
            level.as_ref().and_then(|l| solidcraft_engine::sketch::intersect_infinite_lines(origin, dir, l.at, Vec2::X).map(|q| (q, l.clone(), "H"))),
            plumb
                .as_ref()
                .and_then(|pl| solidcraft_engine::sketch::intersect_infinite_lines(origin, dir, pl.at, Vec2::Y).map(|q| (q, pl.clone(), "V"))),
        ]
        .into_iter()
        .flatten()
        .filter(|(q, _, _)| (!require_forward || (*q - origin).dot(dir) > 1e-6) && q.dist(lp) <= tol * 1.2)
        .min_by(|a, b| a.0.dist(lp).total_cmp(&b.0.dist(lp)))
    };

    if let Some((_, ci, cid, q_on)) = best_on {
        if let Some((axis_label, origin, dir)) = axis_from_prev
            && let Some(q) = solidcraft_engine::sketch::intersect_curve_with_line(sk, ci, origin, dir, lp, tol * 1.4)
        {
            let mut state = SnapState::new(q);
            state.push_label(axis_label);
            state.push_label("On");
            state.push_curve(cid.clone());
            add_constraints_at_point(sk, prev, &targets, line_tool, mid_only, align_points, true, q, &mut state, &mut lined);
            return finish(state, lined, Some(format!("on:{cid}:{},{}", q.x, q.y)));
        }
        if let (Some(mut inf), Some((origin, dir))) = (dir_inf.clone(), dir_line)
            && inf.curve != cid
            && let Some(q) = solidcraft_engine::sketch::intersect_curve_with_line(sk, ci, origin, dir, lp, tol * 1.4)
        {
            inf.at = q;
            inf.guide.1 = q;
            let mut state = SnapState::new(q);
            state.push_label(inf.label());
            state.push_curve(inf.curve.clone());
            crate::inference::set_pending(inf);
            state.push_label("On");
            state.push_curve(cid.clone());
            add_constraints_at_point(sk, prev, &targets, line_tool, mid_only, align_points, true, q, &mut state, &mut lined);
            return finish(state, lined, Some(format!("on:{cid}:{},{}", q.x, q.y)));
        }
        if let (Some(mut ext), Some((origin, dir))) = (ext_cand.clone(), ext_line)
            && ext.curve != cid
            && let Some(q) = solidcraft_engine::sketch::intersect_curve_with_line(sk, ci, origin, dir, lp, tol * 1.4)
            && (q - origin).dot(dir) > 1e-6
        {
            ext.at = q;
            ext.guide.1 = q;
            let mut state = SnapState::new(q);
            state.push_label(ext.label());
            state.push_curve(ext.curve.clone());
            crate::inference::set_pending(ext);
            state.push_label("On");
            state.push_curve(cid.clone());
            add_constraints_at_point(sk, prev, &targets, line_tool, mid_only, align_points, true, q, &mut state, &mut lined);
            return finish(state, lined, Some(format!("on:{cid}:{},{}", q.x, q.y)));
        }
        let align_hit = [
            level.as_ref().and_then(|l| {
                solidcraft_engine::sketch::intersect_curve_with_line(sk, ci, l.at, Vec2::X, lp, tol * 1.4).map(|q| (q.dist(lp), q, l.clone(), "H"))
            }),
            plumb.as_ref().and_then(|pl| {
                solidcraft_engine::sketch::intersect_curve_with_line(sk, ci, pl.at, Vec2::Y, lp, tol * 1.4).map(|q| (q.dist(lp), q, pl.clone(), "V"))
            }),
        ]
        .into_iter()
        .flatten()
        .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, q, l, align_label)) = align_hit {
            let mut state = SnapState::new(q);
            state.push_label(align_label);
            state.push_point(l.at);
            lined.push(l);
            state.push_label("On");
            state.push_curve(cid.clone());
            add_constraints_at_point(sk, prev, &targets, line_tool, mid_only, align_points, true, q, &mut state, &mut lined);
            return finish(state, lined, Some(format!("on:{cid}:{},{}", q.x, q.y)));
        }
        let mut state = SnapState::new(q_on);
        state.push_label("On");
        state.push_curve(cid.clone());
        add_constraints_at_point(sk, prev, &targets, line_tool, mid_only, align_points, true, q_on, &mut state, &mut lined);
        return finish(state, lined, Some(format!("on:{cid}:{},{}", q_on.x, q_on.y)));
    }

    if let Some((axis_label, pr, axis_dir)) = axis_from_prev {
        if let (Some(mut ext), Some((ext_origin, ext_dir))) = (ext_cand.clone(), ext_line)
            && let Some(q) = solidcraft_engine::sketch::intersect_extension_ray(pr, axis_dir, ext_origin, ext_dir, lp, tol * 1.4)
        {
            ext.at = q;
            ext.guide.1 = q;
            let mut state = SnapState::new(q);
            state.push_label(axis_label);
            state.push_label(ext.label());
            state.push_curve(ext.curve.clone());
            crate::inference::set_pending(ext);
            add_constraints_at_point(sk, prev, &targets, line_tool, mid_only, align_points, true, q, &mut state, &mut lined);
            return finish(state, lined, None);
        }
        let p = match (axis_label, plumb.as_ref(), level.as_ref()) {
            ("H", Some(pl), _) => Vec2::new(pl.at.x, pr.y),
            ("H", None, _) => Vec2::new(g(lp.x, pr.y), pr.y),
            ("V", _, Some(l)) => Vec2::new(pr.x, l.at.y),
            _ => Vec2::new(pr.x, g(lp.y, pr.x)),
        };
        let mut state = SnapState::new(p);
        state.push_label(axis_label);
        if axis_label == "H"
            && let Some(pl) = plumb
        {
            state.push_label("V");
            state.push_point(pl.at);
            lined.push(pl);
        } else if axis_label == "V"
            && let Some(l) = level
        {
            state.push_label("H");
            state.push_point(l.at);
            lined.push(l);
        }
        add_constraints_at_point(sk, prev, &targets, line_tool, mid_only, align_points, true, p, &mut state, &mut lined);
        return finish(state, lined, None);
    }

    if let Some(mut inf) = dir_inf {
        let arg = inf.arg.clone();
        let (p, extra_ext, extra_align) = if let (Some((origin, dir)), Some(mut ext), Some((ext_origin, ext_dir))) = (dir_line, ext_cand, ext_line)
            && ext.curve != inf.curve
            && let Some(q) = solidcraft_engine::sketch::intersect_extension_ray(origin, dir, ext_origin, ext_dir, lp, tol * 1.4)
        {
            inf.at = q;
            inf.guide.1 = q;
            ext.at = q;
            ext.guide.1 = q;
            (q, Some(ext), None)
        } else if let Some((q, l, align_label)) = dir_line.and_then(|(origin, dir)| align_with_line(origin, dir, false)) {
            inf.at = q;
            inf.guide.1 = q;
            (q, None, Some((l, align_label)))
        } else {
            (inf.at, None, None)
        };
        let mut state = SnapState::new(p);
        state.push_label(inf.label());
        state.push_curve(inf.curve.clone());
        crate::inference::set_pending(inf);
        if let Some(ext) = extra_ext {
            state.push_label(ext.label());
            state.push_curve(ext.curve.clone());
            crate::inference::add_pending(ext);
        }
        if let Some((l, align_label)) = extra_align {
            state.push_label(align_label);
            state.push_point(l.at);
            lined.push(l);
        }
        add_constraints_at_point(sk, prev, &targets, line_tool, mid_only, align_points, true, p, &mut state, &mut lined);
        return finish(state, lined, arg);
    }

    if let Some(mut ext) = ext_cand {
        let (p, extra_align) = match ext_line.and_then(|(origin, dir)| align_with_line(origin, dir, true)) {
            Some((q, l, align_label)) => {
                ext.at = q;
                ext.guide.1 = q;
                (q, Some((l, align_label)))
            }
            None => (ext.at, None),
        };
        let arg = ext.arg.clone();
        let mut state = SnapState::new(p);
        state.push_label(ext.label());
        state.push_curve(ext.curve.clone());
        crate::inference::set_pending(ext);
        if let Some((l, align_label)) = extra_align {
            state.push_label(align_label);
            state.push_point(l.at);
            lined.push(l);
        }
        add_constraints_at_point(sk, prev, &targets, line_tool, mid_only, align_points, true, p, &mut state, &mut lined);
        return finish(state, lined, arg);
    }

    if level.is_some() || plumb.is_some() {
        let at = Vec2::new(
            plumb.as_ref().map_or_else(|| g(lp.x, level.as_ref().map_or(lp.y, |l| l.at.y)), |pl| pl.at.x),
            level.as_ref().map_or_else(|| g(lp.y, plumb.as_ref().map_or(lp.x, |pl| pl.at.x)), |l| l.at.y),
        );
        let mut state = SnapState::new(at);
        if let Some(l) = level {
            state.push_label("H");
            state.push_point(l.at);
            lined.push(l);
        }
        if let Some(pl) = plumb {
            state.push_label("V");
            state.push_point(pl.at);
            lined.push(pl);
        }
        return finish(state, lined, None);
    }

    SNAP.with(|s| *s.borrow_mut() = None);
    None
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
    let st = crate::open_meshing::session_world(&app.session);
    match active_sketch(app).and_then(|s| st.sketch(s)) {
        Some(ss) => glyph_deletion(&ss.sketch, id),
        None => vec![solidcraft_engine::Sel::SketchConstraint { id: id.to_string() }],
    }
}

/// The constraint glyph under a screen position (its constraint id).
pub fn glyph_at(pos: Pos2) -> Option<String> {
    let reach = GLYPH_PX.with(Cell::get) * 0.6;
    GLYPHS.with(|g| nearest_glyph(g.borrow().iter().map(|(id, c)| (id.as_str(), *c)), pos, reach).map(|(id, _)| id.to_owned()))
}

/// The glyph within `reach` of `pos` whose centre is nearest (its id and centre). Large glyphs'
/// hit regions overlap, so the first one in reach could be a neighbour of the one under the
/// pointer (#10794); hover and click both pick with this rule, and so do the plane handles.
pub(crate) fn nearest_glyph<K>(glyphs: impl IntoIterator<Item = (K, Pos2)>, pos: Pos2, reach: f32) -> Option<(K, Pos2)> {
    glyphs.into_iter().filter(|(_, c)| c.distance(pos) <= reach).min_by(|a, b| a.1.distance(pos).total_cmp(&b.1.distance(pos)))
}

/// Constraint glyphs drawn last frame: (constraint id, screen centre).
pub fn glyph_positions() -> Vec<(String, Pos2)> {
    GLYPHS.with(|g| g.borrow().clone())
}

#[derive(Clone, Debug)]
struct DetachPreview {
    curve_id: String,
    point_id: String,
    point_rect: egui::Rect,
    glyph_rect: egui::Rect,
    distance_mm: f64,
}

/// If `pos` is over the simulated coincident break-off glyph, detach that curve from the point.
pub fn detach_simulated_at(app: &mut SolveApp, pos: Pos2) -> bool {
    if active_sketch(app).is_none() || app.ui.pick_off.iter().any(|x| x == "constraints") {
        return false;
    }
    let hit = DETACH_PREVIEW.with(|cell| {
        let mut st = cell.borrow_mut();
        let preview = st.as_ref()?;
        if preview.glyph_rect.expand(3.0).contains(pos) {
            let out = (preview.curve_id.clone(), preview.point_id.clone(), preview.distance_mm);
            *st = None;
            Some(out)
        } else {
            None
        }
    });
    let Some((curve, point, distance)) = hit else { return false };
    let _ = app.run("sketch.detach_point", json!({ "curve": curve, "point": point, "distance": distance }));
    true
}

fn selected_detach_curve(app: &SolveApp, sid: u64, sk: &solidcraft_engine::sketch::Sketch) -> Option<(String, usize)> {
    if app.tool.is_some() || app.dialog.as_ref().is_some_and(|d| d.wants_picks()) {
        return None;
    }
    app.session.selection.iter().rev().find_map(|sel| match sel {
        solidcraft_engine::Sel::SketchCurve { id, sketch } if sketch.is_none_or(|s| s == sid) => {
            sk.curve_index(id).filter(|&ci| !sk.is_linked_curve(ci)).map(|ci| (id.clone(), ci))
        }
        solidcraft_engine::Sel::SketchMidpoint { id } => sk.curve_index(id).filter(|&ci| !sk.is_linked_curve(ci)).map(|ci| (id.clone(), ci)),
        _ => None,
    })
}

fn point_is_detachable_for_curve(sk: &solidcraft_engine::sketch::Sketch, ci: usize, pi: usize) -> bool {
    if !sk.curves.get(ci).is_some_and(|c| c.kind.uses(pi)) {
        return false;
    }
    let users = sk.curves.iter().filter(|c| c.kind.uses(pi)).count();
    let has_coincident =
        sk.constraints.iter().any(|c| matches!(c.kind, solidcraft_engine::sketch::ConstraintKind::Coincident { p, q } if p == pi || q == pi));
    users > 1 || has_coincident || pi == 0
}

fn resolve_detach_target(
    app: &SolveApp,
    sid: u64,
    sk: &solidcraft_engine::sketch::Sketch,
    hover: Option<Pos2>,
    scale: f32,
) -> Option<(String, usize, String)> {
    let (curve_id, ci) = selected_detach_curve(app, sid, sk)?;
    let curve = sk.curves.get(ci)?;
    if let Some(Hit::SketchPoint { sketch, id, .. }) = app.viewport.hover.as_ref()
        && *sketch == sid
        && let Some(hovered_pi) = sk.resolve_point(id)
        && curve.kind.uses(hovered_pi)
        && point_is_detachable_for_curve(sk, ci, hovered_pi)
        && let Some(pt) = sk.points.get(hovered_pi)
    {
        return Some((curve_id, hovered_pi, pt.id.clone()));
    }
    let h = hover?;
    DETACH_PREVIEW.with(|cell| {
        let st = cell.borrow();
        let prev = st.as_ref()?;
        if prev.curve_id != curve_id {
            return None;
        }
        let keep = prev.glyph_rect.expand(10.0 * scale).contains(h) || prev.point_rect.expand(8.0 * scale).contains(h);
        if !keep {
            return None;
        }
        let target_pi = sk.resolve_point(&prev.point_id)?;
        if point_is_detachable_for_curve(sk, ci, target_pi) { Some((curve_id, target_pi, prev.point_id.clone())) } else { None }
    })
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
    let _ = app.run("sketch.delete", json!({ "entities": entities, "preserve_projection_link": app.ui.project_link_tools }));
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

/// Whether a tool places points on the sketch plane (and so takes snap points, #11229).
pub fn places_points(t: &Tool) -> bool {
    match t.kind {
        Kind::Draw(_) => true,
        Kind::Ext => matches!(mode(t.cmd), Some(Mode::Points(_))),
        _ => false,
    }
}

fn mode(id: &str) -> Option<Mode> {
    if solidcraft_engine::cmd::construction_line_mode(id).is_some() {
        return Some(Mode::Construction);
    }
    Some(match id {
        "sketch.project" | "sketch.intersect" | "sketch.include_3d" | "sketch.fit_curves_to_section" | "sketch.isoparametric_curve" => Mode::ModelRef,
        "sketch.repair_link" => Mode::ModelRef,
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

/// Whether an extra sketch tool draws new geometry from clicks rather than picking existing entities.
pub fn is_drawing_tool(id: &str) -> bool {
    matches!(mode(id), Some(Mode::Points(_) | Mode::Construction))
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
        line_type: crate::sketch_construction::last_line_type(),
        classic: false,
        arc_modes: false,
        preselected: false,
        finished: false,
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
            "sketch.repair_link" => "Repair Link: click the edge, face, vertex or sketch curve the projection should follow now",
            "sketch.intersect" => "Intersect: click faces, edges or other sketches' curves to cut with the sketch plane",
            "sketch.include_3d" => "Include 3D Geometry: click edges or vertices",
            "sketch.fit_curves_to_section" => "Fit Curves to Mesh Section: click a body",
            "sketch.isoparametric_curve" => "Isoparametric Curve: click a point on a face (Shift: along)",
            "inspect.curvature_comb" => "Curvature comb: click sketch curves or model edges",
            "inspect.minimum_radius" => "Minimum radius: click sketch curves, edges or faces",
            "inspect.center_of_mass" => "Center of mass of all bodies; click a body for its own",
            "inspect.isocurve" => "Isocurves: click faces",
            "inspect.continuity" => "Continuity: click an edge for its numbers (the button again turns the colours off)",
            "canvas.insert" => solidcraft_ui_text::tr("canvas.place_hint"),
            "canvas.decal" => solidcraft_ui_text::tr("canvas.decal_place_hint"),
            "sketch.trim" => tr("sketch.hint.trim"),
            "sketch.extend" => tr("sketch.hint.extend"),
            "sketch.break" => tr("sketch.hint.break"),
            "sketch.line.midpoint" => "Midpoint line: click the middle, then an end",
            "sketch.arc.tangent" => "Tangent arc: click the end of a line or arc, then the arc's end",
            "sketch.ellipse" => "Ellipse: click the centre, the end of the major axis, then a point on it",
            "sketch.conic" => "Conic: click the start, the end, then the apex; type Rho to shape it (0.5 is a parabola)",
            "sketch.slot.center_point" => "Slot: click the centre, an arc centre, then the width",
            "sketch.slot.arc_three_point" => "Arc slot: click start, a point on the arc, the end, then the width",
            "sketch.slot.arc_center" => "Arc slot: click the centre, the start, the end, then the width",
            "sketch.spline.fit_point" => {
                "Spline: click fit points; click the start to close smoothly, or right-click to finish; then select the spline to drag its tangent handles"
            }
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
            "sketch.intersection_curve" => "Intersection curve: click two faces or curves of other sketches",
            "sketch.spun_profile" => "Spun profile: click the body, then the axis line",
            _ => return None,
        }
        .to_string(),
    )
}

fn active_sketch(app: &SolveApp) -> Option<u64> {
    app.session.active_sketch
}

/// A click with Project. An edge, face, vertex or other sketch entity projects as itself.
/// The centre of a hole is none of those — the opening has no triangles — so the ray is
/// tested against hole openings and the rim's edges are projected. Anything else is said
/// out loud: a miss used to return without running the command, so the click did nothing.
fn project_click(app: &mut SolveApp, proj: &Proj, pos: Pos2, hits: &[Hit]) {
    let picked = hits.iter().find_map(|h| match h {
        Hit::Vertex { .. } | Hit::Edge { .. } | Hit::Face { .. } | Hit::SketchCurve { .. } | Hit::SketchPoint { .. } => model_ref(app, h),
        _ => None,
    });
    let refs = if let Some(r) = picked {
        vec![r]
    } else if let Some(rim) = hole_under_cursor(app, proj, pos) {
        rim
    } else if let Some(r) = hits.iter().find_map(|h| model_ref(app, h)) {
        vec![r]
    } else {
        app.set_status(app.tr("sketch.project.nothing_under_cursor").to_string(), true);
        return;
    };
    if refs.is_empty() {
        app.set_status(app.tr("sketch.project.nothing_under_cursor").to_string(), true);
        return;
    }
    // A command failure is already reported by `SolveApp::run`. A refusal that returns
    // before that (a file operation in progress) is not, so surface it here.
    if let Err(error) = app.run("sketch.project", json!({"refs": refs}))
        && !app.status.as_ref().is_some_and(|(_, _, is_error)| *is_error)
    {
        app.set_status(error, true);
    }
}

/// Edge references for the hole opening under the cursor, when the ray passes through one.
fn hole_under_cursor(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<Vec<Value>> {
    let (origin, dir) = proj.ray(pos);
    let state = crate::open_meshing::shown_world(app);
    let rim = solidcraft_engine::doc::project::hole_rim_along_ray(&state, origin, dir, |name| app.session.body_visible(name))?;
    let refs: Vec<Value> = rim.edges.iter().filter(|p| p.is_finite()).map(|p| json!({"edge": [p.x, p.y, p.z], "body": rim.body})).collect();
    (!refs.is_empty()).then_some(refs)
}

/// A model reference for a hit (for Project and friends).
/// Did the last click end its tool (the job is done)? Reading it resets it.
pub(crate) fn take_ended() -> bool {
    ENDED.with(|e| e.replace(false))
}

/// Start Repair Link for `link` of the sketch being edited: the next model reference clicked
/// becomes its source.
pub(crate) fn start_repair(app: &mut SolveApp, link: &str) {
    REPAIR.with(|l| *l.borrow_mut() = Some(link.to_string()));
    app.start("sketch.repair_link");
}

/// Translate Project's default-off session choice into explicit command inputs. Shared by dialog preview,
/// deferred requests and immediate commands; an explicit caller's choice is kept.
pub(crate) fn projection_policy(app: &SolveApp, command: &str, params: &mut Value) {
    if matches!(command, "sketch.project" | "sketch.intersect" | "sketch.include_3d") {
        if let Some(object) = params.as_object_mut() {
            object.entry("strict_occurrence_references").or_insert(json!(app.ui.project_link_tools));
        }
        if app.ui.project_link_tools && params.get("strict_occurrence_references").and_then(Value::as_bool) == Some(true) {
            projection_paths(app, params);
        }
    }
    if matches!(command, "sketch.delete" | "selection.delete")
        && let Some(object) = params.as_object_mut()
    {
        object.entry("preserve_projection_link").or_insert(json!(app.ui.project_link_tools));
    }
}

/// Put the placement actually displayed by the viewport into opt-in projection requests.
/// Model bodies carry a world instance name; sketches are displayed at their first placement
/// (`world_state`), so a repeated sketch pick must name that path rather than become ambiguous.
pub(crate) fn projection_paths(app: &SolveApp, params: &mut Value) {
    let doc = &app.session.doc;
    let state = app.session.model.state();
    let bodies = doc.body_frames_with_source(&state);
    let one = |value: &mut Value| {
        let Some(reference) = value.as_object_mut() else { return };
        if reference.contains_key("path") {
            return;
        }
        let path = if let Some(body) = reference.get("body").and_then(Value::as_str) {
            bodies.iter().find(|(world, ..)| world == body).map(|(_, _, _, path, _)| path.clone())
        } else if let Some(sketch) = reference.get("sketch") {
            let key = sketch.as_str().map(str::to_string).or_else(|| sketch.as_u64().map(|id| id.to_string()));
            key.and_then(|key| doc.find_feature(&key)).map(|feature| doc.first_path(feature.component))
        } else {
            None
        };
        if let Some(path) = path {
            reference.insert("path".into(), json!(path));
        }
    };
    if let Some(refs) = params.get_mut("refs") {
        if let Some(refs) = refs.as_array_mut() {
            for reference in refs.iter_mut().take(1000) {
                one(reference);
            }
        } else {
            one(refs);
        }
    } else if let Some(reference) = params.get_mut("ref") {
        one(reference);
    }
}

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
/// the corner commands resolve them in from a `point` parameter. An ellipse's axis is never a
/// side (the engine refuses it), and construction curves are ignored when more than two meet.
/// Each side must be a line, or for a fillet (`curves_ok`) an arc, a conic or an open control
/// point spline (#9178).
pub(crate) fn corner_curves(sk: &solidcraft_engine::sketch::Sketch, pi: usize, curves_ok: bool) -> Option<(usize, usize)> {
    use solidcraft_engine::sketch::CurveKind;
    let candidates = sk
        .curves
        .iter()
        .enumerate()
        // The cheap end test first: the axis test scans the constraints.
        .filter(|(ci, c)| c.kind.ends().is_some_and(|(a, b)| a == pi || b == pi) && !sk.is_linked_curve(*ci) && !sk.is_ellipse_axis(*ci));
    let mut curves: Vec<_> = candidates.clone().take(3).collect();
    // Center rectangles have construction diagonals at their corners. Match the engine's
    // corner selection: when more than two curves meet, use the profile sides.
    if curves.len() > 2 {
        curves = candidates.filter(|(_, c)| !c.construction).take(3).collect();
    }
    let takes = |c: &solidcraft_engine::sketch::Curve| match c.kind {
        CurveKind::Line { .. } => true,
        CurveKind::Arc { .. } | CurveKind::Conic { .. } | CurveKind::Spline { periodic: false, control: true, .. } => curves_ok,
        _ => false,
    };
    if curves.iter().any(|(_, c)| !takes(c)) {
        return None;
    }
    match curves[..] {
        [(a, _), (b, _)] => Some((a, b)),
        _ => None,
    }
}

/// Corner tools accept endpoints shared by two editable curves. Pick among those endpoints
/// directly so the origin, a circle centre or an overlapping loose point cannot hide a corner.
pub(crate) fn corner_point(app: &SolveApp, proj: &Proj, pos: Pos2, curves_ok: bool) -> Option<(Vec2, String)> {
    let st = crate::open_meshing::session_world(&app.session);
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
            corner_curves(sk, pi, curves_ok)?;
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
    let st = crate::open_meshing::session_world(&app.session);
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

/// The fewest distinct points a spline closed into a loop takes: 3 for a fit spline, and degree +
/// 1 for a control spline (the engine's rule for a closed control spline).
fn closed_loop_min_points(cmd: &str) -> Option<usize> {
    match cmd {
        "sketch.spline.fit_point" => Some(3),
        "sketch.spline.control_point" => Some(4),
        "sketch.spline.control_point_5" => Some(6),
        _ => None,
    }
}

/// A click with an extra tool.
pub fn periodic_start_snap(app: &SolveApp, tool: &Tool, proj: &Proj, pos: Pos2) -> Option<Vec2> {
    let least = closed_loop_min_points(tool.cmd)?;
    if tool.pts.len() < least || crate::drag_snap::suppressed() {
        return None;
    }
    let first = tool.pts.first()?.0;
    let mut distinct = Vec::new();
    for p in &tool.pts {
        if !distinct.contains(&p.0) {
            distinct.push(p.0);
        }
        if distinct.len() == least {
            break;
        }
    }
    if distinct.len() < least {
        return None;
    }
    let state = crate::open_meshing::session_world(&app.session);
    let plane = &state.sketch(app.session.active_sketch?)?.plane;
    let screen = proj.to_screen(plane.to_world(first))?;
    (screen.distance(pos) <= 8.0).then_some(first)
}

/// A viewport click while a sketch tool is running: picks under `pos` and dispatches on the
/// tool's mode. Sketch Project goes through `project_click`.
pub fn on_click(app: &mut SolveApp, tool: &mut Tool, proj: &Proj, pos: Pos2) {
    let Some(m) = mode(tool.cmd) else { return };
    let hits = pick(app, proj, pos);
    let cmd = tool.cmd;
    let at = tool_point_at(app, proj, pos, Some(cmd));
    match m {
        Mode::Construction => crate::sketch_construction::on_click(app, tool, &hits, at),
        Mode::ModelRef if cmd == "inspect.curvature_comb" => {
            let params = hits.iter().find_map(|h| match h {
                Hit::SketchCurve { sketch, id, .. } => Some(json!({"sketch": sketch, "curves": [id]})),
                Hit::Edge { mid, .. } => Some(json!({"edges": [[mid.x, mid.y, mid.z]]})),
                _ => None,
            });
            if let Some(p) = params {
                comb(app, p);
            }
        }
        Mode::ModelRef if cmd == "inspect.isocurve" => {
            let face = hits.iter().find_map(|h| if let Hit::Face { point, .. } = h { Some(*point) } else { None });
            // The face's curves join those shown (the viewport draws them, `Session::isocurves`); a
            // failure says why on the status line.
            if let Some(at) = face {
                let _ = app.run(cmd, json!({"faces": [[at.x, at.y, at.z]], "add": true}));
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
                let st = crate::open_meshing::session_world(&app.session);
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
            // On the plane of the sketch being edited, the image follows a construction rectangle
            // drawn around it in that sketch (#18405), when the tool's option in the Sketch Palette
            // asks for it (`sketch_palette`, "Frame the image in the sketch"). The engine
            // decides whether the image lies on that plane ("auto"); off it, the image is free.
            // With no sketch open, the image is framed in a new sketch on its own plane (true).
            let frame = app.preferences.frame_images_in_sketch.then(|| if app.session.active_sketch.is_some() { json!("auto") } else { json!(true) });
            let mut p = if decal {
                json!({"path": path, "face": if plane.get("face").is_some() { plane } else { Value::Null }})
            } else {
                json!({"path": path, "plane": plane, "at": [at.x, at.y, at.z]})
            };
            if let (Some(frame), Some(o)) = (frame, p.as_object_mut()) {
                o.insert("frame".into(), frame);
            }
            if let Ok(v) = app.run(cmd, p) {
                IMAGE.with(|i| *i.borrow_mut() = None);
                app.tool = None;
                // A framed image is placed and sized by its rectangle's constraints, not the card.
                if let Some(id) = v["canvas"].as_u64().filter(|_| v.get("frame").is_none()) {
                    app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(app, id, false));
                }
            }
        }
        Mode::ModelRef if cmd == "sketch.project" => project_click(app, proj, pos, &hits),
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
                "sketch.repair_link" => {
                    let Some(link) = REPAIR.with(|l| l.borrow().clone()) else { return };
                    if app.run(cmd, json!({"link": link, "ref": r})).is_ok() {
                        REPAIR.with(|l| *l.borrow_mut() = None);
                        app.set_status(solidcraft_ui_text::tr("sketch.projection_repaired"), false);
                        ENDED.with(|e| e.set(true));
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
                            app.set_tool_hint("Extend: Shift-click the second curve");
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
                    let p = if cmd == "sketch.break" { picked_shared_end(app, &id, &hits).unwrap_or(p) } else { p };
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
                let applied = run_entities(app, tool);
                if applied && tool.preselected {
                    tool.finished = true;
                }
                if !applied && tool.preselected {
                    tool.picks.pop();
                    if tool.pts.len() > tool.picks.len() {
                        tool.pts.pop();
                    }
                } else {
                    tool.picks.clear();
                    tool.pts.clear();
                }
            }
        }
        Mode::Steps => match cmd {
            "sketch.intersection_curve" => {
                // Either side is a face or a curve of another sketch (#11048).
                if let Some(r) = hits.iter().find_map(|h| model_ref(app, h)).filter(|r| r.get("face").is_some() || r.get("curve").is_some()) {
                    tool.picks.push(r.to_string());
                    if tool.picks.len() >= 2 {
                        let a: Value = serde_json::from_str(&tool.picks[0]).unwrap_or(Value::Null);
                        let b: Value = serde_json::from_str(&tool.picks[1]).unwrap_or(Value::Null);
                        // Done: the hint asks for the next pair again (a failure keeps its error).
                        if app.run(cmd, json!({"a": a, "b": b})).is_ok() {
                            app.set_tool_hint(crate::tools::hint(cmd));
                        }
                        tool.picks.clear();
                    } else {
                        app.set_tool_hint(app.tr("ui.intersection_curve_second_pick").to_string());
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
            app.set_status(tr_engine_error(&error), true);
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
        app.set_status(tr_engine_error(&error), true);
        return;
    }
    let p = crate::sketch_dims::placed(app, tool, &p);
    // Tangent Arc starts at the end of a line, arc or spline. A first click anywhere else (on a
    // line between its ends, in empty space) is refused now, saying where to click, instead of
    // failing at the second click on a point the command can't take (#10653).
    if tool.cmd == "sketch.arc.tangent" && tool.pts.is_empty() && !tangent_start(app, &p) {
        app.set_status(hint(tool.cmd).unwrap_or_default(), true);
        return;
    }
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

/// Whether a click picked a sketch point a tangent arc can start at.
fn tangent_start(app: &SolveApp, p: &(Vec2, Option<String>)) -> bool {
    let Some(id) = &p.1 else { return false };
    let state = crate::open_meshing::session_world(&app.session);
    app.session.active_sketch.and_then(|sid| state.sketch(sid)).is_some_and(|ss| solidcraft_engine::cmd::tangent_arc_starts_at(&ss.sketch, id))
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
    // Esc or a right click ends the comb tool and turns its analysis off.
    if tool.cmd == "inspect.curvature_comb" {
        let _ = app.run(tool.cmd, json!({"clear": true}));
        return false;
    }
    let Some(m) = mode(tool.cmd) else { return false };
    match m {
        Mode::Points(0) if tool.pts.len() >= 2 => {
            run_points(app, tool);
            tool.pts.clear();
            true
        }
        Mode::Entities(0) if !tool.picks.is_empty() => {
            let applied = run_entities(app, tool);
            tool.picks.clear();
            tool.pts.clear();
            !(applied && tool.preselected)
        }
        // Intersection Curve: the right-click starts the picks over (#11048).
        Mode::Steps if tool.cmd == "sketch.intersection_curve" && !tool.picks.is_empty() => {
            tool.picks.clear();
            app.set_tool_hint(crate::tools::hint(tool.cmd));
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

type SolvedSketchRef = std::sync::Arc<solidcraft_engine::doc::SolvedSketch>;

/// The shown state's solve of `sketch`, shared.
fn solved_sketch(app: &SolveApp, sketch: u64) -> Option<SolvedSketchRef> {
    crate::open_meshing::session_world(&app.session).sketches.iter().find(|s| s.feature == sketch).cloned()
}

/// Which sketch state a hover cache was made from: the design revision and the solve itself,
/// held weakly so its place is not reused while the cache remembers it (a live drag swaps in a
/// new solve without a new revision).
struct SketchStateKey {
    revision: u64,
    solve: std::sync::Weak<solidcraft_engine::doc::SolvedSketch>,
    curve: String,
}

impl SketchStateKey {
    fn new(app: &SolveApp, solve: &SolvedSketchRef, curve: &str) -> SketchStateKey {
        SketchStateKey { revision: app.session.revision, solve: std::sync::Arc::downgrade(solve), curve: curve.to_string() }
    }
    fn matches(&self, app: &SolveApp, solve: &SolvedSketchRef, curve: &str) -> bool {
        self.revision == app.session.revision && std::ptr::eq(self.solve.as_ptr(), std::sync::Arc::as_ptr(solve)) && self.curve == curve
    }
}

/// Trim's hover over one curve: the cuts on it, made once per sketch state (they intersect the
/// curve with every other one), and the last piece the cursor chose (the viewport's hover and the
/// preview both ask for it each frame).
struct TrimHoverCache {
    key: SketchStateKey,
    hover: Option<solidcraft_engine::cmd::TrimHover>,
    piece: Option<([u64; 2], Option<Vec<Vec2>>)>,
}

/// Extend's preview of one curve, per sketch state and growing end (it depends on the cursor
/// only through the end, [`solidcraft_engine::cmd::extend_end_at`]).
struct ExtendPreviewCache {
    key: SketchStateKey,
    at_b: bool,
    piece: Option<Vec<Vec2>>,
}

thread_local! {
    static TRIM_HOVER: RefCell<Option<TrimHoverCache>> = const { RefCell::new(None) };
    static EXTEND_PREVIEW: RefCell<Option<ExtendPreviewCache>> = const { RefCell::new(None) };
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
    let solve = solved_sketch(app, sketch)?;
    let cursor = [at.x.to_bits(), at.y.to_bits()];
    TRIM_HOVER.with(|c| {
        let mut c = c.borrow_mut();
        let cache = match c.take() {
            Some(cache) if cache.key.matches(app, &solve, id) => c.insert(cache),
            _ => c.insert(TrimHoverCache {
                key: SketchStateKey::new(app, &solve, id),
                hover: solidcraft_engine::cmd::trim_hover(&solve.sketch, id),
                piece: None,
            }),
        };
        if let Some((k, piece)) = &cache.piece
            && *k == cursor
        {
            return piece.clone();
        }
        let piece = cache.hover.as_ref().and_then(|h| h.piece(*at));
        cache.piece = Some((cursor, piece.clone()));
        piece
    })
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
    let solve = solved_sketch(app, sketch)?;
    let sk = &solve.sketch;
    let ci = sk.curve_index(id)?;
    // An infinite line in the way is cut short about the cursor, so the preview then follows it.
    let at_b = solidcraft_engine::cmd::extend_end_at(sk, ci, hover.0).filter(|_| !sk.curves.iter().any(|c| c.infinite));
    let Some(at_b) = at_b else { return extend_preview_uncached(sk, ci, hover.0) };
    EXTEND_PREVIEW.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(cache) = c.as_ref()
            && cache.at_b == at_b
            && cache.key.matches(app, &solve, id)
        {
            return cache.piece.clone();
        }
        let piece = extend_preview_uncached(sk, ci, hover.0);
        *c = Some(ExtendPreviewCache { key: SketchStateKey::new(app, &solve, id), at_b, piece: piece.clone() });
        piece
    })
}

/// [`extend_preview_piece`] worked out afresh.
fn extend_preview_uncached(sk: &solidcraft_engine::sketch::Sketch, ci: usize, hover_pos: Vec2) -> Option<Vec<Vec2>> {
    let kind = sk.curves.get(ci)?.kind.clone();
    // Arcs extend along their circle either way round, up to the first geometry in the way.
    if let solidcraft_engine::sketch::CurveKind::Arc { .. } = kind {
        return solidcraft_engine::cmd::extend_arc_preview(sk, ci, hover_pos);
    }
    let solidcraft_engine::sketch::CurveKind::Line { a, b } = kind else { return None };
    let at_b = hover_pos.dist(sk.point(b)?) <= hover_pos.dist(sk.point(a)?);
    let end = sk.point(if at_b { b } else { a })?;
    let hit = solidcraft_engine::cmd::extend_preview(sk, ci, hover_pos)?;
    Some(vec![end, hit])
}

/// Rubber band for point tools and pick markers.
pub fn preview(app: &SolveApp, t: &Tool, painter: &egui::Painter, proj: &Proj) {
    if solidcraft_engine::cmd::construction_line_mode(t.cmd).is_some() {
        crate::sketch_construction::preview(app, t, painter, proj);
        return;
    }
    let Some(sid) = active_sketch(app) else { return };
    let st = crate::open_meshing::session_world(&app.session);
    let Some(ss) = st.sketch(sid) else { return };
    let to = |p: Vec2| proj.to_screen(ss.plane.to_world(p));
    let tk = solidcraft_theme::Tokens::get();
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
    if !crate::tools::is_constraint_pick_tool(t)
        && let Some((hp, snap)) = &t.hover
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
    // Picked curves and points highlighted with normal selection styling.
    let mut display_budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
    for id in t.picks.iter().filter(|x| *x != "|") {
        if let Some(ci) = ss.sketch.curve_index(id) {
            crate::tools::highlight_curve(&ss.sketch, ci, &ss.plane, painter, proj, &mut display_budget);
        } else if let Some(sp) = ss.sketch.resolve_point(id).and_then(|pi| ss.sketch.point(pi)).and_then(to) {
            crate::tools::highlight_point(painter, sp);
        }
    }
}

/// Constraint glyphs of the active sketch: a small badge per constraint at its anchor
/// (failing ones in `sketch_conflict`).
/// Glyph size and spacing (screen pixels) at the default `constraint_glyph_scale` of 1.
const GLYPH: f32 = 15.0;

/// The `constraint_glyph_scale` values Preferences › Sketch offers and the glyphs honour (#10794).
pub const GLYPH_SCALES: std::ops::RangeInclusive<f32> = 1.0..=3.0;

/// The side of a constraint glyph on screen for the `constraint_glyph_scale` preference. A value
/// outside [`GLYPH_SCALES`] (a hand-edited settings file) is brought into it; one that is not a
/// number gives the default size.
pub fn glyph_px(scale: f32) -> f32 {
    let scale = if scale.is_finite() { scale.clamp(*GLYPH_SCALES.start(), *GLYPH_SCALES.end()) } else { 1.0 };
    GLYPH * scale
}

/// How far below and to the right of the pointer's hot spot a cursor can reach (screen pixels):
/// an arrow or a hand hangs down and to the right of the point it marks.
const CURSOR_REACH: f32 = 32.0;

/// The widest the hovered glyph's label text may be in `bounds`: the view less the label's
/// 3-pixel padding on each side (#10794).
fn glyph_label_wrap_width(bounds: egui::Rect) -> f32 {
    (bounds.width() - 6.0).max(1.0)
}

/// Where the hovered glyph's label goes (its background, `size` included): above the glyph and the
/// pointer, where no cursor reaches, so the cursor never hides it (#10794). Without room above in
/// `bounds`, it goes to the left of both; without room there either, below them, past the cursor,
/// or to the right of the cursor. When none of those fits (a wrapped label in a short view), it is
/// cut to whichever of those sides of the view has the most room, still clear of the cursor; only
/// when no side has any room is it kept inside `bounds` regardless, cut to the view's height.
pub fn glyph_label_rect(pointer: Pos2, glyph: egui::Rect, size: egui::Vec2, bounds: egui::Rect) -> egui::Rect {
    const GAP: f32 = 6.0;
    let fit_x = |left: f32| left.min(bounds.right() - size.x).max(bounds.left());
    let fit_y = |top: f32| top.min(bounds.bottom() - size.y).max(bounds.top());
    let above = pointer.y.min(glyph.top()) - GAP - size.y;
    if above >= bounds.top() {
        return egui::Rect::from_min_size(egui::pos2(fit_x(glyph.left()), above), size);
    }
    let left = pointer.x.min(glyph.left()) - GAP - size.x;
    if left >= bounds.left() && size.y <= bounds.height() {
        let top = fit_y(glyph.center().y - size.y * 0.5);
        return egui::Rect::from_min_size(egui::pos2(left, top), size);
    }
    let below = (pointer.y + CURSOR_REACH).max(glyph.bottom()) + GAP;
    if below + size.y <= bounds.bottom() {
        return egui::Rect::from_min_size(egui::pos2(fit_x(glyph.left()), below), size);
    }
    let right = (pointer.x + CURSOR_REACH).max(glyph.right()) + GAP;
    if right + size.x <= bounds.right() && size.y <= bounds.height() {
        return egui::Rect::from_min_size(egui::pos2(right, fit_y(glyph.center().y - size.y * 0.5)), size);
    }
    // No side holds the whole label: cut it to the cursor-free side of the view with the most
    // room, so it never covers the pointer or the cursor.
    let beside = glyph.center().y - size.y * 0.5;
    let sides = [
        (egui::Rect::from_min_max(bounds.min, egui::pos2(bounds.right(), above + size.y)), egui::pos2(glyph.left(), above)),
        (egui::Rect::from_min_max(bounds.min, egui::pos2(left + size.x, bounds.bottom())), egui::pos2(left, beside)),
        (egui::Rect::from_min_max(egui::pos2(bounds.left(), below), bounds.max), egui::pos2(glyph.left(), below)),
        (egui::Rect::from_min_max(egui::pos2(right, bounds.top()), bounds.max), egui::pos2(right, beside)),
    ];
    let room = |side: &egui::Rect| side.width().clamp(0.0, size.x) * side.height().clamp(0.0, size.y);
    let mut best: Option<(egui::Rect, egui::Pos2)> = None;
    for (side, at) in sides {
        if room(&side) > best.map_or(0.0, |(b, _)| room(&b)) {
            best = Some((side, at));
        }
    }
    if let Some((side, at)) = best {
        let x = at.x.min(side.right() - size.x).max(side.left());
        let y = at.y.min(side.bottom() - size.y).max(side.top());
        return egui::Rect::from_min_size(egui::pos2(x, y), size).intersect(side);
    }
    let fitted = egui::Rect::from_min_size(egui::pos2(fit_x(glyph.left()), fit_y(below)), size);
    egui::Rect::from_min_max(fitted.min, egui::pos2(fitted.max.x, fitted.max.y.min(bounds.bottom())))
}

/// Screen pixels per millimetre of the sketch plane below which constraint glyphs hide when the
/// `hide_constraint_glyphs_zoomed_out` preference is on (#8980). About two pixels per millimetre:
/// a 10 mm feature is then smaller than a glyph.
pub const MIN_GLYPH_PX_PER_MM: f32 = 2.0;

/// Below `MIN_GLYPH_PX_PER_MM`, glyphs still draw while the sketch's points and curves span at
/// least this many screen pixels (the larger side of their bounds), twenty glyphs across (#10518):
/// a large sketch that still fills the view keeps its glyphs; a small one far away loses them, as
/// before.
pub const MIN_GLYPH_SPAN_PX: f32 = 20.0 * GLYPH;

/// Whether constraint glyphs should draw at this sketch-plane scale (#8980), given how large the
/// sketch is on screen (`span_px`, only measured when zoomed out, #10518).
pub fn constraint_glyphs_visible_at_scale(px_per_mm: f32, hide_when_zoomed_out: bool, span_px: impl FnOnce() -> f32) -> bool {
    !hide_when_zoomed_out || px_per_mm >= MIN_GLYPH_PX_PER_MM || span_px() >= MIN_GLYPH_SPAN_PX
}

/// The larger side, in screen pixels, of the bounds of the active sketch's points and curves. It
/// stops measuring once that reaches `MIN_GLYPH_SPAN_PX`, so a large sketch costs no more.
fn sketch_span_px(ss: &solidcraft_engine::doc::SolvedSketch, proj: &Proj) -> f32 {
    let sk = &ss.sketch;
    let at = sk.points.iter().map(|p| p.pos).chain((0..sk.curves.len()).flat_map(|c| sk.polyline(c)));
    let mut r = egui::Rect::NOTHING;
    for q in at {
        if let Some(p) = proj.to_screen(ss.plane.to_world(q)) {
            r.extend_with(p);
            if r.width().max(r.height()) >= MIN_GLYPH_SPAN_PX {
                return r.width().max(r.height());
            }
        }
    }
    if r.min.x <= r.max.x { r.width().max(r.height()) } else { 0.0 }
}

/// The constraints that draw a glyph: no dimensions, one MidPoint glyph where lines' midpoints
/// meet, and a pattern's constraints once, on its first instance.
fn glyph_constraints(sk: &solidcraft_engine::sketch::Sketch) -> impl Iterator<Item = &solidcraft_engine::sketch::Constraint> {
    let mut midpoints = std::collections::HashSet::new();
    sk.constraints.iter().filter(move |c| {
        if c.kind.is_dimension() {
            return false;
        }
        if let solidcraft_engine::sketch::ConstraintKind::Midpoint { p, .. } = c.kind
            && !c.driven
            && !midpoints.insert(p)
        {
            return false;
        }
        !(matches!(
            c.kind,
            solidcraft_engine::sketch::ConstraintKind::PatternPoint { .. } | solidcraft_engine::sketch::ConstraintKind::PatternRadius { .. }
        ) && !sk.patterns.iter().any(|p| p.instances.first().and_then(|i| i.constraints.first()) == Some(&c.id)))
    })
}

/// Pixels on screen for one millimetre along the sketch plane's U axis, near the plane origin.
fn sketch_px_per_mm(plane: &Plane, proj: &Proj) -> Option<f32> {
    let a = proj.to_screen(plane.to_world(Vec2::ZERO))?;
    let b = proj.to_screen(plane.to_world(Vec2::new(1.0, 0.0)))?;
    Some(a.distance(b))
}

fn glyphs(app: &SolveApp, painter: &egui::Painter, proj: &Proj, hover: Option<Pos2>) {
    let Some(sid) = active_sketch(app) else {
        DETACH_PREVIEW.with(|c| *c.borrow_mut() = None);
        return;
    };
    // While a dimension's value is typed, the glyphs sit where that value puts their geometry.
    let st = crate::dim_view::preview_world(app).unwrap_or_else(|| crate::open_meshing::session_world(&app.session));
    let Some(ss) = st.sketch(sid) else {
        DETACH_PREVIEW.with(|c| *c.borrow_mut() = None);
        return;
    };
    // The badge's side, from Preferences › Sketch (#10794); 15 px at the default.
    let glyph = glyph_px(app.preferences.constraint_glyph_scale);
    let scale = glyph / GLYPH;
    GLYPH_PX.with(|g| g.set(glyph));
    if ss.sketch.view.hide_constraints {
        GLYPHS.with(|g| g.borrow_mut().clear());
        DETACH_PREVIEW.with(|c| *c.borrow_mut() = None);
        return;
    }
    // Zoomed out far enough: fixed-size glyphs would dominate the sketch (#8980), unless the
    // sketch still spreads them across the view (#10518).
    let px_per_mm = sketch_px_per_mm(&ss.plane, proj);
    if let Some(scale) = px_per_mm
        && !constraint_glyphs_visible_at_scale(scale, app.preferences.hide_constraint_glyphs_zoomed_out, || sketch_span_px(ss, proj))
    {
        GLYPHS.with(|g| g.borrow_mut().clear());
        DETACH_PREVIEW.with(|c| *c.borrow_mut() = None);
        return;
    }
    let tk = solidcraft_theme::Tokens::get();
    let picked = PICKED.with(|p| p.borrow().clone());
    // Where each glyph goes: beside its entity (to its left, away from the line), every entity
    // of a two-entity constraint getting one; glyphs that would overlap step along. A glyph
    // that would sit on a dimension's value goes to the other side of its line.
    let dim_texts: Vec<egui::Rect> = crate::dim_view::text_rects().into_iter().map(|(_, r)| r.expand(2.0)).collect();
    let on_dim = |p: Pos2| {
        let r = egui::Rect::from_center_size(p, egui::vec2(glyph, glyph));
        dim_texts.iter().any(|d| d.intersects(r))
    };
    let mut placed: Vec<(String, Pos2, Pos2)> = Vec::new();
    for c in glyph_constraints(&ss.sketch) {
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
            let free = |sp: Pos2| !placed.iter().any(|(_, q, _)| q.distance(sp) < glyph + 1.0);
            let start = |s: egui::Vec2| base + s * (glyph * 0.9);
            let side = if on_dim(start(side)) && !on_dim(start(-side)) { -side } else { side };
            // Step along until clear of other glyphs, preferring a spot clear of dimension
            // values too.
            let settle = |avoid_dims: bool| {
                let mut sp = start(side);
                for _ in 0..12 {
                    if free(sp) && !(avoid_dims && on_dim(sp)) {
                        return Some(sp);
                    }
                    sp += step * (glyph + 2.0);
                }
                None
            };
            let sp = settle(true).or_else(|| settle(false)).unwrap_or_else(|| start(side) + step * (12.0 * (glyph + 2.0)));
            placed.push((c.id.clone(), sp, base));
        }
    }
    // The hovered glyph instance (a Parallel draws one per entity, all with the same id).
    let hovered_at =
        hover.and_then(|h| nearest_glyph(placed.iter().map(|(id, q, _)| (id.as_str(), *q)), h, glyph * 0.6)).map(|(id, q)| (id.to_owned(), q));
    let hovered = hovered_at.as_ref().map(|(id, _)| id.clone());
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
        if sp.distance(*base) > glyph * 1.6 {
            painter.line_segment([*base, *sp], Stroke::new(0.8, tk.text_dim.gamma_multiply(0.6)));
        }
        let r = egui::Rect::from_center_size(*sp, egui::vec2(glyph, glyph));
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
        if let Some(icon) = pattern_icon.or_else(|| glyph_icon(c.kind.name())) {
            crate::icons::paint_icon(painter, r.shrink(2.0 * scale), icon, f32::from(col.a()) / 255.0);
        }
        drawn.push((id.clone(), *sp));
    }
    // Simulated coincident break-off glyph (selected curve + hovered shared endpoint).
    let mut detach_tooltip: Option<(egui::Rect, String)> = None;
    let next_detach = resolve_detach_target(app, sid, &ss.sketch, hover, scale).and_then(|(curve_id, pi, point_id)| {
        let base = proj.to_screen(ss.plane.to_world(ss.sketch.point(pi)?))?;
        let point_rect = egui::Rect::from_center_size(base, egui::vec2(6.0 * scale, 6.0 * scale));
        let reused = DETACH_PREVIEW.with(|c| {
            c.borrow()
                .as_ref()
                .filter(|p| p.curve_id == curve_id && p.point_id == point_id && p.point_rect.center().distance(base) < 2.0)
                .map(|p| p.glyph_rect)
        });
        let glyph_rect = reused.unwrap_or_else(|| {
            let dirs = [
                egui::vec2(0.7, -0.7),
                egui::vec2(-0.7, -0.7),
                egui::vec2(0.7, 0.7),
                egui::vec2(-0.7, 0.7),
                egui::vec2(1.0, 0.0),
                egui::vec2(-1.0, 0.0),
                egui::vec2(0.0, -1.0),
                egui::vec2(0.0, 1.0),
            ];
            let free = |sp: Pos2| !placed.iter().any(|(_, q, _)| q.distance(sp) < glyph + 2.0) && !on_dim(sp);
            let center = dirs.iter().map(|d| base + *d * (glyph * 1.15)).find(|&sp| free(sp)).unwrap_or(base + dirs[0] * (glyph * 1.15));
            egui::Rect::from_center_size(center, egui::vec2(glyph, glyph))
        });
        let lit = hover.is_some_and(|h| glyph_rect.expand(2.0).contains(h));
        painter.line_segment([base, glyph_rect.center()], Stroke::new(1.0, tk.accent.gamma_multiply(0.8)));
        let rounding = tk.radius_sm.saturating_sub(1);
        painter.rect_filled(glyph_rect, rounding, if lit { tk.accent_soft } else { tk.glyph_bg });
        painter.rect_stroke(glyph_rect, rounding, Stroke::new(1.2, tk.accent), egui::StrokeKind::Inside);
        crate::icons::paint_icon(painter, glyph_rect.shrink(2.0 * scale), "c_coincident", 1.0);
        if lit {
            detach_tooltip = Some((glyph_rect, format!("Coincident on {curve_id} (right-click to detach)")));
        }
        let distance_mm = 10.0 / px_per_mm.unwrap_or(1.0).max(1e-3) as f64;
        Some(DetachPreview { curve_id, point_id, point_rect, glyph_rect, distance_mm })
    });
    DETACH_PREVIEW.with(|c| *c.borrow_mut() = next_detach);

    if let (Some((anchor_rect, label)), Some(h)) = (detach_tooltip, hover) {
        let g =
            painter.layout(label, egui::FontId::proportional(solidcraft_theme::Tokens::get().font_small), tk.text, glyph_label_wrap_width(proj.rect));
        let chip = glyph_label_rect(h, anchor_rect, g.size() + egui::vec2(6.0, 6.0), proj.rect);
        crate::scenario::publish_rect("sketch:glyph_label", chip);
        painter.rect_filled(chip, tk.radius_sm.saturating_sub(1), tk.panel);
        painter.with_clip_rect(chip.intersect(painter.clip_rect())).galley(chip.min + egui::vec2(3.0, 3.0), g, tk.text);
    } else if let (Some((id, at_glyph)), Some(h)) = (hovered_at, hover)
        && let Some(c) = ss.sketch.constraints.iter().find(|c| c.id == id)
    {
        let ids = glyph_group(&ss.sketch, &id).join(", ");
        let label = if ss.report.redundant.contains(&id) {
            format!("{} ({ids}), redundant: already implied by other constraints", c.kind.name())
        } else {
            format!("{} ({ids})", c.kind.name())
        };
        // Wrapped to the view's width so a narrow view never clips it (#10794).
        let g =
            painter.layout(label, egui::FontId::proportional(solidcraft_theme::Tokens::get().font_small), tk.text, glyph_label_wrap_width(proj.rect));
        // Clear of the cursor, which hangs below and right of the pointer, and next to the
        // glyph instance under it (#10794).
        let chip = glyph_label_rect(h, egui::Rect::from_center_size(at_glyph, egui::vec2(glyph, glyph)), g.size() + egui::vec2(6.0, 6.0), proj.rect);
        crate::scenario::publish_rect("sketch:glyph_label", chip);
        painter.rect_filled(chip, tk.radius_sm.saturating_sub(1), tk.panel);
        // A label cut to fit beside the cursor shows only what fits in its chip.
        painter.with_clip_rect(chip.intersect(painter.clip_rect())).galley(chip.min + egui::vec2(3.0, 3.0), g, tk.text);
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
        PointOnCurve { p, c } | PointAlong { p, c, .. } => (vec![c], vec![p]),
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
        Coincident { p, .. } | Fix { p } | Midpoint { p, .. } | PointOnCurve { p, .. } | PointAlong { p, .. } => pt(p).into_iter().collect(),
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

/// Active snap/constraint labels at the cursor (in display order), for tests.
#[cfg(test)]
pub(crate) fn current_snap_labels() -> Vec<&'static str> {
    SNAP.with(|s| s.borrow().as_ref().map(|snap| snap.labels.clone()).unwrap_or_default())
}

/// Place a snap/constraint preview badge row near `sp` without overlapping placed constraint glyphs
/// or the tool-cursor badge.
fn clear_snap_badge_rect(app: &SolveApp, painter: &egui::Painter, view_rect: egui::Rect, sp: egui::Pos2, size: egui::Vec2, glyph: f32) -> egui::Rect {
    let default_rect = crate::tool_cursor::clear_of_badge(app, painter, view_rect, egui::Rect::from_min_size(sp + egui::vec2(12.0, -24.0), size));
    let glyph_rects: Vec<egui::Rect> =
        GLYPHS.with(|g| g.borrow().iter().map(|(_, c)| egui::Rect::from_center_size(*c, egui::vec2(glyph, glyph)).expand(2.0)).collect());
    let clear_of_glyphs = |cand: egui::Rect| !glyph_rects.iter().any(|g| g.intersects(cand));
    if clear_of_glyphs(default_rect) {
        return default_rect;
    }
    let step = glyph + 4.0;
    let mut mins = Vec::with_capacity(28);
    for k in 1..=4 {
        let d = step * k as f32;
        mins.push(default_rect.min + egui::vec2(0.0, -d));
        mins.push(default_rect.min + egui::vec2(d, 0.0));
    }
    mins.push(egui::pos2(sp.x - 12.0 - size.x, sp.y - 24.0));
    mins.push(egui::pos2(sp.x - 12.0 - size.x, sp.y + 10.0));
    mins.push(egui::pos2(sp.x + 12.0, sp.y + 32.0));
    for k in 1..=3 {
        let d = step * k as f32;
        mins.push(egui::pos2(sp.x - 12.0 - size.x, sp.y - 24.0 - d));
        mins.push(egui::pos2(sp.x - 12.0 - size.x - d, sp.y - 24.0));
        mins.push(egui::pos2(sp.x - 12.0 - size.x, sp.y + 10.0 + d));
        mins.push(egui::pos2(sp.x + 12.0, sp.y + 32.0 + d));
    }
    for require_in_view in [true, false] {
        for min in &mins {
            let cand = crate::tool_cursor::clear_of_badge(app, painter, view_rect, egui::Rect::from_min_size(*min, size));
            if clear_of_glyphs(cand) && (!require_in_view || view_rect.contains_rect(cand)) {
                return cand;
            }
        }
    }
    default_rect
}

/// Paint one or more snap/constraint preview badges next to `sp`, scaled with `constraint_glyph_scale`.
pub(crate) fn paint_snap_badges(
    app: &SolveApp,
    painter: &egui::Painter,
    view_rect: egui::Rect,
    sp: egui::Pos2,
    labels: &[&'static str],
    publish_hint: bool,
) {
    let items: Vec<(&'static str, Option<&'static str>)> = labels.iter().map(|&label| (label, snap_icon(label))).collect();
    paint_badge_row(app, painter, view_rect, sp, &items, if publish_hint { "viewport:snap_hint" } else { "viewport:drag_snap_badge" });
}

/// Paint the active constraint/dimension operator icon beside the cursor using the exact same
/// style and glyph-scale sizing as line placement's constraint previews.
pub(crate) fn paint_constraint_cursor(app: &SolveApp, painter: &egui::Painter, proj: &Proj, at: Option<egui::Pos2>, shift: bool) {
    let (Some(at), Some(tool)) = (at, app.tool.as_ref()) else { return };
    let Some(icon) = crate::tools::constraint_cursor_icon(app, tool, proj, at, shift) else { return };
    paint_badge_row(app, painter, proj.rect, at, &[("", Some(icon))], "viewport:constraint_cursor");
}

fn paint_badge_row(
    app: &SolveApp,
    painter: &egui::Painter,
    view_rect: egui::Rect,
    sp: egui::Pos2,
    items: &[(&'static str, Option<&'static str>)],
    target: &'static str,
) {
    if items.is_empty() {
        return;
    }
    let tk = solidcraft_theme::Tokens::get();
    let glyph = glyph_px(app.preferences.constraint_glyph_scale);
    let scale = glyph / GLYPH;
    let gap = (3.0 * scale).round();
    let badges: Vec<(&'static str, Option<&'static str>, f32)> = items
        .iter()
        .map(|&(label, icon)| {
            let w = if icon.is_some() { glyph } else { (24.0 * scale).max(glyph) };
            (label, icon, w)
        })
        .collect();
    let total_w: f32 = badges.iter().map(|(_, _, w)| *w).sum::<f32>() + gap * badges.len().saturating_sub(1) as f32;
    let r = clear_snap_badge_rect(app, painter, view_rect, sp, egui::vec2(total_w, glyph), glyph);
    crate::scenario::publish_rect(target, r);
    let rounding = tk.radius_sm.saturating_sub(1);
    let mut x = r.left();
    for (label, icon, w) in badges {
        let badge = egui::Rect::from_min_size(egui::pos2(x, r.top()), egui::vec2(w, glyph));
        painter.rect_filled(badge, rounding, tk.glyph_bg);
        painter.rect_stroke(badge, rounding, Stroke::new(1.0, tk.accent), egui::StrokeKind::Inside);
        match icon {
            Some(icon) => crate::icons::paint_accented(painter, badge.shrink(2.0 * scale), icon, None, 1.0),
            None => {
                painter.text(badge.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(tk.font_micro * scale), tk.accent);
            }
        }
        x += w + gap;
    }
}

/// The inference badge(s) next to the snapped cursor (while a drawing tool is active), plus
/// highlights on every affected curve or point.
fn snap_hint(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if app.tool.as_ref().is_none_or(crate::tools::is_constraint_pick_tool) {
        return;
    }
    let Some(snap) = SNAP.with(|s| s.borrow().clone()) else { return };
    if snap.labels.is_empty() {
        return;
    }
    let st = crate::open_meshing::session_world(&app.session);
    let Some(ss) = active_sketch(app).and_then(|s| st.sketch(s)) else { return };
    let Some(sp) = proj.to_screen(ss.plane.to_world(snap.at)) else { return };
    // Avoid double-stroking curves already highlighted by `inference::show` (which runs on `sketch.line`).
    let inf_curves: Vec<String> = (app.tool.as_ref().is_some_and(|t| t.cmd == "sketch.line"))
        .then(crate::inference::all)
        .unwrap_or_default()
        .into_iter()
        .map(|i| i.curve)
        .collect();
    let mut display_budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
    for target in &snap.targets {
        match target {
            SnapTarget::Curve(id) if !inf_curves.iter().any(|c| c == id) => {
                if let Some(ci) = ss.sketch.curve_index(id) {
                    crate::tools::highlight_curve(&ss.sketch, ci, &ss.plane, painter, proj, &mut display_budget);
                }
            }
            SnapTarget::Point(pt) => {
                if let Some(pt_sp) = proj.to_screen(ss.plane.to_world(*pt)) {
                    crate::tools::highlight_point(painter, pt_sp);
                }
            }
            SnapTarget::Curve(_) => {}
        }
    }
    let tk = solidcraft_theme::Tokens::get();
    // A dotted guide from each point the snap lined the cursor up with (#7828).
    let guides = LINED.with(|l| l.borrow().as_ref().filter(|(at, _)| at.dist(snap.at) < 1e-9).map(|(_, lined)| lined.clone()).unwrap_or_default());
    for from in guides.iter().filter_map(|l| proj.to_screen(ss.plane.to_world(l.at))) {
        painter.add(egui::Shape::dashed_line(&[from, sp], Stroke::new(1.0, tk.rubber_band), 5.0, 4.0));
    }
    crate::scenario::publish_count("sketch_align_guides", guides.len() as f64);
    paint_snap_badges(app, painter, proj.rect, sp, &snap.labels, true);
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
    comb_options_card(app, ui.ctx(), proj);
    if DEFERRED.with(|d| d.replace(false)) {
        analysis(app, "inspect.center_of_mass", json!({}));
    }
    marks(app, painter, proj);
}

#[cfg(test)]
mod glyph_zoom_tests {
    use super::{
        CURSOR_REACH, GLYPH, MIN_GLYPH_PX_PER_MM, MIN_GLYPH_SPAN_PX, constraint_glyphs_visible_at_scale, glyph_label_rect, glyph_label_wrap_width,
        glyph_px, nearest_glyph,
    };

    /// A sketch that is small on screen.
    fn small() -> f32 {
        40.0
    }

    #[test]
    fn glyphs_stay_visible_when_zoomed_in() {
        assert!(constraint_glyphs_visible_at_scale(MIN_GLYPH_PX_PER_MM, true, small));
        assert!(constraint_glyphs_visible_at_scale(10.0, true, small));
    }

    #[test]
    fn glyphs_hide_below_the_zoom_threshold() {
        assert!(!constraint_glyphs_visible_at_scale(MIN_GLYPH_PX_PER_MM - f32::EPSILON, true, small));
        assert!(!constraint_glyphs_visible_at_scale(0.5, true, small));
        assert!(!constraint_glyphs_visible_at_scale(0.5, true, || MIN_GLYPH_SPAN_PX - 1.0));
    }

    #[test]
    fn preference_off_keeps_glyphs_at_every_zoom() {
        assert!(constraint_glyphs_visible_at_scale(0.1, false, small));
        assert!(constraint_glyphs_visible_at_scale(MIN_GLYPH_PX_PER_MM - 1.0, false, small));
    }

    /// A 500 mm sketch zoomed to fit is below two pixels a millimetre yet fills the view: its
    /// glyphs stay (#10518).
    #[test]
    fn glyphs_stay_while_a_large_sketch_fills_the_view() {
        assert!(constraint_glyphs_visible_at_scale(1.5, true, || 500.0 * 1.5));
        assert!(constraint_glyphs_visible_at_scale(0.05, true, || MIN_GLYPH_SPAN_PX));
    }

    /// A Divide Curve point's constraint has a glyph on its point, an icon, and lights its point
    /// and curve, as a point-on-curve constraint does (#8969).
    #[test]
    fn a_point_along_constraint_has_a_glyph_on_its_point() {
        use solidcraft_engine::sketch::{ConstraintKind, CurveKind, Sketch};
        let mut sk = Sketch::new();
        let (Ok(a), Ok(b), Ok(q)) = (
            sk.add_point(super::Vec2::new(0.0, 0.0), None),
            sk.add_point(super::Vec2::new(10.0, 0.0), None),
            sk.add_point(super::Vec2::new(2.5, 0.0), None),
        ) else {
            panic!("points");
        };
        let Ok(l) = sk.add_curve(CurveKind::Line { a, b }, None) else { panic!("line") };
        let k = ConstraintKind::PointAlong { p: q, c: l, at: 0.25 };
        assert_eq!(super::constraint_entities(&k), (vec![l], vec![q]));
        let anchors = super::glyph_anchors(&sk, &k);
        assert_eq!(anchors.len(), 1);
        assert!(anchors.first().is_some_and(|(at, dir)| (at.x - 2.5).abs() < 1e-9 && at.y.abs() < 1e-9 && dir.is_none()));
        assert_eq!(crate::icons::markers::glyph_icon(k.name()), Some("c_midpoint"));
    }

    #[test]
    fn glyph_size_follows_the_preference_and_defaults_to_the_old_size() {
        assert_eq!(glyph_px(crate::prefs::Prefs::default().constraint_glyph_scale), GLYPH);
        assert_eq!(GLYPH, 15.0, "the default badge keeps its size (#10794)");
        assert_eq!(glyph_px(2.0), 30.0);
        // A hand-edited settings file can hold anything: it never gives a glyph out of range.
        for hostile in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -4.0, 0.0, 1e9] {
            let px = glyph_px(hostile);
            assert!((GLYPH..=GLYPH * 3.0).contains(&px), "{hostile} gives {px}");
        }
    }

    /// The hovered glyph's label never sits where the cursor hangs, below and right of the
    /// pointer, wherever in the view the glyph is (#10794).
    #[test]
    fn glyph_label_stays_clear_of_the_cursor() {
        let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let size = egui::vec2(140.0, 20.0);
        for glyph_px in [GLYPH, GLYPH * 3.0] {
            for x in [0.0, 5.0, 60.0, 400.0, 790.0, 800.0] {
                for y in [0.0, 5.0, 30.0, 300.0, 590.0, 600.0] {
                    let glyph = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(glyph_px, glyph_px));
                    for pointer in [glyph.center(), glyph.left_top(), glyph.right_bottom(), glyph.center_top()] {
                        let cursor = egui::Rect::from_min_size(pointer, egui::vec2(CURSOR_REACH, CURSOR_REACH));
                        let label = glyph_label_rect(pointer, glyph, size, view);
                        assert!(!label.intersects(cursor), "label {label:?} under the cursor at {pointer:?}");
                        assert!(!label.contains(pointer), "label {label:?} on the pointer {pointer:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn glyph_label_goes_above_and_stays_in_the_view() {
        let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let size = egui::vec2(140.0, 20.0);
        let glyph = egui::Rect::from_center_size(egui::pos2(400.0, 300.0), egui::vec2(GLYPH, GLYPH));
        let label = glyph_label_rect(glyph.center(), glyph, size, view);
        assert!(label.bottom() < glyph.top(), "above the glyph: {label:?}");
        // At the right edge it moves left to stay readable.
        let glyph = egui::Rect::from_center_size(egui::pos2(795.0, 300.0), egui::vec2(GLYPH, GLYPH));
        assert!(view.contains_rect(glyph_label_rect(glyph.center(), glyph, size, view)));
    }

    /// At 300% the hit regions of neighbouring glyphs overlap: the pointer picks the glyph it is
    /// nearest, not whichever was placed first (#10794).
    #[test]
    fn overlapping_glyphs_pick_the_nearest() {
        let reach = glyph_px(3.0) * 0.6;
        let glyphs = [("c1", egui::pos2(100.0, 100.0)), ("c2", egui::pos2(147.0, 100.0))];
        let pointer = egui::pos2(125.0, 100.0);
        assert!(glyphs.iter().all(|(_, c)| c.distance(pointer) <= reach), "the test needs overlapping reach");
        assert_eq!(nearest_glyph(glyphs, pointer, reach).map(|(id, _)| id), Some("c2"));
        assert_eq!(nearest_glyph(glyphs, egui::pos2(110.0, 100.0), reach).map(|(id, _)| id), Some("c1"));
        assert_eq!(nearest_glyph(glyphs, egui::pos2(400.0, 400.0), reach), None);
    }

    /// A Parallel draws a glyph per entity under one id: the hovered instance is the one the
    /// label goes next to, not the first one drawn (#10794).
    #[test]
    fn hovered_instance_of_a_shared_glyph_is_the_one_under_the_pointer() {
        let far = egui::pos2(500.0, 400.0);
        let glyphs = [("parallel", egui::pos2(100.0, 100.0)), ("parallel", far)];
        let (_, at) = nearest_glyph(glyphs, far + egui::vec2(3.0, 2.0), GLYPH * 0.6).unwrap_or(("", egui::Pos2::ZERO));
        assert_eq!(at, far);
        let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let glyph = egui::Rect::from_center_size(at, egui::vec2(GLYPH, GLYPH));
        let label = glyph_label_rect(far, glyph, egui::vec2(140.0, 20.0), view);
        assert!(label.center().distance(far) < 100.0, "label {label:?} beside the hovered glyph");
    }

    /// A label wrapped to the view's width fits a view narrower than its one-line text.
    #[test]
    fn glyph_label_fits_a_narrow_view() {
        let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(120.0, 600.0));
        let size = egui::vec2(glyph_label_wrap_width(view) + 6.0, 40.0);
        for x in [0.0, 60.0, 120.0] {
            let glyph = egui::Rect::from_center_size(egui::pos2(x, 300.0), egui::vec2(GLYPH, GLYPH));
            let label = glyph_label_rect(glyph.center(), glyph, size, view);
            assert!(label.left() >= view.left() && label.right() <= view.right(), "label {label:?} in {view:?}");
        }
    }

    /// A wrapped label in a short view stays inside it wherever the glyph is, and one taller than
    /// the view is cut to the view's height (#10794).
    #[test]
    fn glyph_label_stays_inside_a_short_view() {
        let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 120.0));
        for size in [egui::vec2(140.0, 80.0), egui::vec2(294.0, 80.0), egui::vec2(140.0, 200.0)] {
            for x in [0.0, 150.0, 300.0] {
                for y in [0.0, 10.0, 60.0, 110.0, 120.0] {
                    let glyph = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(GLYPH, GLYPH));
                    for pointer in [glyph.center(), glyph.left_top(), glyph.right_bottom()] {
                        let label = glyph_label_rect(pointer, glyph, size, view);
                        assert!(view.contains_rect(label), "label {label:?} of {size:?} outside {view:?}");
                        assert!(label.width() > 0.0 && label.height() > 0.0, "label {label:?} vanished");
                    }
                }
            }
        }
    }

    /// A label that fits on no side of a short view (above, left, below, right) is cut to a side
    /// with room rather than laid over the pointer and the cursor (#10794).
    #[test]
    fn glyph_label_too_big_for_a_short_view_stays_clear_of_the_cursor() {
        let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 120.0));
        for size in [egui::vec2(140.0, 80.0), egui::vec2(294.0, 80.0), egui::vec2(140.0, 200.0)] {
            for x in [0.0, 5.0, 150.0, 290.0, 300.0] {
                for y in [0.0, 5.0, 60.0, 110.0, 120.0] {
                    let glyph = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(GLYPH, GLYPH));
                    for pointer in [glyph.center(), glyph.left_top(), glyph.right_bottom()] {
                        let cursor = egui::Rect::from_min_size(pointer, egui::vec2(CURSOR_REACH, CURSOR_REACH));
                        let label = glyph_label_rect(pointer, glyph, size, view);
                        assert!(view.contains_rect(label), "label {label:?} of {size:?} outside {view:?}");
                        assert!(!label.intersects(cursor), "label {label:?} of {size:?} under the cursor at {pointer:?}");
                        assert!(label.width() > 0.0 && label.height() > 0.0, "label {label:?} vanished");
                    }
                }
            }
        }
    }

    #[test]
    fn constraint_glyph_and_snap_icons_cover_every_kind_without_placeholders() {
        for name in [
            "Coincident",
            "Collinear",
            "Concentric",
            "Equal",
            "Fix",
            "Horizontal",
            "MidPoint",
            "Parallel",
            "Perpendicular",
            "Pierce",
            "PointAlong",
            "Smooth",
            "Symmetry",
            "Tangent",
            "Vertical",
        ] {
            let icon = crate::icons::markers::glyph_icon(name).unwrap_or_else(|| panic!("missing icon for {name}"));
            assert!(!crate::icon_svg::resolve(icon).placeholder, "{name} -> {icon} fell back to placeholder");
        }
        for (snap, expected) in [
            ("//", Some("c_parallel")),
            ("\u{22a5}", Some("c_perpendicular")),
            ("T", Some("c_tangent")),
            ("Mid", Some("c_midpoint")),
            ("H", Some("c_horizontal")),
            ("V", Some("c_vertical")),
            ("H+V", Some("c_hv")),
            ("On", Some("c_coincident")),
            ("End", Some("c_coincident")),
            ("Pt", Some("c_coincident")),
            ("X", None),
            ("+", None),
            ("Ext", None),
            ("Ctr", None),
        ] {
            assert_eq!(crate::icons::markers::snap_icon(snap), expected, "snap label {snap}");
            if let Some(icon) = expected {
                assert!(!crate::icon_svg::resolve(icon).placeholder, "{snap} -> {icon} fell back to placeholder");
            }
        }
    }

    #[test]
    fn is_constraint_pick_tool_matches_pick_hv_and_entity_constraint_tools() {
        for cmd in [
            "sketch.constraint.parallel",
            "sketch.constraint.coincident",
            "sketch.constraint.horizontal_vertical",
            "sketch.constraint.curvature",
            "sketch.constrainer",
            "sketch.constraint.polygon",
            "sketch.dimension",
        ] {
            let t = crate::tools::Tool::for_command(cmd).unwrap_or_else(|| panic!("tool for {cmd}"));
            assert!(crate::tools::is_constraint_pick_tool(&t), "{cmd} should be classified as a constraint pick tool");
        }
        for cmd in ["sketch.line", "sketch.circle.center", "sketch.trim"] {
            let t = crate::tools::Tool::for_command(cmd).unwrap_or_else(|| panic!("tool for {cmd}"));
            assert!(!crate::tools::is_constraint_pick_tool(&t), "{cmd} should not be classified as a constraint pick tool");
        }
    }

    #[test]
    fn snap_hint_scales_with_constraint_glyph_scale_preference() {
        use serde_json::json;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        h.settle();
        h.step(&json!({"start": "sketch.line"})).unwrap();
        h.step(&json!({"click": {"sketch": [0.0, 0.0]}})).unwrap();
        h.app.preferences.constraint_glyph_scale = 1.0;
        h.step(&json!({"move": {"sketch": [20.0, 0.2]}})).unwrap();
        h.frames(2);
        let small = crate::scenario::rect_of("viewport:snap_hint").expect("snap_hint at 1x");
        assert!((small.height() - glyph_px(1.0)).abs() < 0.01, "expected {}, got {}", glyph_px(1.0), small.height());
        h.app.preferences.constraint_glyph_scale = 2.0;
        h.frames(2);
        let large = crate::scenario::rect_of("viewport:snap_hint").expect("snap_hint at 2x");
        assert!((large.height() - glyph_px(2.0)).abs() < 0.01, "expected {}, got {}", glyph_px(2.0), large.height());
        h.step(&json!({"key": "Escape"})).unwrap();
    }

    #[test]
    fn multi_constraint_snap_shows_both_badges_and_applies_both_constraints() {
        use serde_json::json;
        use solidcraft_engine::sketch::ConstraintKind;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.line", json!({"points": [[0, 10], [20, 20]], "infer": false})).unwrap();
        h.app.run("sketch.line", json!({"points": [[30, -10], [30, 30]], "infer": false})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        h.settle();

        // Start a line at origin and move near [30, 15]: simultaneously Parallel to l1 and On l2.
        h.step(&json!({"start": "sketch.line"})).unwrap();
        h.step(&json!({"click": {"sketch": [0.0, 0.0]}})).unwrap();
        h.step(&json!({"move": {"sketch": [30.1, 15.1]}})).unwrap();
        h.frames(2);
        assert_eq!(super::current_snap_labels(), vec!["//", "On"]);
        let hint = crate::scenario::rect_of("viewport:snap_hint").expect("multi snap_hint");
        assert!(hint.width() > 2.0 * glyph_px(1.0), "expected two side-by-side badges, got {hint:?}");
        h.step(&json!({"click": {"sketch": [30.1, 15.1]}})).unwrap();
        h.step(&json!({"key": "Escape"})).unwrap();

        let st = crate::open_meshing::session_world(&h.app.session);
        let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Parallel { .. })), "Parallel missing");
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::PointOnCurve { .. })), "PointOnCurve missing");
    }

    #[test]
    fn extension_coexists_with_horizontal_and_applies_both_constraints() {
        use serde_json::json;
        use solidcraft_engine::sketch::ConstraintKind;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.line", json!({"points": [[0, 0], [10, 10]], "infer": false})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        h.settle();

        // Start at [0, 20] and move near [20, 20]: Horizontal + Extension of [0,0]->[10,10].
        h.step(&json!({"start": "sketch.line"})).unwrap();
        h.step(&json!({"click": {"sketch": [0.0, 20.0]}})).unwrap();
        h.step(&json!({"move": {"sketch": [20.1, 20.1]}})).unwrap();
        h.frames(2);
        assert_eq!(super::current_snap_labels(), vec!["H", "Ext"]);
        h.step(&json!({"click": {"sketch": [20.1, 20.1]}})).unwrap();
        h.step(&json!({"key": "Escape"})).unwrap();

        let st = crate::open_meshing::session_world(&h.app.session);
        let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Horizontal { .. })), "Horizontal missing");
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::PointOnCurve { .. })), "PointOnCurve missing");
    }

    #[test]
    fn extension_coexists_with_perpendicular_and_applies_both_constraints() {
        use serde_json::json;
        use solidcraft_engine::sketch::ConstraintKind;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.line", json!({"points": [[20, 0], [30, 0]], "infer": false})).unwrap();
        h.app.run("sketch.line", json!({"points": [[0, 0], [10, 10]], "infer": false})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        h.settle();

        // Start at [0, 10] and move near [10, 0]: Perpendicular to l2 + Extension of l1.
        h.step(&json!({"start": "sketch.line"})).unwrap();
        h.step(&json!({"click": {"sketch": [0.0, 10.0]}})).unwrap();
        h.step(&json!({"move": {"sketch": [10.1, 0.1]}})).unwrap();
        h.frames(2);
        assert_eq!(super::current_snap_labels(), vec!["⊥", "Ext"]);
        h.step(&json!({"click": {"sketch": [10.1, 0.1]}})).unwrap();
        h.step(&json!({"key": "Escape"})).unwrap();

        let st = crate::open_meshing::session_world(&h.app.session);
        let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Perpendicular { .. })), "Perpendicular missing");
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::PointOnCurve { .. })), "PointOnCurve missing");
    }

    #[test]
    fn snap_badge_avoids_placed_constraint_glyphs() {
        use serde_json::json;
        use solidcore_record::Vec2;
        use solidcraft_engine::geom::prelude::*;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        // A horizontal line along y = 0 places its Horizontal glyph directly above the midpoint.
        h.app.run("sketch.line", json!({"points": [[0, 0], [30, 0]], "infer": true})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        h.settle();
        h.frames(2);

        let glyph = glyph_px(h.app.preferences.constraint_glyph_scale);
        let placed = super::glyph_positions();
        assert!(!placed.is_empty(), "expected placed Horizontal glyph");
        let (_, glyph_center) = placed[0];

        // Start a vertical line ending on [15, 0] (the midpoint where the Horizontal glyph sits),
        // so its snap preview would naturally sit at sp + (12, -24) right over the placed glyph.
        let mut t = crate::tools::Tool::for_command("sketch.line").unwrap();
        t.pts.push((Vec2::new(15.0, -20.0), None));
        h.app.tool = Some(t);
        let view_rect = h.app.viewport.rect.unwrap();
        let proj = crate::viewport::projection(&h.app, view_rect);
        let target_world = {
            let st = crate::open_meshing::session_world(&h.app.session);
            let ss = st.sketch(h.app.session.active_sketch.unwrap()).unwrap();
            ss.plane.to_world(Vec2::new(15.0, 0.0))
        };
        let sp = proj.to_screen(target_world).unwrap();
        // Put the obstacle glyph right on top of the default badge position sp + (12, -24).
        let default_center = sp + egui::vec2(12.0, -24.0) + egui::vec2(glyph * 0.5, glyph * 0.5);
        super::GLYPHS.with(|g| g.borrow_mut().push((placed[0].0.clone(), default_center)));
        let ctx = crate::fonts::test_context();
        let _ = ctx.run_ui(egui::RawInput { screen_rect: Some(view_rect), ..Default::default() }, |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            super::paint_snap_badges(&h.app, &painter, view_rect, sp, &["V", "Mid"], true);
        });
        let preview_rect = crate::scenario::rect_of("viewport:snap_hint").expect("snap_hint rect");
        let obstacle = egui::Rect::from_center_size(default_center, egui::vec2(glyph, glyph));
        assert!(!preview_rect.intersects(obstacle), "preview {preview_rect:?} overlaps obstacle glyph {obstacle:?} (orig {glyph_center:?})");
        h.app.tool = None;
    }

    #[test]
    fn line_tangent_to_circle_previews_and_commits_both_tangent_and_coincident() {
        use serde_json::json;
        use solidcore_record::Vec2;
        use solidcraft_engine::sketch::ConstraintKind;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        h.app.run("sketch.circle.center", json!({"center": [20, 0], "radius": 10})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        h.settle();

        // Start a line at (0, 0): the upper tangent point on circle(center=(20,0), r=10) is
        // at (15, 5*sqrt(3)) = (15, 8.660254...).
        h.step(&json!({"start": "sketch.line"})).unwrap();
        h.step(&json!({"click": {"sketch": [0.0, 0.0]}})).unwrap();
        let tan = Vec2::new(15.0, 5.0 * 3.0_f64.sqrt());
        h.step(&json!({"move": {"sketch": [tan.x, tan.y]}})).unwrap();
        h.frames(2);

        let snap = super::SNAP.with(|s| s.borrow().clone()).expect("tangent snap active");
        assert!(snap.labels.contains(&"T"), "expected Tangent badge, got {:?}", snap.labels);
        assert!(snap.labels.contains(&"On"), "expected Coincident/On badge alongside Tangent, got {:?}", snap.labels);

        h.step(&json!({"click": {"sketch": [tan.x, tan.y]}})).unwrap();
        h.frames(2);
        let st = crate::open_meshing::session_world(&h.app.session);
        let sk = &st.sketch(h.app.session.active_sketch.unwrap()).unwrap().sketch;
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Tangent { .. })), "Tangent constraint missing: {:?}", sk.constraints);
        assert!(
            sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::PointOnCurve { .. })),
            "PointOnCurve/Coincident constraint missing: {:?}",
            sk.constraints
        );
        h.step(&json!({"key": "Escape"})).unwrap();
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
