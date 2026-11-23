//! Certified meetings of two quadrics and a plane, including a family of edge sections.
//!
//! Not upstream. Krawczyk's operator (Krawczyk 1969; Moore 1977) proves a displacement
//! `d` with `Q_a(p + d) = Q_b(p + d) = 0` and `n.d = h`. On an edge, `p` and `n` are
//! the old point and tangent over an entire parameter piece, and `h = 0`. At a corner
//! they are the vertex and the third face's normal, with `h = n.(origin - p)`.
//! Newton supplies only the centre: acceptance is strict interval inclusion, for every
//! section in the piece, followed by both cones' nappe checks. A tangent, separated or
//! otherwise unproved meeting is refused. No recursion; the shared solvers have fixed caps.

use super::{mean_value, Di, Dv, Implicit, Ivl, Kernel, Result, Vec3, LOCAL_OP};
use crate::geom::expansion as exact;
use crate::geom::krawczyk::{inverse_of_columns3, krawczyk, newton};

type I3 = [Ivl; 3];

fn values(x: Dv) -> I3 {
    [x.x.v, x.y.v, x.z.v]
}

fn enclosed(x: I3) -> Dv {
    let [x, y, z] = x.map(|v| Di {
        v,
        d: Ivl::from(0.0),
    });
    Dv { x, y, z }
}

/// `1 - axis.axis` (that is, `−δ` for `|axis|² = 1 + δ`), without losing its small value to the rounding of a unit norm.
/// Exact products are accumulated as an expansion; out-of-range products retain their
/// rounded enclosure. Zero is kept exact, including cardinal axes.
pub(super) fn norm_defect(axis: Vec3) -> Option<Ivl> {
    let mut expansion = vec![1.0];
    let mut remainder = None;
    for a in [axis.x, axis.y, axis.z] {
        if !a.is_finite() {
            return None;
        }
        if let Some(product) = exact::product(a, a) {
            expansion = exact::sub(&expansion, &product);
        } else {
            let squared = Ivl::from(a) * Ivl::from(a);
            remainder = Some(remainder.unwrap_or_else(|| Ivl::from(0.0)) - squared);
        }
    }
    let mut terms = expansion.into_iter().filter(|&v| v != 0.0);
    let first = terms.next().map_or_else(|| Ivl::from(0.0), Ivl::from);
    let defect = terms.fold(first, |sum, v| sum + Ivl::from(v));
    let defect = remainder.map_or(defect, |r| defect + r);
    defect.is_finite().then_some(defect)
}

/// The polynomial used by `implicit::lies_on`, its exact gradient, and the cone radius.
/// The cone is `v.v - z² - (r + tz)²`: this does not assume that a stored unit axis has
/// exactly unit squared length. All arithmetic, including its parameter derivative, is
/// rounded outward.
fn evaluate(q: &Implicit, x: Dv) -> Option<(Di, Dv, Di)> {
    let two = Di::constant(2.0);
    match *q {
        Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        } => {
            let v = x.sub(Dv::constant(origin));
            let a = Dv::constant(axis);
            let z = v.dot(a);
            let t = Di::constant(tan_angle);
            let r = Di::constant(radius) + t * z;
            let pv = v.sub(a.scale(z));
            // Algebraically v.v - z², even when axis.axis is only nearly 1.
            // The projected form avoids subtracting two large axial squares. For an
            // exactly unit axis these are the previous net solver's operations, bit for bit.
            let defect = norm_defect(axis)?;
            let base = pv.dot(pv) - r * r;
            let f = if defect.lo == 0.0 && defect.hi == 0.0 {
                base
            } else {
                base + Di {
                    v: defect,
                    d: Ivl::from(0.0),
                } * z
                    * z
            };
            Some((f, pv.scale(two).sub(a.scale(two * r * t)), r))
        }
        Implicit::Sphere { centre, radius } => {
            let v = x.sub(Dv::constant(centre));
            let r = Di::constant(radius);
            Some((v.dot(v) - r * r, v.scale(two), Di::constant(1.0)))
        }
        _ => None,
    }
}

