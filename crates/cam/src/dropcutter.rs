//! The drop-cutter: lower a cutter along its axis onto a surface until it first touches, and
//! report where its tip stops. Every 3D finishing and roughing toolpath here is built from it,
//! and every multi-axis pose is checked with it, because a tip placed where the dropped cutter
//! stops cannot cut into the surface.
//!
//! Against one triangle the cutter touches either a corner, the inside of the face, or an
//! edge. Corners and faces are worked out directly. Along an edge the height the tip must have
//! to touch the edge's point at parameter `t` is `z(t) - h(d(t))`, with `z` linear, `d` the
//! (convex) distance from the axis and `h` the cutter's (convex, rising) height profile, so it
//! is concave in `t`: a flat end's maximum is at the end of the stretch under the cutter, a
//! ball's and a tapered cutter's are found in closed form, and a bull nose's by golden-section
//! search to far below any machining tolerance.

use std::cell::Cell;

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::cutter::Cutter;
use crate::frame::{Frame, unit};
use crate::surface::{Surface, capsule_meets_box};
use crate::{Result, invalid};

#[cfg(test)]
thread_local! {
    /// How many triangles this thread's drops have opened (the tests' measure of their work).
    pub(crate) static OPENED: Cell<usize> = const { Cell::new(0) };
    /// How many bull-nose edge searches this thread's drops have run.
    pub(crate) static EDGE_SEARCHES: Cell<usize> = const { Cell::new(0) };
    /// Set to search every edge the cutter reaches, as before edges were passed over by
    /// their bound (the tests' reference).
    pub(crate) static NO_EDGE_BOUND: Cell<bool> = const { Cell::new(false) };
}

/// Golden-section steps along an edge for a bull nose: the parameter to 0.618^48 ≈ 1e-10 of
/// the stretch under the cutter. Near the maximum the height changes with the square of the
/// parameter's error, so the height is found far more closely still.
const EDGE_STEPS: usize = 48;

/// The highest tip height at which `cutter`, its axis vertical through `(x, y)`, touches the
/// triangle `t`; `None` when the cutter does not reach over it.
pub fn drop_triangle(cutter: &Cutter, t: &[Vec3; 3], x: f64, y: f64) -> Option<f64> {
    drop_triangle_above(cutter, t, x, y, f64::NEG_INFINITY)
}

