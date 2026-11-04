// SPDX-License-Identifier: Apache-2.0

use super::{
    carrier, external_owner_table, nested_fixture, two_component_stream, BTreeSet,
    DesignComponentOccurrencePlacement, COMPONENT_ROOT, DOCUMENT_ROOT,
};

#[test]
fn an_identity_owner_carrier_places_external_members_when_body_projection_is_deferred() {
    let components = two_component_stream(Some(276));
    let carriers = [carrier(
        225,
        "00000000-0000-4000-8000-000000000225",
        DesignComponentOccurrencePlacement::Identity {
            ordinal: std::num::NonZeroU32::new(2).unwrap(),
        },
        DOCUMENT_ROOT,
    )];
    assert!(carriers[0].transform().is_none());
    let sources = super::super::LocalComponentSources {
        hidden_bodies: None,
        scopes: &[],
        occurrences: &carriers,
        paths: &[],
        components: &components,
        body_bindings: &[],
        bodies: None,
        root_ordinal_start: 0,
        limits: cadmpeg_core::decode::ResourceLimits::default(),
    };
    let local = super::super::project_local_components(&sources).unwrap();
    assert!(local.occurrences.is_empty());
    let projection = crate::xref::project_occurrences(
        &external_owner_table(COMPONENT_ROOT),
        &sources,
        &local.occurrences,
    )
    .unwrap();
    assert!(projection.losses.is_empty());
    assert_eq!(projection.occurrences.len(), 1);
    assert_eq!(projection.occurrences[0].transform.rows()[0][3], 20.0);
}

#[test]
fn a_unique_owner_chain_composes_and_unresolved_owners_keep_the_root_placement() {
    for case in ["unique", "ambiguous", "unreachable", "cycle"] {
        let (mut carriers, components) = nested_fixture();
        if case != "ambiguous" {
            carriers.pop();
        }
        if case == "unreachable" {
            carriers[0].owner_entity = 999;
        }
        if case == "cycle" {
            carriers[0].owner_entity = COMPONENT_ROOT;
        }
        let bodies = BTreeSet::new();
        let sources = super::super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: &carriers,
            paths: &[],
            components: &components,
            body_bindings: &[],
            bodies: Some(&bodies),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        };
        let projection =
            crate::xref::project_occurrences(&external_owner_table(COMPONENT_ROOT), &sources, &[])
                .unwrap();
        if case == "unique" {
            assert!(projection.losses.is_empty());
            assert_eq!(projection.occurrences.len(), 1);
            assert_eq!(
                projection.occurrences[0].transform.rows(),
                [
                    [0.0, -1.0, 0.0, 30.0],
                    [1.0, 0.0, 0.0, 60.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0]
                ]
            );
        } else {
            assert_eq!(projection.occurrences.len(), 1, "{case}");
            assert_eq!(
                projection.occurrences[0].parent,
                cadmpeg_ir::products::OccurrenceParent::Root {}
            );
            assert_eq!(projection.occurrences[0].transform.rows()[0][3], 20.0);
            assert_eq!(projection.losses.len(), 1, "{case}");
            assert_eq!(
                projection.losses[0].code,
                crate::loss::F3dLossCode::XrefOwnerUnresolved.kind()
            );
        }
    }
}
