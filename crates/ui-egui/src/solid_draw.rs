//! Drawing a primitive in the viewport: its base is clicked out on a plane and its height set
//! after, so nothing appears until the shape has been said. Picking the tool used to put a
//! body of a default size at the origin straight away, which is not where it was wanted.
//!
//! Three stages for a box or a cylinder, two for a sphere:
//!
//! | stage | box | cylinder | sphere |
//! |---|---|---|---|
//! | 0 | first corner | centre of the base | centre |
//! | 1 | opposite corner | a point on the rim | a point on the surface |
//! | 2 | height | height | — |
//!
//! The plane is the planar face under the first click, else the ground plane. Every point comes
//! from the snap ([`crate::model_snap`]), so a corner, the middle of an edge or the centre of a
//! hole places the shape exactly on it; the height runs along the plane's normal and is read off
//! the cursor with [`crate::viewport::along`].
//!
//! The dialog stays the numbers: the gesture writes its fields as it goes and they can be typed
//! over afterwards, as they always could.
//!
//! A box carries the frame it was drawn in (`FeatureKind::Box`'s `axis` and `x_axis`), as a
//! cylinder carries its axis, so every shape is drawn on any face: the ground plane, or the
//! face under the first click, whichever way that face lies.

use egui::{Pos2, Shape, Stroke};
use serde_json::{Value, json};
use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::dialogs::Kind;
use crate::theme::Tokens;
use crate::viewport::Proj;

/// Sizes below this are no shape at all.
const MIN: f64 = 1e-6;

/// What is being drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prim {
    Box,
    Cylinder,
    Sphere,
}

impl Prim {
    pub fn of_command(cmd: &str) -> Option<Prim> {
        match cmd {
            "solid.box" => Some(Prim::Box),
            "solid.cylinder" => Some(Prim::Cylinder),
            "solid.sphere" => Some(Prim::Sphere),
            _ => None,
        }
    }

    /// How many points the gesture asks for.
    fn stages(self) -> u8 {
        if self == Prim::Sphere { 2 } else { 3 }
    }

    /// What to do now, for the status line.
    pub fn hint(self, stage: u8) -> &'static str {
        match (self, stage) {
            (Prim::Box, 0) => "Click the first corner",
            (Prim::Box, 1) => "Click the opposite corner",
            (Prim::Cylinder, 0) | (Prim::Sphere, 0) => "Click the centre",
            (Prim::Cylinder, 1) | (Prim::Sphere, 1) => "Click to set the radius",
            (_, 2) => "Move to set the height, then click",
            _ => "",
        }
    }
}

/// A primitive being drawn.
#[derive(Clone, Debug)]
pub struct SolidDraw {
    pub prim: Prim,
    /// The plane its base is drawn on, fixed by the first click.
    pub plane: Option<Plane>,
    /// The points clicked so far (world).
    pub pts: Vec<Vec3>,
    /// Where the cursor is now (world, snapped).
    pub cursor: Option<Vec3>,
    pub stage: u8,
    /// The shape is drawn and is being adjusted by its handles; the next click confirms it.
    pub editing: bool,
    /// Which way the base was drawn, so it can be drawn into any quarter.
    pub sx: f64,
    pub sy: f64,
    /// Which side of the plane the height went.
    pub sz: f64,
    pub width: f64,
    pub length: f64,
    pub radius: f64,
    pub height: f64,
    /// Fields the drawing last wrote, so one the user has typed over can be told apart.
    wrote: Vec<(&'static str, String)>,
    /// Fields the user has typed: the drawing stops writing them and uses what is there.
    locked: std::collections::BTreeSet<&'static str>,
    /// An operation key has been pressed, so the pull no longer chooses one.
    op_chosen: bool,
}

impl SolidDraw {
    fn new(prim: Prim) -> SolidDraw {
        SolidDraw {
            prim,
            plane: None,
            pts: Vec::new(),
            cursor: None,
            stage: 0,
            editing: false,
            sx: 1.0,
            sy: 1.0,
            sz: 1.0,
            width: 0.0,
            length: 0.0,
            radius: 0.0,
            height: 0.0,
            wrote: Vec::new(),
            locked: std::collections::BTreeSet::new(),
            op_chosen: false,
        }
    }

