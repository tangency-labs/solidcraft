//! The sheet page (#4894 W16-3 and W16-4; section 1.3 of the 30 Oct list on #5349): the open
//! drawing's sheet in the Drawing workspace's centre, as the sheet scene provider last drew it
//! (`drawing_scene::extract` each frame), while [`crate::drawing_jobs::tick`] brings the views out
//! of date up to date beside the window.
//!
//! - **What it draws is the scene**, nothing of its own: the border, images, views, dimensions and
//!   title block. Strokes are flattened within a fifth of a pixel and stroked at their paper width
//!   (never under a pixel); fills, and text as its glyphs' outlines, are triangulated, a glyph at a
//!   time. That work is done once per scene generation and zoom octave ([`Marks`]); a frame only
//!   moves and scales it, and skips what lies off the canvas. A picture shows only its frame here.
//! - **Pan and zoom** are the shared navigation with orbit off, on the page's own camera, so the
//!   Design camera stays where it was. Zoom stays between 5 % and 3200 %.
//! - **100 % is true size**: one paper millimetre is (96 / 25.4) × calibration / UI zoom points, so
//!   the UI scale never changes it ([`true_size`]). The calibration is a preference, set against a
//!   100 mm bar ([`calibration_bar`]).
//! - **Sheet colour** (White, Warm, Dark, Auto) is a screen preference; Auto follows the theme. The
//!   paper moves lighter where it can, else darker, to stand off the backdrop. Nothing here reaches
//!   the design, undo or a file: print and export are always white.
//! - **A view's state shows in words and shape**, never colour alone: out of date (dashed), nothing
//!   to draw (dotted) and can't be drawn (a cross), each with its words beside the view's boundary.

use std::time::Duration;

use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_engine::drawing_scene::{self, SheetObject, Status};
use solidcraft_engine::geom::paper::text::TextRun;
use solidcraft_engine::geom::paper::{Dashes, Item, ObjectKind, Scene};
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::geom::{Aabb2, Region2Geom, Seg2Geom};
use solidcraft_engine::render::Camera;

use crate::SolveApp;
use crate::theme::Tokens;
use crate::ui_kit::{self, Container, Surface};

/// The least and most the page zooms, as a share of true size.
pub const ZOOM: std::ops::RangeInclusive<f64> = 0.05..=32.0;
/// The calibration Preferences accepts.
pub const CALIBRATION: std::ops::RangeInclusive<f32> = 0.5..=2.0;
/// Most points a sheet's marks may flatten to before the page draws them coarser.
const MAX_POINTS: usize = 1 << 22;
/// How far from a mark's true path its flattened lines may stray, in physical pixels.
const TOLERANCE_PX: f64 = 0.2;
/// How close the pointer must come to an object's box to be over it, in points.
const HOVER_BAND: f64 = 6.0;

/// The sheet's colour on screen (Preferences › Design › Drawings › Sheet colour).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetColour {
    White,
    Warm,
    Dark,
    /// White in a light theme, Dark in a dark one.
    Auto,
}

impl SheetColour {
    /// In the order Preferences lists them.
    pub const ALL: [SheetColour; 4] = [SheetColour::White, SheetColour::Warm, SheetColour::Dark, SheetColour::Auto];

    pub fn parse(s: &str) -> Option<SheetColour> {
        SheetColour::ALL.into_iter().find(|c| c.id() == s)
    }

    /// Its stored name.
    pub fn id(self) -> &'static str {
        match self {
            SheetColour::White => "white",
            SheetColour::Warm => "warm",
            SheetColour::Dark => "dark",
            SheetColour::Auto => "auto",
        }
    }

    fn label(self) -> &'static str {
        match self {
            SheetColour::White => "ui.sheet_page.colour.white",
            SheetColour::Warm => "ui.sheet_page.colour.warm",
            SheetColour::Dark => "ui.sheet_page.colour.dark",
            SheetColour::Auto => "ui.sheet_page.colour.auto",
        }
    }

    /// The paper and its ink in a theme (`dark`), before the paper stands off the backdrop.
    pub fn paper(self, t: &Tokens, dark: bool) -> (Color32, Color32) {
        match self {
            SheetColour::White => (t.paper_white, t.paper_ink),
            SheetColour::Warm => (t.paper_warm, t.paper_ink),
            SheetColour::Dark => (t.paper_dark, t.paper_dark_ink),
            SheetColour::Auto if dark => (t.paper_dark, t.paper_dark_ink),
            SheetColour::Auto => (t.paper_white, t.paper_ink),
        }
    }
}

/// How light a colour looks, 0 to 1 (Rec. 709 weights on its sRGB values).
fn lightness(c: Color32) -> f32 {
    (0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b())) / 255.0
}

