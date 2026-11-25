//! The sheet page (#4894 W16-3 and W16-4; section 1.3 of the 30 Oct list on #5349): the open
//! drawing's sheet in the Drawing workspace's centre, as the sheet scene provider last drew it
//! (`drawing_scene::extract` each frame), while [`crate::drawing_jobs::tick`] brings the views out
//! of date up to date beside the window.
//!
//! - **What it draws is the scene**, nothing of its own: the border, images, views, dimensions and
//!   title block. Strokes are flattened within a fifth of a pixel and stroked at their paper width
//!   (never under a pixel); fills, and text as its glyphs' outlines, are triangulated, a glyph at a
//!   time. That work is done once per scene generation and zoom octave ([`Marks`]); a frame only
//!   moves and scales it, and skips what lies off the canvas. A picture (a shaded view's) shows the
//!   pixels the provider drew for it, uploaded once each ([`PictureTextures`]); until it has them, its
//!   frame.
//! - **Pan and zoom** are the shared navigation with orbit off, on the page's own camera, so the
//!   Design camera stays where it was. Zoom stays between 5 % and 3200 %.
//! - **100 % is true size**: one paper millimetre is (96 / 25.4) × calibration / UI zoom points, so
//!   the UI scale never changes it ([`true_size`]). The calibration is a preference, set against a
//!   100 mm bar ([`calibration_bar`]).
//! - **Sheet colour** (White unless chosen: Follows the theme, Blueprint, Prussian Blue) is a screen preference
//!   (D25 of `docs/design/drawing-ui/decisions.md`). The paper keeps its colour on a neutral stage
//!   that moves away from it to stand off ([`stage_and_paper`]), and has a very slight rounded corner
//!   (D29). Nothing here
//!   reaches the design, undo or a file: print and export are always white.
//! - **A view's state shows in words and shape**, never colour alone (D26): needs update (dashed),
//!   nothing to draw (dotted) and can't be drawn (a cross), each with its words beside the view's
//!   boundary.
//! - **Hover and selection** look as they do in a sketch: what the pointer is over, and what is
//!   selected, is drawn in the accent colour, with no box round it.

mod view_tools;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcore_record::{Loop2, Region2, Seg2, Vec2};
use solidcraft_engine::drawing_scene::{self, SheetObject, SheetPicture, Status};
use solidcraft_engine::geom::paper::text::TextRun;
use solidcraft_engine::geom::paper::{Clip, Dashes, Item, ObjectKind, Scene};
use solidcraft_engine::geom::pixel_grid::{GridWindow, PixelGrid};
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::geom::{Aabb2, Region2Geom, Seg2Geom};
use solidcraft_viewport::Camera;

use crate::SolveApp;
use crate::ui_kit;
use solidcraft_theme::Tokens;

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
/// The paper's corner radius on screen, in points: very slight (D29).
const PAPER_CORNER: u8 = 4;

/// The sheet's colour on screen (Preferences › Design › Drawings › Sheet colour). White unless
/// another is chosen: paper is white, whatever the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetColour {
    /// Follows the theme: White in a light theme, the theme's dark paper in a dark one (a theme
    /// file may set it: the Blueprint theme's is blueprint blue).
    Auto,
    White,
    Blueprint,
    /// Prussian Blue (Historical): a deeper blue with softer light lines.
    PrussianBlue,
}

impl SheetColour {
    /// In the order Preferences lists them.
    pub const ALL: [SheetColour; 4] = [SheetColour::Auto, SheetColour::White, SheetColour::Blueprint, SheetColour::PrussianBlue];

    /// The colour stored as `s`. `auto` was the default's name while the default followed the
    /// theme: a setting saved then was never chosen, and reads as today's default, White.
    pub fn parse(s: &str) -> Option<SheetColour> {
        if s == "auto" {
            return Some(SheetColour::White);
        }
        SheetColour::ALL.into_iter().find(|c| c.id() == s)
    }

    /// Its stored name.
    pub fn id(self) -> &'static str {
        match self {
            SheetColour::Auto => "theme",
            SheetColour::White => "white",
            SheetColour::Blueprint => "blueprint",
            SheetColour::PrussianBlue => "prussian_blue",
        }
    }

    fn label(self) -> &'static str {
        match self {
            SheetColour::Auto => "ui.sheet_page.colour.auto",
            SheetColour::White => "ui.sheet_page.colour.white",
            SheetColour::Blueprint => "ui.sheet_page.colour.blueprint",
            SheetColour::PrussianBlue => "ui.sheet_page.colour.prussian_blue",
        }
    }

    /// The paper and its ink in a theme (`dark`), before the paper stands off the backdrop.
    pub fn paper(self, t: &Tokens, dark: bool) -> (Color32, Color32) {
        match self {
            SheetColour::Auto if dark => (t.paper_dark, t.paper_dark_ink),
            SheetColour::Auto | SheetColour::White => (t.paper_white, t.paper_ink),
            SheetColour::Blueprint => (t.paper_blueprint, t.paper_blueprint_ink),
            SheetColour::PrussianBlue => (t.paper_prussian_blue, t.paper_prussian_blue_ink),
        }
    }
}

/// How light a colour looks, 0 to 1 (Rec. 709 weights on its sRGB values).
fn lightness(c: Color32) -> f32 {
    (0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b())) / 255.0
}

/// How far apart in [`lightness`] the paper and the backdrop must be for the sheet's edge to show.
pub const STAND_OFF: f32 = 0.06;

/// The lightest colour; as a picture's tint, it keeps the picture's own colours.
const WHITE: Color32 = Color32::WHITE;

/// Lightest and darkest: what the stage, or failing that the paper, moves toward.
const ENDS: [Color32; 2] = [WHITE, Color32::BLACK];

/// The first of twenty steps from `from` toward `to`, at most `steps` of them, that stands off `other`.
fn toward(from: Color32, to: Color32, other: Color32, steps: u8) -> Option<Color32> {
    (1..=steps.min(20)).map(|k| from.lerp_to_gamma(to, f32::from(k) / 20.0)).find(|c| (lightness(*c) - lightness(other)).abs() >= STAND_OFF)
}

/// `paper` as shown on `backdrop`: as it is when the two are [`STAND_OFF`] apart, else moved
/// lighter where that's enough, else darker.
pub fn stand_off(paper: Color32, backdrop: Color32) -> Color32 {
    if (lightness(paper) - lightness(backdrop)).abs() >= STAND_OFF {
        return paper;
    }
    toward(paper, ENDS[0], backdrop, 20).or_else(|| toward(paper, ENDS[1], backdrop, 20)).unwrap_or(paper)
}

/// The stage behind the sheet and the paper on it (the design's "neutral stage with the paper on
/// it"): the paper keeps its colour and the stage moves away from it, darker behind a light sheet and
/// lighter behind a dark one, by at most two fifths of the way, until the two stand
/// [`STAND_OFF`] apart. Only when that isn't enough does the paper move instead ([`stand_off`]).
pub fn stage_and_paper(paper: Color32, backdrop: Color32) -> (Color32, Color32) {
    if (lightness(paper) - lightness(backdrop)).abs() >= STAND_OFF {
        return (backdrop, paper);
    }
    let away = if lightness(paper) >= 0.5 { ENDS[1] } else { ENDS[0] };
    match toward(backdrop, away, paper, 8) {
        Some(stage) => (stage, paper),
        None => (backdrop, stand_off(paper, backdrop)),
    }
}

