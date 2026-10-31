//! The custom view navigator: a body of the user's choice drawn in place of the navigation cube
//! (`viewport::body_gizmo` draws it). A right-click on any view navigator offers Axes, Cube and
//! Custom ([`menu_items`]); Custom opens the Custom View Navigator panel, a feature panel with OK
//! and Cancel ([`start`], [`rows`], [`run_dialog`]): the body is picked in the view, then its
//! texture (the cube's colours, or the body's own) and whether the cube's rotation arrows and
//! Home button go around it. The navigator previews the panel's choices while it is open. OK
//! runs `view.navigator` ([`command`]), which scripts and the control channel run too.
//!
//! The body is kept as a snapshot of its display mesh, stored with the preferences and in
//! settings profiles, so it stays across runs and designs and travels with an exported profile.
//! The snapshot is scaled so its longest side is 2 and packed as 16-bit whole numbers of
//! 1/10000 ([`snapshot`]): every body is the same size in the gizmo, and the stored form stays
//! small. Faces and edges keep their identities, so the gizmo can light one face and outline it.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_engine::geom::{Mesh, Vec3};

use crate::SolveApp;
use crate::context_menu::{Item, act};
use crate::dialogs::Kind;
use crate::prefs::{NavigatorStyle, NavigatorTexture};
use crate::selection::{BODIES, SelInput};
use crate::ui_kit;
use solidcraft_engine::Sel;

/// Bodies with more triangles cannot be the gizmo: the snapshot of one at the limit stays well
/// inside a settings profile (about 300 KiB of its 1 MiB), and the gizmo redraws every triangle
/// while the view turns.
pub const MAX_TRIANGLES: usize = 10_000;
/// Caps on what a stored snapshot may hold (the preferences file is input like any other).
const MAX_EDGES: usize = 10_000;
const MAX_EDGE_POINTS: usize = 50_000;
/// The most a body's widest way across may be to its thinnest: a long stick or a thin sheet is
/// a line from some sides, too thin to see or click.
const MAX_ASPECT: f64 = 5.0;
/// A face number that names no face, in a packed snapshot.
const NO_FACE: u16 = u16::MAX;
const MAX_NAME: usize = 256;
/// Stored units per half longest side.
const UNIT: f64 = 10_000.0;

/// The custom navigator's body.
#[derive(Default)]
pub struct State {
    /// The body drawn in place of the cube (`None`: the cube is drawn).
    pub shape: Option<Shape>,
    /// The stored form of `shape`, made once when it is chosen.
    stored: Option<Value>,
}

impl State {
    /// What the preferences file keeps (`null`: no body).
    pub fn stored(&self) -> Value {
        self.stored.clone().unwrap_or(Value::Null)
    }

    /// Read what the preferences file kept; anything unreadable leaves no body.
    pub fn load(&mut self, v: Option<&Value>) {
        self.shape = v.and_then(Shape::from_json);
        self.stored = self.shape.as_ref().map(Shape::to_json);
    }

    fn set(&mut self, shape: Option<Shape>) {
        self.stored = shape.as_ref().map(Shape::to_json);
        self.shape = shape;
    }
}

/// A body's shape as the gizmo draws it.
#[derive(Clone, Debug)]
pub struct Shape {
    /// The body it was taken from (for the window and automation).
    pub name: String,
    /// Different for every snapshot read or taken (what the gizmo's drawing is cached by).
    pub id: u64,
    /// Centred, longest side 2; one vertex per triangle corner, normals smooth within a face.
    pub mesh: Arc<Mesh>,
    /// The body's colour (appearance, material or imported); `None`: the theme's body colour.
    pub colour: Option<[u8; 3]>,
    /// Faces with colours of their own (B-rep face index → colour).
    pub face_colours: BTreeMap<u32, [u8; 3]>,
}

impl Shape {
    /// The snapshot of `mesh` with its colours (`None` when it is empty, flat to a point, or
    /// too large).
    pub fn of(name: &str, mesh: &Mesh, colour: Option<[u8; 3]>, face_colours: &BTreeMap<u32, [u8; 3]>) -> Option<Shape> {
        if !fits(mesh) {
            return None;
        }
        Shape::from_json(&snapshot(name, mesh, colour, face_colours)?)
    }

    /// The snapshot of a body of the session's design, in the colours the viewport draws it.
    pub fn of_body(session: &solidcraft_engine::Session, name: &str) -> Option<Shape> {
        let st = session.world_state();
        let b = st.body(name)?;
        let to8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        let colour = session.doc.body_color(b).map(|c| c.map(to8));
        let faces =
            session.doc.face_colors(b).into_iter().filter_map(|(f, look)| Some((u32::try_from(f).ok()?, look.color))).take(MAX_TRIANGLES).collect();
        Shape::of(name, &b.mesh(), colour, &faces)
    }

