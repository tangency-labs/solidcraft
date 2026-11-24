//! The section of a surface of revolution (a cylinder, cone, sphere or torus) by a plane through
//! its axis, in closed form (not upstream, #17496, #17498, #17499, #17500): the plane meets the
//! surface in its meridians, the profile turned into the plane, which are lines for a cylinder
//! or a cone and circles for a sphere or a torus. A face of the surface keeps the pieces of them
//! between the points where its boundary crosses the plane that lie on the face.

use crate::geom::vector3d::{Point3, Vec3};
use crate::math::PI;

/// A surface of revolution, as a face's analytic surface gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Revolved {
    /// A cylinder: a point on its axis, the axis and the radius.
    Cylinder {
        /// A point on the axis.
        origin: Point3,
        /// The axis direction (any length).
        axis: Vec3,
        /// The radius.
        radius: f64,
    },
    /// A cone: its apex, its axis pointing into the cone and its half angle (radians).
    Cone {
        /// The apex.
        apex: Point3,
        /// The axis direction from the apex into the cone (any length).
        axis: Vec3,
        /// The angle between the axis and each line of the cone (radians).
        half_angle: f64,
    },
    /// A sphere: every plane through its centre holds its axis.
    Sphere {
        /// The centre.
        centre: Point3,
        /// The radius.
        radius: f64,
    },
    /// A torus: its centre, axis, ring (major) and tube (minor) radii.
    Torus {
        /// The centre of the ring.
        centre: Point3,
        /// The axis direction (any length).
        axis: Vec3,
        /// The radius of the circle the tube's centre runs on.
        major: f64,
        /// The tube's radius.
        minor: f64,
    },
}

/// A piece of an axial section, in the plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxialPiece {
    /// A segment of a line of a cylinder or a cone.
    Line {
        /// Where it starts.
        start: Point3,
        /// Where it ends.
        end: Point3,
    },
    /// An arc of a circle of a sphere or a torus, counter-clockwise about the plane's normal
    /// from `start` to `end`.
    Arc {
        /// The circle's centre.
        centre: Point3,
        /// Where it starts.
        start: Point3,
        /// Where it ends.
        end: Point3,
    },
    /// A whole circle of a sphere or a torus.
    Circle {
        /// The circle's centre.
        centre: Point3,
        /// Its radius.
        radius: f64,
    },
}

/// A meridian of the surface in the plane: a line `base + t dir` (from `t = 0` only for a
/// cone's, which starts at the apex), or the circle `centre + radius (cos θ e1 + sin θ e2)`.
#[derive(Clone, Copy, Debug)]
enum Meridian {
    Line {
        base: Point3,
        dir: Vec3,
        from_base: bool,
    },
    Circle {
        centre: Point3,
        radius: f64,
        e1: Vec3,
        e2: Vec3,
    },
}

impl Meridian {
    /// How far `p` lies from the meridian, and its parameter there.
    fn foot(&self, p: Point3) -> (f64, f64) {
        match *self {
            Meridian::Line {
                base,
                dir,
                from_base,
            } => {
                let t = (p - base).dot(dir);
                let t = if from_base { t.max(0.0) } else { t };
                ((base + dir * t).distance(p), t)
            }
            Meridian::Circle {
                centre,
                radius,
                e1,
                e2,
            } => {
                let d = p - centre;
                let (x, y) = (d.dot(e1), d.dot(e2));
                let theta = y.atan2(x).rem_euclid(2.0 * PI);
                let out_of_plane = d.dot(e1.cross(e2));
                (((x * x + y * y).sqrt() - radius).hypot(out_of_plane), theta)
            }
        }
    }

    /// The point at parameter `t`.
    fn at(&self, t: f64) -> Point3 {
        match *self {
            Meridian::Line { base, dir, .. } => base + dir * t,
            Meridian::Circle {
                centre,
                radius,
                e1,
                e2,
            } => centre + (e1 * t.cos() + e2 * t.sin()) * radius,
        }
    }

    /// How long the meridian is from parameter `a` to `b`.
    fn length(&self, a: f64, b: f64) -> f64 {
        match *self {
            Meridian::Line { .. } => (b - a).abs(),
            Meridian::Circle { radius, .. } => radius * (b - a).abs(),
        }
    }
}

