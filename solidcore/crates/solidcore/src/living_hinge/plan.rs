//! The planner's entry (design 6): the area and the plate as loops in the hinge frame, row by row
//! through spans, clean crossings and keep-out zones to the slots.
//!
//! The frame and the area stay inputs, not a sketch: a sheet-metal bend zone feeds the same
//! entry later (sheet-metal addendum).

use std::f64::consts::FRAC_PI_2;

use super::lattice::{FrameBox, Lattice};
use super::near::{sweeps, NearEdge};
use super::rows::plan_rows_zoned;
use super::spans::{clean_ends, row_spans, row_zone};
use super::{LivingHingeError, LivingHingePlan, LivingHingeSpec};

/// The box of `loops` in the frame; an arc counts its ends and the points where it is furthest
/// along `u` or `v`. Empty loops give an inverted box, which the lattice refuses.
pub fn frame_box(loops: &[Vec<NearEdge>]) -> FrameBox {
    let mut b = FrameBox {
        u_min: f64::INFINITY,
        u_max: f64::NEG_INFINITY,
        v_min: f64::INFINITY,
        v_max: f64::NEG_INFINITY,
    };
    let mut add = |p: (f64, f64)| {
        b.u_min = b.u_min.min(p.0);
        b.u_max = b.u_max.max(p.0);
        b.v_min = b.v_min.min(p.1);
        b.v_max = b.v_max.max(p.1);
    };
    for edge in loops.iter().flatten() {
        match *edge {
            NearEdge::Line(a, c) => {
                add(a);
                add(c);
            }
            NearEdge::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let at = |a: f64| (center.0 + radius * a.cos(), center.1 + radius * a.sin());
                add(at(start));
                add(at(start + sweep));
                for quarter in 0_i32..4 {
                    let a = f64::from(quarter) * FRAC_PI_2;
                    if sweeps(start, sweep, a) {
                        add(at(a));
                    }
                }
            }
        }
    }
    b
}

/// Plans the slots of the area `field` in the plate `plate`, both closed loops in the hinge frame
/// (a conic or a Bézier as its chords). `closed` are the area's edges that lie inside the plate
/// (not on its boundary): slots keep `r + m` from them. The lattice is centred on the area's box,
/// or anchored at `anchor`. Each row: its spans (design 6.3), the open tags of crossings that are
/// not clean dropped (6.6 rule 1), its zone `Z_c ∪ Z_o` (6.5), then runners, pieces and short
/// pieces (6.6 to 6.9).
pub fn plan(
    field: &[Vec<NearEdge>],
    plate: &[Vec<NearEdge>],
    closed: &[NearEdge],
    spec: &LivingHingeSpec,
    anchor: Option<(f64, f64)>,
) -> Result<LivingHingePlan, LivingHingeError> {
    spec.check()?;
    if plate.iter().all(Vec::is_empty) {
        return Err(LivingHingeError::Plate("it has no edges".into()));
    }
    let b = frame_box(field);
    let lattice = match anchor {
        Some(a) => Lattice::anchored(spec, &b, a)?,
        None => Lattice::centred(spec, &b)?,
    };
    let rho = spec.edge_reach();
    let closed_reach = spec.radius() + spec.margin;
    Ok(plan_rows_zoned(
        spec,
        &lattice,
        |_, v| {
            let mut spans = row_spans(field, plate, v);
            clean_ends(&mut spans, plate, closed, v, rho, closed_reach);
            spans
        },
        |_, v| row_zone(plate, closed, v, rho, closed_reach),
    ))
}
