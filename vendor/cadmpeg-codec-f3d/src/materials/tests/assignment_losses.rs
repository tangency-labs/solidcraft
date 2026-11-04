// SPDX-License-Identifier: Apache-2.0

use crate::materials::tests::appearance_loss_report;
use crate::materials::tests::material_losses;
use crate::materials::tests::opaque_appearance;

#[test]
fn appearance_loss_stands_when_no_asset_decodes() {
    let ir = cadmpeg_ir::CadIr::empty();
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, false);
    assert_eq!(material_losses(&report).len(), 1);
}

#[test]
fn appearance_loss_clears_when_an_unassigned_catalog_transfers() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.appearances = vec![opaque_appearance("2F0E19C1-0000-4000-8000-000000000001")];
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, false);
    assert!(material_losses(&report).is_empty());
}

#[test]
fn appearance_loss_counts_assets_whose_assignment_is_unresolved() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.appearances = vec![
        opaque_appearance("2F0E19C1-0000-4000-8000-000000000001"),
        opaque_appearance("2F0E19C1-0000-4000-8000-000000000002"),
    ];
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, true);
    let messages = material_losses(&report);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("2 Protein appearance asset(s)"));
}

#[test]
fn appearance_loss_clears_when_an_assignment_resolves() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let appearance = opaque_appearance("2F0E19C1-0000-4000-8000-000000000001");
    ir.model.appearance_bindings = vec![cadmpeg_ir::appearance::AppearanceBinding {
        id: "f3d:appearance:body#0_1:2F0E19C1-0000-4000-8000-000000000001"
            .try_into()
            .expect("valid identity"),
        target: cadmpeg_ir::appearance::AppearanceTarget::Body(
            cadmpeg_ir::ids::BodyId::mint("f3d:brep/a.smbh/brep:entity#1".to_owned())
                .expect("identity grammar"),
        ),
        appearance: appearance.id.clone(),
        source_entity_id: None,
        object_type: None,
        visible: None,
        channels: std::collections::BTreeMap::new(),
    }];
    ir.model.appearances = vec![appearance];
    let mut report = appearance_loss_report();
    crate::decode::reconcile_appearance_loss(&mut report, &ir, true);
    assert!(material_losses(&report).is_empty());
}