/// The stage, paper and ink the page draws in, now.
pub fn sheet_colours(app: &SolveApp) -> (Color32, Color32, Color32) {
    let t = Tokens::get();
    let colour = SheetColour::parse(&app.preferences.drawing_sheet_colour).unwrap_or(SheetColour::White);
    let (paper, ink) = colour.paper(&t, solidcraft_theme::is_dark());
    let (stage, paper) = stage_and_paper(paper, t.viewport_top);
    (stage, paper, ink)
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
    /// The sheet and the size it was last fitted to: Fit sheet again when either changes.
    fitted: Option<(u64, [u64; 2])>,
    /// The canvas size the sheet was fitted on, while nothing has moved it since: the sheet is
    /// fitted again when the canvas changes size (a window resized, a panel opened).
    fitted_on: Option<egui::Vec2>,
    /// The sheet shown, when the drawing still has it.
    sheet: Option<u64>,
    drawing: Option<(u64, u64)>,
    marks: Option<Marks>,
    /// The sheet's pictures' pixels on the GPU: they change apart from the marks, as each is drawn.
    pictures: PictureTextures,
    view_tools: view_tools::State,
    /// The object under the pointer on the last frame, as the selection names it: what a
    /// right-click's menu is about.
    hover: Option<solidcraft_engine::Sel>,
    /// What the last frame showed, for `ui.inspect`.
    shown: Option<Value>,
}

impl Default for Page {
    fn default() -> Self {
        let cam = Camera { yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, fov: 0.0, distance: 1000.0, ..Camera::default() };
        Page {
            cam,
            scale: true_size(1.0, 1.0),
            fitted: None,
            fitted_on: None,
            sheet: None,
            drawing: None,
            marks: None,
            pictures: PictureTextures::default(),
            view_tools: Default::default(),
            hover: None,
            shown: None,
        }
    }
}

/// Load one view's camera/cache without copying its authored sheet or selection.
pub(crate) fn activate_view(app: &mut SolveApp, id: u64) {
    let open: Vec<_> = crate::view_windows::layout_views(app)
        .into_iter()
        .filter(|(_, view)| view.kind == solidcraft_views::ViewKind::Sheet)
        .map(|(_, view)| view.id)
        .collect();
    let mut sheets: Vec<_> = crate::view_windows::layout_views(app)
        .into_iter()
        .filter(|(_, view)| view.kind == solidcraft_views::ViewKind::Sheet)
        .filter_map(|(_, view)| crate::sheet_views::resolve_for_session(&app.session, view.subject.as_deref()))
        .collect();
    if app.drawing_workspace.page_view == 0
        && let Some(((_, drawing), sheet)) = app.drawing_workspace.page.drawing.zip(app.drawing_workspace.page.sheet)
    {
        sheets.push((drawing, sheet));
    }
    let state = &mut app.drawing_workspace;
    if state.page_view != id {
        let outgoing = std::mem::take(&mut state.page);
        state.pages.insert(state.page_view, outgoing);
        state.page = state.pages.remove(&id).unwrap_or_default();
        state.page_view = id;
    }
    state.pages.retain(|view, _| *view == 0 || open.contains(view));
    state.sheet_selection.retain(|key, _| sheets.contains(key));
    state.sheet_picks.retain(|key, _| sheets.contains(key));
}

pub(crate) fn retire_closed(app: &mut SolveApp) {
    let current = app.drawing_workspace.page_view;
    let survives = current == 0
        || crate::view_windows::layout_views(app).into_iter().any(|(_, view)| view.id == current && view.kind == solidcraft_views::ViewKind::Sheet);
    activate_view(app, if survives { current } else { 0 });
}

/// Scope passive painting to an exact sheet. Focus, history and the command receiver return intact.
pub(crate) fn show_view(app: &mut SolveApp, ui: &mut egui::Ui, id: u64) -> bool {
    let descriptor = crate::view_windows::layout_views(app)
        .into_iter()
        .find(|(_, view)| view.id == id && view.kind == solidcraft_views::ViewKind::Sheet)
        .map(|(_, view)| view.clone());
    let Some(descriptor) = descriptor else {
        message(ui, ui.max_rect(), app.tr("sheet_views.missing"), Tokens::get().error);
        return true;
    };
    let subject = descriptor.subject;
    let (drawing, sheet) = match crate::sheet_views::resolve(app, subject.as_deref()) {
        Ok(owner) => owner,
        Err(error) => {
            message(ui, ui.max_rect(), &error, Tokens::get().error);
            return true;
        }
    };
    let previous = (
        app.drawing_workspace.page_view,
        app.session.open_drawing,
        app.ui.tab.clone(),
        app.session.model_focus,
        app.ui.ribbon_context.clone(),
        app.ui.sketch_return_tab.clone(),
    );
    // Navigation is still a command, and its receiver prunes foreign drawing picks.
    // Only the actual focused view may change the authoritative selection.
    let focused_selection = (app.session.view_context.focused() != id).then(|| app.session.selection.clone());
    activate_view(app, id);
    app.session.open_drawing = Some(drawing);
    app.session.model_focus = false;
    select_sheet(app, sheet);
    if !crate::drawing_workspace::active(app) {
        app.ui.tab = "DRAWING LAYOUT".into();
    }
    if focused_selection.is_none() {
        // The focused view's selected object shows its options, as on the classic Drawing canvas.
        crate::drawing_workspace::follow_selection(app);
    }
    let shown = show(app, ui);
    app.session.open_drawing = previous.1;
    app.session.model_focus = previous.3;
    app.ui.tab = previous.2;
    app.ui.ribbon_context = previous.4;
    app.ui.sketch_return_tab = previous.5;
    if let Some(selection) = focused_selection {
        app.session.selection = selection;
    }
    activate_view(app, previous.0);
    shown
}

pub(crate) fn view_badge(app: &SolveApp, id: u64, ctx: &egui::Context) -> Option<(String, String)> {
    let view = app.docs.views.get(id).filter(|view| view.kind == solidcraft_views::ViewKind::Sheet)?;
    let (drawing, sheet) = crate::sheet_views::resolve(app, view.subject.as_deref()).ok()?;
    let sheet = app.session.doc.drawing(drawing)?.sheets.iter().find(|record| record.id == sheet)?;
    let paper = match sheet.sheet.size {
        solidcraft_engine::geom::drawing_sheet::SheetSize::Standard(size) => format!("{size:?}"),
        solidcraft_engine::geom::drawing_sheet::SheetSize::Custom { sides_mm } => format!("{} × {}", sides_mm[0], sides_mm[1]),
    };
    let page = if app.drawing_workspace.page_view == id { Some(&app.drawing_workspace.page) } else { app.drawing_workspace.pages.get(&id) };
    let state = match page {
        Some(page) if page.fitted_on.is_none() && page.fitted.is_some() => {
            solidcraft_ui_text::format("sheet_views.zoom", &[("value", &shown_percent(page.scale / true_size_now(app, ctx)).to_string())])
        }
        _ => app.tr("sheet_views.fit").to_owned(),
    };
    Some((paper, state))
}

/// The canvas sheet, also used by actions that need a default sheet.
pub(crate) fn active_sheet(app: &SolveApp) -> Option<u64> {
    let drawing = app.session.drawing(drawing_scene::CMD).ok()?;
    let page = &app.drawing_workspace.page;
    page.sheet
        .filter(|id| page.drawing == Some((app.session.uid, drawing.id)) && drawing.sheets.iter().any(|s| s.id == *id))
        .or_else(|| drawing.sheets.first().map(|s| s.id))
}

pub(crate) fn select_sheet(app: &mut SolveApp, sheet: u64) {
    let Ok(drawing) = app.session.drawing(drawing_scene::CMD) else { return };
    if !drawing.sheets.iter().any(|s| s.id == sheet) {
        return;
    }
    let page = &mut app.drawing_workspace.page;
    let owner = (app.session.uid, drawing.id);
    if page.drawing != Some(owner) || page.sheet != Some(sheet) {
        page.drawing = Some(owner);
        page.sheet = Some(sheet);
        page.fitted = None;
        page.fitted_on = None;
        page.marks = None;
        page.pictures = PictureTextures::default();
        page.view_tools = Default::default();
        page.hover = None;
        page.shown = None;
    }
}

