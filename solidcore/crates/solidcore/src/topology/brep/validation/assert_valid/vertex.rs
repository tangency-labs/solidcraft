//! The vertex and vertex-use checks.
//!
//! Upstream: `source/SMLib/src/SmVertex.cpp`, `SmVertexuse.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | `vertex` | `SmVertex::AssertValid` |
//! | `vertexuse` | `SmVertexuse::AssertValid` |
//!
//! Deviations: the vertex's local tolerance bounds its face gaps, with the body's
//! intersection tolerance as a floor; a tolerance past twice Test #82's limit excuses
//! nothing (see `seams`). Not upstream: one vertex's check solves each use's UV point for
//! its sector tests once while the B-rep is unwritten (`vertexuse::UvPoints`).

use super::{base, class, seams, Cx};
use crate::error::{EntityRef, Result};
use crate::kernel::Kernel;
use crate::topology::{vertexuse as tvu, Brep, VertexId, VertexuseId, VertexuseOwner};

/// `SmVertex::AssertValid(SM_NO_WALK)`.
pub(super) fn vertex(k: &mut Kernel, b: &mut Brep, cx: &mut Cx<'_>, v: VertexId) -> Result<bool> {
    const C: i64 = class::VERTEX;
    let this = EntityRef::Vertex(v);
    let mut rtn = true;

    // call the base class AssertValid
    if cx.bases() {
        rtn &= base::owning(b, cx, this)?;
    }

    // not upstream: the geometric tests of a vertex whose neighbourhood passed them on a
    // body before are skipped ([`crate::geom::check_memo`]); the structural ones run
    let geo = !crate::geom::check_memo::vertex_clean(k, v);

    // 1: Vertex location must meet its faces within its local tolerance;
    // the check runs at every level, its report is level 2
    let (located, max_gap) = if geo {
        seams::assert_test_vertex1(k, b, v)?
    } else {
        (true, 0.0)
    };
    if !located {
        // Local uncertainty with SmTol::GetXSectTol3d(GetBrep()) as the floor.
        let xsect_tol = seams::vertex1_tolerance(k, b, v)?;
        if max_gap > xsect_tol {
            let ok = !cx.rep.need(2);
            rtn &= cx
                .rep
                .this_value(b, C, this, 1, ok, xsect_tol, || max_gap)?;
        }
    }

    // 2: Vertex has multiple Vertexuse Vertex/Face connections one of which is a
    // VertexLoop
    let ok = !cx.rep.need(0) || seams::vertex_check_pointers(k, b, v)?;
    rtn &= cx.rep.this_bool(b, C, this, 2, ok)?;

    // Check all connected Vertexuses; not upstream: the UV points their sector tests
    // solve are kept while the B-rep is unwritten (`tvu::UvPoints`)
    let mut points = tvu::UvPoints::default();
    for vu in b.vertex(v)?.vertexuses.clone() {
        rtn &= vertexuse(k, b, cx, vu, geo, &mut points)?;
    }

    // all done
    Ok(rtn)
}

/// `SmVertexuse::AssertValid(SM_NO_WALK)`.
fn vertexuse(
    k: &mut Kernel,
    b: &mut Brep,
    cx: &mut Cx<'_>,
    vu: VertexuseId,
    geo: bool,
    points: &mut tvu::UvPoints,
) -> Result<bool> {
    const C: i64 = class::VERTEXUSE;
    let this = EntityRef::Vertexuse(vu);
    let mut rtn = true;

    // call the base class AssertValid
    if cx.bases() {
        rtn &= base::topology(b, cx, this)?;
    }

    // 0: SmVertexuse and the objects it attaches to need to share common contexts;
    // 1: its type matches its owner's (intrinsic: one enum)
    let owner = b.vertexuse(vu)?.owner;
    let has_owner = matches!(
        owner,
        VertexuseOwner::Edgeuse(Some(_))
            | VertexuseOwner::Loopuse(Some(_))
            | VertexuseOwner::Shell(Some(_))
    );
    if has_owner {
        rtn &= cx.rep.this_bool(b, C, this, 0, true)?;
        rtn &= cx.rep.this_bool(b, C, this, 1, true)?;
    }

    // 5: Vertexuse->Sector contains a FaceSrf->Seam not marked by SeamEdge
    let missing_seam = geo && tvu::is_sector_missing_seam_with(k, b, vu, true, points)?.missing;
    rtn &= cx
        .rep
        .this_bool(b, C, this, 5, !cx.rep.need(0) || !missing_seam)?;

    // 6: Vertexuse connects to Faceuse for LoopuseTypes and EdgeuseTypes not connected
    // to wires
    if matches!(
        owner,
        VertexuseOwner::Loopuse(_) | VertexuseOwner::Edgeuse(_)
    ) {
        let edge = match tvu::edgeuse(b, vu)? {
            Some(eu) => b.edgeuse(eu)?.edge,
            None => None,
        };
        let ok = !cx.rep.need(0)
            || tvu::faceuse(b, vu)?.is_some()
            || match edge {
                Some(e) => tvu::edge_is_wire(b, e)?,
                None => false,
            };
        rtn &= cx.rep.this_bool(b, C, this, 6, ok)?;
    }

    // all done
    Ok(rtn)
}
