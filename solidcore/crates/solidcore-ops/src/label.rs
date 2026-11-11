//! What a record's labels mean in their own parameters ([`solidcore::eval::label`]): the natural
//! (u, v) ranges of a surface as it is named, an edge's interval on its circle or ellipse, and
//! whether a face's NURBS form turns its natural normal against its surface's. The records are
//! converted through `solidcore-convert` and the kernel computes.

use solidcore::eval::label as k;
use solidcore_record::brep::{Curve, Edge, Face, Natural, Surface};

/// The record's natural range of the kernel's.
fn natural(n: k::Natural) -> Natural {
    match n {
        k::Natural::Periodic => Natural::Periodic,
        k::Natural::Range(a, b) => Natural::Range(a, b),
    }
}

/// The ranges of u and v of `s` as it is named (its own (u, v), not its NURBS form's)
/// ([`solidcore::eval::label::surface_natural`]). Call on a validated record; a record too deep to
/// convert has no range (`Range(0, 0)` both ways).
#[must_use]
pub fn surface_natural(s: &Surface) -> [Natural; 2] {
    match solidcore_convert::surface(s) {
        Ok(s) => k::surface_natural(&s).map(natural),
        Err(_) => [Natural::Range(0., 0.); 2],
    }
}

/// The edge's interval on its `curve` itself, for code that reads what the edge is (a circle's
/// angles) ([`curve_interval`] of its parts). `None` where its NURBS can't be evaluated.
#[must_use]
pub fn edge_curve_interval(edge: &Edge) -> Option<[f64; 2]> {
    curve_interval(&edge.curve, edge.nurbs.as_ref(), edge.interval)
}

/// An edge's interval on its `curve`, given by its parts: its `curve`, its NURBS form and its
/// interval on that form ([`solidcore::eval::label::curve_interval`]).
#[must_use]
pub fn curve_interval(
    curve: &Curve,
    nurbs: Option<&Curve>,
    interval: [f64; 2],
) -> Option<[f64; 2]> {
    let curve = solidcore_convert::curve(curve).ok()?;
    let nurbs = nurbs.map(solidcore_convert::curve).transpose().ok()?;
    k::curve_interval(&curve, nurbs.as_ref(), interval)
}

/// Whether the face's NURBS form turns its natural normal against its surface's
/// ([`solidcore::eval::label::form_turns`]). A shell's `forward` is about [`Face::parameters`],
/// so the side the surface is used on is `forward != form_turns(face)`. `Some(false)` without a
/// form; `None` where the form does not evaluate.
#[must_use]
pub fn form_turns(face: &Face) -> Option<bool> {
    let surface = solidcore_convert::surface(&face.surface).ok()?;
    let nurbs = face
        .nurbs
        .as_ref()
        .map(solidcore_convert::surface)
        .transpose()
        .ok()?;
    k::form_turns(&surface, nurbs.as_ref(), face.domain)
}