/// The object of the open drawing under the pointer on the last frame, as the selection names it.
pub(crate) fn hovered(app: &SolveApp) -> Option<solidcraft_engine::Sel> {
    app.drawing_workspace.page.hover.clone()
}

/// Fit Sheet: the sheet is framed again on the next frame.
pub fn fit(app: &mut SolveApp) {
    app.drawing_workspace.page.fitted = None;
}

/// Clear the selected view or cancel its drag.
pub(crate) fn clear_view_selection(app: &mut SolveApp) -> bool {
    view_tools::clear(app)
}

/// The sheet's selected drawing view, shared by every camera that shows that sheet.
pub(crate) fn selected_drawing_view(app: &SolveApp) -> Option<u64> {
    let sheet = active_sheet(app)?;
    let drawing = app.session.drawing(drawing_scene::CMD).ok()?;
    match app.session.selection.as_slice() {
        [solidcraft_engine::Sel::Drawing { drawing: owner, object }]
            if *owner == drawing.id && drawing.view(*object).is_some_and(|view| view.sheet == sheet) =>
        {
            Some(*object)
        }
        _ => None,
    }
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

/// The current zoom readout, measured against the calibrated paper scale.
pub(crate) fn zoom_percent(app: &SolveApp, ctx: &egui::Context) -> f32 {
    shown_percent(app.drawing_workspace.page.scale / true_size_now(app, ctx))
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
pub(crate) struct OnScreen {
    centre: Pos2,
    target: Vec2,
    scale: f64,
}

impl OnScreen {
    /// A sheet shown one point a millimetre round its origin, for a tool's test.
    #[cfg(test)]
    pub(crate) fn for_test() -> OnScreen {
        OnScreen { centre: Pos2::ZERO, target: Vec2::ZERO, scale: 1.0 }
    }

    fn of(cam: &Camera, canvas: Rect) -> OnScreen {
        let scale = f64::from(canvas.height().max(1.0)) / (2.0 * cam.half_height());
        OnScreen { centre: canvas.center(), target: Vec2::new(cam.target.x, cam.target.y), scale }
    }

    pub(crate) fn screen(self, p: Vec2) -> Pos2 {
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

    pub(crate) fn paint_preview(self, painter: &egui::Painter, marks: &Marks, position: Vec2, ink: Color32) {
        let shifted = Self { centre: self.screen(position), target: Vec2::ZERO, scale: self.scale };
        let clip = painter.clip_rect();
        // A preview shows the frames of its pictures: its pixels aren't drawn until it is placed.
        marks.paint(painter, &shifted, shifted.visible(clip), ink, painter.ctx().pixels_per_point(), &PictureTextures::default());
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
    let (_, scale) = sheet_fit(area, size, None, true_size, margin).unwrap_or((Vec2::ZERO, true_size.max(1e-9)));
    let x = size.x / 2.0 - f64::from(area.center().x - canvas.center().x) / scale;
    let y = size.y / 2.0 + f64::from(area.center().y - canvas.center().y) / scale;
    (Vec2::new(x, y), scale)
}

/// Pure cell fit; manual pan/zoom is preserved. Fitted paper may shrink below manual zoom limits.
pub(crate) fn sheet_fit(cell: Rect, paper: Vec2, manual: Option<(Vec2, f64)>, true_size: f64, padding: f32) -> Option<(Vec2, f64)> {
    if !cell.is_finite()
        || !cell.is_positive()
        || !paper.x.is_finite()
        || !paper.y.is_finite()
        || paper.x <= 0.0
        || paper.y <= 0.0
        || !true_size.is_finite()
        || true_size <= 0.0
        || !padding.is_finite()
        || padding < 0.0
    {
        return None;
    }
    if let Some((target, scale)) = manual {
        return (target.x.is_finite() && target.y.is_finite() && scale.is_finite() && scale > 0.0).then_some((target, scale));
    }
    let padding = padding.min(cell.width().min(cell.height()) * 0.25);
    let scale =
        (f64::from(cell.width() - 2.0 * padding) / paper.x).min(f64::from(cell.height() - 2.0 * padding) / paper.y).min(true_size * ZOOM.end());
    (scale.is_finite() && scale > 0.0).then_some((paper * 0.5, scale))
}

/// The part of the canvas no ribbon, Browser, History or navigation bar covers.
fn free_area(app: &SolveApp, canvas: Rect) -> Rect {
    let mut area = crate::shell::workspace_content(app, crate::shell::content_rect(app, canvas));
    // The ribbon, the Browser and the History float over the canvas in the classic layout: the
    // sheet is framed beside them, not under them (#19070). Each takes the side that costs the paper the least room.
    let around = area;
    for cover in app.shell.cards.iter().copied().chain(app.shell.browser.filter(|_| app.ui.show_browser)).chain(app.shell.history) {
        if !cover.is_finite() || !cover.is_positive() || !cover.intersects(area) {
            continue;
        }
        // The cover takes the side that costs the paper the least room, whatever its shape: a
        // short or folded side Browser is wider than tall, yet still takes the left, and the
        // ribbon, which reaches the right edge too, takes the top.
        let cost = [
            (cover.right() - area.min.x).max(0.0) * area.height(),
            (area.max.x - cover.left()).max(0.0) * area.height(),
            (cover.bottom() - area.min.y).max(0.0) * area.width(),
            (area.max.y - cover.top()).max(0.0) * area.width(),
        ];
        let nearest = (0..4).min_by(|&a, &b| cost[a].total_cmp(&cost[b])).unwrap_or(0);
        match nearest {
            0 => area.min.x = area.min.x.max(cover.right()),
            1 => area.max.x = area.max.x.min(cover.left()),
            2 => area.min.y = area.min.y.max(cover.bottom()),
            _ => area.max.y = area.max.y.min(cover.top()),
        }
    }
    if !area.is_positive() {
        area = around;
    }
    // Nor the navigation bar: Fit sheet puts the paper above it (below it when the bar is at the
    // top), as the design's layout board does. The compass's corner stays free: the sheet has none.
    let Some(bar) = app.viewport.layout.as_ref().map(|l| l.navigation).filter(|r| r.is_positive() && r.intersects(area)) else {
        return area;
    };
    let mut free = area;
    if bar.center().y > area.center().y {
        free.max.y = free.max.y.min(bar.top());
    } else {
        free.min.y = free.min.y.max(bar.bottom());
    }
    if free.is_positive() { free } else { area }
}

/// The camera's distance that shows `scale` points per millimetre on a canvas `height` tall.
fn distance_for(scale: f64, height: f32) -> f64 {
    (f64::from(height.max(1.0)) / (2.0 * scale.max(1e-9) * 0.4)).clamp(1e-3, 1e7)
}

/// Selects this sheet for the worker, then reads the scene after its tick. The final synchronous
/// step can finish with no work left, so painting the earlier scene would miss its new marks.
fn scene_for_frame(app: &mut SolveApp, sheet: u64) -> solidcraft_engine::Result<(drawing_scene::SheetView, bool)> {
    drawing_scene::extract(&mut app.session, sheet)?;
    let working = crate::drawing_jobs::tick(app);
    let view = drawing_scene::extract(&mut app.session, sheet)?;
    Ok((view, working))
}

/// Draws the sheet of the drawing open, and says whether there was one to draw: without a drawing
/// open the workspace shows its own message.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    app.drawing_workspace.page.shown = None;
    let Ok(drawing) = app.session.drawing(drawing_scene::CMD).cloned() else {
        if crate::drawing_workspace::placing_view(app) {
            crate::drawing_workspace::close_panel(app);
        }
        app.drawing_workspace.page = Page::default();
        return false;
    };
    let Some(sheet) = active_sheet(app) else {
        return false;
    };
    select_sheet(app, sheet);
    let t = Tokens::get();
    let canvas = ui.max_rect();
    let (stage, paper, ink) = sheet_colours(app);
    ui.painter().rect_filled(canvas, 0.0, stage);
    app.viewport.rect = Some(canvas);
    crate::viewport::refresh_layout(app, ui.ctx());
    let response = ui.allocate_rect(canvas, Sense::click_and_drag());
    let (view, working) = match scene_for_frame(app, sheet) {
        Ok(view) => view,
        Err(e) => {
            message(ui, canvas, &e.to_string(), t.error);
            return true;
        }
    };
    // A view worked out beside the window, or text waiting for its font: look again soon.
    if working || view.waiting_for_fonts {
        ui.ctx().request_repaint_after(Duration::from_millis(16));
    }
    let size = view.scene.size_mm;
    let base = true_size_now(app, ui.ctx());
    let owns_tools = app.drawing_workspace.page_view == 0 || app.session.view_context.focused() == app.drawing_workspace.page_view;
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
    let before_navigation = OnScreen::of(&page.cam, canvas);
    // As in the model, a primary drag the navigation does not take is a box selection.
    let mut box_free = true;
    if !owns_tools || !view_tools::owns_drag(app, &response, before_navigation, &view.objects) {
        with_sheet_camera(app, |app| {
            let input = crate::navigation::handle(app, ui, canvas, &response, crate::navigation::NavCaps { orbit: false, pick: false });
            box_free = input.primary_nav.is_none() && input.mode.is_none();
        });
    } else {
        box_free = false;
    }
    let page = &mut app.drawing_workspace.page;
    let scale = OnScreen::of(&page.cam, canvas).scale;
    page.scale = if scale.is_finite() && scale > 0.0 {
        if page.fitted_on.is_some() {
            scale.min(base * ZOOM.end())
        } else {
            // A tiny cell can fit below the manual zoom floor. Panning must retain that scale.
            scale.clamp(page.scale.min(base * ZOOM.start()), base * ZOOM.end())
        }
    } else {
        base
    };
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
    let detail = canvas.width() >= 160.0;
    if page.marks.as_ref().is_none_or(|m| m.generation != view.generation || m.tolerance != tolerance || m.detail != detail) {
        page.marks = Some(if detail {
            Marks::of(&view.scene, view.generation, tolerance)
        } else {
            Marks::of_detail(&view.scene, view.generation, tolerance, false)
        });
    }
    let max_side = ui.ctx().input(|i| i.max_texture_side);
    page.pictures.sync(ui.ctx(), &view.pictures, max_side);
    let painter = ui.painter().with_clip_rect(canvas.intersect(ui.clip_rect()));
    let sheet_rect = at.rect(&Aabb2 { min: Vec2::ZERO, max: size });
    // A very slight rounded corner, on screen only (D29): an export stays square.
    let mut shadow = ui_kit::Container::popup_frame(&ui.ctx().global_style()).shadow;
    shadow.color = shadow.color.gamma_multiply(0.3);
    painter.add(shadow.as_shape(sheet_rect, PAPER_CORNER));
    painter.rect_filled(sheet_rect, PAPER_CORNER, paper);
    painter.rect_stroke(sheet_rect, PAPER_CORNER, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let visible = at.visible(canvas);
    let page = &app.drawing_workspace.page;
    if let Some(marks) = &page.marks {
        marks.paint(&painter, &at, visible, ink, ppp as f32, &page.pictures);
    }
    crate::scenario::publish_rect("sheet:paper", sheet_rect);
    if app.drawing_workspace.page_view != 0 {
        crate::scenario::publish_rect(&format!("sheet:paper:{}", app.drawing_workspace.page_view), sheet_rect);
    }
    let hover = response.hover_pos().filter(|p| !crate::shell::covered(app, *p)).map(|p| at.paper(p));
    let on_paper = hover.filter(|p| (0.0..=size.x).contains(&p.x) && (0.0..=size.y).contains(&p.y));
    let placement_painter = painter.with_clip_rect(sheet_rect.intersect(painter.clip_rect()));
    let placing =
        owns_tools && crate::drawing_workspace::placement::on_sheet(app, &placement_painter, at, &drawing, sheet, on_paper, response.clicked());
    if owns_tools && crate::drawing_workspace::placing_detail(app) {
        crate::drawing_workspace::detail::follow_sheet(app, sheet);
        if let Some(point) = on_paper {
            crate::drawing_workspace::detail::on_sheet(app, &placement_painter, at, &view.objects, sheet, point, response.clicked());
        }
    }
    let placing_note = owns_tools && crate::drawing_workspace::placing_note(app);
    if placing_note && let Some(point) = on_paper {
        crate::drawing_workspace::note::on_sheet(app, &placement_painter, at, point, sheet, response.clicked());
    }
    let placing_dimension = owns_tools && crate::drawing_workspace::placing_dimension(app);
    // A placed dimension's text, or a note, under the pointer comes before the tool's pick, as a
    // sketch dimension's text does: it lights up and drags, and nothing under it is picked. Not
    // once the picks are made, when a click anywhere places the new dimension.
    let band = HOVER_BAND / at.scale;
    let annotation = hover
        .filter(|_| placing_dimension && !crate::drawing_workspace::dimension::has_picks(app))
        .and_then(|p| drawing_scene::hit(&view.objects, p, band).map(|hit| (p, hit)))
        .filter(|(_, hit)| matches!(hit.kind, ObjectKind::Dimension | ObjectKind::Note) && drawing.has_object(hit.object))
        .filter(|(p, hit)| hit.kind == ObjectKind::Note || view_tools::over_text(app, hit.object, *p, band))
        .map(|(_, hit)| hit);
    if placing_dimension
        && annotation.is_none()
        && !view_tools::dragging_dimension()
        && let Some(point) = on_paper
    {
        // The second click of a double-click is not a click of the tool's: it would place the
        // dimension where the first click picked its edge, on top of the edge.
        let clicked = response.clicked() && !response.double_clicked();
        crate::drawing_workspace::dimension::on_sheet(app, &placement_painter, at, sheet, point, HOVER_BAND / at.scale, clicked);
    }
    // With the tool in hand a double-click on a placed dimension's text edits it where it is, as
    // the sketch's Dimension tool does, and the tool stays in hand; anywhere else it changes nothing.
    if response.double_clicked()
        && let Some(hit) = annotation.filter(|hit| hit.kind == ObjectKind::Dimension)
        && let Some(pointer) = response.interact_pointer_pos()
    {
        crate::drawing_workspace::edit_in_place(app, hit.object, pointer);
    }
    let placing = placing || placing_note || placing_dimension || (owns_tools && crate::drawing_workspace::placing_detail(app));
    view_tools::Canvas {
        interactive: owns_tools,
        at,
        painter: &painter,
        response: &response,
        drawing: &drawing,
        sheet,
        objects: &view.objects,
        free: free_area(app, canvas),
    }
    .show(app, ui, placing);
    if owns_tools {
        view_tools::box_select(app, ui, &painter, &response, at, &drawing, &view.objects, canvas, box_free && !placing);
    }
    let hit = hover.filter(|_| !placing).and_then(|p| drawing_scene::hit(&view.objects, p, HOVER_BAND / at.scale)).or(annotation);
    app.drawing_workspace.page.hover =
        hit.filter(|hit| drawing.has_object(hit.object)).map(|hit| solidcraft_engine::Sel::Drawing { drawing: drawing.id, object: hit.object });
    // A right-click, as in the model and a sketch: it finishes the tool in hand, or else opens the
    // menu on what is under the pointer.
    if owns_tools
        && response.secondary_clicked()
        && !crate::mouse_gesture::took_press(app)
        && let Some(point) = response.interact_pointer_pos().filter(|point| !crate::shell::covered(app, *point))
    {
        if placing {
            crate::drawing_workspace::close_panel(app);
        } else {
            let held = crate::tool_modifiers::Held::from_modifiers(ui.input(|input| input.modifiers));
            crate::context_menu::open(app, point, held);
        }
    }
    if owns_tools
        && response.double_clicked()
        && !placing
        && let Some(hit) = hit
    {
        let kind = match hit.kind {
            ObjectKind::View => Some("view"),
            ObjectKind::Dimension => Some("dimension"),
            ObjectKind::Note => Some("note"),
            ObjectKind::PartsList => Some("parts_list"),
            _ => None,
        };
        // A dimension's or a note's text is edited where it is, as a sketch dimension's value is.
        let at = response.interact_pointer_pos().or_else(|| response.hover_pos());
        if let Some(kind) = kind
            && !at.is_some_and(|at| crate::drawing_workspace::edit_in_place(app, hit.object, at))
        {
            crate::drawing_workspace::edit_object(app, kind, hit.object);
        }
    }
    // A text cell of the title block likewise: what it shows is typed where it is.
    if owns_tools
        && response.double_clicked()
        && !placing
        && !hit.is_some_and(|hit| drawing.has_object(hit.object) || hit.kind == ObjectKind::PartsList)
        && let (Some(point), Some(at)) = (on_paper, response.interact_pointer_pos())
    {
        crate::drawing_workspace::edit_title_cell(app, sheet, point, at);
    }
    if owns_tools {
        crate::dim_view::show_sheet_editor(app, ui.ctx());
    }
    // What is hovered or selected is drawn again in the accent colour, as a sketch draws its
    // hovered and selected dimensions and curves: no box round it.
    let mut lit = view_tools::selected(app, drawing.id);
    lit.extend(hit.filter(|hit| drawing.has_object(hit.object)).map(|hit| hit.object));
    if !lit.is_empty()
        && let Some(marks) = &app.drawing_workspace.page.marks
    {
        marks.paint_lit(&painter, &at, visible, ppp as f32, &app.drawing_workspace.page.pictures, &lit, t.accent);
    }
    let states = view_states(&painter, &at, &view.views, &view.stale, &view.objects);
    let first_lines = waiting_views(&painter, &at, &drawing, &view);
    // Views a deferred drawing holds stay out of date without being worked on: their words say so.
    let waiting = view.stale.len() + view.stale_dimensions.len();
    if working {
        let text = solidcraft_ui_text::format("ui.sheet_page.working", &[("count", &waiting.max(1).to_string())]);
        hint(&painter, free_area(app, canvas), &text);
    }
    let area = free_area(app, canvas);
    if paint_offscreen_arrow(ui, &painter, area, sheet_rect, &t) {
        let page = &mut app.drawing_workspace.page;
        let offset = area.center() - canvas.center();
        page.cam.target.x = size.x * 0.5 - f64::from(offset.x) / at.scale;
        page.cam.target.y = size.y * 0.5 + f64::from(offset.y) / at.scale;
        page.fitted_on = None;
        ui.ctx().request_repaint();
    }
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
        "stage": hex(stage),
        "paper": hex(paper),
        "ink": hex(ink),
        "generation": view.generation,
        "views": states,
        "waiting_views": first_lines.iter().copied().map(encode).collect::<Vec<_>>(),
        "pictures": app.drawing_workspace.page.pictures.inspect(),
        "working": working,
        "hover": hit.map(|h| json!({"object": encode(h.object), "kind": kind_name(h.kind), "on_edge": h.on_edge, "lit": lit.contains(&h.object)})),
        "error": app.drawing_workspace.page.marks.as_ref().and_then(|m| m.error.clone()),
    }));
    // The shared toasts, as the model and render pages show them: what a command or an export or
    // print just said, which the sheet would otherwise leave unseen.
    crate::viewport::transient_messages(app, ui, canvas, None);
    true
}

/// Intersect the ray towards the paper's centre with the usable viewport edge. Inset the
/// arrow's centre enough to keep the whole arrow inside the viewport.
fn offscreen_arrow(area: Rect, sheet: Rect, margin: f32) -> Option<(Pos2, egui::Vec2)> {
    let inset = area.shrink(margin);
    if !inset.is_positive() || !sheet.is_finite() || !sheet.is_positive() || area.intersects(sheet) {
        return None;
    }
    let centre = inset.center();
    let toward = sheet.center() - centre;
    let distance = (inset.width() * 0.5 / toward.x.abs()).min(inset.height() * 0.5 / toward.y.abs());
    Some((centre + toward * distance, toward.normalized()))
}

fn paint_offscreen_arrow(ui: &mut egui::Ui, painter: &egui::Painter, area: Rect, sheet: Rect, t: &Tokens) -> bool {
    let diameter = t.space_24 * 2.5;
    let Some((centre, direction)) = offscreen_arrow(area, sheet, diameter * 0.5 + t.space_8) else { return false };
    let rect = Rect::from_center_size(centre, egui::Vec2::splat(diameter));
    let response = ui.interact(rect, ui.make_persistent_id("sheet:offscreen_arrow"), Sense::click());
    let label = solidcraft_ui_text::tr("ui.drawing_offscreen.recenter");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let clicked = response.clicked();

    let normal = egui::vec2(-direction.y, direction.x);
    let tip = centre + direction * t.space_16;
    let head = tip - direction * t.space_12;
    let stroke = Stroke::new(t.space_2, if response.hovered() { t.accent } else { t.text });
    painter.line_segment([centre - direction * t.space_16, tip], stroke);
    painter.line_segment([head + normal * t.space_8, tip], stroke);
    painter.line_segment([tip, head - normal * t.space_8], stroke);
    crate::ui_kit::BackingTooltipExt::backing_on_hover_text(response.on_hover_cursor(egui::CursorIcon::PointingHand), label);
    crate::scenario::publish_rect("sheet:offscreen_arrow", rect);
    clicked
}

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::View => "view",
        ObjectKind::Sketch => "sketch",
        ObjectKind::Fill => "fill",
        ObjectKind::Dimension => "dimension",
        ObjectKind::Note => "note",
        ObjectKind::CentreMark => "centre_mark",
        ObjectKind::CentreLine => "centre_line",
        ObjectKind::ViewLabel => "view_label",
        ObjectKind::Balloon => "balloon",
        ObjectKind::RevisionTag => "revision_tag",
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
/// with the boundary's shape: dashed when it needs an update, dotted when there's nothing to draw,
/// crossed when it can't be drawn. Returns each view's state for `ui.inspect`.
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
        // D26's shape for Needs update, ahead of the words.
        let mark = if out_of_date { "⟳ " } else { "" };
        say(painter, r, format!("{mark}{}", words.join(" · ")), colour, state.view);
    }
    out
}