    /// Whether the user has typed this field, so the drawing leaves it alone.
    pub fn locked(&self, key: &str) -> bool {
        self.locked.contains(key)
    }

    /// Whether enough has been said to make the shape.
    pub fn defined(&self) -> bool {
        match self.prim {
            Prim::Sphere => self.radius > MIN,
            Prim::Box => self.stage >= 2 && self.width > MIN && self.length > MIN && self.height.abs() > MIN,
            Prim::Cylinder => self.stage >= 2 && self.radius > MIN && self.height.abs() > MIN,
        }
    }

    pub fn hint(&self) -> &'static str {
        if self.editing { "Drag the handles to adjust, then click to confirm" } else { self.prim.hint(self.stage) }
    }

    /// The base point in plane coordinates.
    fn base(&self) -> Option<Vec2> {
        Some(self.plane?.to_local(*self.pts.first()?))
    }
}

/// Start drawing `cmd`, if it is a primitive that can be drawn. The dialog is left open for its
/// numbers and told not to preview until the shape has been drawn.
pub fn start(app: &mut SolveApp, cmd: &str) {
    // Drawing a primitive is one of Plasticity's habits, and this base already has its own
    // way of placing one from the viewport: the card takes a face pick, which a Cylinder
    // then dimensions from two reference edges (#5638, #6380). Both cannot own the click, so
    // the control scheme says which applies -- SolidCraft's card, or Plasticity's drawing.
    if !crate::plasticity::active(app) {
        return;
    }
    let Some(prim) = Prim::of_command(cmd) else { return };
    app.solid_draw = Some(SolidDraw::new(prim));
    if let Some(d) = app.dialog.as_mut() {
        d.preview_enabled = false;
    }
}

/// The keys that set what the shape being drawn will be made as, and the operation each one
/// means (an index into [`crate::dialogs::OPS`]).
///
/// They are taken only while a shape is being drawn, so they do not stand in the way of the
/// commands that carry the same letters the rest of the time.
pub const OPERATION_KEYS: [(&str, usize); 5] = [("B", 0), ("Q", 1), ("W", 2), ("Shift+E", 3), ("Shift+Q", 4)];

/// The key that keeps the shape being drawn as a body of its own as well.
pub const KEEP_TOOLS_KEY: &str = "T";

/// Set what the shape will be made as, while one is being drawn. Says whether the key was taken.
///
/// Which bodies a cut or a join goes into is left to the command, which takes the ones the shape
/// reaches; `targets` names them instead where that is not what was wanted.
pub fn handle_key(app: &mut SolveApp, key: &str) -> bool {
    if app.solid_draw.is_none() {
        return false;
    }
    // Backspace takes the last point back, so a corner put in the wrong place costs one key
    // rather than the whole shape.
    if key == "Backspace" || key == "Ctrl+Z" {
        return step_back(app);
    }
    // Keep Tools: the shape stays a body of its own as well as doing what it does to the solid
    // it is drawn into. It says nothing about a New Body shape, which is a body anyway.
    if key == KEEP_TOOLS_KEY {
        let Some(d) = app.dialog.as_mut() else { return false };
        let keep = !d.extra.get("keep_tools").and_then(Value::as_bool).unwrap_or(false);
        d.extra.insert("keep_tools".into(), json!(keep));
        return true;
    }
    let Some((_, op)) = OPERATION_KEYS.iter().find(|(k, _)| *k == key) else { return false };
    let Some(dialog) = app.dialog.as_mut() else { return false };
    // The window chooses its own operation on this base (`op_auto`, #6901). A key is the
    // person saying what they want, which is what that module calls picked by hand.
    dialog.op_auto.manual = true;
    let Some(Kind::Primitive { operation, .. }) = Some(&mut dialog.kind) else { return false };
    *operation = *op;
    if let Some(d) = app.solid_draw.as_mut() {
        d.op_chosen = true;
    }
    true
}

/// Stop drawing, leaving the dialog as it is.
pub fn clear(app: &mut SolveApp) {
    app.solid_draw = None;
}

/// Whether a primitive is being drawn and still wants points.
pub fn wants_points(app: &SolveApp) -> bool {
    app.solid_draw.as_ref().is_some_and(|d| d.editing || d.stage < d.prim.stages())
}