/// How far apart in [`lightness`] the paper and the backdrop must be for the sheet's edge to show.
pub const STAND_OFF: f32 = 0.06;

/// `paper` as shown on `backdrop`: as it is when the two are [`STAND_OFF`] apart, else moved
/// lighter where that's enough, else darker.
pub fn stand_off(paper: Color32, backdrop: Color32) -> Color32 {
    let apart = |c: Color32| (lightness(c) - lightness(backdrop)).abs() >= STAND_OFF;
    if apart(paper) {
        return paper;
    }
    let toward = |to: Color32| (1..=20).map(|k| paper.lerp_to_gamma(to, k as f32 / 20.0)).find(|c| apart(*c));
    toward(Color32::WHITE).or_else(|| toward(Color32::BLACK)).unwrap_or(paper)
}

/// The paper and ink the page draws in, now.
pub fn paper_and_ink(app: &SolveApp) -> (Color32, Color32) {
    let t = Tokens::get();
    let colour = SheetColour::parse(&app.preferences.drawing_sheet_colour).unwrap_or(SheetColour::Auto);
    let (paper, ink) = colour.paper(&t, crate::theme::is_dark());
    (stand_off(paper, t.viewport_top), ink)
}

/// Points per paper millimetre at 100 %: (96 / 25.4) × `calibration` / `zoom_factor` (the UI scale),
/// so a millimetre measures a millimetre whatever the UI scale.
pub fn true_size(calibration: f32, zoom_factor: f32) -> f64 {
    let calibration = if CALIBRATION.contains(&calibration) { calibration } else { 1.0 };
    let zoom_factor = if zoom_factor.is_finite() && zoom_factor > 0.0 { zoom_factor } else { 1.0 };
    96.0 / 25.4 * f64::from(calibration) / f64::from(zoom_factor)
}

fn true_size_now(app: &SolveApp, ctx: &egui::Context) -> f64 {
    true_size(app.preferences.drawing_calibration, ctx.zoom_factor())
}

/// The page's view of its sheet, kept between frames and never saved.
pub struct Page {
    /// Its own camera, looking down on the paper (x right, y up, millimetres): the shared
    /// navigation moves it while the page has the pointer.
    cam: Camera,
    /// Points per paper millimetre: the zoom, kept as the canvas changes size.
    scale: f64,
    /// The sheet and the size it was last fitted to: Fit Sheet again when either changes.
    fitted: Option<(u64, [u64; 2])>,
    /// The canvas size the sheet was fitted on, while nothing has moved it since: the sheet is
    /// fitted again when the canvas changes size (a window resized, a panel opened).
    fitted_on: Option<egui::Vec2>,
    /// The sheet shown, when the drawing still has it.
    pub(crate) sheet: Option<u64>,
    marks: Option<Marks>,
    /// The zoom card, so the canvas under it takes no camera input.
    pub(crate) card: Option<Rect>,
    /// What the last frame showed, for `ui.inspect`.
    shown: Option<Value>,
}

impl Default for Page {
    fn default() -> Self {
        let cam = Camera { yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, fov: 0.0, distance: 1000.0, ..Camera::default() };
        Page { cam, scale: true_size(1.0, 1.0), fitted: None, fitted_on: None, sheet: None, marks: None, card: None, shown: None }
    }
}

/// Fit Sheet: the sheet is framed again on the next frame.
pub fn fit(app: &mut SolveApp) {
    app.drawing_workspace.page.fitted = None;
}

/// The sheet at true size, about the canvas's centre.
pub fn actual_size(app: &mut SolveApp, ctx: &egui::Context) {
    app.drawing_workspace.page.fitted_on = None;
    app.drawing_workspace.page.scale = true_size_now(app, ctx);
}

/// The sheet at `zoom` (1 is 100 %), about the canvas's centre, within [`ZOOM`].
pub fn set_zoom(app: &mut SolveApp, ctx: &egui::Context, zoom: f64) {
    if zoom.is_finite() {
        app.drawing_workspace.page.fitted_on = None;
        app.drawing_workspace.page.scale = true_size_now(app, ctx) * zoom.clamp(*ZOOM.start(), *ZOOM.end());
    }
}

/// What the page showed on its last frame (`ui.inspect`'s `sheet_page`): null while no sheet is shown.
pub fn inspect(app: &SolveApp) -> Value {
    if crate::drawing_workspace::active(app) { app.drawing_workspace.page.shown.clone().unwrap_or(Value::Null) } else { Value::Null }
}