/// A view's state in words on a chip above its box `r`, for every state alike.
fn say(painter: &egui::Painter, r: Rect, text: String, colour: Color32, view: u64) {
    let t = Tokens::get();
    let galley = painter.layout_no_wrap(text, egui::FontId::proportional(t.font_label), colour);
    let at = r.left_top() + vec2(t.space_4, -(galley.size().y + t.space_8));
    let chip = Rect::from_min_size(at, galley.size()).expand2(vec2(t.space_4, t.space_2));
    painter.rect_filled(chip, chip.height() / 2.0, t.panel);
    painter.galley(at, galley, colour);
    crate::scenario::publish_rect(&format!("sheet:view:{}:state", solidcraft_engine::doc::ids::encode(view)), chip);
}

/// The views placed on the sheet whose first lines are still being worked out: each says
/// "Projecting…" at its place on the sheet, from the frame it is placed, so a view that takes long
/// is never silence. Their ids, for `ui.inspect`. Where it says so is the view's centre, which the
/// drawing has without measuring anything: the window never works a view's extents out for it. A
/// view drawn before keeps its last lines and says Needs update instead ([`view_states`]).
fn waiting_views(
    painter: &egui::Painter,
    at: &OnScreen,
    drawing: &solidcraft_engine::doc::drawing::Drawing,
    view: &drawing_scene::SheetView,
) -> Vec<u64> {
    let t = Tokens::get();
    let waiting: Vec<u64> =
        view.stale.iter().copied().filter(|id| !view.objects.iter().any(|o| o.kind == ObjectKind::View && o.object == *id)).collect();
    for id in &waiting {
        let Ok(centre) = drawing.view_centre(*id) else { continue };
        // The words alone, on the chip the other states use: no outline round an empty place.
        let place = Rect::from_center_size(at.screen(centre), egui::Vec2::ZERO);
        say(painter, place, solidcraft_ui_text::tr("ui.sheet_page.projecting").to_owned(), t.text_dim, *id);
    }
    waiting
}