/// The meridians of `surface` in the plane through `plane_point` with unit normal `n`, when the
/// plane holds the axis within `tol` wherever it passes the points `extent` (the face's box:
/// along the axis as far as they reach, wherever the surface's own point is), or at its point
/// when there are none (a sphere's: when it holds the centre); `None` otherwise. The axis is taken into the plane, so the meridians lie in it.
fn meridians(
    surface: &Revolved,
    plane_point: Point3,
    n: Vec3,
    extent: &[Point3],
    tol: f64,
) -> Option<Vec<Meridian>> {
    let into_plane = |p: Point3| p - n * (p - plane_point).dot(n);
    let on_plane = |p: Point3| (p - plane_point).dot(n).abs() <= tol;
    // The axis through `o`, in the plane: its point and its unit direction there, and the unit
    // direction square to it in the plane (counter-clockwise from it about `n`).
    let axial = |o: Point3, axis: Vec3| -> Option<(Point3, Vec3, Vec3)> {
        let a = axis.unitize().ok()?;
        // The axis's height above the plane changes linearly along it, so it is greatest at
        // one of the points of the axis abreast of the extent's farthest points each way.
        let along: Vec<f64> = extent.iter().map(|p| (*p - o).dot(a)).collect();
        let lo = along.iter().copied().reduce(f64::min).unwrap_or(0.0);
        let hi = along.iter().copied().reduce(f64::max).unwrap_or(0.0);
        if !(on_plane(o + a * lo) && on_plane(o + a * hi)) {
            return None;
        }
        let e1 = (a - n * a.dot(n)).unitize().ok()?;
        Some((into_plane(o), e1, n.cross(e1)))
    };
    let pair = |make: &dyn Fn(f64) -> Meridian| vec![make(1.0), make(-1.0)];
    Some(match *surface {
        Revolved::Cylinder {
            origin,
            axis,
            radius,
        } => {
            let (o, e1, e2) = axial(origin, axis)?;
            pair(&|s| Meridian::Line {
                base: o + e2 * (s * radius),
                dir: e1,
                from_base: false,
            })
        }
        Revolved::Cone {
            apex,
            axis,
            half_angle,
        } => {
            let (o, e1, e2) = axial(apex, axis)?;
            let (sin, cos) = half_angle.sin_cos();
            pair(&|s| Meridian::Line {
                base: o,
                dir: e1 * cos + e2 * (s * sin),
                from_base: true,
            })
        }
        Revolved::Sphere { centre, radius } => {
            if !on_plane(centre) {
                return None;
            }
            let [_, e1, _] = n.make_unit_ortho_vectors(None).ok()?;
            vec![Meridian::Circle {
                centre: into_plane(centre),
                radius,
                e1,
                e2: n.cross(e1),
            }]
        }
        Revolved::Torus {
            centre,
            axis,
            major,
            minor,
        } => {
            let (o, e1, e2) = axial(centre, axis)?;
            pair(&|s| Meridian::Circle {
                centre: o + e2 * (s * major),
                radius: minor,
                e1,
                e2,
            })
        }
    })
}

/// The pieces of one meridian between `cuts` (parameters, sorted), each with whether it lies
/// on the face (`on_face` at its middle), joined where neighbours both do.
fn trimmed<E>(
    m: &Meridian,
    cuts: &[f64],
    on_face: &mut impl FnMut(Point3) -> Result<bool, E>,
) -> Result<Vec<AxialPiece>, E> {
    // The spans between the cuts: on a line, between neighbours (a face is bounded, so it ends
    // at a cut each way); on a circle, round it from each to the next.
    let mut spans: Vec<(f64, f64)> = cuts
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some((*a, *b)),
            _ => None,
        })
        .collect();
    let circle = matches!(m, Meridian::Circle { .. });
    if circle {
        match (cuts.first(), cuts.last()) {
            (Some(first), Some(last)) => spans.push((*last, *first + 2.0 * PI)),
            _ => spans.push((0.0, 2.0 * PI)),
        }
    }
    let mut kept = Vec::new();
    for &(a, b) in &spans {
        kept.push(on_face(m.at(0.5 * (a + b)))?);
    }
    // Joined runs of kept spans; on a circle a run may wrap past the last cut to the first.
    let mut runs: Vec<(f64, f64)> = Vec::new();
    let mut open: Option<(f64, f64)> = None;
    for (&(a, b), &keep) in spans.iter().zip(&kept) {
        match (keep, open.as_mut()) {
            (true, Some(run)) => run.1 = b,
            (true, None) => open = Some((a, b)),
            (false, _) => runs.extend(open.take()),
        }
    }
    let whole = circle && !kept.is_empty() && kept.iter().all(|k| *k);
    if let Some(run) = open {
        match runs.first_mut() {
            Some(first) if circle && kept.first() == Some(&true) => first.0 = run.0 - 2.0 * PI,
            _ => runs.push(run),
        }
    }
    Ok(match *m {
        Meridian::Circle { centre, radius, .. } if whole => {
            vec![AxialPiece::Circle { centre, radius }]
        }
        Meridian::Circle { centre, .. } => runs
            .into_iter()
            .map(|(a, b)| AxialPiece::Arc {
                centre,
                start: m.at(a),
                end: m.at(b),
            })
            .collect(),
        Meridian::Line { .. } => runs
            .into_iter()
            .map(|(a, b)| AxialPiece::Line {
                start: m.at(a),
                end: m.at(b),
            })
            .collect(),
    })
}

