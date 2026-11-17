//! A current sheet's bend-slot plan, through the exact kernel primitive.
//!
//! Converts current flat material and bend spans, calls the kernel planner and exact planar
//! differences to certify coverage, and returns record geometry. No coordinate, length,
//! angle or curve is computed in this workflow layer.

use solidcore::bend_slots as kernel;
use solidcore_convert as convert;
use solidcore_record::{Region2, Vec2};

pub use kernel::{
    BendSlotError, BendSlotOptions, BendSlotRow, FoldedBend, MAX_BENDS, MAX_CUTS,
    MAX_PICK_TOPOLOGY, MAX_PROFILE_SEGMENTS, MAX_SOURCE_BENDS,
};

/// One current uninterrupted bend, in the sheet's own flat coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BendSpan {
    /// Current centre-line start.
    pub start: Vec2,
    /// Current centre-line end.
    pub end: Vec2,
}

/// Exact slot and end-relief cuts, with the integer rows selected by the kernel.
#[derive(Clone, Debug, PartialEq)]
pub struct BendSlotPlan {
    /// Closed internal slot profiles.
    pub slots: Vec<Region2>,
    /// Open-edge relief profiles.
    pub reliefs: Vec<Region2>,
    /// One integer-pattern record per input bend.
    pub rows: Vec<BendSlotRow>,
}

/// Plan cuts in the current material, refusing boundary, opening and cut intersections.
pub fn plan(
    material: &[Region2],
    bends: &[BendSpan],
    options: &BendSlotOptions,
) -> Result<BendSlotPlan, BendSlotError> {
    // Bound data copies before converting a public caller's record vectors.
    if bends.is_empty() {
        return Err(BendSlotError::Parameter("at least one bend is required"));
    }
    if bends.len() > MAX_BENDS {
        return Err(BendSlotError::Budget);
    }
    let [source] = material else {
        return Err(BendSlotError::Material(
            "one connected flat material region is required",
        ));
    };
    if source.holes.len() > MAX_PROFILE_SEGMENTS {
        return Err(BendSlotError::Budget);
    }
    let segments = source.holes.iter().fold(source.outer.segs.len(), |n, h| {
        n.saturating_add(h.segs.len())
    });
    if segments > MAX_PROFILE_SEGMENTS {
        return Err(BendSlotError::Budget);
    }
    let field: Vec<_> = material.iter().map(convert::region2).collect();
    let spans: Vec<_> = bends
        .iter()
        .map(|b| kernel::BendSpan {
            start: convert::vec2(b.start),
            end: convert::vec2(b.end),
        })
        .collect();
    let plan = kernel::plan(&field, &spans, options)?;
    let bands: Vec<_> = plan
        .bend_bands
        .iter()
        .map(convert::record_region2)
        .collect();
    let outside =
        crate::prism::planar_regions_boolean(&bands, material, crate::BooleanOp::Subtract, 1e-7)
            .map_err(|error| BendSlotError::Geometry(error.to_string()))?;
    if !outside.is_empty() {
        return Err(BendSlotError::Material(
            "a selected bend band crosses an exterior edge or an existing opening",
        ));
    }
    let result = BendSlotPlan {
        slots: plan.slots.iter().map(convert::record_region2).collect(),
        reliefs: plan.reliefs.iter().map(convert::record_region2).collect(),
        rows: plan.rows,
    };
    let outside = crate::prism::planar_regions_boolean(
        &result.reliefs,
        material,
        crate::BooleanOp::Subtract,
        1e-7,
    )
    .map_err(|error| BendSlotError::Geometry(error.to_string()))?;
    if !outside.is_empty() {
        return Err(BendSlotError::Material(
            "an end relief crosses another exterior edge",
        ));
    }
    Ok(result)
}

