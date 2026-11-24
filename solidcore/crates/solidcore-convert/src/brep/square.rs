//! A record's frames and planes whose axes are a little off square, squared as it is read
//! ([`solidcore::eval::square`]): a plane keeps its u axis and its points, its faces' pcurves and
//! box sheared to the square axes; a frame keeps its x axis and its z. Records written before
//! the writer squared a plane's axes (tangency-labs/solidcraft#21509) then open, and their faces and edges are where
//! they were.
//!
//! Only what a reader would refuse is touched ([`square::frame_is_square`],
//! [`square::plane_is_square`]): an edge's circle or ellipse, a plane, and a cylinder, cone,
//! sphere or torus face without a NURBS form (with one, its label is taken as stored and kept
//! only where the form proves it, so it opens already). Any other record is read to the bit as
//! before. Axes that cannot
//! be squared (of no length, not finite, parallel) are left for the reader to refuse with its
//! reason.

use solidcore::eval::square;
use solidcore_record::brep as r;

/// Where a squared frame or plane is in the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SquaredAt {
    /// The surface of the face with this index.
    Face(usize),
    /// The curve of the edge with this index.
    Edge(usize),
}

/// One frame or plane squared on read, and how far off square it was (the cosine between a
/// plane's axes, or a frame's [`square::frame_deviation`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Squared {
    /// Where it is.
    pub at: SquaredAt,
    /// How far off square it was.
    pub deviation: f64,
}

impl Squared {
    /// Whether it was further off square than [`square::SQUARE_TOLERANCE`]: squared all the
    /// same, but worth a warning.
    #[must_use]
    pub fn worth_a_warning(&self) -> bool {
        self.deviation > square::SQUARE_TOLERANCE || self.deviation.is_nan()
    }
}

fn frame_needs(f: &r::Frame) -> bool {
    !square::frame_is_square(f.x_axis, f.y_axis)
        && square::square_frame(f.x_axis, f.y_axis).is_some()
}

fn curve_needs(c: &r::Curve) -> bool {
    match c {
        r::Curve::Circle { frame, .. } | r::Curve::Ellipse { frame, .. } => frame_needs(frame),
        _ => false,
    }
}

fn plane_axes(s: &r::Surface) -> Option<([f64; 3], [f64; 3])> {
    match s {
        r::Surface::Plane { u_axis, v_axis, .. } => Some((*u_axis, *v_axis)),
        r::Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => Some((
            core::array::from_fn(|i| u_point[i] - origin[i]),
            core::array::from_fn(|i| v_point[i] - origin[i]),
        )),
        _ => None,
    }
}

/// A face's label read through its own numbers: a cylinder, cone, sphere or torus without a
/// NURBS form (with one, the label is taken as stored and only kept where the form proves it),
/// or a plane.
fn surface_needs(f: &r::Face) -> bool {
    match &f.surface {
        r::Surface::Cylinder { frame, .. }
        | r::Surface::Cone { frame, .. }
        | r::Surface::Sphere { frame, .. }
        | r::Surface::Torus { frame, .. } => f.nurbs.is_none() && frame_needs(frame),
        s => plane_axes(s).is_some_and(|(du, dv)| {
            !square::plane_is_square(du, dv) && square::square_plane(du, dv).is_some()
        }),
    }
}

/// Whether [`square_axes`] would change `b`: a frame or plane in it a reader refuses as off
/// square, and that can be squared.
#[must_use]
pub fn needs_squaring(b: &r::Brep) -> bool {
    b.edges
        .iter()
        .any(|e| curve_needs(&e.curve) || e.nurbs.as_ref().is_some_and(curve_needs))
        || b.faces.iter().any(surface_needs)
}

/// `frame` squared, with how far off it was; `None` when it needs nothing or cannot be squared.
fn square_frame(frame: &mut r::Frame) -> Option<f64> {
    if !frame_needs(frame) {
        return None;
    }
    let deviation = square::frame_deviation(frame.x_axis, frame.y_axis);
    let (x, y) = square::square_frame(frame.x_axis, frame.y_axis)?;
    frame.x_axis = x;
    frame.y_axis = y;
    Some(deviation)
}

fn square_curve(c: &mut r::Curve) -> Option<f64> {
    match c {
        r::Curve::Circle { frame, .. } | r::Curve::Ellipse { frame, .. } => square_frame(frame),
        _ => None,
    }
}

/// Shear a pcurve's points `(a, b)` to `(a + s b, b)` ([`square::shear_uv`]); homogeneous
/// points the same way, as the map is linear.
fn shear_pcurve(c: &mut r::Curve2, s: f64) {
    match c {
        r::Curve2::Spline { points, .. } => {
            for p in points {
                let (a, b) = square::shear_uv(s, p[0], p[1]);
                *p = [a, b];
            }
        }
        r::Curve2::RationalSpline { homogeneous, .. } => {
            for h in homogeneous {
                let (a, b) = square::shear_uv(s, h[0], h[1]);
                h[0] = a;
                h[1] = b;
            }
        }
    }
}

/// A face's plane squared, its pcurves and box sheared when they are in the plane's own
/// parameters (no NURBS form); `None` when it needs nothing or cannot be squared.
fn square_plane_face(f: &mut r::Face) -> Option<f64> {
    let (du, dv) = plane_axes(&f.surface)?;
    if square::plane_is_square(du, dv) {
        return None;
    }
    let skew = square::plane_skew(du, dv);
    let (w, s) = square::square_plane(du, dv)?;
    let origin = match &f.surface {
        r::Surface::Plane { origin, .. } | r::Surface::PlaneThrough { origin, .. } => *origin,
        _ => return None,
    };
    f.surface = r::Surface::Plane {
        origin,
        u_axis: du,
        v_axis: w,
    };
    if f.nurbs.is_none() {
        f.domain = square::shear_box(s, f.domain);
        for l in &mut f.loops {
            if let r::Loop::Edges { edges } = l {
                for u in edges {
                    shear_pcurve(&mut u.pcurve, s);
                }
            }
        }
    }
    Some(skew)
}

/// Square every frame and plane of `b` a reader refuses as off square, in place, and say which
/// and how far off they were. A record [`needs_squaring`] refuses is given back unchanged.
pub fn square_axes(b: &mut r::Brep) -> Vec<Squared> {
    let mut out = Vec::new();
    for (i, e) in b.edges.iter_mut().enumerate() {
        let curve = square_curve(&mut e.curve);
        let form = e.nurbs.as_mut().and_then(square_curve);
        if let Some(deviation) = curve.into_iter().chain(form).reduce(f64::max) {
            out.push(Squared {
                at: SquaredAt::Edge(i),
                deviation,
            });
        }
    }
    for (i, f) in b.faces.iter_mut().enumerate() {
        let form = f.nurbs.is_some();
        let deviation = match &mut f.surface {
            r::Surface::Cylinder { frame, .. }
            | r::Surface::Cone { frame, .. }
            | r::Surface::Sphere { frame, .. }
            | r::Surface::Torus { frame, .. } => {
                if form {
                    None
                } else {
                    square_frame(frame)
                }
            }
            _ => square_plane_face(f),
        };
        if let Some(deviation) = deviation {
            out.push(Squared {
                at: SquaredAt::Face(i),
                deviation,
            });
        }
    }
    out
}