/// The plane to draw on: the planar face under the cursor, else the ground plane through the
/// point. A box has no orientation, so it only takes a plane square to the world.
fn plane_for(app: &SolveApp, _prim: Prim, proj: &Proj, pos: Pos2, at: Vec3) -> Plane {
    let ground = Plane { origin: Vec3::new(0.0, 0.0, at.z), ..Plane::XY };
    let snapped = crate::model_snap::current().and_then(|s| face_normal(app, &s));
    let Some(n) = snapped.or_else(|| face_under(app, proj, pos).map(|(_, n)| n)) else {
        return ground;
    };
    Plane::from_normal(at, n).unwrap_or(ground)
}

/// The planar face under the cursor and the point the pick lands on it, taken from an ordinary
/// pick rather than from a snap: what a shape is drawn on must not wait for the snap chord, since
/// the face under the cursor is the face being drawn on whether or not anything locks onto it.
fn face_under(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<(Vec3, Vec3)> {
    let (body, index, point) = crate::viewport::pick(app, proj, pos).into_iter().find_map(|h| match h {
        crate::viewport::Hit::Face { body, index, point } => Some((body, index, point)),
        _ => None,
    })?;
    let st = crate::open_meshing::shown_world(app);
    let m = st.body(&body)?.mesh();
    Some((point, planar_normal(&m, u32::try_from(index).ok()?)?))
}

/// Which way a planar face faces, from its own triangles. A plane to draw on needs the direction
/// and nothing else, so this reads a bounded sample of them and calls the face planar while they
/// all face the same way: `snap::planar_face_center` also works out a centre, and gives up on a
/// face with many triangles -- the sample plate's top, with its holes, is one such.
fn planar_normal(m: &solidcraft_engine::geom::Mesh, face: u32) -> Option<Vec3> {
    /// The most triangles of one face read before it is taken as it looks so far.
    const SAMPLE: usize = 512;
    /// How closely two of its triangles must face the same way to count as one plane.
    const SAME_WAY: f64 = 1.0 - 1e-6;
    let mut normal: Option<Vec3> = None;
    for [a, b, c] in m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| **f == face).filter_map(|(t, _)| m.tri(t)).take(SAMPLE) {
        let n = (b - a).cross(c - a);
        let w = n.len();
        if w <= 0.0 {
            continue;
        }
        match normal {
            None => normal = n.normalized(),
            Some(first) if first.dot(n) / w < SAME_WAY => return None,
            Some(_) => {}
        }
    }
    normal
}

/// The normal of the planar face a snapped place sits on.
fn face_normal(app: &SolveApp, s: &crate::model_snap::Snap) -> Option<Vec3> {
    let (body, index) = s.place_on.clone()?;
    let st = crate::open_meshing::shown_world(app);
    let m = st.body(&body)?.mesh();
    planar_normal(&m, u32::try_from(index).ok()?)
}

/// Whether a drawn point goes to the grid: the preference, which the suppress-snap chord turns
/// round. The chord takes the places on the model away, so it hands the grid over in their
/// place rather than leaving the point to float wherever the ray happens to land.
///
/// Under Plasticity's controls the chord does the opposite -- it is what asks for the places on
/// the model -- so it has nothing to hand the grid over for. The grid is then the preference's
/// alone, as the arrow's round steps are: a point lands on it whether or not the chord is held,
/// and the chord adds the places on the model on top of it.
fn grid_snapping(app: &SolveApp) -> bool {
    if crate::plasticity::active(app) {
        return app.preferences.draw_grid_snap;
    }
    app.preferences.draw_grid_snap != crate::drag_snap::suppressed()
}

/// A length taken to the nearest line of the grid the view is drawing.
fn to_grid(app: &SolveApp, v: f64) -> f64 {
    let step = solidcraft_engine::view::grid_step(app.cam.half_height()).0;
    if step > 0.0 { (v / step).round() * step } else { v }
}

