// SPDX-License-Identifier: Apache-2.0

use super::body_target_groups::{group, SCOPE};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::construction::DesignConstructionOperandRole;
use crate::records::topology::extrude_selection::DesignOperandRole;

fn scope(kind: DesignFeatureKind, record_index: u32) -> DesignParameterScope {
    DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{record_index}"),
        kind,
        record_index,
    )
}

#[test]
fn only_a_role_0x21_group_of_a_loft_scope_is_a_loft_point_group() {
    let point = group(DesignConstructionOperandRole::Other(
        DesignOperandRole::ROLE_0X21,
    ));
    let profile = group(DesignConstructionOperandRole::Other(
        DesignOperandRole::PROFILE,
    ));
    let loft = [scope(DesignFeatureKind::Loft, SCOPE)];
    assert!(crate::design::decode::operands::is_loft_point_group(
        &point, &loft
    ));
    assert!(!crate::design::decode::operands::is_loft_point_group(
        &profile, &loft
    ));
    for scopes in [
        vec![scope(DesignFeatureKind::Hole, SCOPE)],
        vec![scope(DesignFeatureKind::Loft, SCOPE + 1)],
        Vec::new(),
        vec![
            scope(DesignFeatureKind::Loft, SCOPE),
            scope(DesignFeatureKind::Hole, SCOPE),
        ],
        vec![
            scope(DesignFeatureKind::Loft, SCOPE),
            scope(DesignFeatureKind::Loft, SCOPE),
        ],
    ] {
        assert!(!crate::design::decode::operands::is_loft_point_group(
            &point, &scopes
        ));
    }
}