/// Exact B-rep edge indices that belong uniquely to one current source bend.
///
/// Only actual two-face edges with an adjacent analytic cylinder and a line/circle label
/// participate. The kernel certifies the trimmed curve, including a stored rational form;
/// unsupported edges are absent rather than assigned to the nearest bend. Indices here
/// belong only to this current B-rep; callers persist the resolved source bend identity.
pub fn edge_bends(
    record: &solidcore_record::brep::Brep,
    bends: &[FoldedBend],
) -> Result<Vec<(usize, usize)>, BendSlotError> {
    use solidcore_record::brep::{Curve, Surface};
    if bends.len() > MAX_SOURCE_BENDS
        || record.edges.len() > MAX_PICK_TOPOLOGY
        || record.faces.len() > MAX_PICK_TOPOLOGY
        || record
            .edges
            .iter()
            .fold(0usize, |n, e| n.saturating_add(e.uses.len()))
            > MAX_PICK_TOPOLOGY.saturating_mul(4)
    {
        return Err(BendSlotError::Budget);
    }
    let mut out = Vec::new();
    for (edge_index, edge) in record.edges.iter().enumerate() {
        if !matches!(edge.curve, Curve::Line { .. } | Curve::Circle { .. }) || edge.uses.len() != 2
        {
            continue;
        }
        let mut faces = Vec::new();
        let mut adjacent = Vec::new();
        for use_ in &edge.uses {
            let face_index = use_.face as usize;
            let face = record
                .faces
                .get(face_index)
                .ok_or(BendSlotError::Parameter("invalid bend-edge face index"))?;
            if faces.contains(&face_index) {
                continue;
            }
            faces.push(face_index);
            if let Surface::Cylinder { frame, radius } = &face.surface {
                adjacent.push(kernel::BendCylinder {
                    frame: convert::frame(frame),
                    radius: *radius,
                });
            }
        }
        if faces.len() != 2 || adjacent.is_empty() {
            continue;
        }
        let actual = edge.nurbs.as_ref().unwrap_or(&edge.curve);
        let bounded = match actual {
            Curve::Line { .. } | Curve::Circle { .. } => true,
            Curve::Spline { knots, points } => knots.len() <= 160 && points.len() <= 128,
            Curve::RationalSpline { knots, homogeneous } => {
                knots.len() <= 160 && homogeneous.len() <= 128
            }
            _ => false,
        };
        if !bounded {
            continue;
        }
        let curve = convert::curve(actual)
            .map_err(|_| BendSlotError::Parameter("invalid bend-edge curve"))?;
        if let Some(bend) =
            kernel::source_bend_for_edge(&curve, edge.interval, &adjacent, bends, 1e-7)?
        {
            out.push((edge_index, bend));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;
    use solidcore_record::{Loop2, Seg2};

    fn options() -> BendSlotOptions {
        BendSlotOptions {
            slot_length: 8.0,
            slot_width: 1.2,
            bridge: 4.0,
            end_margin: 3.0,
            relief_depth: 4.0,
            corner_radius: 2.0,
            rounded: true,
            reliefs: true,
        }
    }

    fn material(notch: bool) -> Vec<Region2> {
        let mut points = vec![
            Vec2::new(0.0, -25.0),
            Vec2::new(88.0, -25.0),
            Vec2::new(88.0, 25.0),
        ];
        if notch {
            points.extend([
                Vec2::new(1.3, 25.0),
                Vec2::new(1.3, 0.7),
                Vec2::new(1.0, 0.7),
                Vec2::new(1.0, 25.0),
            ]);
        }
        points.push(Vec2::new(0.0, 25.0));
        vec![Region2 {
            outer: Loop2::polygon(&points),
            holes: Vec::new(),
        }]
    }

    fn spans() -> [BendSpan; 1] {
        [BendSpan {
            start: Vec2::new(0.0, 0.0),
            end: Vec2::new(88.0, 0.0),
        }]
    }

    #[test]
    fn current_material_covers_every_band_and_end_relief() {
        let plan = plan(&material(false), &spans(), &options()).unwrap();
        assert_eq!(plan.slots.len(), 6);
        assert_eq!(plan.reliefs.len(), 2);
    }

    #[test]
    fn exact_coverage_refuses_a_concave_edge_past_the_relief_entry_probe() {
        // A narrow notch reaches only the relief's upper entry. The mouth is whole,
        // the centre-line band is covered, and the entry probe remains inside the panel.
        // The full relief still crosses the notch and must not be silently clipped.
        let error = plan(&material(true), &spans(), &options()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("relief crosses another exterior edge"),
            "{error}"
        );
    }

    #[test]
    fn an_existing_opening_between_slots_still_interrupts_the_bend() {
        let mut material = material(false);
        let opening = Loop2 {
            segs: vec![Seg2::Arc {
                center: Vec2::new(20.0, 0.0),
                radius: 0.3,
                start: 0.0,
                sweep: std::f64::consts::TAU,
            }],
        };
        material[0].holes.push(opening);
        let error = plan(&material, &spans(), &options()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("interrupted by an existing opening"),
            "{error}"
        );
    }
}