/// [`drop_triangle`], skipping the work that cannot find a tip height above `above`: the
/// answer is exact when it is above `above`, and may be `None` or anything not above it
/// otherwise.
fn drop_triangle_above(cutter: &Cutter, t: &[Vec3; 3], x: f64, y: f64, above: f64) -> Option<f64> {
    #[cfg(test)]
    OPENED.with(|n| n.set(n.get() + 1));
    let r = cutter.radius;
    let mut best: Option<f64> = None;
    let keep = |best: &mut Option<f64>, z: f64| {
        if z.is_finite() {
            *best = Some(best.map_or(z, |b: f64| b.max(z)));
        }
    };
    // Corners.
    for v in t {
        if let Some(h) = cutter.height((v.x - x).hypot(v.y - y)) {
            keep(&mut best, v.z - h);
        }
    }
    // The inside of the face.
    if let Some(mut n) = (t[1] - t[0]).cross(t[2] - t[0]).normalized() {
        if n.z < 0.0 {
            n = -n;
        }
        // A face this close to vertical is met at its edges and corners first.
        if n.z > 1e-9
            && let Some(cone) = cutter.cone
        {
            // The tip height that touches the plane at distance `d` from the axis, towards
            // where the plane is highest, is linear in `d` on either side of the tip's rim: a
            // cone meets a plane first at its tip's rim or, on a plane steeper than its side,
            // at its top rim.
            let s = n.x.hypot(n.y);
            let d = if s / n.z > 1.0 / cone.half_angle.tan() { r } else { cone.tip_radius };
            let (qx, qy) = if s > 1e-12 { (x - d * n.x / s, y - d * n.y / s) } else { (x, y) };
            let z = t[0].z - (n.x * (qx - t[0].x) + n.y * (qy - t[0].y)) / n.z - cutter.height(d).unwrap_or(0.0);
            let top = t[0].z.max(t[1].z).max(t[2].z);
            if inside_xy(t, qx, qy) && z <= top + 1e-9 {
                keep(&mut best, z);
            }
        } else if n.z > 1e-9 {
            let s = n.x.hypot(n.y);
            let a = cutter.flat_radius();
            // The flat bottom's edge (or centre) on the side where the plane is highest.
            let (qx, qy) = if s > 1e-12 { (x - a * n.x / s, y - a * n.y / s) } else { (x, y) };
            let rc = cutter.corner;
            let zc = t[0].z + (rc - n.x * (qx - t[0].x) - n.y * (qy - t[0].y)) / n.z;
            let cc = Vec3::new(qx, qy, zc) - n * rc;
            // The contact is on the face and the cutter's surface is never below its tip, so a
            // tip above the face's highest corner is a point outside it accepted by rounding.
            let top = t[0].z.max(t[1].z).max(t[2].z);
            if inside_xy(t, cc.x, cc.y) && zc - rc <= top + 1e-9 {
                keep(&mut best, zc - rc);
            }
        }
    }
    // Edges: a tip touching an edge is never above the edge's higher end, so an edge whose
    // higher end is no higher than what is already found cannot raise it.
    for (p, q) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
        let floor = best.unwrap_or(f64::NEG_INFINITY).max(above);
        if p.z.max(q.z) <= floor {
            continue;
        }
        if let Some(z) = drop_edge(cutter, p, q, x, y, r, floor) {
            keep(&mut best, z);
        }
    }
    best
}

/// The highest tip at which a ball of radius `r` on the vertical through `(x, y)` touches the
/// line through `p` and `q` within the segment: the centre is `r` from the line, a quadratic
/// in the centre's height. `None` when the touching point is outside the segment (a corner
/// is then the contact) or the ball does not reach the line.
fn ball_edge(p: Vec3, q: Vec3, x: f64, y: f64, r: f64) -> Option<f64> {
    let e = q - p;
    let len = e.len();
    let u = e.normalized()?;
    let a = 1.0 - u.z * u.z;
    if a <= 1e-12 {
        return None;
    }
    let w0 = Vec3::new(x - p.x, y - p.y, -p.z);
    let wu = w0.dot(u);
    let b = 2.0 * (w0.z - wu * u.z);
    let c = w0.len2() - wu * wu - r * r;
    let disc = b * b - 4.0 * a * c;
    if !(disc >= 0.0) {
        return None;
    }
    let zc = (-b + disc.sqrt()) / (2.0 * a);
    let s = wu + zc * u.z;
    (s >= 0.0 && s <= len).then_some(zc - r)
}