/// The paper's camera drives the shared navigation for a frame: the Design camera, its animation
/// and pending framing are left as they were (as the Render result's camera does).
fn with_sheet_camera(app: &mut SolveApp, navigate: impl FnOnce(&mut SolveApp)) {
    let design = std::mem::replace(&mut app.cam, app.drawing_workspace.page.cam);
    let perspective = std::mem::replace(&mut app.ui.perspective, false);
    let up = app.ui.up_axis;
    let animation = app.cam_anim.take();
    let pending = app.pending_fit.take();
    let request = app.session.camera.take();
    let free = app.viewport.orbit_free.take();
    let spin = std::mem::replace(&mut app.viewport.spin, false);
    navigate(app);
    app.drawing_workspace.page.cam = std::mem::replace(&mut app.cam, design);
    app.ui.perspective = perspective;
    app.ui.up_axis = up;
    app.cam_anim = animation;
    app.pending_fit = pending;
    app.session.camera = request;
    app.viewport.orbit_free = free;
    app.viewport.spin = spin;
}

/// Where paper millimetres land on the canvas, and back.
#[derive(Clone, Copy, Debug)]
struct OnScreen {
    centre: Pos2,
    target: Vec2,
    scale: f64,
}

impl OnScreen {
    fn of(cam: &Camera, canvas: Rect) -> OnScreen {
        let scale = f64::from(canvas.height().max(1.0)) / (2.0 * cam.half_height());
        OnScreen { centre: canvas.center(), target: Vec2::new(cam.target.x, cam.target.y), scale }
    }

    fn screen(self, p: Vec2) -> Pos2 {
        let x = f64::from(self.centre.x) + (p.x - self.target.x) * self.scale;
        let y = f64::from(self.centre.y) - (p.y - self.target.y) * self.scale;
        pos2(x as f32, y as f32)
    }

    fn paper(self, p: Pos2) -> Vec2 {
        let x = self.target.x + f64::from(p.x - self.centre.x) / self.scale;
        let y = self.target.y - f64::from(p.y - self.centre.y) / self.scale;
        Vec2::new(x, y)
    }

    fn rect(&self, b: &Aabb2) -> Rect {
        Rect::from_two_pos(self.screen(b.min), self.screen(b.max))
    }

    fn visible(&self, canvas: Rect) -> Aabb2 {
        let mut b = Aabb2::EMPTY;
        b.add(self.paper(canvas.min));
        b.add(self.paper(canvas.max));
        b
    }
}

/// The camera that shows `size` paper millimetres whole in `area`, a part of `canvas`, within
/// [`ZOOM`] of `true_size`: its target and the scale, in points per millimetre.
fn framed(canvas: Rect, area: Rect, size: Vec2, true_size: f64) -> (Vec2, f64) {
    let margin = Tokens::get().space_24;
    let w = f64::from((area.width() - 2.0 * margin).max(1.0));
    let h = f64::from((area.height() - 2.0 * margin).max(1.0));
    let scale = (w / size.x.max(1e-3)).min(h / size.y.max(1e-3)).clamp(true_size * ZOOM.start(), true_size * ZOOM.end());
    let x = size.x / 2.0 - f64::from(area.center().x - canvas.center().x) / scale;
    let y = size.y / 2.0 + f64::from(area.center().y - canvas.center().y) / scale;
    (Vec2::new(x, y), scale)
}

/// The part of the canvas no ribbon, Browser or History covers.
fn free_area(app: &SolveApp, canvas: Rect) -> Rect {
    crate::shell::workspace_content(app, crate::shell::content_rect(app, canvas))
}

/// The camera's distance that shows `scale` points per millimetre on a canvas `height` tall.
fn distance_for(scale: f64, height: f32) -> f64 {
    (f64::from(height.max(1.0)) / (2.0 * scale.max(1e-9) * 0.4)).clamp(1e-3, 1e7)
}

