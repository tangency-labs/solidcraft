//! Emboss: the marker moves the picked profiles across the model, as Offset X and Offset Y, and
//! the ring turns them, as Angle (#20386).
//!
//! The marker stands where the build puts the middle of the box round the profiles, moved by the
//! offsets ([`Onto`]): in the sketch itself with no Target Face, on a flat Target Face along the
//! sketch's normal, or round a cylinder's wall as far as the sketch has it across the axis. A
//! drag puts that middle under the pointer on the Target Face. With none picked, the point of the
//! model under the pointer is taken to the sketch along its normal, which is under the pointer
//! where the sketch lies on that face. The ring's grip points the way the profiles' x runs
//! there, and its readout is the Angle. While either works, the box round the profiles is
//! outlined where it is built.
//!
//! The offsets take a round step of the view and the Angle a step of 5°, unless Alt or Ctrl is
//! held, as other dragged values do. A field a handle does not change keeps what is typed in it,
//! an expression included; a value brought to zero empties its field, which leaves it out of the
//! command.
//!
//! A Target Face that is neither flat nor a cylinder's has no handles: its Emboss is placed by
//! the fields, as before. Nor has a cylinder's while its sketch touches the wall and the pick is
//! on the far side of it: the build wraps such a sketch from the line it touches or lays it round
//! the picked side, by where the faces of the wall end, which the kernel alone reads. With no
//! Target Face, a sketch that touches a cylinder is wrapped round it by the command, which finds
//! the wall itself: the marker is on the wall once it is picked on the side the sketch touches.
//!
//! A sketch off a wall is laid against it, each profile flat at its own middle: a wide one stands
//! off the wall a little where its outline, drawn on the wall, does not. One whose x runs neither
//! round the wall nor along it is built with each profile's x round the wall or along the axis:
//! the grip then shows the line the profiles stand along, not the way each one faces.
//!
//! The landing on a wall repeats what the kernel's `emboss_project_face` and its wrap do
//! (`laid`, `wall_frame`); `emboss_tests.rs` holds it to what the build makes.

use solidcore_record::{Plane, Region2, Vec2, Vec3};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::expr::Kind as ValueKind;
use solidcraft_engine::doc::{FeatureKind, ProfileSel, SolvedSketch};
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::kernel::{self, FaceSurface};

use super::{Place, PlaceSource, Spec, Turn, angle_of, dir_at, one, stepped};
use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::viewport::Hit;

pub(super) static SPEC: Spec = Spec {
    name: "emboss",
    serves: |app, d| super::on(app) && matches!(d.kind, Kind::Emboss { .. }),
    snap_commands: &[],
    move_tip: "Drag to move the profiles across the model. Hold Alt or Ctrl to move them freely.",
    many: false,
    place: PlaceSource::Own { places, move_to },
    click: None,
    footprint,
    freedom: super::anywhere,
    turn: Some(Turn {
        tip: "Drag to turn the profiles in 5° steps. Hold Alt or Ctrl to turn them freely.",
        reach,
        angle,
        set,
        readout: Some(readout),
    }),
};

/// Rotation step without a modifier (degrees).
const STEP_DEG: f64 = 5.0;
/// While Alt or Ctrl frees a drag, a move smaller than this share of a step is the pointer
/// standing still, and so is a turn smaller than this many degrees, half of what an Angle is
/// written to.
const STILL: f64 = 1e-3;
const STILL_DEG: f64 = 0.005;
/// How far either side of the middle a direction is followed between the sketch and the model (mm).
const NUDGE: f64 = 1e-3;
/// A sketch touches a cylinder's wall when its normal is this square to the axis and it is this
/// near the radius from the axis, as a share of the radius. Both are wider than the kernel's own
/// (`wall_frame`), so no sketch the kernel wraps is taken for one it lays.
const ALONG: f64 = 1e-6;
const TOUCHES: f64 = 1e-3;
/// How far off a cylinder a picked point of its wall may be, as a share of the radius: a pick is
/// on the mesh the wall is drawn as, which cuts across it.
const ON_WALL: f64 = 0.02;

/// Where the build puts a point of the sketch (the Emboss of `eval_more`, the kernel's
/// `emboss_project_face` and its wrap), and so where the handles stand.
enum Onto {
    /// No Target Face: the profiles are built where the sketch has them.
    Sketch,
    /// A flat Target Face, face `face` of `body`, lying in `lies`: onto it along the sketch's
    /// normal.
    Flat { body: String, face: usize, lies: Plane },
    /// A Target Face on a cylinder: the profiles are laid round the wall of `body`, as far round
    /// it as the sketch has them across the axis. `unrolled` has its origin on the axis, its x
    /// round the wall from the line of the wall the profiles are counted from, its y along the
    /// axis.
    Wall { body: String, unrolled: Plane, radius: f64 },
}

