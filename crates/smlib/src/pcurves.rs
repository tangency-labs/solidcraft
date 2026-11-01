//! SMLib's own pcurves for a face's uses that have no closed form (an approximated intersection
//! curve on a cone, a file's spline edge on a spline surface): the face goes to SMLib alone, an
//! open sheet, with the trims it has and without the others, and SMLib makes those as it makes
//! every trim it needs (`SmEdgeuse::GetOrCreateUVTrimCurve`, when the body is exported). They are
//! SMLib's, relayed as it holds them on the record's parameters; nothing here fits a curve.

use std::collections::HashSet;

use solidcraft_geom::brep::pcurve::{SURFACE_GAP_MARGIN, TrimFace, Trims};
use solidcraft_geom::brep::{self as b, Curve2, EdgeUseRef, Loop, OrientedEdge, OrientedFace, Region, Shell, Vertex};

/// [`Trims`] by SMLib.
#[derive(Clone, Copy, Debug, Default)]
pub struct SmlibTrims;

impl Trims for SmlibTrims {
    fn trims(&self, face: &TrimFace<'_>) -> Result<Vec<Vec<Curve2>>, String> {
        trims(face).map_err(|e| format!("SMLib pcurves: {e}"))
    }
}

fn trims(face: &TrimFace<'_>) -> Result<Vec<Vec<Curve2>>, String> {
    let (record, bare) = sheet(face)?;
    let mut arrays = crate::import::arrays_with_bare_uses(&record, &bare).map_err(|e| e.to_string())?;
    let import = |a: &crate::import::Arrays| crate::flat::Flat::from_arrays(a).and_then(|f| f.import()).map_err(|e| e.to_string());
    let mut body = import(&arrays)?;
    if cover_gaps(&body, face, &bare, &mut arrays)? {
        body = import(&arrays)?;
    }
    let (out, order) = body.to_native_ordered().map_err(|e| e.to_string())?;
    let [made] = out.faces.as_slice() else { return Err(format!("SMLib made {} faces of one", out.faces.len())) };
    let made_uses: Vec<&OrientedEdge> = made
        .loops
        .iter()
        .flat_map(|l| match l {
            Loop::Edges { edges } => edges.as_slice(),
            Loop::Vertex { .. } => &[],
        })
        .collect();
    // SMLib keeps the edges in the order they came; the record renumbers them.
    let edge_of = |u: u32| order.edges.get(u as usize).copied().ok_or("an edge SMLib lost");
    // A sheet's loops come back the way SMLib runs them, which may be against ours: a use is the
    // same pcurve either way (on its edge's interval in its curve's direction), and the loop's
    // way round is read from an edge the face uses once.
    let ours = face.loops.iter().flatten();
    let once = ours.clone().find(|u| ours.clone().filter(|v| v.edge == u.edge).count() == 1).ok_or("a face of seam edges only")?;
    let at = edge_of(once.edge)?;
    let reversed = made_uses.iter().find(|m| m.edge as usize == at).ok_or("a use SMLib did not keep")?.forward != once.forward;
    // Each edge's tolerance as SMLib held it (raised where it lies off the surface).
    let held = arrays.reals.get(&crate::flat::EDGE_TOLERANCE).ok_or("edge tolerances")?;
    let mut result = Vec::with_capacity(face.loops.len());
    for l in &face.loops {
        let mut pcs = Vec::with_capacity(l.len());
        for u in l {
            let e = edge_of(u.edge)?;
            let edge = out.edges.get(e).ok_or("an edge SMLib lost")?;
            let ours = face.edges.get(u.edge as usize).ok_or("edge reference")?;
            let found = made_uses.iter().find(|m| m.edge as usize == e && m.forward == (u.forward != reversed));
            let pc = found.ok_or("a use SMLib did not keep")?.pcurve.clone();
            let tolerance = held.get(u.edge as usize).copied().unwrap_or(ours.tolerance).max(ours.tolerance);
            pcs.push(on_our_interval(pc, ours, edge.interval, tolerance)?);
        }
        result.push(pcs);
    }
    Ok(result)
}