/// The point under the cursor: the snapped place when there is one, else where the ray meets the
/// plane being drawn on (or the ground while there is none), taken to the grid when that is on.
fn point_at(app: &SolveApp, proj: &Proj, pos: Pos2, plane: Option<Plane>) -> Option<Vec3> {
    if let Some(s) = crate::model_snap::current().filter(|s| s.exact()) {
        return Some(s.at);
    }
    // With no plane to draw on yet, the place sliding over the face under the cursor is where the
    // shape goes: a base clicked out anywhere on a face belongs on that face, not on the ground
    // below it, which is what the ray would otherwise meet. Only a place that knows the face it
    // sits on will do, since that face is what [`plane_for`] then takes the plane from.
    if plane.is_none() {
        if let Some(s) = crate::model_snap::current().filter(|s| s.place_on.is_some()) {
            return Some(s.at);
        }
        // With no snap to go by -- under Plasticity's controls there is none until the chord is
        // held -- the face under the cursor still says where the shape goes: the pick's own point
        // on it, which the plane taken from that face then takes to the grid.
        if let Some((p, _)) = face_under(app, proj, pos) {
            return Some(p);
        }
    }
    let (o, d) = proj.ray(pos);
    let plane = plane.unwrap_or(Plane::XY);
    let at = plane.intersect_ray(o, d)?;
    if !grid_snapping(app) {
        return Some(at);
    }
    // Rounded in the plane's own coordinates, so the grid lies on the face being drawn on.
    let q = plane.to_local(at);
    Some(plane.to_world(Vec2::new(to_grid(app, q.x), to_grid(app, q.y))))
}

/// Follow the cursor: size the base, or the height once the base is down.
pub fn on_move(app: &mut SolveApp, proj: &Proj, pos: Pos2) {
    let Some(mut d) = app.solid_draw.take() else { return };
    // Once it is being adjusted the handles own the sizes, not the cursor.
    if d.editing {
        app.solid_draw = Some(d);
        return;
    }
    if d.stage == 2 {
        if let (Some(plane), Some(base)) = (d.plane, d.pts.get(1).or_else(|| d.pts.first()).copied()) {
            let h = crate::canvas::along(proj, pos, base, plane.normal()).unwrap_or(0.0);
            let h = if grid_snapping(app) { to_grid(app, h) } else { h };
            d.sz = if h < 0.0 { -1.0 } else { 1.0 };
            d.height = h.abs();
            operation_from_pull(app, &d, plane.normal());
        }
    } else {
        d.cursor = point_at(app, proj, pos, d.plane);
        if d.stage == 1 {
            size_from_cursor(&mut d);
        }
    }
    write_fields(app, &mut d);
    app.solid_draw = Some(d);
}

/// Pulled into the solid it stands on, a shape takes material out of it; pulled away from it, it
/// is a body of its own. That is how a hole is made with the Cylinder -- draw it on the face and
/// pull it in -- and it is the rule an extrude already follows
/// ([`solidcraft_engine::auto_operation`]). The operation keys say otherwise where that is wanted,
/// and once one has been pressed the pull leaves the choice alone.
fn operation_from_pull(app: &mut SolveApp, d: &SolidDraw, n: Vec3) {
    if d.op_chosen {
        return;
    }
    let want = if pulls_into_a_body(app, d, n) { 2 } else { 0 };
    if let Some(dialog) = app.dialog.as_mut() {
        // The pull is a gesture of the person's, so it holds against the window's own
        // automatic choice (`op_auto`, #6901), which would otherwise answer from the
        // material alone and overwrite it.
        dialog.op_auto.manual = true;
        if let Kind::Primitive { operation, .. } = &mut dialog.kind {
            *operation = want;
        }
    }
}

/// Whether the pull starts in material: the place just past the middle of the base the way the
/// height is going, or halfway along it, lies in a body. Two points rather than one, so a pull
/// carried right through a body and out the far side still reads as a cut.
fn pulls_into_a_body(app: &SolveApp, d: &SolidDraw, n: Vec3) -> bool {
    let h = d.sz * d.height;
    if !h.is_finite() || h.abs() < MIN {
        return false;
    }
    let Some(centre) = base_centre(d) else { return false };
    let Ok(st) = app.session.world_state() else { return false };
    // As far past the base as `auto_operation` steps, and no further than halfway.
    let near = h.signum() * (h.abs() * 0.5).min(0.1);
    [centre + n * near, centre + n * (h * 0.5)].iter().any(|q| st.bodies.iter().any(|b| b.mesh().contains(*q)))
}