/// Draws the sheet of the drawing open, and says whether there was one to draw: without a drawing
/// open the workspace shows its own message.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    // The zoom card stays covered from last frame while the camera reads this frame's input.
    app.drawing_workspace.page.shown = None;
    let Ok(drawing) = app.session.drawing(drawing_scene::CMD).cloned() else {
        app.drawing_workspace.page.card = None;
        return false;
    };
    let page = &mut app.drawing_workspace.page;
    let sheet = page.sheet.filter(|id| drawing.sheets.iter().any(|s| s.id == *id)).or_else(|| drawing.sheets.first().map(|s| s.id));
    let Some(sheet) = sheet else {
        page.card = None;
        return false;
    };
    page.sheet = Some(sheet);
    let t = Tokens::get();
    let canvas = ui.max_rect();
    ui.painter().rect_filled(canvas, 0.0, t.viewport_top);
    app.viewport.rect = Some(canvas);
    crate::viewport::refresh_layout(app, ui.ctx());
    let response = ui.allocate_rect(canvas, Sense::click_and_drag());
    let view = match drawing_scene::extract(&mut app.session, sheet) {
        Ok(view) => view,
        Err(e) => {
            app.drawing_workspace.page.card = None;
            message(ui, canvas, &e.to_string(), t.error);
            return true;
        }
    };
    // A view worked out beside the window, or text waiting for its font: look again soon.
    let working = crate::drawing_jobs::tick(app);
    if working || view.waiting_for_fonts {
        ui.ctx().request_repaint_after(Duration::from_millis(16));
    }
    let size = view.scene.size_mm;
    let base = true_size_now(app, ui.ctx());
    let key = (sheet, [size.x.to_bits(), size.y.to_bits()]);
    let page = &mut app.drawing_workspace.page;
    if page.fitted_on.is_some_and(|size| size != canvas.size()) {
        page.fitted = None;
    }
    if page.fitted != Some(key) {
        let area = free_area(app, canvas);
        let (target, scale) = framed(canvas, area, size, base);
        let page = &mut app.drawing_workspace.page;
        page.cam.target.x = target.x;
        page.cam.target.y = target.y;
        page.scale = scale;
        page.fitted = Some(key);
        page.fitted_on = Some(canvas.size());
    }
    let framed_as = (app.drawing_workspace.page.cam.target, app.drawing_workspace.page.scale);
    let page = &mut app.drawing_workspace.page;
    page.cam.distance = distance_for(page.scale, canvas.height());
    with_sheet_camera(app, |app| {
        crate::navigation::handle(app, ui, canvas, &response, crate::navigation::NavCaps { orbit: false, pick: false });
    });
    let page = &mut app.drawing_workspace.page;
    let scale = OnScreen::of(&page.cam, canvas).scale;
    page.scale = if scale.is_finite() && scale > 0.0 { scale.clamp(base * ZOOM.start(), base * ZOOM.end()) } else { base };
    page.cam.distance = distance_for(page.scale, canvas.height());
    // Moved by the user, not only worked out again from its distance.
    if page.cam.target != framed_as.0 || (page.scale / framed_as.1 - 1.0).abs() > 1e-9 {
        page.fitted_on = None;
    }
    let at = OnScreen::of(&page.cam, canvas);
    let ppp = f64::from(ui.ctx().pixels_per_point().max(0.1));
    // One flattening per zoom octave: finer than a fifth of a pixel at its most zoomed out.
    let wanted = (TOLERANCE_PX / (at.scale * ppp)).max(1e-5);
    let tolerance = 2f64.powi(wanted.log2().floor() as i32);
    if page.marks.as_ref().is_none_or(|m| m.generation != view.generation || m.tolerance != tolerance) {
        page.marks = Some(Marks::of(&view.scene, view.generation, tolerance));
    }
    let (paper, ink) = paper_and_ink(app);
    let painter = ui.painter().with_clip_rect(canvas.intersect(ui.clip_rect()));
    let sheet_rect = at.rect(&Aabb2 { min: Vec2::ZERO, max: size });
    painter.add(t.shell_shadow().as_shape(sheet_rect, 0));
    painter.rect_filled(sheet_rect, 0.0, paper);
    let visible = at.visible(canvas);
    if let Some(marks) = &app.drawing_workspace.page.marks {
        marks.paint(&painter, &at, visible, ink, ppp as f32);
    }
    crate::scenario::publish_rect("sheet:paper", sheet_rect);
    let hover = response.hover_pos().filter(|p| !crate::shell::covered(app, *p)).map(|p| at.paper(p));
    let hit = hover.and_then(|p| drawing_scene::hit(&view.objects, p, HOVER_BAND / at.scale));
    if let Some(hit) = hit
        && let Some(object) = view.objects.iter().find(|o| o.object == hit.object)
    {
        let b = object.boundary_mm.unwrap_or(object.bounds_mm);
        painter.rect_stroke(at.rect(&b), 0.0, Stroke::new(t.space_2 * 0.5, t.accent), egui::StrokeKind::Outside);
    }
    let states = view_states(&painter, &at, &view.views, &view.stale, &view.objects);
    // Views a deferred drawing holds stay out of date without being worked on: their words say so.
    let waiting = view.stale.len() + view.stale_dimensions.len();
    if working {
        let text = crate::i18n::format("ui.sheet_page.working", &[("count", &waiting.max(1).to_string())]);
        hint(&painter, free_area(app, canvas), &text);
    }
    zoom_card(app, ui, canvas, at.scale / base);
    crate::navigation_bar_view::show(app, ui, canvas);
    let encode = solidcraft_engine::doc::ids::encode;
    let paper_mm = at.visible(canvas);
    app.drawing_workspace.page.shown = Some(json!({
        "drawing": encode(drawing.id),
        "sheet": encode(sheet),
        "size_mm": [size.x, size.y],
        "zoom": at.scale / base,
        "points_per_mm": at.scale,
        "true_size_points_per_mm": base,
        "visible_mm": [paper_mm.min.x, paper_mm.min.y, paper_mm.max.x, paper_mm.max.y],
        "sheet_colour": app.preferences.drawing_sheet_colour,
        "paper": hex(paper),
        "ink": hex(ink),
        "generation": view.generation,
        "views": states,
        "working": working,
        "hover": hit.map(|h| json!({"object": encode(h.object), "kind": kind_name(h.kind), "on_edge": h.on_edge})),
        "error": app.drawing_workspace.page.marks.as_ref().and_then(|m| m.error.clone()),
    }));
    true
}

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::View => "view",
        ObjectKind::Dimension => "dimension",
        ObjectKind::TitleBlock => "title_block",
        ObjectKind::PartsList => "parts_list",
        ObjectKind::RevisionTable => "revision_table",
        ObjectKind::SheetImage => "sheet_image",
        ObjectKind::SheetBorder => "sheet_border",
        ObjectKind::DrawingBorder => "drawing_border",
    }
}