/// What the handles place: the picked profiles in their sketch, as the dialog stands.
struct Picked {
    /// The sketch's plane, as shown.
    plane: Plane,
    regions: Vec<Region2>,
    /// Where the offsets put the middle of the box round the profiles, which Angle turns them
    /// about.
    middle: Vec2,
    /// Offset X and Offset Y (mm).
    shift: Vec2,
    /// Angle (radians).
    angle: f64,
    onto: Onto,
}

impl Picked {
    /// Where the sketch point `at` is built, and the way the surface faces there (out from the
    /// axis, on a wall), which the ring lies across. None for a flat face the sketch stands
    /// square to.
    fn land(&self, at: Vec2) -> Option<(Vec3, Vec3)> {
        let from = self.plane.to_world(at);
        match &self.onto {
            Onto::Sketch => Some((from, self.plane.normal())),
            Onto::Flat { lies, .. } => lies.intersect_ray(from, self.plane.normal()).map(|on| (on, lies.normal())),
            Onto::Wall { unrolled, radius, .. } => {
                let across = unrolled.to_local(from);
                let (sin, cos) = (across.x / radius).sin_cos();
                let out = unrolled.normal() * cos + unrolled.x * sin;
                Some((unrolled.origin + unrolled.y * across.y + out * *radius, out))
            }
        }
    }

    /// The sketch point that is built at `on`, a point of the model: [`Picked::land`] the other
    /// way round.
    fn lift(&self, on: Vec3) -> Option<Vec2> {
        let Onto::Wall { unrolled, radius, .. } = &self.onto else { return Some(self.plane.to_local(on)) };
        // As far across the axis as `on` is round the wall, at its own height.
        let round = nearer(angle_of(on - unrolled.origin, unrolled.y) - angle_of(unrolled.normal(), unrolled.y));
        let across = unrolled.to_world(Vec2::new(round * radius, unrolled.to_local(on).y));
        self.plane.intersect_ray(across, unrolled.normal()).map(|at| self.plane.to_local(at))
    }

    /// The point of the model `hit` is at, if the profiles can go there: on the Target Face when
    /// one is picked (anywhere on its wall, which is several faces between its seams), else on
    /// any face.
    fn takes(&self, hit: &Hit) -> Option<Vec3> {
        let Hit::Face { body, index, point } = hit else { return None };
        let there = match &self.onto {
            Onto::Sketch => true,
            Onto::Flat { body: target, face, .. } => body == target && index == face,
            Onto::Wall { body: target, unrolled, radius } => {
                let (round, out) = (unrolled.to_local(*point).x, unrolled.height(*point));
                body == target && ((round * round + out * out).sqrt() - radius).abs() < ON_WALL * radius
            }
        };
        there.then_some(*point)
    }

    /// The way the profiles' x runs in the sketch.
    fn x(&self) -> Vec2 {
        Vec2::new(self.angle.cos(), self.angle.sin())
    }

    /// The way it runs on the model: from where a point a little before the middle is built to
    /// where one a little past it is.
    fn run(&self) -> Option<Vec3> {
        let step = self.x() * NUDGE;
        Some(self.land(self.middle + step)?.0 - self.land(self.middle - step)?.0)
    }
}

