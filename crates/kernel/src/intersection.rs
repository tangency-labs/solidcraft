//! Where two bodies meet, each whole or one face of it: the edges SMLib's imprint of the first by
//! the second makes (`capi/include/smlib.h`, "Imprint": each face the tool meets is cut along the
//! intersection). SMLib intersects the faces' surfaces and trims each curve to both faces, so an
//! edge it makes is exactly where they meet: its record curve (a B-spline SMLib fitted within its
//! tolerance, or the line, circle or ellipse it is) on its interval. Which edges are new comes from
//! the imprint's own history: an edge no edge of either operand became.

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::Curve;

use crate::body::{Body, MeshTolerance};
use crate::{KernelError, Result};

/// One edge where two bodies meet.
#[derive(Clone, Debug, PartialEq)]
pub struct MeetingEdge {
    /// The edge's record curve, exactly as SMLib made it.
    pub curve: Curve,
    /// Its interval on `curve`.
    pub interval: [f64; 2],
    /// Its points at the chord tolerance asked for, from its start to its end.
    pub points: Vec<Vec3>,
}

fn failed(what: &str, e: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("SMLib intersection curve: {what}: {e}"))
}

/// `body` itself, or its face `face` (in its face order) alone as a surface body.
fn part(body: &Body, face: Option<usize>) -> Result<Body> {
    match face {
        None => Ok(body.clone()),
        Some(f) if f < body.record().faces.len() => crate::surfaces::taken(body, body.record(), &[(f, false)]),
        Some(f) => Err(KernelError::Invalid(format!("the body has no face {f}"))),
    }
}

/// Where `a` (or its face `a_face`) meets `b` (or its face `b_face`): every edge SMLib's imprint
/// of the one by the other makes, its points at `chord` mm. Empty where they do not meet.
pub fn meeting_edges(a: &Body, a_face: Option<usize>, b: &Body, b_face: Option<usize>, chord: f64) -> Result<Vec<MeetingEdge>> {
    let (a, b) = (part(a, a_face)?, part(b, b_face)?);
    let ta = a.smlib()?.tagged(0).map_err(|e| failed("tag", e))?;
    let tb = b.smlib()?.tagged(1).map_err(|e| failed("tag", e))?;
    let Some(cut) = ta.imprint(&tb).map_err(|e| failed("imprint", e))? else { return Ok(Vec::new()) };
    let history = cut.history().map_err(|e| failed("history", e))?;
    let (body, order) = crate::native::body_from_smlib_ordered(&cut)?;
    let order = order.ok_or_else(|| failed("history", "the imprint's edges cannot be numbered"))?;
    let mesh = body.mesh(&MeshTolerance::chord(chord))?;
    let record = body.record();
    let mut out = Vec::new();
    for (i, edge) in history.edges.iter().enumerate() {
        if !edge.origins.is_empty() {
            continue;
        }
        let at = if order.edges.is_empty() { Some(i) } else { order.edges.get(i).copied() };
        let (Some(e), Some(points)) = (at.and_then(|k| record.edges.get(k)), at.and_then(|k| mesh.edges.get(k))) else {
            return Err(failed("history", format!("edge {i} is not in the imprinted body")));
        };
        out.push(MeetingEdge { curve: e.parameters().clone(), interval: e.interval, points: points.clone() });
    }
    Ok(out)
}