/// The middle of the base being drawn: a cylinder's centre, or the middle of a box's rectangle.
fn base_centre(d: &SolidDraw) -> Option<Vec3> {
    let first = d.pts.first().copied()?;
    match (d.prim, d.pts.get(1)) {
        (Prim::Box, Some(far)) => Some((first + *far) * 0.5),
        _ => Some(first),
    }
}

/// The base sizes the cursor stands for.
fn size_from_cursor(d: &mut SolidDraw) {
    let (Some(plane), Some(a), Some(cursor)) = (d.plane, d.base(), d.cursor) else { return };
    let b = plane.to_local(cursor);
    match d.prim {
        Prim::Box => {
            let (w, l) = (b.x - a.x, b.y - a.y);
            d.sx = if w < 0.0 { -1.0 } else { 1.0 };
            d.sy = if l < 0.0 { -1.0 } else { 1.0 };
            d.width = w.abs();
            d.length = l.abs();
        }
        Prim::Cylinder | Prim::Sphere => d.radius = a.dist(b),
    }
}

/// Take the point under the cursor. Returns whether the gesture used the click.
pub fn on_click(app: &mut SolveApp, proj: &Proj, pos: Pos2) -> bool {
    if !wants_points(app) {
        return false;
    }
    // A click while it is being adjusted is the one that confirms it.
    if app.solid_draw.as_ref().is_some_and(|d| d.editing) {
        finish(app);
        return true;
    }
    on_move(app, proj, pos);
    let Some(mut d) = app.solid_draw.take() else { return false };
    if d.plane.is_none() {
        let at = d.cursor.or_else(|| point_at(app, proj, pos, None)).unwrap_or(Vec3::ZERO);
        d.plane = Some(plane_for(app, d.prim, proj, pos, at));
        // The plane fixes where the ray lands, so read the point again on it.
        d.cursor = point_at(app, proj, pos, d.plane);
    }
    match d.cursor {
        Some(p) => d.pts.push(p),
        None => {
            app.solid_draw = Some(d);
            return true;
        }
    }
    if d.stage == 1 {
        size_from_cursor(&mut d);
    }
    d.stage += 1;
    // The last point does not make the shape: it opens the handles, so the sizes can be put
    // right before anything is committed.
    d.editing = d.stage >= d.prim.stages();
    write_fields(app, &mut d);
    app.solid_draw = Some(d);
    true
}

/// Put the drawn sizes and placement into the dialog, and let it preview once there is a shape.
fn write_fields(app: &mut SolveApp, d: &mut SolidDraw) {
    let defined = d.defined();
    let wrote = std::mem::take(&mut d.wrote);
    let mut locked = std::mem::take(&mut d.locked);
    let placement = corner_of(d);
    let Some(dialog) = app.dialog.as_mut() else { return };
    dialog.preview_enabled = defined;
    // A box is placed by its corner outright, since `at` would centre it on the point.
    match (d.prim, placement) {
        (Prim::Box, Some((corner, _))) => {
            dialog.extra.insert("drawn_corner".into(), serde_json::json!([corner.x, corner.y, corner.z]));
            // The frame it was drawn in goes with the corner, so a box standing on a face keeps
            // that face's own directions instead of the world's.
            if let Some((_, axis, x)) = box_frame_of(d) {
                dialog.extra.insert("drawn_axis".into(), serde_json::json!([axis.x, axis.y, axis.z]));
                dialog.extra.insert("drawn_x_axis".into(), serde_json::json!([x.x, x.y, x.z]));
            }
        }
        // A cylinder is placed by the centre of its base, which carries the way its axis
        // runs; a sphere by its centre. They go through the extras too, because `at` on this
        // base belongs to the window's own pick and placement machinery, which clears it as
        // the window syncs -- the drawing's placement would not survive there.
        (Prim::Cylinder, Some((base, axis))) => {
            dialog.extra.insert("drawn_base".into(), serde_json::json!([base.x, base.y, base.z]));
            if let Some(axis) = axis {
                dialog.extra.insert("drawn_axis".into(), serde_json::json!([axis.x, axis.y, axis.z]));
            }
        }
        (Prim::Sphere, Some((centre, _))) => {
            dialog.extra.insert("drawn_center".into(), serde_json::json!([centre.x, centre.y, centre.z]));
        }
        _ => {
            for key in ["drawn_corner", "drawn_base", "drawn_center", "drawn_axis", "drawn_x_axis"] {
                dialog.extra.remove(key);
            }
        }
    }
    let Kind::Primitive { fields, at, .. } = &mut dialog.kind else { return };
    // A field whose text is no longer what the drawing last wrote has been typed over, and the
    // drawing leaves it alone from then on: a size said in numbers outranks one said by hand.
    for (key, written) in &wrote {
        if fields.iter().any(|f| f.0 == *key && f.1 != *written) {
            locked.insert(key);
        }
    }
    let mut written_now: Vec<(&'static str, String)> = Vec::new();
    let mut set = |key: &'static str, v: f64| {
        if locked.contains(&key) {
            return;
        }
        if let Some(f) = fields.iter_mut().find(|f| f.0 == key) {
            f.1 = format_len(v);
            written_now.push((key, f.1.clone()));
        }
    };
    match d.prim {
        Prim::Box => {
            set("length", d.width);
            set("width", d.length);
            set("height", d.height);
        }
        Prim::Cylinder => {
            set("diameter", d.radius * 2.0);
            set("height", d.height);
        }
        Prim::Sphere => set("diameter", d.radius * 2.0),
    }
    *at = placement;
    d.wrote = written_now;
    d.locked = locked;
}