/// Each bare use's edge measured against the face's surface as SMLib measures it (over the
/// surface's own domain: an edge run a few um past its patch measures how far), and its
/// `EDGE_TOLERANCE` in `arrays` raised to cover that gap with [`SURFACE_GAP_MARGIN`]: SMLib keeps
/// an edge whole when making its trim only when its zone tolerance (half that) covers the gap, and
/// otherwise trims the edge back to the patch. Whether any was raised; an edge farther off than
/// SMLib's tolerances reach is refused with how far.
fn cover_gaps(body: &crate::Brep, face: &TrimFace<'_>, bare: &HashSet<(u32, u32, u32)>, arrays: &mut crate::import::Arrays) -> Result<bool, String> {
    let mut edges: Vec<usize> = Vec::new();
    for (li, l) in face.loops.iter().enumerate() {
        for (p, u) in l.iter().enumerate() {
            let (Ok(li), Ok(p)) = (u32::try_from(li), u32::try_from(p)) else { return Err("too many uses".into()) };
            if bare.contains(&(0, li, p)) && !edges.contains(&(u.edge as usize)) {
                edges.push(u.edge as usize);
            }
        }
    }
    let mut raised = false;
    for e in edges {
        let gap = body.edge_face_distance(e, 0).map_err(|e| e.to_string())?;
        let need = SURFACE_GAP_MARGIN * gap;
        let tolerances = arrays.reals.get_mut(&crate::flat::EDGE_TOLERANCE).ok_or("edge tolerances")?;
        let given = tolerances.get_mut(e).ok_or("edge reference")?;
        if need <= *given {
            continue;
        }
        if !(need <= crate::flat::MAX_TOLERANCE) {
            return Err(format!("an edge lies {gap} mm off the face's surface"));
        }
        *given = need;
        raised = true;
    }
    Ok(raised)
}

/// A pcurve SMLib made on its edge's interval `theirs`, on ours. SMLib may set an edge's end to
/// a parameter of its own (a knot its vertex lies at, the end of the part on the face: where an
/// edge runs past its face's patch, SMLib ends it where its drop onto the patch ends, when that
/// is within the edge's cross-section tolerance of its vertex): where each end it chose is
/// within `tolerance` of ours on the edge's curve (the edge's tolerance as SMLib held it,
/// [`cover_gaps`]), the pcurve is mapped onto our interval by the affine change of parameter
/// (exact for a B-spline: its knots move), and the record's check measures it against the edge
/// as any pcurve. Farther is SMLib trimming the edge, refused with how far.
pub(crate) fn on_our_interval(
    pc: Curve2,
    ours: &solidcraft_geom::brep::pcurve::TrimEdge<'_>,
    theirs: [f64; 2],
    tolerance: f64,
) -> Result<Curve2, String> {
    let [a, b] = ours.interval;
    let [c, d] = theirs;
    if a == c && b == d {
        return Ok(pc);
    }
    let curve = solidcraft_geom::brep::eval::CurveEval::new(ours.nurbs.unwrap_or(ours.curve)).map_err(|e| e.to_string())?;
    let at = |t: f64| curve.point(t).ok_or_else(|| format!("the edge's curve has no point at {t}"));
    let shift = at(a)?.dist(at(c)?).max(at(b)?.dist(at(d)?));
    if !(shift <= tolerance) || !(d > c) {
        return Err(format!(
            "SMLib moved an edge's interval from {:?} to {:?}, its ends {shift} mm along its curve (tolerance {tolerance} mm)",
            ours.interval, theirs
        ));
    }
    let map = |k: f64| {
        if k == c {
            a
        } else if k == d {
            b
        } else {
            a + (k - c) * ((b - a) / (d - c))
        }
    };
    Ok(match pc {
        Curve2::Spline { knots, points } => Curve2::Spline { knots: knots.into_iter().map(map).collect(), points },
        Curve2::RationalSpline { knots, homogeneous } => Curve2::RationalSpline { knots: knots.into_iter().map(map).collect(), homogeneous },
    })
}