/// The highest tip touching the edge from `p` to `q`; `None` when the cutter does not reach
/// it or, for a bull nose, when no tip touching it can be above `floor`.
fn drop_edge(cutter: &Cutter, p: Vec3, q: Vec3, x: f64, y: f64, r: f64, floor: f64) -> Option<f64> {
    let (ex, ey, ez) = (q.x - p.x, q.y - p.y, q.z - p.z);
    let a = ex * ex + ey * ey;
    if a <= 1e-24 {
        // A vertical edge: its top corner is the highest point of it the cutter can meet.
        return None;
    }
    let (wx, wy) = (p.x - x, p.y - y);
    let b = 2.0 * (ex * wx + ey * wy);
    let c = wx * wx + wy * wy - r * r;
    let disc = b * b - 4.0 * a * c;
    if !(disc >= 0.0) {
        return None;
    }
    let root = disc.sqrt();
    let lo = ((-b - root) / (2.0 * a)).max(0.0);
    let hi = ((-b + root) / (2.0 * a)).min(1.0);
    if !(lo <= hi) {
        return None;
    }
    let f = |t: f64| {
        let d = (wx + ex * t).hypot(wy + ey * t);
        // The ends of the stretch are where `d` reaches the radius; rounding may put them a
        // hair outside it.
        let h = cutter.height(d.min(r)).unwrap_or(cutter.corner);
        p.z + ez * t - h
    };
    if let Some(cone) = cutter.cone {
        return Some(cone_edge(&f, cone, Vec3::new(ex, ey, ez), (wx, wy), (lo, hi)).max(f(lo)).max(f(hi)));
    }
    if cutter.corner <= 0.0 {
        return Some(f(lo).max(f(hi)));
    }
    if cutter.is_ball() {
        // The ends of the stretch are corners or rim points, which the corner test and `f`
        // cover; the closed form gives the inside.
        return Some(ball_edge(p, q, x, y, r).unwrap_or(f64::NEG_INFINITY).max(f(lo)).max(f(hi)));
    }
    // Every point of the stretch is at least as far from the axis as the segment's nearest
    // point and no higher than the stretch's higher end, and the cutter's height rises with
    // distance, so no tip touching it is above that end less the height at the nearest
    // distance. The slack (far beyond rounding) keeps it a true bound on what the search
    // computes.
    let t0 = (-(ex * wx + ey * wy) / a).clamp(0.0, 1.0);
    let near = (wx + ex * t0).hypot(wy + ey * t0);
    let slack = 1e-9 * (1.0 + wx.abs() + wy.abs() + ex.abs() + ey.abs());
    let lift = cutter.height((near - slack).max(0.0).min(r)).unwrap_or(0.0);
    let bounded = (p.z + ez * lo).max(p.z + ez * hi) + 1e-9 * (1.0 + p.z.abs() + q.z.abs()) - lift <= floor;
    #[cfg(test)]
    let bounded = bounded && !NO_EDGE_BOUND.with(Cell::get);
    if bounded {
        return None;
    }
    #[cfg(test)]
    EDGE_SEARCHES.with(|n| n.set(n.get() + 1));
    let g = 0.5 * (5f64.sqrt() - 1.0);
    let (mut a0, mut b0) = (lo, hi);
    let (mut x1, mut x2) = (b0 - g * (b0 - a0), a0 + g * (b0 - a0));
    let (mut f1, mut f2) = (f(x1), f(x2));
    for _ in 0..EDGE_STEPS {
        if f1 < f2 {
            a0 = x1;
            (x1, f1) = (x2, f2);
            x2 = a0 + g * (b0 - a0);
            f2 = f(x2);
        } else {
            b0 = x2;
            (x2, f2) = (x1, f1);
            x1 = b0 - g * (b0 - a0);
            f1 = f(x1);
        }
    }
    Some(f(0.5 * (a0 + b0)).max(f(lo)).max(f(hi)))
}

/// The highest of `f`, the tip height touching an edge at its parameter for a cutter tapered as
/// `cone`, inside the stretch `[lo, hi]` under the cutter, in closed form. `e` is the edge from
/// its start, `w` its start from the axis seen from above. Along the edge the distance from the
/// axis is `√(u² + D²)`, `u` the distance along the edge from its point nearest the axis and
/// `D` the distance of its line from the axis. Over the flat tip `f` is linear, so its highest
/// there is where the tip's rim crosses the edge; over the side it is `m·u − √(u² + D²) / tan`
/// and a constant (`m` the edge's slope), highest where `u / √(u² + D²) = m·tan`. `f` is
/// concave, so the highest of these points, each kept within the stretch, and of the stretch's
/// ends is its maximum.
fn cone_edge(f: &impl Fn(f64) -> f64, cone: crate::cutter::Cone, e: Vec3, w: (f64, f64), (lo, hi): (f64, f64)) -> f64 {
    let a = e.x * e.x + e.y * e.y;
    let len = a.sqrt();
    let clamp = |t: f64| if t.is_finite() { t.clamp(lo, hi) } else { lo };
    let t0 = -(e.x * w.0 + e.y * w.1) / a;
    let dd = (w.0 + e.x * t0).hypot(w.1 + e.y * t0);
    let mut best = f64::NEG_INFINITY;
    let rt = cone.tip_radius;
    if rt > dd {
        let u = (rt * rt - dd * dd).max(0.0).sqrt();
        best = best.max(f(clamp(t0 - u / len))).max(f(clamp(t0 + u / len)));
    }
    let k = e.z / len * cone.half_angle.tan();
    if k.abs() < 1.0 {
        let u = dd * k / (1.0 - k * k).sqrt();
        best = best.max(f(clamp(t0 + u / len)));
    }
    best
}