/// The frame a drawn box stands in: the corner its sizes run from, the way its height goes, and
/// the way its length runs across that.
///
/// The height follows the pull, so a box taken into a face runs against that face's normal; the
/// length keeps the plane's own x whichever way the rectangle was dragged. The corner is then the
/// corner of the drawn box that is least along all three, so `length`, `width` and `height` run
/// forwards from it however the box was drawn or pulled. Flipping the frame's directions instead
/// made the box span backwards out of its corner, and a box pulled into a solid cut its pocket
/// beside the shape that was drawn.
fn box_frame_of(d: &SolidDraw) -> Option<(Vec3, Vec3, Vec3)> {
    let plane = d.plane?;
    let a = d.base()?;
    let n = plane.normal();
    let x = plane.x;
    let axis = if d.sz < 0.0 { -n } else { n };
    let y = axis.cross(x);
    let far = Vec2::new(a.x + d.width * d.sx, a.y + d.length * d.sy);
    let rise = n * (d.sz * d.height);
    let near = plane.to_world(a);
    let across = plane.to_world(far);
    let corners = [near, across, near + rise, across + rise];
    let least = |v: Vec3| corners.iter().map(|p| p.dot(v)).fold(f64::INFINITY, f64::min);
    let corner = x * least(x) + y * least(y) + axis * least(axis);
    corner.is_finite().then_some((corner, axis, x))
}

/// Where the shape sits, as the dialog's `at`: the corner the sizes run from for a box, the
/// centre of the base for a cylinder, the centre for a sphere.
fn corner_of(d: &SolidDraw) -> Option<(Vec3, Option<Vec3>)> {
    let plane = d.plane?;
    let a = d.base()?;
    let n = plane.normal();
    match d.prim {
        // `at` recentres a click, so the corner is given outright, which keeps it where it was put.
        Prim::Box => box_frame_of(d).map(|(corner, _, _)| (corner, None)),
        Prim::Cylinder => {
            let base = plane.to_world(a);
            let axis = n * d.sz;
            Some((base, Some(axis)))
        }
        Prim::Sphere => Some((plane.to_world(a), None)),
    }
}

/// The shape is drawn: run the dialog's command and close it, as OK does.
fn finish(app: &mut SolveApp) {
    let defined = app.solid_draw.as_ref().is_some_and(SolidDraw::defined);
    clear(app);
    let Some(mut dialog) = app.dialog.take() else { return };
    dialog.preview_enabled = true;
    if !defined {
        app.dialog = Some(dialog);
        return;
    }
    if let Err(why) = crate::dialogs::run_dialog(app, &dialog) {
        // It stays open with the numbers that were drawn, so they can be corrected.
        dialog.error = Some(why);
        app.dialog = Some(dialog);
    }
}

