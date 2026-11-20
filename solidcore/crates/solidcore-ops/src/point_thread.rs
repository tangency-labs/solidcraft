// SPDX-License-Identifier: Apache-2.0
//! Cutters for conical threads and screw points. The kernel supplies the depth laws,
//! profiles and section frames. Ops builds the lofts and combines the crossed crests.

use solidcore::sweep::point_thread::{self, Crossed, Grooves, Loft, PlanError, Profile};
use solidcore_record::{Plane, Region2};

pub use solidcore::sweep::point_thread::{ConicalThread, Shank, ShankEnd, ThreadHand};

use crate::boolean::{boolean, union_all, BooleanOp};
use crate::error::status;
use crate::sweep::{self, Curve};
use crate::{Body, Error, Result};

/// A helix through the conical groove's meridian profile.
pub struct Start {
    /// Starting profile plane.
    pub plane: Plane,
    /// Closed meridian region to sweep.
    pub region: Region2,
    /// Directed helix axis.
    pub axis: [f64; 3],
    /// Signed advance per turn.
    pub lead: f64,
}

/// The profile and cuts for the ordinary radial screw sweep. Clients that
/// control the screw sweep's flank topology use their configured sweep here.
pub struct Swept {
    /// One meridian profile per start and hand.
    pub starts: Vec<Start>,
    /// Total turns of each sweep.
    pub turns: f64,
    /// Radial offsets in increasing turn order.
    pub law: Vec<(f64, f64)>,
    /// Planes at the low and high limits of the cut.
    pub trim: [Plane; 2],
    /// Optional exact root to subtract from the cutter.
    pub core: Option<(Plane, Region2)>,
}

/// A conical cutter, as tools ready to cut or as an ordinary screw sweep.
pub enum Cutter {
    /// Solids ready to cut from the blank.
    Tools(Vec<Body>),
    /// Profiles and placements for a radial screw sweep.
    Swept(Box<Swept>),
}

/// Plan the cutter and assemble transverse grooves and crossed crests.
///
/// # Errors
/// The kernel refuses invalid thread geometry or a cutter that cannot form a solid.
pub fn grooves(thread: &ConicalThread) -> Result<Cutter> {
    Ok(
        match point_thread::grooves(thread).map_err(|error| match error {
            PlanError::Geometry(error) => status(error),
            PlanError::TooSteep(limit) => Error::ThreadConeTooSteep(limit),
        })? {
            Grooves::Lofted(plans) => Cutter::Tools(plans.iter().map(loft).collect::<Result<_>>()?),
            Grooves::Crossed(plan) => Cutter::Tools(vec![crossed(plan)?]),
            Grooves::Swept(plan) => Cutter::Swept(Box::new(Swept {
                starts: plan
                    .tools
                    .into_iter()
                    .map(|screw| {
                        let (plane, region) = profile(&screw.profile);
                        Start {
                            plane,
                            region,
                            axis: screw.axis,
                            lead: screw.lead,
                        }
                    })
                    .collect(),
                turns: plan.turns,
                law: plan.law,
                trim: plan
                    .trim
                    .map(|frame| solidcore_convert::record_plane(&frame)),
                core: plan.core.as_ref().map(profile),
            })),
        },
    )
}

fn profile(p: &Profile) -> (Plane, Region2) {
    let plane = solidcore_convert::record_plane(&p.frame);
    let outer = solidcore_convert::record_loop2(&p.outline);
    (
        plane,
        Region2 {
            outer,
            holes: Vec::new(),
        },
    )
}

fn curves(p: &Profile) -> Result<Vec<Curve>> {
    let (plane, region) = profile(p);
    region
        .outer
        .segs
        .iter()
        .map(|seg| Curve::from_seg2(seg, &plane).ok_or(Error::Argument))
        .collect()
}

fn loft(plan: &Loft) -> Result<Body> {
    let sections = plan
        .sections
        .iter()
        .map(curves)
        .collect::<Result<Vec<_>>>()?;
    let body = sweep::loft_aligned(&sections, plan.degree, plan.apex)?.through_record()?;
    if let Some(matrix) = plan.placement {
        body.transform(&matrix)?;
        return body.through_record();
    }
    Ok(body)
}

fn revolve(p: &Profile, axis: [f64; 3]) -> Result<Body> {
    sweep::revolve(
        &curves(p)?,
        p.frame.origin,
        axis,
        std::f64::consts::TAU,
        true,
    )
}

/// The lobes in a batch are disjoint. Compound their records to make one Boolean operand.
fn joined(lobes: Vec<Body>) -> Result<Body> {
    if lobes.len() == 1 {
        return lobes.into_iter().next().ok_or(Error::Empty);
    }
    let records = lobes
        .iter()
        .map(|b| b.to_record().map(|(r, _)| r))
        .collect::<Result<Vec<_>>>()?;
    let records: Vec<_> = records.iter().collect();
    let compound = solidcore_record::brep::part::compound(&records).map_err(|_| Error::NotSolid)?;
    Body::from_record(&compound.brep, true).map(|(body, _)| body)
}

fn crossed(plan: Crossed) -> Result<Body> {
    let mut core = revolve(&plan.core, plan.axis)?.through_record()?;
    let mut lobes = plan.lobes.into_iter();
    loop {
        let batch = lobes
            .by_ref()
            .take(256)
            .map(|pieces| {
                let tools = pieces.iter().map(loft).collect::<Result<Vec<_>>>()?;
                if tools.len() == 1 {
                    tools.into_iter().next().ok_or(Error::Empty)
                } else {
                    union_all(tools)?.through_record()
                }
            })
            .collect::<Result<Vec<_>>>()?;
        if batch.is_empty() {
            break;
        }
        core = boolean(core, joined(batch)?, BooleanOp::Union)?
            .ok_or(Error::Empty)?
            .through_record()?;
    }
    let stock = revolve(&plan.stock, plan.axis)?.through_record()?;
    boolean(stock, core, BooleanOp::Subtract)?.ok_or(Error::Empty)
}