fn message(ui: &egui::Ui, canvas: Rect, text: &str, colour: Color32) {
    let t = Tokens::get();
    let galley = ui.painter().layout(text.to_owned(), egui::FontId::proportional(t.font_body), colour, (canvas.width() - t.space_24 * 2.0).max(40.0));
    ui.painter().galley(canvas.center() - galley.size() / 2.0, galley, colour);
}

/// The words for each view whose state isn't simply drawn and current, drawn beside its boundary
/// with the boundary's shape: dashed when out of date, dotted when there's nothing to draw, crossed
/// when it can't be drawn. Returns each view's state for `ui.inspect`.
fn view_states(painter: &egui::Painter, at: &OnScreen, views: &[drawing_scene::ViewState], stale: &[u64], objects: &[SheetObject]) -> Vec<Value> {
    let t = Tokens::get();
    let mut out = Vec::new();
    for state in views {
        let out_of_date = stale.contains(&state.view);
        let words = words(&state.status, out_of_date);
        let colour = match &state.status {
            Status::Drawn => t.warning,
            Status::Placeholder(_) => t.text_dim,
            Status::Failed(_) => t.error,
        };
        out.push(json!({
            "view": solidcraft_engine::doc::ids::encode(state.view),
            "status": state.status.name(),
            "out_of_date": out_of_date,
            "words": words,
        }));
        let Some(object) = objects.iter().find(|o| o.kind == ObjectKind::View && o.object == state.view) else { continue };
        if words.is_empty() {
            continue;
        }
        let r = at.rect(&object.boundary_mm.unwrap_or(object.bounds_mm));
        let stroke = Stroke::new(t.space_2 * 0.75, colour);
        let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        match &state.status {
            Status::Failed(_) => {
                painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
                painter.line_segment([r.left_top(), r.right_bottom()], stroke);
                painter.line_segment([r.right_top(), r.left_bottom()], stroke);
            }
            Status::Placeholder(_) if !out_of_date => {
                painter.extend(egui::Shape::dotted_line(&corners, colour, t.space_8, t.space_2 * 0.75));
            }
            _ => {
                painter.extend(egui::Shape::dashed_line(&corners, stroke, t.space_8, t.space_4));
            }
        }
        let text = words.join(" · ");
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(t.font_label), colour);
        let at = r.left_top() - vec2(0.0, galley.size().y + t.space_4);
        let chip = Rect::from_min_size(at, galley.size()).expand(t.space_2);
        painter.rect_filled(chip, t.space_2, t.panel);
        painter.galley(at, galley, colour);
        crate::scenario::publish_rect(&format!("sheet:view:{}:state", solidcraft_engine::doc::ids::encode(state.view)), chip);
    }
    out
}

/// What is said beside a view: why it draws nothing, and whether it is out of date. Nothing for a
/// view drawn and current.
pub(crate) fn words(status: &Status, out_of_date: bool) -> Vec<String> {
    let mut words = Vec::new();
    match status {
        Status::Drawn => {}
        Status::Placeholder(why) => words.push(crate::i18n::format("ui.sheet_page.placeholder", &[("reason", &why.to_string())])),
        Status::Failed(why) => words.push(crate::i18n::format("ui.sheet_page.failed", &[("reason", why)])),
    }
    if out_of_date {
        words.push(crate::i18n::tr("ui.sheet_page.out_of_date").to_owned());
    }
    words
}

/// The "views still being drawn" hint, at the top of the free canvas.
fn hint(painter: &egui::Painter, area: Rect, text: &str) {
    let t = Tokens::get();
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(t.font_label), t.text);
    let at = pos2(area.center().x - galley.size().x / 2.0, area.top() + t.space_8 * 7.0);
    let chip = Rect::from_min_size(at, galley.size()).expand(t.space_4);
    painter.rect_filled(chip, t.space_4, t.panel);
    painter.galley(at, galley, t.text);
    crate::scenario::publish_rect("sheet:working", chip);
}