/// The number a field holds now, when it is a plain length: a handle drag writes the field, so
/// the drawing reads it back rather than trusting what the cursor last said.
fn field_value(app: &SolveApp, key: &str, fallback: f64) -> f64 {
    let Some(Kind::Primitive { fields, .. }) = app.dialog.as_ref().map(|d| &d.kind) else { return fallback };
    fields
        .iter()
        .find(|f| f.0 == key)
        .and_then(|f| f.1.split_whitespace().next()?.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(fallback)
}

/// The sizes as they stand: what the handles have been dragged to, else what was drawn.
fn sizes(app: &SolveApp, d: &SolidDraw) -> (f64, f64, f64, f64) {
    match d.prim {
        Prim::Box => (field_value(app, "length", d.width), field_value(app, "width", d.length), field_value(app, "height", d.height), 0.0),
        Prim::Cylinder => (0.0, 0.0, field_value(app, "height", d.height), field_value(app, "diameter", d.radius * 2.0) / 2.0),
        Prim::Sphere => (0.0, 0.0, 0.0, field_value(app, "diameter", d.radius * 2.0) / 2.0),
    }
}

/// The handles of the shape being adjusted: a draggable arrow per size, each writing its own
/// field, drawn with the manipulator's own arrow so they snap and read like every other one.
///
/// They stand where the size they set runs: the height up the plane's normal from the middle of
/// the base, the width and length out along the plane's own axes, a radius out across the base.
pub fn handles(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) {
    let Some(d) = app.solid_draw.as_ref().filter(|d| d.editing) else { return };
    let (Some(plane), Some(a)) = (d.plane, d.base()) else { return };
    let (prim, sx, sy, sz) = (d.prim, d.sx, d.sy, d.sz);
    let (w, l, h, r) = sizes(app, d);
    let half = app.cam.half_height();
    let n = plane.normal() * sz;
    // Where the base sits, and the arrows that grow from it.
    let (centre, arrows): (Vec2, Vec<(&'static str, Vec3, &'static str)>) = match prim {
        Prim::Box => {
            let c = Vec2::new(a.x + w * sx / 2.0, a.y + l * sy / 2.0);
            (
                c,
                vec![
                    ("height", n, "sc_draw_height"),
                    ("length", plane.dir_to_world(Vec2::new(sx, 0.0)), "sc_draw_length"),
                    ("width", plane.dir_to_world(Vec2::new(0.0, sy)), "sc_draw_width"),
                ],
            )
        }
        Prim::Cylinder => (a, vec![("height", n, "sc_draw_height"), ("diameter", plane.dir_to_world(Vec2::X), "sc_draw_radius")]),
        Prim::Sphere => (a, vec![("diameter", plane.dir_to_world(Vec2::X), "sc_draw_radius")]),
    };
    for (key, dir, id) in arrows {
        // Each arrow starts where its own size is measured from and shows how long it is now.
        let (base, len) = match (prim, key) {
            (_, "height") => (plane.to_world(centre), h),
            (Prim::Box, "length") => (plane.to_world(Vec2::new(a.x, centre.y)) + n * (h / 2.0), w),
            (Prim::Box, _) => (plane.to_world(Vec2::new(centre.x, a.y)) + n * (h / 2.0), l),
            // A radius is a half-width, so its arrow shows and writes the diameter from the axis.
            (Prim::Cylinder, _) => (plane.to_world(a) + n * (h / 2.0), r * 2.0),
            (Prim::Sphere, _) => (plane.to_world(a), r * 2.0),
        };
        let Some(bs) = proj.to_screen(base) else { continue };
        let Some(Kind::Primitive { fields, .. }) = app.dialog.as_mut().map(|d| &mut d.kind) else { return };
        let Some(field) = fields.iter_mut().find(|f| f.0 == key) else { continue };
        #[allow(clippy::too_many_arguments)]
        crate::canvas::drag_arrow(&app.session.doc, ui, painter, proj, base, bs, dir, len, false, egui::Id::unique(id), half, 0.0, &mut field.1);
    }
}

/// Take the last point back: out of the adjusting stage, or off the points already clicked.
/// With nothing left to take back the drawing waits for its first point rather than ending.
fn step_back(app: &mut SolveApp) -> bool {
    let Some(mut d) = app.solid_draw.take() else { return false };
    if d.editing {
        d.editing = false;
    } else if d.stage > 0 {
        d.stage -= 1;
        d.pts.pop();
    }
    // Back at the start, the plane is chosen afresh with the next point.
    if d.stage == 0 {
        d.plane = None;
    }
    d.locked.clear();
    d.wrote.clear();
    write_fields(app, &mut d);
    app.solid_draw = Some(d);
    true
}

/// What to do now and the keys the drawing answers to, for the strip ([`crate::key_hints`]).
pub fn key_hints(app: &SolveApp) -> Option<(String, Vec<crate::key_hints::Hint>)> {
    let d = app.solid_draw.as_ref()?;
    let op = match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Primitive { operation, .. }) => *operation,
        _ => 0,
    };
    let mut keys: Vec<crate::key_hints::Hint> = OPERATION_KEYS
        .iter()
        .filter_map(|(k, o)| crate::dialogs::OP_LABELS.get(*o).map(|label| crate::key_hints::Hint::set(k, label, *o == op)))
        .collect();
    let keep = app.dialog.as_ref().and_then(|d| d.extra.get("keep_tools")).and_then(Value::as_bool).unwrap_or(false);
    keys.push(crate::key_hints::Hint::set(KEEP_TOOLS_KEY, "Keep Tools", keep));
    keys.push(crate::key_hints::Hint::new("Backspace", "Back"));
    keys.push(crate::key_hints::Hint::new("Esc", "Cancel"));
    Some((d.hint().to_string(), keys))
}