/// The shared interval value and gradient of a cone, cylinder or sphere.
pub(super) fn value_and_gradient(q: &Implicit, x: I3) -> Option<(Ivl, I3)> {
    evaluate(q, enclosed(x)).map(|(f, g, _)| (f.v, values(g)))
}

/// The radical plane of two cylinders on one axis direction: where their implicit
/// functions are equal. For the axis `a` (the second's the same, or its exact negation)
/// and `P v = v − a (a·v)`, each cylinder is `|P (x − o)|² − r² = 0`, and `|P v|² =
/// v·v − (2 − a·a)(a·v)²` has the same quadratic part for both, so their difference is
/// affine: `2 (x − o₁)·n − (r₁² − r₂² + |P w|²)` with `w = o₂ − o₁` and
/// `n = w − (2 − a·a) a (a·w)`. The cylinders meet exactly where the first meets this plane,
/// so their meeting with a third plane, or in an edge's normal section, is a plane's with a
/// quadric ([`super::line_with_quadric`], [`super::trace_quadric_piece`]): in closed form, a
/// tangent meeting (a round tangent to a bore) included, which Krawczyk's operator cannot
/// prove (the two gradients are parallel there). The normal and the height are enclosed
/// in rounded interval arithmetic, so the plane is the exact one.
///
/// A cone and a cylinder on one axis line (a countersink's cone and the hole or the well
/// beside it) meet on a plane too, square to the axis. With `v = x − o₁` from the cone's
/// origin and `z = a·v`, both functions are `v·v − z² − ρ(z)²` (the cone's `ρ = r₁ + t z`,
/// the cylinder's `ρ = r₂` about `o₂`), and their difference is
/// `−(r₁ + t z)² + r₂² + 2 v·n − w·n` with `w = o₂ − o₁` and `n = w − a (a·w)`. Where `n` is
/// zero exactly (the cylinder's origin on the cone's axis line, shown in error-free
/// arithmetic, [`on_one_line`]), it is
/// `r₂² − (r₁ + t z)²`, zero on the two planes `r₁ + t z = ±r₂`; the cone's own nappe
/// (`r₁ + t z ≥ 0`) is on `t a·(x − o₁) = r₂ − r₁` alone, a circle of radius `r₂` on the
/// cylinder, wherever the cone's apex lies. So the meeting is the cylinder's with that plane
/// ([`Radical::quadric`]), in closed form; Newton started at the old edge would converge on
/// the other nappe once the moved cone's apex passed the edge.
///
/// Not for two cones, a cone and a cylinder whose axis lines are not certified one, a
/// cylinder's radius below zero (or a cone's cylinder at radius zero), or axes that differ
/// in any bit: [`meet`] decides those.
#[derive(Clone, Copy)]
pub(super) struct Radical {
    origin: Vec3,
    normal: Dv,
    height: Di,
    /// The quadric the plane meets where the two do: the first of two cylinders, the
    /// cylinder of a cone and a cylinder.
    on: Implicit,
}