/// The profiles picked in the sketch of the first one, with the dialog's offsets, angle and
/// Target Face. None while nothing is picked, and for a Target Face without handles.
fn picked(app: &SolveApp, d: &Dialog) -> Option<Picked> {
    let Kind::Emboss { offset_x, offset_y, angle, .. } = &d.kind else { return None };
    let items = &d.inputs.first()?.items;
    let Sel::Profile { sketch, .. } = items.first()? else { return None };
    let world = crate::open_meshing::session_world(&app.session);
    let drawn = world.sketches.iter().find(|solved| solved.feature == *sketch)?;
    // An edit that still takes every profile builds what was drawn, which can be fewer than the
    // dialog's items: a profile that projected edges close is among those, and is not built.
    let every = !d.profile_selection_changed
        && d.editing
            .and_then(|(id, _)| app.session.doc.feature(id))
            .is_some_and(|f| matches!(f.kind, FeatureKind::Emboss { profiles: ProfileSel::All, .. }));
    let regions: Vec<Region2> = if every {
        every_profile(drawn)?
    } else {
        let region = |item: &Sel| match item {
            Sel::Profile { sketch: id, index } if id == sketch => drawn.profiles.get(*index).map(|p| p.region.clone()),
            _ => None,
        };
        items.iter().filter_map(region).collect()
    };
    if regions.is_empty() {
        return None;
    }
    // An edited Emboss on a face keeps the placement it was made in, where its sketch can be
    // another copy of its part than the one shown: the dialog has its profile guide there too.
    let plane = match crate::dialogs::emboss_plane_of(app, d, *sketch) {
        Ok(Some(plane)) => plane,
        Ok(None) => crate::canvas::profile_plane(app, *sketch)?,
        Err(_) => return None,
    };
    let onto = match d.inputs.get(1).and_then(|input| input.items.first()) {
        Some(Sel::Face { body, index, point: pick }) => {
            let target = &world.body(body)?.body;
            match kernel::guard("emboss target", || Ok(kernel::face_surface(target, *index))).ok().flatten()? {
                FaceSurface::Plane { normal, point } => Onto::Flat { body: body.clone(), face: *index, lies: Plane::from_normal(point, normal)? },
                FaceSurface::Cylinder { origin, axis, radius } => {
                    Onto::Wall { body: body.clone(), unrolled: unrolled(&plane, origin, axis.vec(), radius, *pick)?, radius }
                }
                _ => return None,
            }
        }
        _ => Onto::Sketch,
    };
    let shift = Vec2::new(read(app, offset_x, ValueKind::Length), read(app, offset_y, ValueKind::Length));
    Some(Picked {
        middle: solidcraft_engine::doc::emboss_middle(&regions) + shift,
        plane,
        regions,
        shift,
        angle: read(app, angle, ValueKind::Angle),
        onto,
    })
}

thread_local! {
    /// Every profile of the sketch it holds, as [`every_profile`] found them.
    static EVERY: std::cell::RefCell<Option<(std::sync::Arc<SolvedSketch>, Vec<Region2>)>> = const { std::cell::RefCell::new(None) };
}

/// Every profile of `drawn` as an Emboss of them all builds it ([`select_profiles`]). They are
/// found once for a solved sketch: the search through its curves is too slow for every hook of
/// every frame.
///
/// [`select_profiles`]: solidcraft_engine::doc::select_profiles
fn every_profile(drawn: &std::sync::Arc<SolvedSketch>) -> Option<Vec<Region2>> {
    EVERY.with_borrow_mut(|kept| {
        if !kept.as_ref().is_some_and(|(of, _)| std::sync::Arc::ptr_eq(of, drawn)) {
            *kept = Some((drawn.clone(), solidcraft_engine::doc::select_profiles(drawn, &ProfileSel::All).ok()?));
        }
        kept.as_ref().map(|(_, regions)| regions.clone())
    })
}

/// The wall of the cylinder of radius `radius` about `axis` through `origin`, unrolled as the
/// build lays a sketch on `plane` round it: from the line of the wall square to the way the
/// sketch reads round it, which is its x unless that runs more along the axis than its y, on the
/// side of `pick`, the Target Face's point. A sketch that touches the wall on that side is
/// wrapped from the same line. None for a sketch square to the axis, which no line of the wall
/// faces, and for one that touches the wall on the other side from `pick`: wrapped, it starts
/// where it touches, and laid, on the side of `pick`.
fn unrolled(plane: &Plane, origin: Vec3, axis: Vec3, radius: f64, pick: Vec3) -> Option<Plane> {
    Plane::new(origin, axis, plane.normal())?;
    let along = plane.to_local(plane.origin + axis);
    let reading = if along.x.abs() > along.y.abs() { plane.y } else { plane.x };
    let facing = Plane::new(origin, reading, axis)?;
    let (touch, side) = (facing.height(plane.origin), facing.height(pick));
    let touches = plane.height(plane.origin + axis).abs() < ALONG && (touch.abs() - radius).abs() < TOUCHES * radius;
    if touches && (touch < 0.0) != (side < 0.0) {
        return None;
    }
    let out = if side < 0.0 { -facing.normal() } else { facing.normal() };
    Plane::new(origin, Plane::new(origin, axis, out)?.normal(), axis)
}

/// The marker: where the point Angle turns the profiles about is built.
fn places(app: &SolveApp, d: &Dialog) -> Vec<Place> {
    one(picked(app, d).and_then(|p| p.land(p.middle)))
}

/// Puts the middle of the profiles under the pointer. False when nothing there can take them.
fn move_to(app: &SolveApp, d: &mut Dialog, hits: &[Hit], _: usize) -> bool {
    let Some(p) = picked(app, d) else { return false };
    let Some(to) = hits.iter().find_map(|hit| p.takes(hit)).and_then(|on| p.lift(on)) else { return false };
    // The offsets count from where the sketch has the middle.
    let to = to - (p.middle - p.shift);
    let Kind::Emboss { offset_x, offset_y, .. } = &mut d.kind else { return false };
    write_length(app, offset_x, to.x);
    write_length(app, offset_y, to.y);
    d.error = None;
    true
}

