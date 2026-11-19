//! A projected outline's planar pieces, tidied, for a sketch's projection of a whole body: the
//! kernel's [`solidcore::eval::outline::pieces`] on the record's vectors. The body's outline
//! stretches on its edges are [`crate::query::Query::outline_stretches`].
//!
//! Nothing here computes: every answer is the kernel's, its vectors copied field by field.

use solidcore::eval::outline::pieces as k;
use solidcore_convert::{record_vec2, vec2};
use solidcore_record::Vec2;

/// One piece of a planar outline (the kernel's [`k::Piece`], on the record's vectors).
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    /// A point.
    Point(Vec2),
    /// A line from the first point to the second.
    Line(Vec2, Vec2),
    /// A circle: its centre and radius.
    Circle(Vec2, f64),
    /// A counter-clockwise arc about `c` from `a` to `b`.
    Arc {
        /// Its centre.
        c: Vec2,
        /// Its start.
        a: Vec2,
        /// Its end.
        b: Vec2,
    },
    /// A rational quadratic conic from `a` to `b` with control `apex` and shape `rho`.
    Conic {
        /// Its start.
        a: Vec2,
        /// Its middle control point.
        apex: Vec2,
        /// Its end.
        b: Vec2,
        /// Its shape: the middle weight `w` over `1 + w`.
        rho: f64,
    },
    /// A full ellipse: its centre, the end of its major axis and its minor radius.
    Ellipse {
        /// Its centre.
        c: Vec2,
        /// The end of its major axis.
        major: Vec2,
        /// Its minor radius.
        minor: f64,
    },
    /// A spline through (`control` false) or over (`control`) its points, with its own knots and
    /// weights where it has them.
    Spline {
        /// Its points.
        pts: Vec<Vec2>,
        /// The points are control points.
        control: bool,
        /// Its degree.
        degree: u8,
        /// It closes on itself.
        periodic: bool,
        /// Its knots and weights, where they are not uniform.
        nurbs: Option<(Vec<f64>, Vec<f64>)>,
    },
}

fn to_kernel(p: Piece) -> k::Piece {
    match p {
        Piece::Point(a) => k::Piece::Point(vec2(a)),
        Piece::Line(a, b) => k::Piece::Line(vec2(a), vec2(b)),
        Piece::Circle(c, r) => k::Piece::Circle(vec2(c), r),
        Piece::Arc { c, a, b } => k::Piece::Arc {
            c: vec2(c),
            a: vec2(a),
            b: vec2(b),
        },
        Piece::Conic { a, apex, b, rho } => k::Piece::Conic {
            a: vec2(a),
            apex: vec2(apex),
            b: vec2(b),
            rho,
        },
        Piece::Ellipse { c, major, minor } => k::Piece::Ellipse {
            c: vec2(c),
            major: vec2(major),
            minor,
        },
        Piece::Spline {
            pts,
            control,
            degree,
            periodic,
            nurbs,
        } => k::Piece::Spline {
            pts: pts.into_iter().map(vec2).collect(),
            control,
            degree,
            periodic,
            nurbs,
        },
    }
}

fn from_kernel(p: k::Piece) -> Piece {
    match p {
        k::Piece::Point(a) => Piece::Point(record_vec2(a)),
        k::Piece::Line(a, b) => Piece::Line(record_vec2(a), record_vec2(b)),
        k::Piece::Circle(c, r) => Piece::Circle(record_vec2(c), r),
        k::Piece::Arc { c, a, b } => Piece::Arc {
            c: record_vec2(c),
            a: record_vec2(a),
            b: record_vec2(b),
        },
        k::Piece::Conic { a, apex, b, rho } => Piece::Conic {
            a: record_vec2(a),
            apex: record_vec2(apex),
            b: record_vec2(b),
            rho,
        },
        k::Piece::Ellipse { c, major, minor } => Piece::Ellipse {
            c: record_vec2(c),
            major: record_vec2(major),
            minor,
        },
        k::Piece::Spline {
            pts,
            control,
            degree,
            periodic,
            nurbs,
        } => Piece::Spline {
            pts: pts.into_iter().map(record_vec2).collect(),
            control,
            degree,
            periodic,
            nurbs,
        },
    }
}

fn all_to_kernel(geom: Vec<Piece>) -> Vec<k::Piece> {
    geom.into_iter().map(to_kernel).collect()
}

fn all_from_kernel(geom: Vec<k::Piece>) -> Vec<Piece> {
    geom.into_iter().map(from_kernel).collect()
}

/// The projected pieces of one stretch of an edge, tidied ([`k::conic_spans`], [`k::flat_conic`],
/// [`k::round_conics`]): a point (an edge seen end-on) dropped, a rational quadratic spline with
/// double inner knots as its conic spans, a conic seen edge-on as its line, and circular conics as
/// arcs of their circle, joined.
#[must_use]
pub fn tidy_stretch(geom: Vec<Piece>) -> Vec<Piece> {
    let pieces = geom
        .into_iter()
        .filter(|g| !matches!(g, Piece::Point(_)))
        .map(to_kernel)
        .flat_map(k::conic_spans)
        .map(k::flat_conic)
        .collect();
    all_from_kernel(k::round_conics(pieces))
}

/// One silhouette's exact pieces tidied ([`k::flat_conic`], [`k::round_conics`]): a conic seen
/// edge-on as its line, circular conics as arcs of their circle, joined.
#[must_use]
pub fn tidy_silhouette(geom: Vec<Piece>) -> Vec<Piece> {
    all_from_kernel(k::round_conics(
        geom.into_iter().map(to_kernel).map(k::flat_conic).collect(),
    ))
}

/// Whether `p` lies within `tol` of the piece `g` ([`k::near`]).
#[must_use]
pub fn near(g: &Piece, p: Vec2, tol: f64) -> bool {
    // A point or a spline never holds it: not copied for the kernel to say so.
    if matches!(g, Piece::Point(_) | Piece::Spline { .. }) {
        return false;
    }
    k::near(&to_kernel(g.clone()), vec2(p), tol)
}

/// The pieces with ends within `gap` of an end met before moved onto it
/// ([`k::meet_at_shared_ends`]).
#[must_use]
pub fn meet_at_shared_ends(geom: Vec<Piece>, gap: f64) -> Vec<Piece> {
    let mut pieces = all_to_kernel(geom);
    k::meet_at_shared_ends(&mut pieces, gap);
    all_from_kernel(pieces)
}

/// The pieces with those that coincide within `tol` drawn once ([`k::drop_coincident`]).
#[must_use]
pub fn drop_coincident(geom: Vec<Piece>, tol: f64) -> Vec<Piece> {
    all_from_kernel(k::drop_coincident(all_to_kernel(geom), tol))
}