impl Radical {
    /// The radical plane of `a` and `b` (`None` for any other pair, above).
    pub fn of(a: &Implicit, b: &Implicit) -> Option<Self> {
        let (
            Implicit::Cone {
                origin: oa,
                axis: aa,
                radius: ra,
                tan_angle: ta,
            },
            Implicit::Cone {
                origin: ob,
                axis: ab,
                radius: rb,
                tan_angle: tb,
            },
        ) = (*a, *b)
        else {
            return None;
        };
        // the same stored axis line direction, exactly
        if !(ab == aa || ab == -aa) {
            return None;
        }
        let axis = Dv::constant(aa);
        let w = Dv::constant(ob).sub(Dv::constant(oa));
        let z = w.dot(axis);
        let pw = w.sub(axis.scale(z));
        match (ta == 0.0, tb == 0.0) {
            // two cylinders
            (true, true) => {
                if !(ra >= 0.0 && rb >= 0.0) {
                    return None;
                }
                let k = Di::constant(2.0) - axis.dot(axis);
                let (ra, rb) = (Di::constant(ra), Di::constant(rb));
                Some(Self {
                    origin: oa,
                    normal: w.sub(axis.scale(k * z)),
                    height: (ra * ra - rb * rb + pw.dot(pw)) * Di::constant(0.5),
                    on: *a,
                })
            }
            // a cone and a cylinder, the cone's origin `o₁` (`n` from the other origin is
            // its negation: zero the same)
            (false, true) => {
                Self::cone_and_cylinder(oa, axis, ra, ta, on_one_line(oa, ob, aa), rb, *b)
            }
            // the cone's own axis: the cylinder's may be the opposite sense
            (true, false) => Self::cone_and_cylinder(
                ob,
                Dv::constant(ab),
                rb,
                tb,
                on_one_line(oa, ob, aa),
                ra,
                *a,
            ),
            (false, false) => None,
        }
    }

    /// [`Radical::of`] a cone `(o, a, r1, t)` and the cylinder `cylinder` of radius `r2`,
    /// `one_line` whether their axis lines are shown one ([`on_one_line`]).
    fn cone_and_cylinder(
        o: Vec3,
        axis: Dv,
        r1: f64,
        t: f64,
        one_line: bool,
        r2: f64,
        cylinder: Implicit,
    ) -> Option<Self> {
        if !(r2 > 0.0 && one_line) {
            return None;
        }
        Some(Self {
            origin: o,
            normal: axis.scale(Di::constant(t)),
            height: Di::constant(r2) - Di::constant(r1),
            on: cylinder,
        })
    }

    /// The plane's normal and its height `n·(x − p)` over `p`.
    pub fn over(&self, p: Dv) -> (Dv, Di) {
        let height = self.height - p.sub(Dv::constant(self.origin)).dot(self.normal);
        (self.normal, height)
    }

    /// The quadric whose meeting with the plane is the two quadrics' meeting.
    pub fn quadric(&self) -> &Implicit {
        &self.on
    }
}

/// Whether `n = w − a (a·w)`, `w = ob − oa`, is zero exactly: computed in error-free
/// arithmetic, so `false` where it is not, or where a product is out of the range that
/// arithmetic holds exactly (not shown zero). In rounded intervals the two terms of a
/// component along the axis are each enclosed apart and their difference never encloses
/// zero alone, even for origins on one line.
fn on_one_line(oa: Vec3, ob: Vec3, a: Vec3) -> bool {
    let w = [
        exact::sub(&[ob.x], &[oa.x]),
        exact::sub(&[ob.y], &[oa.y]),
        exact::sub(&[ob.z], &[oa.z]),
    ];
    let axis = [a.x, a.y, a.z];
    let along = w.iter().zip(axis).try_fold(Vec::new(), |sum, (wi, ai)| {
        Some(exact::add(&sum, &exact::scale(wi, ai)?))
    });
    let Some(along) = along else {
        return false;
    };
    w.iter().zip(axis).all(|(wi, ai)| {
        exact::scale(&along, ai).is_some_and(|part| exact::is_zero(&exact::sub(wi, &part)))
    })
}

/// A plane through a displacement `d`, `normal.d = height`, based at `point`.
#[derive(Clone, Copy)]
pub(super) struct Section {
    pub point: Dv,
    pub normal: Dv,
    pub height: Di,
}

impl Section {
    /// A normal section of the old edge.
    pub fn edge((point, normal): (Dv, Dv)) -> Self {
        Self {
            point,
            normal,
            height: Di::constant(0.0),
        }
    }

    /// The third face's plane at a vertex.
    pub fn corner(point: Vec3, origin: Vec3, normal: Vec3) -> Self {
        let point = Dv::constant(point);
        let normal = Dv::constant(normal);
        Self {
            point,
            normal,
            height: Dv::constant(origin).sub(point).dot(normal),
        }
    }