    /// The stored form, as [`Shape::from_json`] reads it.
    pub fn to_json(&self) -> Value {
        snapshot(&self.name, &self.mesh, self.colour, &self.face_colours).unwrap_or(Value::Null)
    }

    /// The colour of face `face` (`None`: the theme's body colour).
    pub fn colour_of(&self, face: Option<u32>) -> Option<[u8; 3]> {
        face.and_then(|f| self.face_colours.get(&f).copied()).or(self.colour)
    }

    /// Read a stored snapshot ([`snapshot`] packs it); `None` for anything malformed or over the
    /// caps.
    pub fn from_json(v: &Value) -> Option<Shape> {
        let name: String = v.get("name")?.as_str()?.chars().take(MAX_NAME).collect();
        let points: Vec<i16> = unpack(v, "v", 9 * MAX_TRIANGLES * 2)?.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
        let words = |key: &str, cap: usize| -> Option<Vec<u16>> {
            Some(unpack(v, key, cap * 2)?.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect())
        };
        let tris = words("t", 3 * MAX_TRIANGLES)?;
        let faces = words("f", MAX_TRIANGLES)?;
        let point = |p: &[i16; 3]| Vec3::new(f64::from(p[0]) / UNIT, f64::from(p[1]) / UNIT, f64::from(p[2]) / UNIT);
        let points = points.as_chunks::<3>().0;
        let mut corners: Vec<([Vec3; 3], u32)> = Vec::new();
        for (t, face) in tris.as_chunks::<3>().0.iter().zip(faces.iter().chain(std::iter::repeat(&0))) {
            let at = |i: u16| points.get(usize::from(i)).map(point);
            let (Some(a), Some(b), Some(c)) = (at(t[0]), at(t[1]), at(t[2])) else { continue };
            corners.push(([a, b, c], u32::from(*face)));
        }
        if corners.is_empty() {
            return None;
        }
        // Normals smooth across a face's shared corners, sharp between faces.
        let mut sums: HashMap<(u32, [u64; 3]), Vec3> = HashMap::new();
        let key = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
        for (p, face) in &corners {
            let n = (p[1] - p[0]).cross(p[2] - p[0]);
            for v in p {
                let e = sums.entry((*face, key(*v))).or_insert(Vec3::ZERO);
                *e += n;
            }
        }
        let mut mesh = Mesh::default();
        for (p, face) in &corners {
            let flat = (p[1] - p[0]).cross(p[2] - p[0]).normalized().unwrap_or(Vec3::Z);
            let base = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
            for v in p {
                mesh.positions.push(*v);
                mesh.normals.push(sums.get(&(*face, key(*v))).and_then(|n| n.normalized()).unwrap_or(flat));
            }
            mesh.triangles.push([base, base.saturating_add(1), base.saturating_add(2)]);
            mesh.tri_face.push(*face);
        }
        // Edges are optional: a snapshot without readable ones is drawn without its lines.
        let edge_points: Vec<i16> =
            unpack(v, "e", MAX_EDGE_POINTS * 6).unwrap_or_default().as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
        let mut edge_points = edge_points.as_chunks::<3>().0.iter().map(point);
        let edge_faces = words("ef", 2 * MAX_EDGES).unwrap_or_default();
        let lengths = v.get("en").and_then(Value::as_array).map(|a| a.iter().take(MAX_EDGES).collect::<Vec<_>>()).unwrap_or_default();
        for (i, n) in lengths.into_iter().enumerate() {
            let Some(n) = n.as_u64().and_then(|n| usize::try_from(n).ok()) else { break };
            let line: Vec<Vec3> = edge_points.by_ref().take(n).collect();
            if line.len() < n {
                break;
            }
            let faces = [2 * i, 2 * i + 1].into_iter().filter_map(|j| edge_faces.get(j).filter(|f| **f != NO_FACE).map(|f| u32::from(*f))).collect();
            mesh.edges.push(line);
            mesh.edge_faces.push(faces);
        }
        let colour = v.get("c").and_then(rgb);
        let face_colours = v
            .get("fc")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .take(MAX_TRIANGLES)
                    .filter_map(|e| {
                        let e = e.as_array()?;
                        let face = u32::try_from(e.first()?.as_u64()?).ok()?;
                        Some((face, rgb(&Value::from(e.get(1..)?.to_vec()))?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Some(Shape { name, id, mesh: Arc::new(mesh), colour, face_colours })
    }

    /// The direction to look from to see the face of triangle `tri` straight on: a planar
    /// face's normal, or a curved face's at that triangle.
    pub fn face_normal(&self, tri: usize) -> Option<Vec3> {
        let m = &self.mesh;
        let normal = |t: &[u32; 3]| m.tri(t).and_then(|p| (p[1] - p[0]).cross(p[2] - p[0]).normalized());
        let seen = normal(m.triangles.get(tri)?)?;
        let face = *m.tri_face.get(tri)?;
        let mut sum = Vec3::ZERO;
        let mut planar = true;
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            if *f != face {
                continue;
            }
            let Some(p) = m.tri(t) else { continue };
            let area = (p[1] - p[0]).cross(p[2] - p[0]);
            planar &= area.normalized().is_none_or(|n| n.dot(seen) > 1.0 - 1e-6);
            sum += area;
        }
        Some(if planar { sum.normalized().unwrap_or(seen) } else { seen })
    }
}

/// A mesh the gizmo can draw: some triangles, not too many, and not too long and thin.
fn fits(mesh: &Mesh) -> bool {
    refusal(mesh).is_none()
}

/// Why a body can't be the gizmo, as a message id (`None`: it can).
fn refusal(mesh: &Mesh) -> Option<&'static str> {
    if !(1..=MAX_TRIANGLES).contains(&mesh.triangles.len()) {
        return Some("view_gizmo.too_detailed");
    }
    let (thinnest, widest) = widths(&mesh.positions);
    (!(widest > 0.0 && thinnest * MAX_ASPECT >= widest)).then_some("view_gizmo.too_thin")
}

/// The thinnest and the widest the points are across, measured along the three axes, the six
/// face diagonals and the four body diagonals, so a stick lying diagonally is found too.
fn widths(points: &[Vec3]) -> (f64, f64) {
    let mut directions = vec![Vec3::X, Vec3::Y, Vec3::Z];
    for (a, b) in [(Vec3::X, Vec3::Y), (Vec3::Y, Vec3::Z), (Vec3::Z, Vec3::X)] {
        directions.extend([a + b, a - b]);
    }
    directions.extend([Vec3::new(1.0, 1.0, 1.0), Vec3::new(1.0, 1.0, -1.0), Vec3::new(1.0, -1.0, 1.0), Vec3::new(-1.0, 1.0, 1.0)]);
    let (mut thinnest, mut widest) = (f64::INFINITY, 0.0_f64);
    for d in directions.into_iter().filter_map(|d| d.normalized()) {
        let (lo, hi) =
            points.iter().map(|p| p.dot(d)).filter(|x| x.is_finite()).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let width = if hi >= lo { hi - lo } else { 0.0 };
        thinnest = thinnest.min(width);
        widest = widest.max(width);
    }
    (thinnest, widest)
}

fn rgb(c: &Value) -> Option<[u8; 3]> {
    let a = c.as_array()?;
    let ch = |i: usize| a.get(i).and_then(Value::as_u64).and_then(|x| u8::try_from(x).ok());
    Some([ch(0)?, ch(1)?, ch(2)?])
}

/// The bytes of a packed field (`None`: missing, not base64, or over `cap` bytes).
fn unpack(v: &Value, key: &str, cap: usize) -> Option<Vec<u8>> {
    let text = v.get(key)?.as_str()?;
    if text.len() > cap.div_ceil(3) * 4 {
        return None;
    }
    solidcraft_engine::doc::canvas::base64_decode(text)
}

/// Little-endian 16-bit words, as base64.
fn pack<T: Copy>(words: &[T], bytes: impl Fn(T) -> [u8; 2]) -> String {
    let raw: Vec<u8> = words.iter().flat_map(|w| bytes(*w)).collect();
    solidcraft_engine::doc::canvas::base64_encode(&raw)
}

/// The stored form of `mesh`, centred and scaled to longest side 2, in whole 1/10000s. Each
/// list is little-endian 16-bit words as base64, about a third of the size of a JSON list:
/// `v` the points (x, y, z), `t` three point indices per triangle, `f` each triangle's B-rep
/// face, `e` the points of every edge one after another with `en` the count of each, and `ef`
/// the two faces each edge bounds ([`NO_FACE`]: none). `None` when the mesh is empty or flat to
/// a point, or has a face number too large to store.
fn snapshot(name: &str, mesh: &Mesh, colour: Option<[u8; 3]>, faces: &BTreeMap<u32, [u8; 3]>) -> Option<Value> {
    let b = mesh.bounds();
    if b.is_empty() {
        return None;
    }
    let size = b.max - b.min;
    let half = size.x.max(size.y).max(size.z) * 0.5;
    if !half.is_finite() || half <= 0.0 {
        return None;
    }
    let centre = b.center();
    let unit = |x: f64| (x.round().clamp(-UNIT, UNIT)) as i16;
    let q = |p: Vec3| {
        let d = (p - centre) / half * UNIT;
        [unit(d.x), unit(d.y), unit(d.z)]
    };
    let mut index: HashMap<[i16; 3], u16> = HashMap::new();
    let (mut v, mut t, mut f) = (Vec::new(), Vec::new(), Vec::new());
    for (i, tri) in mesh.triangles.iter().enumerate().take(MAX_TRIANGLES) {
        let Some(p) = mesh.tri(tri) else { continue };
        for p in p {
            let k = q(p);
            let id = match index.get(&k) {
                Some(id) => *id,
                None => {
                    let id = u16::try_from(index.len()).ok()?;
                    index.insert(k, id);
                    v.extend(k);
                    id
                }
            };
            t.push(id);
        }
        f.push(u16::try_from(mesh.tri_face.get(i).copied().unwrap_or(0)).ok().filter(|f| *f != NO_FACE)?);
    }
    let (mut e, mut en, mut ef) = (Vec::new(), Vec::new(), Vec::new());
    for (i, line) in mesh.edges.iter().enumerate().take(MAX_EDGES) {
        if mesh.seams.get(i) == Some(&true) || mesh.smooth_edges.get(i) == Some(&true) || e.len() / 3 + line.len() > MAX_EDGE_POINTS {
            continue;
        }
        e.extend(line.iter().flat_map(|p| q(*p)));
        en.push(line.len());
        let bounds = mesh.edge_faces.get(i).map(Vec::as_slice).unwrap_or_default();
        ef.extend((0..2).map(|j| bounds.get(j).and_then(|f| u16::try_from(*f).ok()).unwrap_or(NO_FACE)));
    }
    let fc: Vec<Value> = faces.iter().map(|(f, c)| json!([f, c[0], c[1], c[2]])).collect();
    Some(json!({
        "name": name,
        "v": pack(&v, i16::to_le_bytes),
        "t": pack(&t, u16::to_le_bytes),
        "f": pack(&f, u16::to_le_bytes),
        "e": pack(&e, i16::to_le_bytes),
        "en": en,
        "ef": pack(&ef, u16::to_le_bytes),
        "c": colour,
        "fc": fc,
    }))
}

/// The menu a right-click on the view navigator opens: Axes, Cube and Custom, the current one
/// marked. Custom opens the Custom View Navigator panel, which sets it on OK.
pub(crate) fn menu_items(app: &SolveApp) -> Vec<Item> {
    NavigatorStyle::ALL
        .into_iter()
        .map(|style| {
            let (label, icon) = match style {
                NavigatorStyle::Axes => (app.tr("prefs.axes"), "origin"),
                NavigatorStyle::Cube => (crate::i18n::tr_label("Cube"), "box"),
                NavigatorStyle::Custom => (crate::i18n::tr_label("Custom"), "body"),
            };
            act("ui.navigatorStyle", label, icon).with(json!({ "style": style })).key(if app.preferences.navigator_style == style {
                "current"
            } else {
                ""
            })
        })
        .collect()
}

/// A menu pick: Axes and Cube at once; Custom opens its panel, as the toolbar would.
pub(crate) fn pick_style(app: &mut SolveApp, style: NavigatorStyle) {
    if style == NavigatorStyle::Custom {
        app.start("view.navigator");
    } else if let Err(e) = app.run("view.navigator", json!({ "style": style })) {
        app.set_status(e, true);
    }
}

/// Draw body `name` of the open design as the gizmo (`None`: no body, the cube again).
fn set_body(app: &mut SolveApp, name: Option<&str>) -> Result<(), String> {
    let shape = match name {
        None => None,
        Some(name) => Some(snapshot_of(&app.session, name).map_err(|reason| crate::i18n::tr(reason).to_string())?),
    };
    app.view_gizmo.set(shape);
    Ok(())
}

/// The snapshot of body `name`, or why it can't be the navigator (a message id).
fn snapshot_of(session: &solidcraft_engine::Session, name: &str) -> Result<Shape, &'static str> {
    let st = session.world_state();
    let body = st.body(name).ok_or("view_gizmo.no_such_body")?;
    if let Some(reason) = refusal(&body.mesh()) {
        return Err(reason);
    }
    Shape::of_body(session, name).ok_or("view_gizmo.too_detailed")
}

/// `view.navigator`, after the engine checked its parameters: set what it names, then reply with
/// the whole navigator. A refused body changes nothing.
pub(crate) fn command(app: &mut SolveApp, v: &Value) -> Result<Value, String> {
    let field = |key: &str| v.get(key).filter(|x| !x.is_null()).cloned();
    let style: Option<NavigatorStyle> = field("style").map(serde_json::from_value).transpose().map_err(|e| e.to_string())?;
    let texture: Option<NavigatorTexture> = field("texture").map(serde_json::from_value).transpose().map_err(|e| e.to_string())?;
    match v.get("body") {
        None => {}
        Some(Value::Null) => set_body(app, None)?,
        Some(Value::String(name)) => set_body(app, Some(name))?,
        Some(_) => return Err("`body` must be a body name or null".into()),
    }
    let p = &mut app.preferences;
    if let Some(style) = style {
        p.navigator_style = style;
    }
    if let Some(texture) = texture {
        p.navigator_texture = texture;
    }
    if let Some(on) = field("arrows").and_then(|x| x.as_bool()) {
        p.navigator_arrows = on;
    }
    if let Some(on) = field("home").and_then(|x| x.as_bool()) {
        p.navigator_home = on;
    }
    Ok(json!({
        "style": p.navigator_style,
        "body": app.view_gizmo.shape.as_ref().map(|s| s.name.clone()),
        "texture": p.navigator_texture,
        "arrows": p.navigator_arrows,
        "home": p.navigator_home,
    }))
}

/// The Custom View Navigator panel's settings; its body is the panel's selection input.
#[derive(Clone, Debug)]
pub struct Form {
    pub texture: NavigatorTexture,
    pub arrows: bool,
    pub home: bool,
    /// The picked body and its snapshot (or why it can't be the navigator), which the navigator
    /// previews while the panel is open.
    preview: Option<(String, Result<Shape, &'static str>)>,
}

/// The panel opened by `view.navigator` from the toolbar or the navigator's menu, filled with
/// the navigator as it is (its body picked when the open design has it).
pub(crate) fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    (id == "view.navigator").then(|| {
        let mut body = SelInput::new("Body", BODIES, false);
        if let Some(shape) = &app.view_gizmo.shape
            && app.session.world_state().body(&shape.name).is_some()
        {
            body.items.push(Sel::Body { name: shape.name.clone() });
        }
        let p = &app.preferences;
        (Kind::ViewNavigator(Form { texture: p.navigator_texture, arrows: p.navigator_arrows, home: p.navigator_home, preview: None }), vec![body])
    })
}

/// The body picked in the panel.
fn picked(inputs: &[SelInput]) -> Option<&str> {
    inputs.first()?.items.iter().find_map(|s| match s {
        Sel::Body { name } => Some(name.as_str()),
        _ => None,
    })
}

/// The panel's rows under its body input: why a picked body can't be the navigator, the
/// texture, the arrows and Home.
pub(crate) fn rows(app: &SolveApp, ui: &mut egui::Ui, form: &mut Form, inputs: &[SelInput]) {
    let body = picked(inputs);
    if form.preview.as_ref().map(|(name, _)| name.as_str()) != body {
        form.preview = body.map(|name| (name.to_string(), snapshot_of(&app.session, name)));
    }
    if let Some((_, Err(reason))) = &form.preview {
        ui_kit::text(ui, crate::i18n::tr(reason), ui_kit::Text::Error("view_gizmo_refused"));
    }
    ui_kit::row(ui, crate::i18n::tr("view_gizmo.texture"), |ui| {
        let mut texture = usize::from(form.texture == NavigatorTexture::Modeled);
        let changed = ui_kit::segmented(
            ui,
            &mut texture,
            &[crate::i18n::tr("view_gizmo.themed"), crate::i18n::tr_label("Modeled")],
            ui_kit::Segments { intrinsic: true, ..Default::default() },
        );
        if changed {
            form.texture = if texture == 1 { NavigatorTexture::Modeled } else { NavigatorTexture::Themed };
        }
    });
    let arrows = ui_kit::checkbox(ui, crate::i18n::tr("view_gizmo.arrows"), &mut form.arrows);
    crate::scenario::publish_target("view_gizmo:arrows", arrows.rect);
    let home = ui_kit::checkbox(ui, crate::i18n::tr("view_gizmo.home"), &mut form.home);
    crate::scenario::publish_target("view_gizmo:home", home.rect);
    ui_kit::text(ui, crate::i18n::tr("view_gizmo.note"), ui_kit::Text::Note("view_gizmo_note"));
}

/// OK: the custom navigator with the panel's body (or the one it has), texture, arrows and Home.
pub(crate) fn run_dialog(app: &mut SolveApp, form: &Form, inputs: &[SelInput]) -> Result<(), String> {
    if let Some((_, Err(reason))) = &form.preview {
        return Err(crate::i18n::tr(reason).to_string());
    }
    let mut params = json!({"style": NavigatorStyle::Custom, "texture": form.texture, "arrows": form.arrows, "home": form.home});
    if let (Some(body), Some(o)) = (picked(inputs), params.as_object_mut()) {
        o.insert("body".into(), json!(body));
    }
    app.run("view.navigator", params).map(|_| ())
}

/// What the navigator shows now: while the Custom View Navigator panel is open, the custom
/// navigator as the panel stands (its picked body, else the kept one); otherwise as set.
pub(crate) struct Shown {
    pub style: NavigatorStyle,
    pub shape: Option<Shape>,
    pub texture: NavigatorTexture,
    pub arrows: bool,
    pub home: bool,
}

pub(crate) fn shown(app: &SolveApp) -> Shown {
    let p = &app.preferences;
    match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::ViewNavigator(form)) => Shown {
            style: NavigatorStyle::Custom,
            shape: match &form.preview {
                Some((_, Ok(shape))) => Some(shape.clone()),
                _ => app.view_gizmo.shape.clone(),
            },
            texture: form.texture,
            arrows: form.arrows,
            home: form.home,
        },
        _ => Shown {
            style: p.navigator_style,
            shape: app.view_gizmo.shape.clone(),
            texture: p.navigator_texture,
            arrows: p.navigator_arrows,
            home: p.navigator_home,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_mesh() -> Arc<Mesh> {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 20, "height": 10})).unwrap();
        s.world_state().bodies[0].mesh()
    }

    #[test]
    fn snapshots_round_trip_at_the_same_size_whatever_the_body() {
        let m = box_mesh();
        let shape = Shape::of("Body1", &m, Some([200, 30, 40]), &BTreeMap::from([(2, [0, 128, 255])])).unwrap();
        let b = shape.mesh.bounds();
        let size = b.max - b.min;
        assert!((size.x.max(size.y).max(size.z) - 2.0).abs() < 1e-3, "longest side 2: {size:?}");
        assert!(b.center().len() < 1e-3);
        assert_eq!(shape.mesh.triangles.len(), m.triangles.len());
        assert_eq!(shape.mesh.edges.len(), 12);
        let again = Shape::from_json(&shape.to_json()).unwrap();
        assert_eq!(again.mesh.positions, shape.mesh.positions);
        assert_eq!(again.mesh.tri_face, shape.mesh.tri_face);
        assert_eq!(
            (again.colour_of(Some(2)), again.colour_of(Some(1)), again.colour_of(None)),
            (Some([0, 128, 255]), Some([200, 30, 40]), Some([200, 30, 40]))
        );
        // A box 25 times larger snapshots to the same shape.
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 1000, "width": 500, "height": 250})).unwrap();
        let big = Shape::of("Big", &s.world_state().bodies[0].mesh(), None, &BTreeMap::new()).unwrap();
        let bb = big.mesh.bounds();
        assert!((bb.max - bb.min).dist(b.max - b.min) < 1e-3);
    }

    #[test]
    fn face_normals_are_the_planar_face_normals() {
        let shape = Shape::of("Body1", &box_mesh(), None, &BTreeMap::new()).unwrap();
        for tri in 0..shape.mesh.triangles.len() {
            let n = shape.face_normal(tri).unwrap();
            let axes = [n.x.abs(), n.y.abs(), n.z.abs()];
            assert!(axes.iter().any(|a| (a - 1.0).abs() < 1e-9), "{n:?}");
        }
    }

    #[test]
    fn hostile_snapshots_never_panic() {
        let good = Shape::of("Body1", &box_mesh(), None, &BTreeMap::new()).unwrap().to_json();
        let words = |w: &[u16]| pack(w, u16::to_le_bytes);
        let triangle = words(&[0, 0, 0, 1, 0, 0, 0, 1, 0]);
        let mut cases = vec![
            json!(null),
            json!({}),
            json!({"name": 3}),
            json!({"name": "x", "v": "", "t": "", "f": ""}),
            json!({"name": "x", "v": [0, 0, 0], "t": [0, 1, 2], "f": [0]}),
            json!({"name": "x", "v": "not base64!", "t": words(&[0, 1, 2]), "f": words(&[0])}),
            json!({"name": "x", "v": triangle, "t": words(&[0, 1, 99]), "f": ""}),
            json!({"name": "x", "v": triangle, "t": words(&[0, 1]), "f": words(&[u16::MAX])}),
            json!({"name": "x", "v": "AAA", "t": words(&[0, 1, 2]), "f": words(&[0])}),
            json!({"name": "x", "v": triangle, "t": words(&[0, 1, 2]), "f": words(&[0]), "e": triangle, "en": [u64::MAX, 2], "ef": 7}),
            json!({"name": "x", "v": triangle, "t": words(&[0, 1, 2]), "f": words(&[0]), "e": "no", "en": ["no"], "ef": words(&[NO_FACE; 5])}),
            json!({"name": "x".repeat(10_000), "v": triangle, "t": words(&[0, 1, 2]), "f": words(&[0])}),
            json!({"name": "x", "v": triangle, "t": words(&[0, 1, 2]), "f": words(&[0]), "c": [300, -1, "r"], "fc": [[0], [1, 2], "no", [u64::MAX, 1, 2, 3]]}),
        ];
        let mut huge = good.clone();
        huge["t"] = json!(words(&vec![0; 3 * MAX_TRIANGLES + 3]));
        cases.push(huge);
        for c in &cases {
            if let Some(s) = Shape::from_json(c) {
                assert!(s.name.chars().count() <= MAX_NAME);
                assert!(s.mesh.triangles.len() <= MAX_TRIANGLES);
                for t in 0..s.mesh.triangles.len() {
                    let _ = s.face_normal(t);
                }
            }
            let mut state = State::default();
            state.load(Some(c));
        }
        assert!(Shape::from_json(&json!({"name": "x", "v": [0, 0, 0], "t": [0, 1, 2], "f": [0]})).is_none());
        let mut over = good;
        over["t"] = json!(words(&vec![0; 3 * MAX_TRIANGLES + 3]));
        assert!(Shape::from_json(&over).is_none(), "more triangles than the limit are refused");
    }

    #[test]
    fn long_sticks_and_thin_sheets_are_refused_whichever_way_they_lie() {
        let mut s = solidcraft_engine::Session::default();
        let mesh = |s: &mut solidcraft_engine::Session, params: Value| {
            s.execute("solid.box", &params).unwrap();
            s.world_state().bodies.last().unwrap().mesh()
        };
        // A 4:2:1 block is fine; a 6:1:1 stick and a 6:6:1 sheet are not.
        assert_eq!(refusal(&mesh(&mut s, json!({"length": 40, "width": 20, "height": 10}))), None);
        assert_eq!(refusal(&mesh(&mut s, json!({"length": 60, "width": 10, "height": 10, "operation": "new"}))), Some("view_gizmo.too_thin"));
        assert_eq!(refusal(&mesh(&mut s, json!({"length": 60, "width": 60, "height": 10, "operation": "new"}))), Some("view_gizmo.too_thin"));
        // The same stick turned to lie along a diagonal: its bounding box is a cube.
        let mut stick = (*mesh(&mut s, json!({"length": 60, "width": 10, "height": 10, "operation": "new"}))).clone();
        let along = Vec3::new(1.0, 1.0, 1.0).normalized().unwrap();
        let side = Vec3::new(1.0, -1.0, 0.0).normalized().unwrap();
        let up = along.cross(side);
        for p in &mut stick.positions {
            *p = along * p.x + side * p.y + up * p.z;
        }
        assert_eq!(refusal(&stick), Some("view_gizmo.too_thin"));
        assert_eq!(refusal(&Mesh::default()), Some("view_gizmo.too_detailed"));
    }

    #[test]
    fn a_body_at_the_limit_fits_in_a_settings_profile_with_room_to_spare() {
        // A sphere close to the triangle limit: as many points as a smooth body has.
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.sphere", &json!({"radius": 10})).unwrap();
        let mesh = s.world_state().bodies[0].mesh();
        let mut big = (*mesh).clone();
        while big.triangles.len() + mesh.triangles.len() <= MAX_TRIANGLES {
            let offset = u32::try_from(big.positions.len()).unwrap();
            let shift = Vec3::new(0.0, 0.0, 0.001 * big.triangles.len() as f64);
            big.positions.extend(mesh.positions.iter().map(|p| *p + shift));
            big.normals.extend(mesh.normals.iter().copied());
            big.triangles.extend(mesh.triangles.iter().map(|t| t.map(|i| i + offset)));
            big.tri_face.extend(mesh.tri_face.iter().copied());
        }
        assert!(fits(&big) && big.triangles.len() * 3 > MAX_TRIANGLES * 2, "{} triangles", big.triangles.len());
        let shape = Shape::of("Sphere", &big, Some([1, 2, 3]), &BTreeMap::new()).expect("a body under the limit snapshots");
        let bytes = shape.to_json().to_string().len();
        assert!(bytes < 400 * 1024, "{bytes} bytes for {} triangles", big.triangles.len());
    }

    #[test]
    fn a_right_click_anywhere_on_the_navigator_offers_axes_cube_and_custom() {
        use crate::context_menu::{Target, items};
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 10})).unwrap();
        h.frames(2);
        let design = h.app.session.doc.to_json();
        for (from, pick, to) in [("axes", "Cube", NavigatorStyle::Cube), ("cube", "Axes", NavigatorStyle::Axes)] {
            h.call("ui.prefs", json!({"navigator_style": from}));
            h.frames(2);
            let outer = crate::scenario::rect_of("axis_view:outer").expect("the navigator is shown");
            // The middle, a corner of its box, and its Home button on the cube.
            let mut spots = vec![outer.center(), outer.left_bottom() + egui::vec2(3.0, -3.0)];
            spots.extend(crate::scenario::handle_at("navigation_cube:control:home"));
            for at in spots {
                crate::context_menu::close(&mut h.app);
                h.call("ui.click", json!({"x": at.x, "y": at.y, "button": "right"}));
                assert_eq!(h.app.menu.target, Target::ViewGizmo, "{from} at {at:?}: the navigator's own menu");
                assert!(h.app.viewport.context_menu.is_some());
            }
            let rows = items(&h.app, &Target::ViewGizmo);
            let labels: Vec<_> = rows.iter().map(|r| r.label.as_str()).collect();
            assert_eq!(labels, ["Axes", "Cube", "Custom"], "just the three types");
            let current: Vec<_> = rows.iter().filter(|i| i.shortcut == "current").collect();
            assert_eq!(current.len(), 1);
            assert_eq!(current[0].params.as_ref().map(|p| p["style"].clone()), Some(json!(from)));
            assert_eq!(h.call("ui.menuPick", json!({"item": pick}))["ok"], true, "{pick}");
            assert_eq!(h.app.preferences.navigator_style, to, "Axes and Cube apply at once");
            assert!(h.app.dialog.is_none());
        }
        assert_eq!(h.app.session.doc.to_json(), design);
        assert!(h.app.session.selection.is_empty(), "a right-click on the navigator selects nothing");
    }

    #[test]
    fn custom_opens_a_panel_whose_ok_sets_the_navigator_and_cancel_leaves_it() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 10})).unwrap();
        h.call("ui.prefs", json!({"navigator_style": "cube"}));
        h.frames(2);
        let body = h.app.session.world_state().bodies[0].name.clone();
        let open = |h: &mut crate::scenario::Harness| {
            let c = crate::scenario::rect_of("axis_view:outer").unwrap().center();
            h.call("ui.click", json!({"x": c.x, "y": c.y, "button": "right"}));
            assert_eq!(h.call("ui.menuPick", json!({"item": "Custom"}))["ok"], true);
            h.frames(2);
            assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(Kind::ViewNavigator(_))), "the Custom View Navigator panel");
            assert_eq!(h.app.preferences.navigator_style, NavigatorStyle::Cube, "nothing changes before OK");
        };
        // Pick the body as a click in the view would, and turn Home off (it starts on).
        let fill = |h: &mut crate::scenario::Harness| {
            let d = h.app.dialog.as_mut().unwrap();
            d.inputs[0].items = vec![Sel::Body { name: body.clone() }];
            if let Kind::ViewNavigator(form) = &mut d.kind {
                assert!(form.home && form.arrows, "Home and the arrows start on");
                form.home = false;
                form.texture = NavigatorTexture::Modeled;
            }
            h.frames(2);
        };
        open(&mut h);
        fill(&mut h);
        assert!(crate::scenario::count_of("body_gizmo:faces").is_some_and(|n| n >= 3.0), "the navigator previews the picked body");
        assert!(crate::scenario::handle_at("navigation_cube:control:home").is_none(), "and the panel's Home, off");
        assert!(crate::scenario::handle_at("navigation_cube:control:left").is_some(), "with its arrows");
        h.step(&json!({"click": {"handle": "dialog:cancel"}})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.preferences.navigator_style, NavigatorStyle::Cube, "Cancel keeps the cube");
        assert!(h.app.view_gizmo.shape.is_none() && h.app.preferences.navigator_home, "Cancel keeps Home on");

        open(&mut h);
        fill(&mut h);
        h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
        h.frames(2);
        assert!(h.app.dialog.is_none());
        assert_eq!(h.app.preferences.navigator_style, NavigatorStyle::Custom);
        assert_eq!(h.app.view_gizmo.shape.as_ref().map(|s| s.name.as_str()), Some(body.as_str()));
        assert!(!h.app.preferences.navigator_home && h.app.preferences.navigator_arrows);
        assert_eq!(h.app.preferences.navigator_texture, NavigatorTexture::Modeled);
        assert!(h.app.session.undo.is_empty(), "the navigator is never part of the design");

        // Opened again, the panel starts from the navigator as it is.
        open(&mut h);
        let d = h.app.dialog.as_ref().unwrap();
        assert_eq!(d.inputs[0].items, vec![Sel::Body { name: body.clone() }]);
        assert!(matches!(&d.kind, Kind::ViewNavigator(f) if !f.home && f.texture == NavigatorTexture::Modeled));
    }

    #[test]
    fn the_body_is_kept_with_the_preferences_and_unreadable_ones_are_dropped() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 10})).unwrap();
        let name = h.app.session.world_state().bodies[0].name.clone();
        let reply = h.app.run("view.navigator", json!({"body": name})).unwrap();
        assert_eq!(reply["body"], json!(name), "{reply}");
        assert!(h.app.dialog.is_none(), "a programmatic call never opens the panel");
        let mut other = crate::scenario::Harness::new();
        other.app.load_prefs(&h.app.prefs());
        let kept = other.app.view_gizmo.shape.as_ref().expect("the body survives a restart");
        assert_eq!(kept.name, name);
        assert_eq!(kept.mesh.triangles.len(), h.app.view_gizmo.shape.as_ref().unwrap().mesh.triangles.len());
        assert!(h.app.run("view.navigator", json!({"body": "No such body"})).is_err());
        assert!(h.app.run("view.navigator", json!({"body": 3})).is_err());
        h.app.run("solid.box", json!({"length": 60, "width": 10, "height": 10, "operation": "new"})).unwrap();
        let stick = h.app.session.world_state().bodies.last().unwrap().name.clone();
        assert!(h.app.run("view.navigator", json!({"body": stick})).is_err(), "a stick is refused");
        assert_eq!(h.app.view_gizmo.shape.as_ref().map(|s| s.name.clone()), Some(name), "a refused body changes nothing");
        assert_eq!(h.app.run("view.navigator", json!({"body": null})).unwrap()["body"], Value::Null);
        other.app.load_prefs(&json!({"view_gizmo": {"name": "x", "v": "broken"}}).to_string());
        assert!(other.app.view_gizmo.shape.is_none());
    }
}
