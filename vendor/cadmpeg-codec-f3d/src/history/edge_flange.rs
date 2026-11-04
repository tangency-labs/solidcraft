// SPDX-License-Identifier: Apache-2.0

use super::{historical_edge_axis, parallel_directions, same_axis_line, unique_history_state_pair};
use crate::history_records::{AsmHistoricalCylinder, AsmHistory};
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_ir::{math::Vector3, scalar::PositiveLength};

const PAIR_THICKNESS_REL_TOLERANCE: f64 = 1.0e-7;
const COMMON_RADIUS_REL_TOLERANCE: f64 = 1.0e-7;

pub(crate) fn inside_radius_for_scope(
    scope: &DesignParameterScope,
    edge_slots: [i64; 2],
    thickness: f64,
    histories: &[AsmHistory],
) -> Option<PositiveLength> {
    let state_id = scope.history_state_id()?;
    let previous_id = super::effective_scope_previous_history_state_id(scope, histories)?;
    let (history, state, previous) = unique_history_state_pair(histories, state_id, previous_id)?;
    let previous_topology = history.topology(previous)?;
    let current_topology = history.topology(state)?;
    let transition = state.transition.as_ref()?;
    let axes = [
        historical_edge_axis(edge_slots[0], previous_topology)?.1,
        historical_edge_axis(edge_slots[1], previous_topology)?.1,
    ];
    common_inside_radius(
        current_topology
            .surface_cylinders
            .iter()
            .filter(|cylinder| {
                transition
                    .topology
                    .surfaces
                    .inserted
                    .contains(&cylinder.surface)
                    && !previous_topology
                        .surface_cylinders
                        .iter()
                        .any(|old| old.surface == cylinder.surface)
            }),
        axes,
        thickness,
    )
}

fn common_inside_radius<'a>(
    cylinders: impl Iterator<Item = &'a AsmHistoricalCylinder>,
    axes: [Vector3; 2],
    thickness: f64,
) -> Option<PositiveLength> {
    if !thickness.is_finite() || thickness <= 0.0 || parallel_directions(axes[0], axes[1]) {
        return None;
    }
    let mut pairs: [[Option<&AsmHistoricalCylinder>; 2]; 2] = [[None; 2]; 2];
    for cylinder in cylinders {
        let matches = axes.map(|axis| parallel_directions(axis, cylinder.axis));
        let ordinal = match matches {
            [true, false] => 0,
            [false, true] => 1,
            _ => return None,
        };
        let pair = pairs.get_mut(ordinal)?;
        let slot = pair.iter_mut().find(|slot| slot.is_none())?;
        *slot = Some(cylinder);
    }
    let mut common: Option<f64> = None;
    for pair in pairs {
        let [Some(first), Some(second)] = pair else {
            return None;
        };
        if !same_axis_line((first.origin, first.axis), (second.origin, second.axis)) {
            return None;
        }
        let inner = first.radius.min(second.radius);
        let outer = first.radius.max(second.radius);
        if !first.radius.is_finite() || !second.radius.is_finite() || inner <= 0.0 || outer <= inner
        {
            return None;
        }
        let thickness_tolerance = PAIR_THICKNESS_REL_TOLERANCE * (1.0 + outer.abs() + thickness);
        if ((outer - inner) - thickness).abs() > thickness_tolerance {
            return None;
        }
        if let Some(previous) = common {
            let radius_tolerance = COMMON_RADIUS_REL_TOLERANCE * (1.0 + inner.abs());
            if f64::abs(previous - inner) > radius_tolerance {
                return None;
            }
        } else {
            common = Some(inner);
        }
    }
    PositiveLength::new(common?)
}

#[cfg(test)]
mod tests;