/// What is said beside a view: why it draws nothing, and whether it needs an update (D26's
/// "Needs update"). Nothing for a
/// view drawn and current.
pub(crate) fn words(status: &Status, out_of_date: bool) -> Vec<String> {
    let mut words = Vec::new();
    match status {
        Status::Drawn => {}
        Status::Placeholder(why) => words.push(solidcraft_ui_text::format("ui.sheet_page.placeholder", &[("reason", &why.to_string())])),
        Status::Failed(why) => words.push(solidcraft_ui_text::format("ui.sheet_page.failed", &[("reason", why)])),
    }
    if out_of_date {
        words.push(solidcraft_ui_text::tr("ui.sheet_page.needs_update").to_owned());
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

/// The zoom as the navigation bar shows it, in percent: whole above 10 %, to a tenth below.
pub(crate) fn shown_percent(zoom: f64) -> f32 {
    let percent = zoom * 100.0;
    let shown = if percent < 10.0 { (percent * 10.0).round() / 10.0 } else { percent.round() };
    shown as f32
}

/// What older versions could draw on the stage round the sheet: nothing is drawn there now, ever
/// (it didn't zoom with the sheet). The preference is still read, so a saved one still loads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SheetBackdrop {
    DatumCloud,
    Grid,
    #[default]
    None,
}

impl SheetBackdrop {
    const ALL: [(Self, &str, &str); 3] = [
        (Self::DatumCloud, "datum_cloud", "ui.drawing_backdrop.datum_cloud"),
        (Self::Grid, "grid", "ui.drawing_backdrop.grid"),
        (Self::None, "none", "ui.drawing_backdrop.none"),
    ];

    pub(crate) fn parse(id: &str) -> Option<Self> {
        Self::ALL.iter().find_map(|(pattern, value, _)| (*value == id).then_some(*pattern))
    }
}

/// Preferences' Sheet colour choice.
pub(crate) fn colour_choice(app: &mut SolveApp, ui: &mut egui::Ui) {
    let labels: Vec<&str> = SheetColour::ALL.iter().map(|c| app.tr(c.label())).collect();
    let current = SheetColour::parse(&app.preferences.drawing_sheet_colour).unwrap_or(SheetColour::White);
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
    detail: bool,
    pub(crate) marks: Vec<Mark>,
    /// The object each mark draws, by index: what is hovered or selected is drawn in the accent
    /// colour, as a sketch draws its hovered and selected dimensions and curves.
    owners: Vec<u64>,
    /// Whether each mark, by index, is a detail view's callout: its outline and letter, drawn on the
    /// parent view. It belongs to the parent's place on the sheet, so dragging the detail leaves it.
    on_parent: Vec<bool>,
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
    /// A picture: its pixels from [`PictureTextures`] over `rect` at `opacity`, else its frame. Only
    /// what lies `within` (the sheet, inside its object's clip's box) is drawn, and inside `clip`'s
    /// circle, as every writer cuts it.
    Picture {
        source: u64,
        rect: Aabb2,
        opacity: f64,
        clip: Option<Clip>,
        within: Aabb2,
    },
}

impl Marks {
    /// `scene`'s marks within `tolerance` millimetres, or coarser when that would be more than
    /// [`MAX_POINTS`]; a scene that can't be checked draws nothing and says why.
    pub(crate) fn of(scene: &Scene, generation: u64, tolerance: f64) -> Marks {
        Self::of_detail(scene, generation, tolerance, true)
    }

    fn of_detail(scene: &Scene, generation: u64, tolerance: f64, detail: bool) -> Marks {
        let mut tol = tolerance;
        for _ in 0..8 {
            match if detail { marks(scene, tol) } else { marks_owned_detail(scene, tol, false) } {
                Ok((marks, owners, on_parent)) => {
                    let error = (tol != tolerance).then(|| solidcraft_ui_text::tr("ui.sheet_page.coarse").to_owned());
                    return Marks { generation, tolerance, detail, marks, owners, on_parent, error };
                }
                Err(Fault::TooMany) => tol *= 8.0,
                Err(Fault::Invalid(why)) => {
                    return Marks { generation, tolerance, detail, marks: Vec::new(), owners: Vec::new(), on_parent: Vec::new(), error: Some(why) };
                }
            }
        }
        Marks {
            generation,
            tolerance,
            detail,
            marks: Vec::new(),
            owners: Vec::new(),
            on_parent: Vec::new(),
            error: Some(solidcraft_ui_text::tr("ui.sheet_page.coarse").to_owned()),
        }
    }

    fn paint(&self, painter: &egui::Painter, at: &OnScreen, visible: Aabb2, ink: Color32, ppp: f32, pictures: &PictureTextures) {
        self.paint_some(painter, at, visible, ink, ppp, pictures, None, false);
    }

    /// The marks of the objects in `lit` again, over the sheet, in `accent`: a hovered or selected
    /// object, as a sketch shows its hovered and selected dimensions and curves, with no box round it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_lit(
        &self,
        painter: &egui::Painter,
        at: &OnScreen,
        visible: Aabb2,
        ppp: f32,
        pictures: &PictureTextures,
        lit: &[u64],
        accent: Color32,
    ) {
        self.paint_some(painter, at, visible, accent, ppp, pictures, Some(lit), false);
    }

    /// The marks of the objects in `moving` in `accent`, as a drag shows them where they go: a detail
    /// view's callout on its parent is left out, since it stays where the parent is drawn.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_moving(
        &self,
        painter: &egui::Painter,
        at: &OnScreen,
        visible: Aabb2,
        ppp: f32,
        pictures: &PictureTextures,
        moving: &[u64],
        accent: Color32,
    ) {
        self.paint_some(painter, at, visible, accent, ppp, pictures, Some(moving), true);
    }

    /// Every mark, or with `only` the marks of those objects, in `ink`.
    #[allow(clippy::too_many_arguments)]
    fn paint_some(
        &self,
        painter: &egui::Painter,
        at: &OnScreen,
        visible: Aabb2,
        ink: Color32,
        ppp: f32,
        pictures: &PictureTextures,
        only: Option<&[u64]>,
        skip_callouts: bool,
    ) {
        let hair = 1.0 / ppp.max(0.1);
        let mut mesh = egui::Mesh::default();
        for (index, mark) in self.marks.iter().enumerate() {
            if skip_callouts && self.on_parent.get(index).copied().unwrap_or(false) {
                continue;
            }
            if let Some(only) = only
                && !self.owners.get(index).is_some_and(|owner| only.contains(owner))
            {
                continue;
            }
            match mark {
                Mark::Line { points, closed, width_mm, bounds } => {
                    if !bounds.inflated(*width_mm).meets(&visible) {
                        continue;
                    }
                    let width = ((width_mm * at.scale) as f32).max(hair);
                    let points: Vec<Pos2> = points.iter().map(|p| at.screen(*p)).collect();
                    round_line(painter, &points, *closed, Stroke::new(width, ink));
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
                Mark::Picture { source, rect, opacity, clip, within } => {
                    // The fills before it lie under it, as in the scene; those after it, on it.
                    flush(painter, &mut mesh);
                    let uploaded = pictures.get(*source);
                    match uploaded.filter(|u| !u.tiles.is_empty()) {
                        Some(u) => {
                            // Cut to its place, as the writers cut it; a stale picture is its last one,
                            // shown over its own grid, where it was drawn, until the new one comes.
                            let (x0, y0, x1, y1) = u.grid.covers();
                            let over = if u.stale { Aabb2 { min: Vec2::new(x0, y0), max: Vec2::new(x1, y1) } } else { *rect };
                            let shown = within.intersection(&over);
                            if shown.is_empty() || !shown.meets(&visible) {
                                continue;
                            }
                            // A fifth of a pixel, in millimetres, for a circle clip's edge.
                            let tol = (TOLERANCE_PX / (at.scale * f64::from(ppp))).max(1e-5);
                            for tile in &u.tiles {
                                if let Some(mesh) = tile_mesh(at, &u.grid, tile, shown, clip.as_ref(), *opacity, tol) {
                                    painter.add(egui::Shape::mesh(mesh));
                                }
                            }
                        }
                        None if !rect.meets(&visible) => {}
                        None => {
                            let r = at.rect(rect);
                            let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                            let dim = ink.gamma_multiply(0.6);
                            painter.extend(egui::Shape::dashed_line(&corners, Stroke::new(hair, dim), 6.0, 4.0));
                            // Can't be drawn: why, in words beside the frame (D26), not colour alone.
                            if let Some(why) = uploaded.and_then(|u| u.why.as_deref()) {
                                let t = Tokens::get();
                                let text = solidcraft_ui_text::format("ui.sheet_page.picture_failed", &[("reason", why)]);
                                let at = r.left_top() + vec2(t.space_4, t.space_4);
                                painter.text(at, egui::Align2::LEFT_TOP, text, egui::FontId::proportional(t.font_label), dim);
                            }
                        }
                    }
                }
            }
        }
        flush(painter, &mut mesh);
    }
}

