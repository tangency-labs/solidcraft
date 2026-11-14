//! The checks a record passes before the kernel takes it: the record's structural validation
//! ([`Brep::validate`], [`Brep::validate_built`]) and then the kernel's checks of every edge's
//! and face's geometric records ([`solidcore::eval::check`]: frames orthonormal, planes that span
//! two directions, placed revolutions about a unit axis under a non-singular placement).

use solidcore::eval::check as k;
use solidcore_record::brep::{Brep, Curve, Frame, Invalid, Result, Surface};

/// [`Brep::validate`] (a record from outside the kernel), then [`geometry`].
///
/// # Errors
/// The first check the record fails.
pub fn validate(b: &Brep) -> Result<()> {
    b.validate()?;
    geometry(b)
}

/// [`Brep::validate_built`] (a record the kernel built), then [`geometry`].
///
/// # Errors
/// The first check the record fails.
pub fn validate_built(b: &Brep) -> Result<()> {
    b.validate_built()?;
    geometry(b)
}

/// Every edge's and face's geometric records ([`curve`], [`surface`]). Call on a record its
/// structural validation accepted.
///
/// # Errors
/// The first geometric record the kernel refuses.
pub fn geometry(b: &Brep) -> Result<()> {
    for e in &b.edges {
        curve(&e.curve)?;
        if let Some(n) = &e.nurbs {
            curve(n)?;
        }
    }
    for f in &b.faces {
        // (a blend's label is its definition, which its structural validation checks and the
        // kernel certifies its net against when the face is built; its geometry is its form)
        if !matches!(f.surface, Surface::Blend { .. }) {
            surface(&f.surface)?;
        }
        if let Some(n) = &f.nurbs {
            surface(n)?;
        }
    }
    Ok(())
}

/// A frame's axes are unit and perpendicular, to 1e-8 ([`solidcore::eval::check::frame`]).
///
/// # Errors
/// They are not.
pub fn frame(f: &Frame) -> Result<()> {
    k::frame(&solidcore_convert::frame(f)).map_err(Invalid)
}

/// A curve's geometric records ([`solidcore::eval::check::curve`]). Call on a curve its
/// structural validation accepted.
///
/// # Errors
/// The record the kernel refuses.
pub fn curve(c: &Curve) -> Result<()> {
    k::curve(&solidcore_convert::curve(c)?).map_err(Invalid)
}

/// A surface's geometric records ([`solidcore::eval::check::surface`]). Call on a surface its
/// structural validation accepted.
///
/// # Errors
/// The record the kernel refuses.
pub fn surface(s: &Surface) -> Result<()> {
    k::surface(&solidcore_convert::surface(s)?).map_err(Invalid)
}

pub use solidcore::eval::agreement::{Mismatch, Problem, Record, AREA_SAMPLES};

/// That the record's geometry agrees with its topology, within its tolerances
/// ([`solidcore::eval::agreement::check`]): each edge's curve meets its vertices, each pcurve lifts
/// onto its edge's curve and continues the use before it, each loop encloses (u, v) area and turns
/// the right way, each vertex loop's vertex lies on its surface. Call on a record its structural
/// validation accepted; a record too deep to convert is refused as unevaluable.
///
/// # Errors
/// The first record that disagrees, and how.
pub fn agreement(b: &Brep) -> std::result::Result<(), Mismatch> {
    use solidcore::eval::agreement as a;
    let unconvertible = |record: Record, e: Invalid| Mismatch {
        record,
        problem: Problem::Unevaluable(e.to_string()),
    };
    let index = |i: usize| u32::try_from(i).unwrap_or(u32::MAX);
    let mut curves = Vec::new();
    for (i, e) in b.edges.iter().enumerate() {
        curves.push(
            solidcore_convert::curve(e.parameters())
                .map_err(|err| unconvertible(Record::Edge(index(i)), err))?,
        );
    }
    let mut surfaces = Vec::new();
    for (i, f) in b.faces.iter().enumerate() {
        surfaces.push(
            solidcore_convert::surface(f.parameters())
                .map_err(|err| unconvertible(Record::Face(index(i)), err))?,
        );
    }
    let pcurves: Vec<Vec<Vec<solidcore::eval::Curve2>>> = b
        .faces
        .iter()
        .map(|f| {
            f.loops
                .iter()
                .map(|l| match l {
                    solidcore_record::brep::Loop::Edges { edges } => edges
                        .iter()
                        .map(|u| solidcore_convert::curve2(&u.pcurve))
                        .collect(),
                    solidcore_record::brep::Loop::Vertex { .. } => Vec::new(),
                })
                .collect()
        })
        .collect();
    let edges: Vec<a::Edge<'_>> = b
        .edges
        .iter()
        .zip(&curves)
        .map(|(e, curve)| a::Edge {
            curve,
            interval: e.interval,
            vertices: e.vertices,
            tolerance: e.tolerance,
        })
        .collect();
    let vertices: Vec<a::Vertex> = b
        .vertices
        .iter()
        .map(|v| a::Vertex {
            point: v.point,
            tolerance: v.tolerance,
        })
        .collect();
    let faces: Vec<a::Face<'_>> = b
        .faces
        .iter()
        .zip(&surfaces)
        .zip(&pcurves)
        .map(|((f, surface), pcs)| a::Face {
            surface,
            loops: f
                .loops
                .iter()
                .zip(pcs)
                .map(|(l, pcs)| match l {
                    solidcore_record::brep::Loop::Vertex { vertex } => a::Loop::Vertex(*vertex),
                    solidcore_record::brep::Loop::Edges { edges } => a::Loop::Edges(
                        edges
                            .iter()
                            .zip(pcs)
                            .map(|(u, pcurve)| a::Use {
                                edge: u.edge,
                                forward: u.forward,
                                pcurve,
                            })
                            .collect(),
                    ),
                })
                .collect(),
        })
        .collect();
    a::check(&a::View {
        edges: &edges,
        vertices: &vertices,
        faces: &faces,
    })
}