/// The box round the profiles where it is built: each corner on the model as the marker is.
fn footprint(app: &SolveApp, d: &Dialog, _: Vec3, _: Vec3) -> Vec<Vec3> {
    let Some(p) = picked(app, d) else { return Vec::new() };
    let Ok(corners) = solidcraft_engine::doc::emboss_box(&p.regions, p.shift, p.angle) else { return Vec::new() };
    corners.iter().map(|c| p.land(*c).map(|(on, _)| on)).collect::<Option<Vec<Vec3>>>().unwrap_or_default()
}

/// Picked profiles can always be turned: half the width of the box round them.
fn reach(app: &SolveApp, d: &Dialog) -> Option<f64> {
    let (lo, hi) = solidcraft_engine::doc::frame::profile_bounds(&picked(app, d)?.regions)?;
    Some((hi.x - lo.x) * 0.5)
}

/// Where the grip stands: the way the profiles' x runs on the model, about `n`.
fn angle(app: &SolveApp, d: &Dialog, n: Vec3) -> f64 {
    picked(app, d).and_then(|p| p.run()).map_or(0.0, |x| angle_of(x, n))
}

/// The readout beside the grip: the Angle as its field holds it, which counts from the sketch's x.
fn readout(app: &SolveApp, d: &Dialog) -> f64 {
    let Kind::Emboss { angle, .. } = &d.kind else { return 0.0 };
    read(app, angle, ValueKind::Angle)
}

/// Turns the profiles until their x runs where the grip points, at `raw` about `n`: that way on
/// the model is read back in the sketch, whose x Angle counts from, about the sketch's own
/// normal. The turn goes the nearer way round, in 5° steps of Angle unless `free`; the Angle is
/// written in degrees without its whole turns, which the build leaves out too, and none empties
/// the field.
fn set(app: &SolveApp, d: &mut Dialog, n: Vec3, raw: f64, free: bool) -> f64 {
    let Some(p) = picked(app, d) else { return raw };
    // The middle is read back too: past half the way round a wall the sketch has it a turn away.
    let read_back = |(base, _): (Vec3, Vec3)| Some((p.lift(base)?, p.lift(base + dir_at(n, raw) * NUDGE)?));
    let Some((from, ahead)) = p.land(p.middle).and_then(read_back) else { return raw };
    let about = p.plane.normal();
    let turn = nearer(angle_of(p.plane.dir_to_world(ahead - from), about) - angle_of(p.plane.dir_to_world(p.x()), about));
    // A grip not turned half a step from where the profiles run leaves the Angle as it is typed.
    if turn.abs() >= (if free { STILL_DEG } else { STEP_DEG / 2.0 }).to_radians()
        && let Kind::Emboss { angle: field, .. } = &mut d.kind
    {
        let turned = (p.angle % std::f64::consts::TAU + turn).to_degrees();
        let degrees = (stepped(turned, STEP_DEG, free) % 360.0 * 100.0).round() / 100.0;
        *field = if degrees == 0.0 { String::new() } else { format!("{degrees} deg") };
    }
    angle(app, d, n)
}

/// An angle taken the nearer way round: within a half turn of none.
fn nearer(turn: f64) -> f64 {
    (turn + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

/// A field's value as the command reads it, in mm or radians; 0 while the field is empty or cannot
/// be read.
fn read(app: &SolveApp, text: &str, kind: ValueKind) -> f64 {
    crate::length_input::eval(&app.session.doc, text, kind).ok().filter(|v| v.is_finite()).unwrap_or(0.0)
}

/// Writes the length a drag came to (mm) into `field`: on a round step of the view, as other
/// snapped drags write theirs, or as it is while Alt or Ctrl is held. A field the drag has not
/// left by half a step keeps its text, a typed value or an expression: the profiles have not
/// moved that way.
fn write_length(app: &SolveApp, field: &mut String, mm: f64) {
    let (doc, step) = (&app.session.doc, crate::canvas::snap_step(app.cam.half_height()));
    let free = crate::tool_modifiers::manipulator_free();
    if (read(app, field, ValueKind::Length) - mm).abs() < step * if free { STILL } else { 0.5 } {
        return;
    }
    let mm = if free { mm } else { (mm / step).round() * step };
    *field = if mm.abs() < 1e-9 {
        String::new()
    } else if free {
        crate::canvas::format_len(doc, mm)
    } else {
        crate::canvas::format_len_drag(doc, mm, step, app.preferences.length_decimals)
    };
}

#[cfg(test)]
#[path = "emboss_tests.rs"]
mod tests;