/// Whether `(x, y)` is inside the triangle's outline seen from above (edges included).
fn inside_xy(t: &[Vec3; 3], x: f64, y: f64) -> bool {
    // Measured from the first corner, so the slack scales with the triangle, not with how far
    // it is from the origin.
    let o = t[0];
    let side = |a: Vec3, b: Vec3| ((b.x - o.x) - (a.x - o.x)) * ((y - o.y) - (a.y - o.y)) - ((b.y - o.y) - (a.y - o.y)) * ((x - o.x) - (a.x - o.x));
    let (s0, s1, s2) = (side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0]));
    let longest = [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])].iter().map(|(a, b)| (b.x - a.x).hypot(b.y - a.y)).fold(0.0, f64::max);
    let scale = 1e-12 * longest * longest;
    (s0 >= -scale && s1 >= -scale && s2 >= -scale) || (s0 <= scale && s1 <= scale && s2 <= scale)
}

/// The tip height at which `cutter`, its axis vertical through `(x, y)`, first touches the
/// surface coming down from above; `None` when no triangle is under it.
pub fn drop_z(surface: &Surface, cutter: &Cutter, x: f64, y: f64) -> Option<f64> {
    drop_z_from(surface, cutter, x, y, f64::NEG_INFINITY)
}

/// [`drop_z`] to the last bit, for any `hint`: a guess at the tip height (such as one a
/// screen worked out in single precision) that, when it is a little below the answer, lets
/// every triangle that cannot reach above it be passed over unopened. The answer never rests
/// on the hint: when no triangle reaches above it, the drop is taken again without it.
pub fn drop_z_from(surface: &Surface, cutter: &Cutter, x: f64, y: f64, hint: f64) -> Option<f64> {
    let r = cutter.radius;
    // The surface straight under the axis touches the cutter's centre, so the tip stops at
    // least that high: starting from it lets most triangles be passed over unopened.
    let top = surface.bounds().max.z + 1.0;
    let under = surface.raycast(Vec3::new(x, y, top), Vec3::new(0.0, 0.0, -1.0)).map_or(f64::NEG_INFINITY, |(t, _)| top - t);
    // A hint no higher than `under` changes nothing; above it, only a tip strictly above the
    // hint is kept, and that tip is the highest of all (every triangle passed over is no higher
    // than the hint), worked out by the same arithmetic as without the hint.
    let start = if hint.is_finite() && hint > under { hint } else { under };
    let best = Cell::new(start);
    // Every point of a box (or triangle) is at least its outline's distance from the axis, and
    // the cutter's height rises with distance, so no tip touching it is above its top less the
    // height at that distance (the slack, far beyond rounding, keeps the bound true).
    let reaches = |x0: f64, x1: f64, y0: f64, y1: f64, z1: f64| {
        let d = (x0 - x).max(x - x1).max(0.0).hypot((y0 - y).max(y - y1).max(0.0));
        let slack = 1e-9 * (1.0 + x.abs() + y.abs() + z1.abs() + r);
        match cutter.height((d - slack).max(0.0)) {
            Some(h) => z1 + slack - h > best.get(),
            None => false,
        }
    };
    surface.visit(
        |b| reaches(b.min.x, b.max.x, b.min.y, b.max.y, b.max.z),
        |_, t| {
            let lo = |f: fn(&Vec3) -> f64| f(&t[0]).min(f(&t[1])).min(f(&t[2]));
            let hi = |f: fn(&Vec3) -> f64| f(&t[0]).max(f(&t[1])).max(f(&t[2]));
            if !reaches(lo(|p| p.x), hi(|p| p.x), lo(|p| p.y), hi(|p| p.y), hi(|p| p.z)) {
                return;
            }
            if let Some(z) = drop_triangle_above(cutter, t, x, y, best.get())
                && z > best.get()
            {
                best.set(z);
            }
        },
    );
    let z = best.get();
    if start > under && !(z > start) {
        // Nothing reaches above the hint: it was too high, so the drop is taken without it.
        return drop_z_from(surface, cutter, x, y, f64::NEG_INFINITY);
    }
    z.is_finite().then_some(z)
}