/// The zoom readout, typed or dragged, with 100 % and Fit Sheet, at the free canvas's lower right.
fn zoom_card(app: &mut SolveApp, ui: &mut egui::Ui, canvas: Rect, zoom: f64) {
    let t = Tokens::get();
    let area = free_area(app, canvas);
    let container = Container::new(ui.ctx(), Surface::Panel);
    let frame = container.frame(ui.ctx(), t.panel, crate::frame::Side::Right).inner_margin(4);
    let size = app.drawing_workspace.page.card.map_or(vec2(t.space_24 * 10.0, t.space_24 + 10.0), |r| r.size());
    let bar = app.viewport.layout.as_ref().map_or(0.0, |l| if l.navigation.is_positive() { l.navigation.height() + t.space_8 } else { 0.0 });
    let position = pos2(area.right() - size.x - t.space_8, area.bottom() - size.y - t.space_8 - bar).max(area.min);
    let mut percent = (zoom * 100.0) as f32;
    let mut chosen = None;
    let card = container.area(egui::Id::new("sc_sheet_zoom")).fixed_pos(position).show(ui.ctx(), |ui| {
        container.show_frame(ui, frame, area, |ui| {
            ui.horizontal(|ui| {
                let field = ui_kit::number_field(ui, &mut percent, 5.0..=3200.0, 1.0, " %").on_hover_text(app.tr("ui.sheet_page.zoom"));
                crate::scenario::publish_target("sheet:zoom", field.rect);
                if field.changed() {
                    chosen = Some("zoom");
                }
                let actual = ui_kit::button(ui, app.tr("ui.sheet_page.actual_size"), ui_kit::Button::Normal);
                crate::scenario::publish_target("sheet:actual_size", actual.rect);
                if actual.clicked() {
                    chosen = Some("actual");
                }
                let fit = ui_kit::button(ui, app.tr("ui.sheet_page.fit"), ui_kit::Button::Normal);
                crate::scenario::publish_target("sheet:fit", fit.rect);
                if fit.clicked() {
                    chosen = Some("fit");
                }
            });
        });
    });
    app.drawing_workspace.page.card = Some(card.response.rect);
    crate::scenario::publish_rect("sheet:zoom_card", card.response.rect);
    match chosen {
        Some("zoom") => set_zoom(app, ui.ctx(), f64::from(percent) / 100.0),
        Some("actual") => actual_size(app, ui.ctx()),
        Some("fit") => fit(app),
        _ => {}
    }
}

/// Preferences' Sheet colour choice.
pub(crate) fn colour_choice(app: &mut SolveApp, ui: &mut egui::Ui) {
    let labels: Vec<&str> = SheetColour::ALL.iter().map(|c| app.tr(c.label())).collect();
    let current = SheetColour::parse(&app.preferences.drawing_sheet_colour).unwrap_or(SheetColour::Auto);
    let mut selected = SheetColour::ALL.iter().position(|c| *c == current).unwrap_or(3);
    let response = ui_kit::dropdown(ui, "pref_drawing_sheet_colour", &mut selected, &labels);
    crate::scenario::publish_target("prefs:drawing_sheet_colour", response.rect);
    if let Some(colour) = SheetColour::ALL.get(selected) {
        app.preferences.drawing_sheet_colour = colour.id().to_owned();
    }
}

/// A 100 mm bar at true size, with a tick each 10 mm, to set the calibration against a ruler.
pub(crate) fn calibration_bar(app: &SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let per_mm = true_size_now(app, ui.ctx()) as f32;
    let length = 100.0 * per_mm;
    let height = t.space_24 + t.space_8;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(length + t.space_8), height), Sense::hover());
    crate::scenario::publish_rect("prefs:calibration_bar", rect);
    let painter = ui.painter_at(rect);
    let left = rect.left() + t.space_4;
    let y = rect.top() + t.space_8;
    let stroke = Stroke::new(1.0, t.text);
    painter.line_segment([pos2(left, y), pos2(left + length, y)], stroke);
    for tick in 0..=10u8 {
        let x = left + f32::from(tick) * 10.0 * per_mm;
        let long = if tick % 5 == 0 { t.space_8 } else { t.space_4 };
        painter.line_segment([pos2(x, y), pos2(x, y + long)], stroke);
    }
    painter.text(
        pos2(left + length, y + t.space_8),
        egui::Align2::RIGHT_TOP,
        app.tr("ui.sheet_page.calibration_bar"),
        egui::FontId::proportional(t.font_label),
        t.text_dim,
    );
}