/// The cosine of the sharpest turn a line is drawn through in one piece: past 30° egui's miter
/// (which has no limit) would show past the round join the writers draw, by more than 3.5% of the
/// half width.
const SMOOTH_TURN_COS: f64 = 0.866;

/// Points of a line closer than this on screen are one point, in pixels.
const SAME_POINT_PX: f64 = 1e-3;

/// A line through `points` with round caps and round joins, as every writer strokes it
/// ([`solidcraft_geom::paper`]): egui strokes with butt caps and unlimited miters, so the line is
/// drawn in the runs the kernel cuts it into at each sharper turn, with a disc one width across at
/// the ends of every run. A line that closes without a sharp turn is one loop with no ends.
fn round_line(painter: &egui::Painter, points: &[Pos2], closed: bool, stroke: Stroke) {
    let points: Vec<[f64; 2]> = points.iter().map(|p| [f64::from(p.x), f64::from(p.y)]).collect();
    let runs = solidcraft_kernel::services::stroke_runs::runs(&points, closed, SAME_POINT_PX, SMOOTH_TURN_COS);
    let r = stroke.width / 2.0;
    let on_screen = |p: &[f64; 2]| pos2(p[0] as f32, p[1] as f32);
    for run in &runs {
        let piece: Vec<Pos2> = run.points.iter().map(on_screen).collect();
        if run.closed {
            painter.add(egui::Shape::closed_line(piece, stroke));
            continue;
        }
        if let Some(start) = piece.first() {
            painter.circle_filled(*start, r, stroke.color);
        }
        if piece.len() > 1 {
            painter.add(egui::Shape::line(piece, stroke));
        }
    }
    if let Some(end) = runs.last().filter(|run| !run.closed && run.points.len() > 1).and_then(|run| run.points.last()) {
        painter.circle_filled(on_screen(end), r, stroke.color);
    }
}