/// Whether the plane through `plane_point` square to `plane_normal` (any length) holds the axis
/// of `surface` within `tol` (the model's tolerance) wherever the axis passes the points
/// `extent` (the corners of a box around the face), or for a sphere its centre: whether
/// [`axial_section`] sections a face there. `false` for a number that is not finite or a normal
/// of no length.
#[must_use]
pub fn holds_axis(
    surface: &Revolved,
    plane_point: Point3,
    plane_normal: Vec3,
    extent: &[Point3],
    tol: f64,
) -> bool {
    usable(surface, plane_point, extent, tol)
        && plane_normal
            .unitize()
            .is_ok_and(|n| meridians(surface, plane_point, n, extent, tol).is_some())
}

/// Whether every number is finite and `tol` positive.
fn usable(surface: &Revolved, plane_point: Point3, points: &[Point3], tol: f64) -> bool {
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    let numbers_ok = match *surface {
        Revolved::Cylinder {
            origin: p,
            axis,
            radius: r,
        }
        | Revolved::Cone {
            apex: p,
            axis,
            half_angle: r,
        } => finite(p) && finite(axis) && r.is_finite(),
        Revolved::Sphere { centre, radius } => finite(centre) && radius.is_finite(),
        Revolved::Torus {
            centre,
            axis,
            major,
            minor,
        } => finite(centre) && finite(axis) && major.is_finite() && minor.is_finite(),
    };
    numbers_ok
        && finite(plane_point)
        && points.iter().all(|p| finite(*p))
        && tol.is_finite()
        && tol > 0.0
}

/// The section of a face on `surface` by the plane through `plane_point` square to
/// `plane_normal` (any length), when the plane holds the surface's axis ([`holds_axis`] over
/// `extent`, the corners of a box around the face). `cuts` are the points where the face's boundary crosses the plane;
/// each lies on a meridian, which it splits. A piece between cuts is the face's when
/// `on_face` says its middle is. Lines start and end at the cuts' feet on them, so on the
/// face's edges; arcs run counter-clockwise about the normal; a circle the face keeps all of
/// is whole. `Ok(None)` when the plane does not hold the axis, or for a number that is not
/// finite or a normal of no length; `Ok(Some(vec![]))` when the plane misses the face.
///
/// # Errors
///
/// `on_face`'s error.
pub fn axial_section<E>(
    surface: &Revolved,
    plane_point: Point3,
    plane_normal: Vec3,
    extent: &[Point3],
    tol: f64,
    cuts: &[Point3],
    mut on_face: impl FnMut(Point3) -> Result<bool, E>,
) -> Result<Option<Vec<AxialPiece>>, E> {
    if !(usable(surface, plane_point, extent, tol) && usable(surface, plane_point, cuts, tol)) {
        return Ok(None);
    }
    let Ok(n) = plane_normal.unitize() else {
        return Ok(None);
    };
    let Some(lines) = meridians(surface, plane_point, n, extent, tol) else {
        return Ok(None);
    };
    // Each cut on the meridian it lies on (the nearest: every point of the boundary is on the
    // surface, so where it crosses the plane is on a meridian).
    let mut on: Vec<Vec<f64>> = vec![Vec::new(); lines.len()];
    for p in cuts {
        let nearest = lines
            .iter()
            .map(|m| m.foot(*p))
            .enumerate()
            .min_by(|(_, a), (_, b)| a.0.total_cmp(&b.0));
        if let Some((i, (_, t))) = nearest {
            if let Some(ts) = on.get_mut(i) {
                ts.push(t);
            }
        }
    }
    let mut out = Vec::new();
    for (m, ts) in lines.iter().zip(&mut on) {
        if let Meridian::Line {
            from_base: true, ..
        } = m
        {
            ts.push(0.0);
        }
        ts.sort_by(f64::total_cmp);
        // Cuts within the tolerance of each other along the meridian are one (an edge's end
        // and its neighbour's start, a vertex).
        ts.dedup_by(|b, a| m.length(*a, *b) <= tol);
        if let (Meridian::Circle { .. }, Some(first), Some(last)) = (m, ts.first(), ts.last()) {
            if ts.len() > 1 && m.length(*last, *first + 2.0 * PI) <= tol {
                ts.pop();
            }
        }
        out.extend(trimmed(m, ts, &mut on_face)?);
    }
    Ok(Some(out))
}