/// The face as a one-face open sheet, and its uses that go without trims.
fn sheet(face: &TrimFace<'_>) -> Result<(b::Brep, HashSet<(u32, u32, u32)>), String> {
    let mut edges: Vec<b::Edge> = face
        .edges
        .iter()
        .map(|e| b::Edge {
            curve: e.curve.clone(),
            nurbs: e.nurbs.cloned(),
            interval: e.interval,
            vertices: e.vertices,
            tolerance: e.tolerance,
            uses: Vec::new(),
        })
        .collect();
    let mut bare = HashSet::new();
    let mut loops = Vec::with_capacity(face.loops.len());
    for (li, l) in face.loops.iter().enumerate() {
        let li = u32::try_from(li).map_err(|_| "too many loops")?;
        let mut uses = Vec::with_capacity(l.len());
        for (p, u) in l.iter().enumerate() {
            let p = u32::try_from(p).map_err(|_| "too many uses")?;
            let edge = edges.get_mut(u.edge as usize).ok_or("edge reference")?;
            edge.uses.push(EdgeUseRef { face: 0, r#loop: li, position: p, upper: !u.forward });
            let pcurve = match &u.pcurve {
                Some(pc) => pc.clone(),
                None => {
                    bare.insert((0, li, p));
                    // Not read: the use goes without a trim (`arrays_with_bare_uses`).
                    Curve2::Spline { knots: vec![edge.interval[0], edge.interval[0], edge.interval[1], edge.interval[1]], points: vec![[0.0; 2]; 2] }
                }
            };
            uses.push(OrientedEdge { edge: u.edge, forward: u.forward, pcurve });
        }
        loops.push(Loop::Edges { edges: uses });
    }
    let vertices = face.vertices.iter().map(|&point| Vertex { point, tolerance: face.tolerance }).collect();
    let record = b::Brep {
        version: b::VERSION,
        tolerance: face.tolerance,
        regions: vec![Region { shells: vec![0], void: false }],
        shells: vec![Shell::Faces { faces: vec![OrientedFace { face: 0, forward: true }], wires: Vec::new() }],
        faces: vec![b::Face { surface: face.surface.clone(), nurbs: face.nurbs.cloned(), domain: face.domain, loops }],
        edges,
        vertices,
    };
    Ok((record, bare))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SMLib's interval [0.1, 9.9] on our line edge [0, 10], within its 0.5 mm tolerance: every
    /// knot moves by the affine change of parameter, the ends onto ours exactly, the points stay.
    /// Ends farther than the tolerance, or turned round, are refused.
    #[test]
    fn a_pcurve_on_smlibs_interval_maps_onto_ours() {
        let line = b::Curve::Line { start: [0.0; 3], end: [1.0, 0.0, 0.0] };
        let edge = solidcraft_geom::brep::pcurve::TrimEdge { curve: &line, nurbs: None, interval: [0.0, 10.0], vertices: [0, 1], tolerance: 0.5 };
        let points = vec![[0.0, 0.0], [1.0, 2.0], [3.0, 2.0], [4.0, 0.0]];
        let pc = Curve2::Spline { knots: vec![0.1, 0.1, 0.1, 2.5, 9.9, 9.9, 9.9], points: points.clone() };
        let Curve2::Spline { knots, points: kept } = on_our_interval(pc.clone(), &edge, [0.1, 9.9], 0.5).unwrap() else { panic!("a spline") };
        assert_eq!(kept, points);
        assert_eq!(&knots[..3], &[0.0; 3]);
        assert_eq!(&knots[4..], &[10.0; 3]);
        assert!((knots[3] - 2.4 * 10.0 / 9.8).abs() < 1e-12, "{knots:?}");
        // Rational: the weights go with the points, the knots as for the spline.
        let h: Vec<[f64; 3]> = points.iter().map(|p| [p[0] * 2.0, p[1] * 2.0, 2.0]).collect();
        let r = Curve2::RationalSpline { knots: vec![0.1, 0.1, 0.1, 2.5, 9.9, 9.9, 9.9], homogeneous: h.clone() };
        let Curve2::RationalSpline { knots: rk, homogeneous } = on_our_interval(r, &edge, [0.1, 9.9], 0.5).unwrap() else { panic!("rational") };
        assert_eq!((rk, homogeneous), (knots, h));
        // The same interval is kept as it is.
        assert_eq!(on_our_interval(pc.clone(), &edge, [0.0, 10.0], 0.5).unwrap(), pc);
        assert!(on_our_interval(pc.clone(), &edge, [0.1, 9.9], 0.05).is_err(), "ends 0.1 mm off, tolerance 0.05");
        assert!(on_our_interval(pc, &edge, [9.9, 0.1], 20.0).is_err(), "turned round");
    }
}