/// Paints the fills gathered so far into `mesh`, and empties it.
fn flush(painter: &egui::Painter, mesh: &mut egui::Mesh) {
    if !mesh.is_empty() {
        painter.add(egui::Shape::mesh(std::mem::take(mesh)));
    }
}

/// The pixels of a sheet's pictures as textures, by picture source, kept between frames. The
/// provider makes a new `rows` for each new picture, so a picture is uploaded once when its rows are
/// new (another `Arc`) and never again for the same rows; one that leaves the sheet is let go.
#[derive(Default)]
pub(crate) struct PictureTextures {
    pictures: HashMap<u64, Uploaded>,
    /// Pictures uploaded so far: one each time a picture's rows are new.
    uploads: u64,
}

/// One picture as the page has it.
struct Uploaded {
    view: u64,
    /// The rows its textures were made from, to tell new rows by their address.
    rows: Option<Arc<[u8]>>,
    grid: PixelGrid,
    /// Its textures, each at most the GPU's largest side: none until it is drawn, or when it can't be.
    tiles: Vec<Tile>,
    stale: bool,
    why: Option<String>,
}

/// Columns and rows `window` of a picture's grid, as one texture.
struct Tile {
    texture: egui::TextureHandle,
    window: GridWindow,
}

impl PictureTextures {
    /// Brings the textures up to `pictures`, the sheet's answer this frame: textures at most
    /// `max_side` pixels a side.
    fn sync(&mut self, ctx: &egui::Context, pictures: &[SheetPicture], max_side: usize) {
        self.pictures.retain(|source, _| pictures.iter().any(|p| p.source == *source));
        for p in pictures {
            if let Some(u) = self.pictures.get_mut(&p.source)
                && same_rows(&u.rows, &p.rows)
            {
                // The same pixels; the engine moves their grid with their view, stale or not.
                (u.grid, u.stale, u.why) = (p.grid, p.stale, p.why.clone());
                continue;
            }
            let tiles = p.rows.as_ref().map(|rows| tiles(ctx, p.source, &p.grid, rows, max_side)).unwrap_or_default();
            self.uploads += u64::from(!tiles.is_empty());
            let uploaded = Uploaded { view: p.view, rows: p.rows.clone(), grid: p.grid, tiles, stale: p.stale, why: p.why.clone() };
            self.pictures.insert(p.source, uploaded);
        }
    }

    fn get(&self, source: u64) -> Option<&Uploaded> {
        self.pictures.get(&source)
    }

    /// Each picture, for `ui.inspect`: how it shows where it is on screen, as its pixels, its frame (not
    /// drawn yet) or failed (with why). Off screen, none of them is painted.
    fn inspect(&self) -> Vec<Value> {
        let mut out: Vec<(u64, Value)> = self
            .pictures
            .iter()
            .map(|(source, u)| {
                let shown = if !u.tiles.is_empty() {
                    "pixels"
                } else if u.why.is_some() {
                    "failed"
                } else {
                    "frame"
                };
                let view = solidcraft_engine::doc::ids::encode(u.view);
                (*source, json!({"source": source, "view": view, "shown": shown, "tiles": u.tiles.len(), "stale": u.stale, "why": u.why}))
            })
            .collect();
        out.sort_by_key(|(source, _)| *source);
        out.into_iter().map(|(_, v)| v).collect()
    }
}

/// The same rows: both none, or the same allocation (the provider's new picture is a new one).
fn same_rows(a: &Option<Arc<[u8]>>, b: &Option<Arc<[u8]>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// `rows` (straight alpha sRGB RGBA, top row first, `grid`'s size) as textures of at most `max_side`
/// a side, left to right then top to bottom. None when the rows aren't the grid's size.
fn tiles(ctx: &egui::Context, source: u64, grid: &PixelGrid, rows: &[u8], max_side: usize) -> Vec<Tile> {
    let (width, height) = (grid.width as usize, grid.height as usize);
    if max_side == 0 || width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(rows.len()) {
        return Vec::new();
    }
    // Zoomed out, mipmaps keep a picture smooth rather than speckled where the renderer makes them
    // (egui_glow); egui-wgpu makes one level only, so there it is filtered linearly (H5-F4).
    let options = egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR };
    let mut out = Vec::new();
    for y0 in (0..height).step_by(max_side) {
        for x0 in (0..width).step_by(max_side) {
            let (x1, y1) = ((x0 + max_side).min(width), (y0 + max_side).min(height));
            let mut pixels = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
            for row in y0..y1 {
                let start = (row * width + x0) * 4;
                let Some(part) = rows.get(start..start + (x1 - x0) * 4) else { return Vec::new() };
                pixels.extend_from_slice(part);
            }
            let image = egui::ColorImage::from_rgba_unmultiplied([x1 - x0, y1 - y0], &pixels);
            let texture = ctx.load_texture(format!("sheet-picture-{source}-{x0}-{y0}"), image, options);
            let window = GridWindow { x0: x0 as u32, y0: y0 as u32, x1: x1 as u32, y1: y1 as u32 };
            out.push(Tile { texture, window });
        }
    }
    out
}

