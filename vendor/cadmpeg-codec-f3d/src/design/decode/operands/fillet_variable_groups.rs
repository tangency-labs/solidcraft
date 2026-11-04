// SPDX-License-Identifier: Apache-2.0
//! Assign ordered variable-radius parameter runs to Fillet groups.

use super::{
    push_fillet_radius_group, DesignConstructionOperandGroup, DesignFilletRadiusGroup,
    DesignFilletRadiusLaw, DesignParameter,
};

pub(super) fn assign(
    stream: &str,
    groups: &[&DesignConstructionOperandGroup],
    parameters: &[(u32, &DesignParameter)],
) -> Option<Vec<DesignFilletRadiusGroup>> {
    let weighted = parameters.first()?.1.source_kind() == "TangencyWeight";
    let first_kind = if weighted {
        "TangencyWeight"
    } else {
        "StartRadius"
    };
    let starts = parameters
        .iter()
        .enumerate()
        .filter_map(|(index, (_, parameter))| {
            (parameter.source_kind() == first_kind).then_some(index)
        })
        .collect::<Vec<_>>();
    if starts.len() != groups.len() || starts.first() != Some(&0) {
        return None;
    }
    let mut out = Vec::new();
    for (ordinal, group) in groups.iter().enumerate() {
        let end = starts.get(ordinal + 1).copied().unwrap_or(parameters.len());
        let mut run = &parameters[starts[ordinal]..end];
        let weight = if weighted {
            let weight = run.first()?.1.record_index;
            run = &run[1..];
            Some(weight)
        } else {
            None
        };
        let [(.., start), (.., end), rest @ ..] = run else {
            return None;
        };
        if start.source_kind() != "StartRadius" || end.source_kind() != "EndRadius" {
            return None;
        }
        let midpoint_count = rest
            .iter()
            .take_while(|(_, parameter)| parameter.source_kind() == "MidRadius")
            .count();
        let (radii, positions) = rest.split_at(midpoint_count);
        if radii.len() != positions.len()
            || positions
                .iter()
                .any(|(_, parameter)| parameter.source_kind() != "MidParams")
        {
            return None;
        }
        let middle = radii
            .iter()
            .zip(positions)
            .map(|((_, radius), (_, position))| {
                crate::records::topology::fillet::DesignFilletMidpoint {
                    radius_parameter_record_index: radius.record_index,
                    parameter_record_index: position.record_index,
                }
            })
            .collect();
        push_fillet_radius_group(
            &mut out,
            stream,
            u32::try_from(ordinal).ok()?,
            group,
            DesignFilletRadiusLaw::Variable {
                start_radius_parameter_record_index: start.record_index,
                end_radius_parameter_record_index: end.record_index,
                middle,
            },
            weight,
        );
    }
    Some(out)
}
