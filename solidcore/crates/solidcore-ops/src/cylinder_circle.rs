//! [`solidcore::surface::cylinder_circle`] on plain numbers.

use solidcore::geom::vector3d::Vec3;
use solidcore::surface::cylinder_circle as cc;

fn vec(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// A cylinder: a point of its axis, its unit axis and its radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cylinder {
    /// A point of the axis.
    pub origin: [f64; 3],
    /// The unit axis.
    pub axis: [f64; 3],
    /// The radius.
    pub radius: f64,
}

impl Cylinder {
    fn kernel(&self) -> cc::Cylinder {
        cc::Cylinder {
            origin: vec(self.origin),
            axis: vec(self.axis),
            radius: self.radius,
        }
    }

    /// How far `p` lies along the axis from [`Cylinder::origin`].
    #[must_use]
    pub fn height(&self, p: [f64; 3]) -> f64 {
        self.kernel().height(vec(p))
    }

    /// The point of the axis level with `p`.
    #[must_use]
    pub fn centre_at(&self, p: [f64; 3]) -> [f64; 3] {
        arr(self.kernel().centre_at(vec(p)))
    }

    /// The circle frame at `p`, `(x_axis, y_axis)` ([`cc::Cylinder::frame_at`]).
    #[must_use]
    pub fn frame_at(&self, p: [f64; 3]) -> ([f64; 3], [f64; 3]) {
        let (x, y) = self.kernel().frame_at(vec(p));
        (arr(x), arr(y))
    }

    /// Whether `p` lies within `tol` of the cylinder's circle at `height`.
    #[must_use]
    pub fn on_circle(&self, p: [f64; 3], height: f64, tol: f64) -> bool {
        self.kernel().on_circle(vec(p), height, tol)
    }

    /// Whether the circle is this cylinder's at `height` ([`cc::Cylinder::is_circle_at`]).
    #[must_use]
    pub fn is_circle_at(
        &self,
        origin: [f64; 3],
        x_axis: [f64; 3],
        y_axis: [f64; 3],
        radius: f64,
        height: f64,
        tol: f64,
    ) -> bool {
        self.kernel()
            .is_circle_at(vec(origin), vec(x_axis), vec(y_axis), radius, height, tol)
    }

    /// Whether `p` and `q` lie on one circle of the cylinder's: the same height within `tol`.
    #[must_use]
    pub fn same_circle(&self, p: [f64; 3], q: [f64; 3], tol: f64) -> bool {
        self.kernel().same_circle(vec(p), vec(q), tol)
    }

    /// Whether the line from `start` to `end` is a ruling ([`cc::Cylinder::is_ruling`]).
    #[must_use]
    pub fn is_ruling(&self, start: [f64; 3], end: [f64; 3], tol: f64) -> bool {
        self.kernel().is_ruling(vec(start), vec(end), tol)
    }

    /// The centre of the circle where the plane `normal · x = d`, square to the axis, cuts the
    /// cylinder, when `a` and `b` both lie on that circle within `tol`.
    #[must_use]
    pub fn plane_circle(
        &self,
        normal: [f64; 3],
        d: f64,
        a: [f64; 3],
        b: [f64; 3],
        tol: f64,
    ) -> Option<[f64; 3]> {
        self.kernel()
            .plane_circle(vec(normal), d, vec(a), vec(b), tol)
            .map(arr)
    }
    /// Whether `other` is this cylinder ([`cc::Cylinder::same_cylinder`]).
    #[must_use]
    pub fn same_cylinder(&self, other: &Cylinder, tol: f64) -> bool {
        self.kernel().same_cylinder(&other.kernel(), tol)
    }

    /// Whether `p` lies within `tol` of the cylinder's wall ([`cc::Cylinder::on_wall`]).
    #[must_use]
    pub fn on_wall(&self, p: [f64; 3], tol: f64) -> bool {
        self.kernel().on_wall(vec(p), tol)
    }

    /// Whether the line from `a` to `b` is a ruling ([`cc::Cylinder::runs_along_axis`]).
    #[must_use]
    pub fn runs_along_axis(&self, a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
        self.kernel().runs_along_axis(vec(a), vec(b), tol)
    }

    /// Where the line `p0 + u·t` meets the cylinder ([`cc::Cylinder::line_hits`]).
    #[must_use]
    pub fn line_hits(&self, p0: [f64; 3], u: [f64; 3], tol: f64) -> Vec<[f64; 3]> {
        self.kernel()
            .line_hits(vec(p0), vec(u), tol)
            .into_iter()
            .map(arr)
            .collect()
    }

    /// The centre and outward side of an edge round the cylinder ([`cc::Cylinder::round_side`]).
    #[must_use]
    pub fn round_side(
        &self,
        pts: &[[f64; 3]],
        a: [f64; 3],
        b: [f64; 3],
        tol: f64,
    ) -> Option<([f64; 3], [f64; 3])> {
        let pts: Vec<Vec3> = pts.iter().map(|p| vec(*p)).collect();
        self.kernel()
            .round_side(&pts, vec(a), vec(b), tol)
            .map(|(c, out)| (arr(c), arr(out)))
    }
}

/// The line where planes `n1·x = d1` and `n2·x = d2` meet ([`cc::plane_pair_line`]).
#[must_use]
pub fn plane_pair_line(
    n1: [f64; 3],
    d1: f64,
    n2: [f64; 3],
    d2: f64,
    parallel_sine: f64,
) -> Option<([f64; 3], [f64; 3])> {
    cc::plane_pair_line(vec(n1), d1, vec(n2), d2, parallel_sine).map(|(p0, u)| (arr(p0), arr(u)))
}

/// An arc about a centre: from its start along `x_axis`, through `sweep` radians towards
/// `y_axis`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    /// From the centre towards the start, unit.
    pub x_axis: [f64; 3],
    /// A quarter turn on from `x_axis`, unit.
    pub y_axis: [f64; 3],
    /// The radius.
    pub radius: f64,
    /// The angle swept, in (0, 2π].
    pub sweep: f64,
}

/// The arc from `a` to `b` about `centre` whose middle lies on `out`'s side ([`cc::arc_toward`]).
#[must_use]
pub fn arc_toward(
    a: [f64; 3],
    b: [f64; 3],
    centre: [f64; 3],
    axis: [f64; 3],
    out: [f64; 3],
    tol: f64,
) -> Option<Arc> {
    cc::arc_toward(vec(a), vec(b), vec(centre), vec(axis), vec(out), tol).map(|arc| Arc {
        x_axis: arr(arc.x_axis),
        y_axis: arr(arc.y_axis),
        radius: arc.radius,
        sweep: arc.sweep,
    })
}

/// The arc from `a` through `mid` to `b` about `centre` ([`cc::circle_arc_through`]).
#[must_use]
pub fn circle_arc_through(
    a: [f64; 3],
    mid: [f64; 3],
    b: [f64; 3],
    centre: [f64; 3],
    axis: [f64; 3],
    tol: f64,
) -> Option<Arc> {
    cc::circle_arc_through(vec(a), vec(mid), vec(b), vec(centre), vec(axis), tol).map(|arc| Arc {
        x_axis: arr(arc.x_axis),
        y_axis: arr(arc.y_axis),
        radius: arc.radius,
        sweep: arc.sweep,
    })
}