/// The base being drawn, and the size beside the cursor.
pub fn show(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some(d) = app.solid_draw.as_ref() else { return };
    let (Some(plane), Some(a)) = (d.plane, d.base()) else { return };
    let t = Tokens::get();
    let to = |p: Vec2| proj.to_screen(plane.to_world(p));
    if d.stage == 1 {
        match d.prim {
            Prim::Box => {
                let (w, l) = (d.width * d.sx, d.length * d.sy);
                let pts: Vec<Pos2> = [Vec2::new(a.x, a.y), Vec2::new(a.x + w, a.y), Vec2::new(a.x + w, a.y + l), Vec2::new(a.x, a.y + l)]
                    .into_iter()
                    .filter_map(to)
                    .collect();
                if pts.len() == 4 {
                    painter.add(Shape::closed_line(pts, Stroke::new(1.5, t.accent)));
                }
            }
            Prim::Cylinder | Prim::Sphere => {
                let ring: Vec<Pos2> = (0..=48)
                    .filter_map(|i| {
                        let ang = std::f64::consts::TAU * f64::from(i) / 48.0;
                        to(Vec2::new(a.x + d.radius * ang.cos(), a.y + d.radius * ang.sin()))
                    })
                    .collect();
                if ring.len() > 8 {
                    painter.add(Shape::line(ring, Stroke::new(1.5, t.accent)));
                }
            }
        }
    }
    if let Some(label) = size_label(d)
        && let Some(at) = d.cursor.and_then(|c| proj.to_screen(c))
    {
        let r = egui::Rect::from_min_size(at + egui::vec2(14.0, 10.0), egui::vec2(10.0 + 7.0 * label.len() as f32, 16.0));
        painter.rect_filled(r, 3.0, t.accent_soft);
        painter.text(r.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(t.font_micro), t.text);
    }
}

/// A length as the manipulator's own labels write one.
fn format_len(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} mm", if s == "-0" { "0" } else { s })
}

/// The size to show beside the cursor while drawing.
fn size_label(d: &SolidDraw) -> Option<String> {
    let len = format_len;
    match (d.prim, d.stage) {
        (Prim::Box, 1) => Some(format!("{} × {}", len(d.width), len(d.length))),
        (Prim::Cylinder | Prim::Sphere, 1) => Some(format!("R {}", len(d.radius))),
        (_, 2) => Some(len(d.height)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "solid_draw_tests.rs"]
mod tests;