    fn equations(self, a: &Implicit, b: &Implicit, d: Dv) -> Option<([Di; 3], [Dv; 3])> {
        let x = self.point.add(d);
        let (fa, ga, _) = evaluate(a, x)?;
        let (fb, gb, _) = evaluate(b, x)?;
        Some((
            [fa, fb, self.normal.dot(d) - self.height],
            [ga, gb, self.normal],
        ))
    }
}

/// A root's distance enclosure and whether both surfaces cross the section transversely.
pub(super) struct Meeting {
    pub distance: Ivl,
    pub transverse: bool,
}

/// Proves one meeting for every section in `over`. The same displacement box holds every
/// section's root. `mid` and `dt` give mean-value enclosures of the equations at the box's
/// centre; their rates hold the displacement fixed, as Krawczyk's value requires.
/// Failed convergence or a nappe not proved positive returns no meeting, never separation.
pub(super) fn meet(
    k: &Kernel,
    a: &Implicit,
    b: &Implicit,
    over: Section,
    mid: Section,
    dt: Ivl,
) -> Result<Option<Meeting>> {
    let at = |y: [f64; 3]| {
        k.check_cancelled(LOCAL_OP)?;
        let [x, y, z] = y;
        Ok(mid
            .equations(a, b, Dv::constant(Vec3::new(x, y, z)))
            .map(|(f, g)| {
                let [ga, gb, n] = g.map(|v| values(v).map(Ivl::mid));
                let [ax, ay, az] = ga;
                let [bx, by, bz] = gb;
                let [nx, ny, nz] = n;
                (
                    f.map(|v| v.v.mid()),
                    [[ax, bx, nx], [ay, by, ny], [az, bz, nz]],
                )
            }))
    };
    let Some(root) = newton([0.0; 3], &at, &inverse_of_columns3)? else {
        return Ok(None);
    };
    let Some((_, j)) = at(root)? else {
        return Ok(None);
    };
    let Some(c) = inverse_of_columns3(&j) else {
        return Ok(None);
    };
    let value = |y: I3| {
        k.check_cancelled(LOCAL_OP)?;
        let d = enclosed(y);
        Ok(over
            .equations(a, b, d)
            .zip(mid.equations(a, b, d))
            .map(|((f, _), (m, _))| {
                let [a, b, c] = f;
                let [x, y, z] = m;
                [
                    mean_value(a, x, dt),
                    mean_value(b, y, dt),
                    mean_value(c, z, dt),
                ]
            }))
    };
    let jacobian = |y: I3| {
        k.check_cancelled(LOCAL_OP)?;
        Ok(over.equations(a, b, enclosed(y)).map(|(_, g)| {
            let [a, b, n] = g.map(values);
            let [ax, ay, az] = a;
            let [bx, by, bz] = b;
            let [nx, ny, nz] = n;
            [[ax, bx, nx], [ay, by, ny], [az, bz, nz]]
        }))
    };
    let Some(root) = krawczyk(root, c, &value, &jacobian)? else {
        return Ok(None);
    };
    let d = enclosed(root);
    let x = over.point.add(d);
    let (Some((_, ga, ra)), Some((_, gb, rb))) = (evaluate(a, x), evaluate(b, x)) else {
        return Ok(None);
    };
    if !(ra.v.lo >= 0.0 && rb.v.lo >= 0.0) {
        return Ok(None);
    }
    let squared = root.into_iter().fold(Ivl::from(0.0), |sum, x| {
        let magnitude = Ivl::new(x.mig(), x.mag());
        sum + magnitude * magnitude
    });
    let distance = squared.sqrt();
    if !distance.is_finite() {
        return Ok(None);
    }
    let det = ga.cross(gb).dot(over.normal);
    Ok(Some(Meeting {
        distance,
        transverse: det.excludes_zero(),
    }))
}