/// Where the tip stops when `cutter`, its axis along the unit-or-not direction `axis` through
/// `point` (the tool body on the `+axis` side), first touches the surface coming in from
/// `+axis` far away: the distance along the unit axis from `point` (negative: behind it).
/// `None` when nothing is in the cutter's way.
pub fn drop_along(surface: &Surface, cutter: &Cutter, point: Vec3, axis: Vec3) -> Result<Option<f64>> {
    drop_along_above(surface, cutter, point, axis, f64::NEG_INFINITY)
}

#[cfg(test)]
thread_local! {
    /// Triangles [`drop_along_above`] tested on this thread: the work a check does.
    pub(crate) static DROP_TRIANGLES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// [`drop_along`] where only a stop above `floor` (along the axis from `point`) is wanted:
/// the same stop when it is above `floor`, else `None`. What lies wholly at or below `floor`
/// is not looked at: whether a cutter is in the surface (a stop above its tip, `floor` 0)
/// does not look down the whole column under it.
pub(crate) fn drop_along_above(surface: &Surface, cutter: &Cutter, point: Vec3, axis: Vec3, floor: f64) -> Result<Option<f64>> {
    let axis = unit(axis, "the tool axis")?;
    if !point.is_finite() {
        return invalid("the point the tool is dropped through must be finite");
    }
    let frame = Frame::from_z(point, axis)?;
    let bounds = surface.bounds();
    if bounds.is_empty() {
        return Ok(None);
    }
    let reach = point.dist(bounds.center()) + bounds.diagonal() + cutter.radius + 1.0;
    let (a, b) = (point - axis * reach, point + axis * reach);
    let r = cutter.radius;
    // As in `drop_z`, the surface on the axis line is a height the tip reaches at least.
    let under = surface.raycast(b, -axis).map_or(f64::NEG_INFINITY, |(t, _)| reach - t);
    let best = Cell::new(under.max(floor));
    surface.visit(
        |bx| {
            // The farthest the box reaches along the axis: nothing in it can stop the tool
            // higher than that.
            let c = bx.center() - point;
            let h = bx.size() * 0.5;
            let top = axis.dot(c) + axis.x.abs() * h.x + axis.y.abs() * h.y + axis.z.abs() * h.z;
            top > best.get() && capsule_meets_box(a, b, r, bx)
        },
        |_, t| {
            #[cfg(test)]
            DROP_TRIANGLES.with(|n| n.set(n.get() + 1));
            let local = t.map(|p| frame.to_local(p));
            if local.iter().all(|p| p.z <= best.get()) {
                return;
            }
            if let Some(z) = drop_triangle_above(cutter, &local, 0.0, 0.0, best.get())
                && z > best.get()
            {
                best.set(z);
            }
        },
    );
    let z = best.get();
    Ok((z.is_finite() && z > floor).then_some(z))
}