/// A sheet's marks, flattened and triangulated for one zoom octave.
pub(crate) struct Marks {
    generation: u64,
    tolerance: f64,
    pub(crate) marks: Vec<Mark>,
    /// Why the sheet is drawn coarser than asked, or not at all.
    error: Option<String>,
}

/// One mark of a sheet, in paper millimetres.
#[derive(Debug)]
pub(crate) enum Mark {
    Line {
        points: Vec<Vec2>,
        closed: bool,
        width_mm: f64,
        bounds: Aabb2,
    },
    /// Triangles, and the outlines they fill, stroked a hair wide so their edges are smooth.
    Fill {
        triangles: Vec<[Vec2; 3]>,
        outlines: Vec<Vec<Vec2>>,
        bounds: Aabb2,
    },
    /// A picture's frame: its pixels aren't drawn on the page yet.
    Picture(Aabb2),
}

impl Marks {
    /// `scene`'s marks within `tolerance` millimetres, or coarser when that would be more than
    /// [`MAX_POINTS`]; a scene that can't be checked draws nothing and says why.
    pub(crate) fn of(scene: &Scene, generation: u64, tolerance: f64) -> Marks {
        let mut tol = tolerance;
        for _ in 0..8 {
            match marks(scene, tol) {
                Ok(marks) => {
                    let error = (tol != tolerance).then(|| crate::i18n::tr("ui.sheet_page.coarse").to_owned());
                    return Marks { generation, tolerance, marks, error };
                }
                Err(Fault::TooMany) => tol *= 8.0,
                Err(Fault::Invalid(why)) => return Marks { generation, tolerance, marks: Vec::new(), error: Some(why) },
            }
        }
        Marks { generation, tolerance, marks: Vec::new(), error: Some(crate::i18n::tr("ui.sheet_page.coarse").to_owned()) }
    }

    fn paint(&self, painter: &egui::Painter, at: &OnScreen, visible: Aabb2, ink: Color32, ppp: f32) {
        let hair = 1.0 / ppp.max(0.1);
        let mut mesh = egui::Mesh::default();
        for mark in &self.marks {
            match mark {
                Mark::Line { points, closed, width_mm, bounds } => {
                    if !bounds.inflated(*width_mm).meets(&visible) {
                        continue;
                    }
                    let width = ((width_mm * at.scale) as f32).max(hair);
                    let points: Vec<Pos2> = points.iter().map(|p| at.screen(*p)).collect();
                    let stroke = Stroke::new(width, ink);
                    painter.add(if *closed { egui::Shape::closed_line(points, stroke) } else { egui::Shape::line(points, stroke) });
                }
                Mark::Fill { triangles, outlines, bounds } => {
                    if !bounds.meets(&visible) {
                        continue;
                    }
                    for tri in triangles {
                        let base = mesh.vertices.len() as u32;
                        for p in tri {
                            mesh.colored_vertex(at.screen(*p), ink);
                        }
                        mesh.add_triangle(base, base + 1, base + 2);
                    }
                    for outline in outlines {
                        let points: Vec<Pos2> = outline.iter().map(|p| at.screen(*p)).collect();
                        painter.add(egui::Shape::closed_line(points, Stroke::new(hair * 0.5, ink)));
                    }
                }
                Mark::Picture(rect) => {
                    if rect.meets(&visible) {
                        let r = at.rect(rect);
                        let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                        painter.extend(egui::Shape::dashed_line(&corners, Stroke::new(hair, ink.gamma_multiply(0.6)), 6.0, 4.0));
                    }
                }
            }
        }
        if !mesh.is_empty() {
            painter.add(egui::Shape::mesh(mesh));
        }
    }
}

enum Fault {
    TooMany,
    Invalid(String),
}

/// Every mark of `scene`'s groups, in paint order, cut to their clips as every writer reads them.
fn marks(scene: &Scene, tol: f64) -> Result<Vec<Mark>, Fault> {
    let prepared = scene.prepare().map_err(|e| Fault::Invalid(e.to_string()))?;
    let mut budget = MAX_POINTS;
    let mut out = Vec::new();
    for group in prepared.groups() {
        for item in &group.items {
            match item {
                Item::Stroke { path, closed, width_mm, pattern } => {
                    let points = flatten(path, tol, &mut budget)?;
                    match pattern {
                        Some(dashes) => {
                            for points in dashed(&points, *closed, dashes, &mut budget)? {
                                out.push(line(points, false, *width_mm));
                            }
                        }
                        None => out.push(line(points, *closed, *width_mm)),
                    }
                }
                Item::Fill { region } => out.extend(fill(&region.loops().cloned().collect::<Vec<_>>(), tol, &mut budget)?),
                Item::Text(run) => {
                    let Some(font) = prepared.font(run) else { continue };
                    // A glyph at a time: one run's slabs across every glyph would be far more triangles.
                    for glyph in &run.glyphs {
                        let one = TextRun {
                            font: run.font,
                            scale: run.scale,
                            origin: run.origin,
                            angle: run.angle,
                            slant: run.slant,
                            glyphs: vec![*glyph],
                            text: String::new(),
                            clusters: Vec::new(),
                            rtl: run.rtl,
                        };
                        let loops: Vec<Loop2> = one.outlines(font).collect();
                        out.extend(fill(&loops, tol, &mut budget)?);
                    }
                }
                Item::Picture(picture) => out.push(Mark::Picture(picture.rect)),
            }
        }
    }
    Ok(out)
}

