// SPDX-License-Identifier: Apache-2.0
use crate::records::{
    decal::DesignRecordHeader,
    feature::scope::{DesignFeatureKind, DesignParameterScope},
    recipes::{ConstructionRecipe, ConstructionRecipeKind},
};
use crate::test_support::indexed_header;

#[test]
fn face_recipe_prefix_type_entry_does_not_terminate_the_operand() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut bytes = Vec::new();
    for index in [700, 700, 701, 702, 703] {
        indexed_header(&mut bytes, *b"317", index);
    }
    let prefix_at = bytes.len();
    indexed_header(&mut bytes, *b"281", 0);
    bytes.extend_from_slice(&24u32.to_le_bytes());
    let name_at = bytes.len();
    bytes.extend_from_slice(b"bounded_face_recipe_data");
    for word in [0i32, -1] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    let next_at = bytes.len();
    indexed_header(&mut bytes, *b"325", 820);
    let scope = DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#60"),
        DesignFeatureKind::OffsetFaces,
        60,
    );
    let header = DesignRecordHeader {
        id: format!("{stream}:design-record-header#0"),
        byte_offset: 0,
        class_tag: "317".to_owned().try_into().unwrap(),
        record_index: 700,
    };
    let recipe = ConstructionRecipe {
        id: format!("{stream}:construction-recipe#{name_at}"),
        byte_offset: u64::try_from(name_at).unwrap(),
        kind: ConstructionRecipeKind::BoundedFace,
        design: None,
        recipe_index: 0,
        record_index: None,
    };
    let operand = super::parse_face_operand(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        (0, None),
        None,
        &header,
        &[recipe],
    )
    .expect("the prefix does not delimit the program");
    assert_eq!(operand.recipe_program, [0, -1]);
    assert_eq!(
        operand.recipe_prefix_offset(),
        u64::try_from(prefix_at).unwrap()
    );
    assert_eq!(operand.recipe_prefix_bytes, &bytes[prefix_at..name_at - 4]);
    assert_eq!(operand.next_record_index, 820);
    assert_eq!(operand.next_byte_offset(), u64::try_from(next_at).unwrap());
}
