//! Operands read at tolerances far apart, made to agree before a boolean where the coarser one
//! allows it (not upstream, tangency-labs/solidcore#3292).
//!
//! A boolean's result takes its operands' larger tolerance: entities the finer operand keeps
//! apart that lie closer than that are one in the result (the two vertices of a short edge
//! merge, its faces' edges run together), and the check refuses the body (`SmFace` test #34,
//! partially coincident edges). A part read from a file at the file's round tolerance
//! (0.01 mm) joined by one with edges of a few microns, such as a letter read finer for its
//! own short edges (`solidcore-step`'s feature tolerance), is the case.

use crate::curve::from_parameters::zone_tolerance;
use crate::error::Result;
use crate::kernel::Kernel;
use crate::topology::brep::p3::brep_copy;
use crate::topology::brep::validation::{check_brep, update_tolerances_after_boolean, CheckLevel};
use crate::topology::face::p3::set_tolerance as face_set_tolerance;
use crate::topology::{edge, vertex, Brep};

/// The share of a body's shortest edge its tolerance may reach: solidcore-step reads a part
/// at this share of its shortest edge where the file's stated tolerance cannot tell its
/// features apart (`FEATURE_MARGIN` there), and the same holds for a boolean's result.
const FEATURE_MARGIN: f64 = 0.25;

/// The shortest distance between the two vertices of an edge of `b` (a closed edge aside);
/// infinite for a body with none.
fn shortest_edge(b: &Brep) -> Result<f64> {
    let mut shortest = f64::INFINITY;
    for e in b.edges().ids().collect::<Vec<_>>() {
        let (Some(p), Some(q)) = (edge::start_vertex(b, e)?, edge::end_vertex(b, e)?) else {
            continue;
        };
        if p == q {
            continue;
        }
        let d = b.vertex(p)?.point.distance(b.vertex(q)?.point);
        if d > 0.0 && d < shortest {
            shortest = d;
        }
    }
    Ok(shortest)
}

/// `a` and `b`, the operands of a boolean, made to agree on a tolerance that resolves the finer
/// one's features, where the coarser one allows it. When the coarser operand's tolerance is
/// over [`FEATURE_MARGIN`] of the finer operand's shortest edge (or the finer one's own
/// tolerance, if larger), every tolerance of the coarser one over that is set to it and then
/// each edge and vertex is grown back to what its own gaps need
/// ([`update_tolerances_after_boolean`]); a copy so set must pass the full check first. A
/// coarse operand whose geometry meets the finer tolerance (most of it does: a file states one
/// round number for everything) keeps its gaps covered where it has them and resolves the
/// finer operand's features everywhere else, so nothing of either is lost. Otherwise both stay
/// as they came, as before.
///
/// # Errors
/// Those of the edge walk, the copy and the tolerance updates; a cancellation or an unported
/// test in the check. A check that fails otherwise leaves the operands as they came.
pub fn matched_tolerances(k: &mut Kernel, a: &mut Brep, b: &mut Brep) -> Result<()> {
    let (coarse, fine) = match a.tolerance().partial_cmp(&b.tolerance()) {
        Some(core::cmp::Ordering::Greater) => (a, b),
        Some(core::cmp::Ordering::Less) => (b, a),
        _ => return Ok(()),
    };
    // (the kernel holds a zone tolerance, half the intersection tolerance a record and a file
    // state)
    let need = zone_tolerance(FEATURE_MARGIN * shortest_edge(fine)?).max(fine.tolerance());
    if need.partial_cmp(&coarse.tolerance()) != Some(core::cmp::Ordering::Less) {
        return Ok(());
    }
    let mut trial = brep_copy(k, coarse)?;
    tightened(k, &mut trial, need)?;
    let valid = match check_brep(k, &mut trial, CheckLevel::Level2) {
        Ok(c) => c.is_valid(),
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => false,
    };
    if valid {
        tightened(k, coarse, need)?;
    }
    Ok(())
}

/// `b` with its tolerance and every face's, edge's and vertex's larger than `zone` set to
/// `zone` (a kernel zone tolerance), smaller ones as they are, and then every edge and vertex
/// grown to cover its own gaps.
///
/// # Errors
/// Those of the entity walks and the tolerance update.
fn tightened(k: &mut Kernel, b: &mut Brep, zone: f64) -> Result<()> {
    for f in b.faces().ids().collect::<Vec<_>>() {
        if b.face(f)?.tolerance > zone {
            face_set_tolerance(b, f, zone, false, false)?;
        }
    }
    for e in b.edges().ids().collect::<Vec<_>>() {
        if b.edge(e)?.tolerance > zone {
            edge::set_tolerance(b, e, zone, false, false)?;
        }
    }
    for v in b.vertices().ids().collect::<Vec<_>>() {
        if b.vertex(v)?.tolerance > zone {
            vertex::set_tolerance(b, v, zone, false, false)?;
        }
    }
    if b.tolerance() > zone {
        b.set_tolerance(zone);
    }
    // what the body's own gaps need, edge by edge and vertex by vertex
    update_tolerances_after_boolean(k, b)?;
    Ok(())
}