/// The part of `tile` inside `shown` (and inside a circle `clip`) as a textured mesh, at `opacity`;
/// a circle's edge within `tol` millimetres. None when nothing of it shows.
fn tile_mesh(at: &OnScreen, grid: &PixelGrid, tile: &Tile, shown: Aabb2, clip: Option<&Clip>, opacity: f64, tol: f64) -> Option<egui::Mesh> {
    // The tile's paper rectangle: its first column's left edge to its last's right, its last row's
    // bottom edge to its first row's top (a tile is never empty).
    let w = tile.window;
    let tile_rect = Aabb2 {
        min: Vec2::new(grid.column(w.x0).0, grid.row(w.y1.saturating_sub(1)).0),
        max: Vec2::new(grid.column(w.x1.saturating_sub(1)).1, grid.row(w.y0).1),
    };
    let area = tile_rect.intersection(&shown);
    if area.is_empty() {
        return None;
    }
    let corners = vec![area.min, Vec2::new(area.max.x, area.min.y), area.max, Vec2::new(area.min.x, area.max.y)];
    let outline = match clip {
        Some(Clip::Circle { center, radius }) => cut_to(circle(*center, *radius, tol), &area),
        _ => corners,
    };
    if outline.len() < 3 {
        return None;
    }
    let size = tile_rect.size();
    let uv = |q: Vec2| pos2(((q.x - tile_rect.min.x) / size.x) as f32, ((tile_rect.max.y - q.y) / size.y) as f32);
    // The texture is premultiplied, so the tint scales every channel.
    let tint = WHITE.gamma_multiply(opacity.clamp(0.0, 1.0) as f32);
    let mut mesh = egui::Mesh::with_texture(tile.texture.id());
    for q in &outline {
        mesh.vertices.push(egui::epaint::Vertex { pos: at.screen(*q), uv: uv(*q), color: tint });
    }
    for i in 1..outline.len() as u32 - 1 {
        mesh.add_triangle(0, i, i + 1);
    }
    Some(mesh)
}

/// A circle as a polygon whose edges stray at most `tol` from it, counter-clockwise.
fn circle(center: Vec2, radius: f64, tol: f64) -> Vec<Vec2> {
    let step = 2.0 * (1.0 - (tol / radius.max(tol)).min(1.0)).acos();
    let n = (std::f64::consts::TAU / step.max(1e-3)).ceil().clamp(16.0, 1024.0) as usize;
    (0..n).map(|i| center + Vec2::from_angle(std::f64::consts::TAU * i as f64 / n as f64) * radius).collect()
}

/// `polygon` (convex) cut to the rectangle `b`: what lies inside both (Sutherland and Hodgman).
fn cut_to(polygon: Vec<Vec2>, b: &Aabb2) -> Vec<Vec2> {
    // How far inside each side a point is: each side keeps its points and adds where an edge crosses it.
    let sides: [fn(Vec2, &Aabb2) -> f64; 4] = [|q, b| q.x - b.min.x, |q, b| b.max.x - q.x, |q, b| q.y - b.min.y, |q, b| b.max.y - q.y];
    let mut out = polygon;
    for inside in sides {
        let input = std::mem::take(&mut out);
        let Some(&last) = input.last() else { return out };
        let mut prev = last;
        for &q in &input {
            let (dq, dp) = (inside(q, b), inside(prev, b));
            if (dq >= 0.0) != (dp >= 0.0) {
                out.push(prev + (q - prev) * (dp / (dp - dq)));
            }
            if dq >= 0.0 {
                out.push(q);
            }
            prev = q;
        }
    }
    out
}

#[derive(Debug)]
enum Fault {
    TooMany,
    Invalid(String),
}

/// Every mark of `scene`'s groups, in paint order, cut to their clips as every writer reads them.
fn marks(scene: &Scene, tol: f64) -> Result<(Vec<Mark>, Vec<u64>, Vec<bool>), Fault> {
    marks_owned_detail(scene, tol, true)
}

#[cfg(test)]
fn marks_detail(scene: &Scene, tol: f64, detail: bool) -> Result<Vec<Mark>, Fault> {
    marks_owned_detail(scene, tol, detail).map(|(marks, _, _)| marks)
}

fn marks_owned_detail(scene: &Scene, tol: f64, detail: bool) -> Result<(Vec<Mark>, Vec<u64>, Vec<bool>), Fault> {
    let prepared = scene.prepare().map_err(|e| Fault::Invalid(e.to_string()))?;
    let mut budget = MAX_POINTS;
    let mut out = Vec::new();
    let mut owners = Vec::new();
    let mut on_parent = Vec::new();
    let sheet = Aabb2 { min: Vec2::ZERO, max: scene.size_mm };
    // `prepare` checked there is one clip an object.
    let clips: HashMap<u64, Clip> = scene.clips.iter().copied().collect();
    for group in prepared.groups() {
        if !detail && group.kind == ObjectKind::Fill {
            continue;
        }
        // Pixels aren't geometry: `prepare` leaves a picture whole and each writer cuts it to the clip.
        let clip = clips.get(&group.object).copied();
        // A detail view's callout on its parent is the one label group that strokes an outline.
        let callout = group.kind == ObjectKind::ViewLabel && group.items.iter().any(|item| matches!(item, Item::Stroke { .. }));
        for item in &group.items {
            match item {
                Item::Stroke { path, closed, width_mm, pattern } => {
                    let points = flatten(path, tol, &mut budget)?;
                    match pattern {
                        Some(dashes) => {
                            // Each dash's round caps lie within its length, as the writers draw it.
                            for points in dashed(&points, *closed, &dashes.under_round_caps(*width_mm), &mut budget)? {
                                out.push(line(points, false, *width_mm));
                            }
                        }
                        None => out.push(line(points, *closed, *width_mm)),
                    }
                }
                Item::Fill { region } => out.extend(fill(&region.loops().cloned().collect::<Vec<_>>(), tol, &mut budget)?),
                Item::Text(run) => {
                    if !detail {
                        continue;
                    }
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
                Item::Picture(picture) => {
                    let within = clip.map_or(sheet, |clip| sheet.intersection(&clip.bounds()));
                    out.push(Mark::Picture { source: picture.source, rect: picture.rect, opacity: picture.opacity, clip, within });
                }
            }
            owners.resize(out.len(), group.object);
            on_parent.resize(out.len(), callout);
        }
    }
    Ok((out, owners, on_parent))
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
    // A dash may be 0 long: a dot under round caps.
    let lengths: Vec<f64> = dashes.lengths_mm.iter().copied().filter(|l| l.is_finite() && *l >= 0.0).collect();
    let period: f64 = lengths.iter().sum();
    if lengths.len() < 2 || period <= 0.0 {
        return Ok(vec![points.to_vec()]);
    }
    let mut path = points.to_vec();
    if closed && let Some(first) = points.first() {
        path.push(*first);
    }
    // Where in the pattern the line starts: on a dot when the phase ends exactly at one, as the
    // writers start.
    let mut into = dashes.phase_mm.rem_euclid(period);
    let mut index = 0usize;
    while let Some(l) = lengths.get(index)
        && (into > *l || (into == *l && *l > 0.0))
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