fn bounds_of(points: &[Vec2]) -> Aabb2 {
    let mut b = Aabb2::EMPTY;
    for p in points {
        b.add(*p);
    }
    b
}

fn line(points: Vec<Vec2>, closed: bool, width_mm: f64) -> Mark {
    let bounds = bounds_of(&points);
    Mark::Line { points, closed, width_mm, bounds }
}

/// `path`'s pieces as one polyline within `tol`, its points taken from `budget`.
fn flatten(path: &[Seg2], tol: f64, budget: &mut usize) -> Result<Vec<Vec2>, Fault> {
    let mut points: Vec<Vec2> = Vec::new();
    for seg in path {
        let piece = seg.polyline(tol);
        *budget = budget.checked_sub(piece.len()).ok_or(Fault::TooMany)?;
        // Each piece starts where the one before ends.
        let skip = usize::from(points.last().is_some_and(|last| piece.first().is_some_and(|first| last.dist(*first) <= 1e-9)));
        points.extend(piece.into_iter().skip(skip));
    }
    Ok(points)
}

/// The dashes of a line through `points` with pattern `dashes`, along its whole length (round a
/// closed one), in paper millimetres.
fn dashed(points: &[Vec2], closed: bool, dashes: &Dashes, budget: &mut usize) -> Result<Vec<Vec<Vec2>>, Fault> {
    let lengths: Vec<f64> = dashes.lengths_mm.iter().copied().filter(|l| l.is_finite() && *l > 0.0).collect();
    if lengths.len() < 2 {
        return Ok(vec![points.to_vec()]);
    }
    let mut path = points.to_vec();
    if closed && let Some(first) = points.first() {
        path.push(*first);
    }
    // Where in the pattern the line starts.
    let period: f64 = lengths.iter().sum();
    let mut into = dashes.phase_mm.rem_euclid(period);
    let mut index = 0usize;
    while let Some(l) = lengths.get(index)
        && into >= *l
    {
        into -= l;
        index = (index + 1) % lengths.len();
    }
    let mut left = lengths.get(index).copied().unwrap_or(period) - into;
    let mut out: Vec<Vec<Vec2>> = Vec::new();
    let mut current: Vec<Vec2> = path.first().filter(|_| index.is_multiple_of(2)).map(|p| vec![*p]).unwrap_or_default();
    for pair in path.windows(2) {
        let [a, b] = pair else { continue };
        let mut from = *a;
        let mut span = a.dist(*b);
        while span > left {
            let at = from + (*b - from) * (left / span);
            if index.is_multiple_of(2) {
                current.push(at);
                *budget = budget.checked_sub(current.len()).ok_or(Fault::TooMany)?;
                out.push(std::mem::take(&mut current));
            } else {
                current = vec![at];
            }
            span -= left;
            from = at;
            index = (index + 1) % lengths.len();
            left = lengths.get(index).copied().unwrap_or(period);
        }
        left -= span;
        if index.is_multiple_of(2) {
            current.push(*b);
        }
    }
    if current.len() > 1 {
        *budget = budget.checked_sub(current.len()).ok_or(Fault::TooMany)?;
        out.push(current);
    }
    Ok(out)
}

/// A filled region of `loops` (even-odd), triangulated within `tol`; none for no loops.
fn fill(loops: &[Loop2], tol: f64, budget: &mut usize) -> Result<Option<Mark>, Fault> {
    let mut each = loops.iter().cloned();
    let Some(outer) = each.next() else { return Ok(None) };
    let region = Region2 { outer, holes: each.collect() };
    let outlines = loops.iter().map(|l| flatten(&l.segs, tol, budget)).collect::<Result<Vec<_>, _>>()?;
    let triangles = region.triangulate(tol);
    *budget = budget.checked_sub(triangles.len().saturating_mul(3)).ok_or(Fault::TooMany)?;
    let mut bounds = Aabb2::EMPTY;
    for outline in &outlines {
        bounds = bounds.union(&bounds_of(outline));
    }
    Ok(Some(Mark::Fill { triangles, outlines, bounds }))
}

#[cfg(test)]
#[path = "drawing_sheet_page_tests.rs"]
mod tests;
