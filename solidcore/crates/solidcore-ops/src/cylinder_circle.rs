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
