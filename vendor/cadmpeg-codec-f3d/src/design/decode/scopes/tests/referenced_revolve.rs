// SPDX-License-Identifier: Apache-2.0

use super::super::parameter_scope::parse_parameter_scope_with_type_guid;
use super::super::path_feature::exact_path_feature_construction;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::{
    DesignPathFeatureConstruction, DesignRevolveConstruction,
};
use crate::records::parameters::DesignParameterOwner;
use crate::records::references::DesignClassTag;
use crate::test_support::{indexed_header, lp_utf16};

const REVOLVE_TYPE_GUID: &str = "E3849A15-2FC6-42A0-AF3A-2F1D7273B406";
const SCOPE: u32 = 70;
const ANGLE: u32 = 95;
const STREAM: &str = "f3d:Design/BulkStream.dat";

#[derive(Clone)]
struct Fixture {
    unmarked: bool,
    references: usize,
    operation: u32,
    zero_run: u8,
    marker: u8,
    zero_value: u32,
    extent_kind: u32,
    direction_kind: u8,
    project_axis: u8,
    solid: u8,
    zero_flags: [u8; 3],
    angle_marker: u8,
    angle: f64,
}

impl Fixture {
    fn marked() -> Self {
        Self {
            unmarked: false,
            references: 6,
            operation: 4,
            zero_run: 0,
            marker: 1,
            zero_value: 0,
            extent_kind: 2,
            direction_kind: 0,
            project_axis: 0,
            solid: 1,
            zero_flags: [0; 3],
            angle_marker: 1,
            angle: 1.5,
        }
    }

    fn unmarked() -> Self {
        Self {
            unmarked: true,
            references: 8,
            operation: 1,
            ..Self::marked()
        }
    }

    /// The scope frame and its angle owner's scalar frame. The kind is the
    /// localized `Révolution`, and zero padding brings the token-independent
    /// frame to 351 bytes for the marked form or 369 for the unmarked form.
    fn bytes(&self) -> Vec<u8> {
        fn marked(bytes: &mut Vec<u8>, marker: u8, record: u32) {
            bytes.push(marker);
            bytes.extend_from_slice(&record.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"331", SCOPE);
        if self.unmarked {
            bytes.push(self.zero_run);
            bytes.extend_from_slice(&[0; 9]);
        } else {
            bytes.push(self.zero_run);
            bytes.extend_from_slice(&[0; 8]);
            bytes.push(self.marker);
            bytes.extend_from_slice(&self.zero_value.to_le_bytes());
        }
        bytes.extend_from_slice(&self.operation.to_le_bytes());
        bytes.extend_from_slice(&self.extent_kind.to_le_bytes());
        bytes.push(self.direction_kind);
        bytes.push(self.project_axis);
        bytes.push(self.solid);
        bytes.extend_from_slice(&self.zero_flags);
        marked(&mut bytes, self.angle_marker, ANGLE);
        let payload = if self.unmarked { 369 } else { 351 };
        let table = [81_u32, 82, 91, 92, 93, 94, ANGLE, 96];
        let table = &table[table.len() - self.references..];
        let padding = payload - bytes.len() - 90 - 11 * table.len();
        bytes.extend(std::iter::repeat_n(0, padding));
        bytes.extend_from_slice(&u32::try_from(table.len()).unwrap().to_le_bytes());
        for &record in table {
            marked(&mut bytes, 1, record);
        }
        bytes.extend_from_slice(&17_u32.to_le_bytes());
        lp_utf16(&mut bytes, "Révolution");
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&2_u32.to_le_bytes());
        tail[31..35].copy_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        indexed_header(&mut bytes, *b"258", SCOPE);
        bytes
    }

    fn decode(&self) -> Option<DesignRevolveConstruction> {
        let bytes = self.bytes();
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let class_tag = DesignClassTag::try_from("331".to_owned()).unwrap();
        let mut scope = parse_parameter_scope_with_type_guid(
            &bytes,
            &records,
            SCOPE,
            &class_tag,
            0,
            Some(REVOLVE_TYPE_GUID),
        )
        .unwrap()
        .expect("Revolve scope");
        scope.id = format!("{STREAM}:design-parameter-scope#0");
        let owner: DesignParameterOwner = serde_json::from_value(serde_json::json!({
            "id": format!("{STREAM}:design-parameter-owner#{ANGLE}"),
            "byte_offset": 2000,
            "frame_length": 104,
            "class_tag": "351",
            "record_index": ANGLE,
            "scope_record_index": SCOPE,
            "local_ordinal": 0,
            "evaluated_value": self.angle,
            "evaluated_value_offset": 2040,
            "parameter_record_index": ANGLE + 1,
            "owned_ordinal": 1,
            "variant": 0,
            "companion_record_index": ANGLE + 2,
        }))
        .unwrap();
        match exact_path_feature_construction(&bytes, &records, &scope, &[owner])? {
            DesignPathFeatureConstruction::Revolve(construction) => Some(construction),
            _ => None,
        }
    }
}

#[test]
fn the_marked_form_reads_its_solid_flag() {
    let solid = Fixture::marked().decode().expect("solid revolve");
    assert_eq!(solid.solid, Some(true));
    assert_eq!(solid.operation, DesignExtrudeOperation::NewBody);
    assert_eq!(
        solid.travel,
        crate::records::feature::path_features::DesignRevolveTravel::Angle(
            cadmpeg_ir::scalar::PositiveAngle::new(1.5).unwrap()
        )
    );
    assert_eq!(solid.angle_record_index, ANGLE);
    assert!(!solid.reversed);
    let mut surface = Fixture::marked();
    surface.solid = 0;
    assert_eq!(surface.decode().unwrap().solid, Some(false));
    for project_axis in [0, 1] {
        let mut fixture = surface.clone();
        fixture.project_axis = project_axis;
        assert_eq!(
            fixture.decode().map(|c| c.solid),
            Some(Some(false)),
            "{project_axis}"
        );
    }
    let mut invalid = Fixture::marked();
    invalid.solid = 2;
    assert!(invalid.decode().is_none());
}

#[test]
fn the_unmarked_form_has_eight_references_and_its_operation_at_offset_21() {
    for (operation, expected) in [
        (1, DesignExtrudeOperation::Join),
        (2, DesignExtrudeOperation::Cut),
    ] {
        let mut fixture = Fixture::unmarked();
        fixture.operation = operation;
        let construction = fixture.decode().expect("unmarked revolve");
        assert_eq!(construction.operation, expected);
        assert_eq!(construction.solid, Some(true));
        assert_eq!(construction.angle_record_index, ANGLE);
        let mut surface = fixture.clone();
        surface.solid = 0;
        assert_eq!(surface.decode().unwrap().solid, Some(false));
    }
    let mut six = Fixture::unmarked();
    six.references = 6;
    assert!(six.decode().is_none());
    let mut eight = Fixture::marked();
    eight.references = 8;
    assert!(eight.decode().is_none());
}

#[test]
fn both_forms_refuse_other_prologue_values() {
    for base in [Fixture::marked(), Fixture::unmarked()] {
        let mut cases = Vec::new();
        for (index, byte) in [(0, 1), (1, 1), (2, 1)] {
            let mut fixture = base.clone();
            fixture.zero_flags[index] = byte;
            cases.push(("zero flag", fixture));
        }
        let mut fixture = base.clone();
        fixture.project_axis = 2;
        cases.push(("project axis", fixture));
        let mut fixture = base.clone();
        fixture.extent_kind = 1;
        cases.push(("extent kind", fixture));
        let mut fixture = base.clone();
        fixture.direction_kind = 1;
        cases.push(("direction kind", fixture));
        if base.unmarked {
            let mut fixture = base.clone();
            fixture.zero_run = 1;
            cases.push(("zero run", fixture));
        } else {
            for marker in [0, 2] {
                let mut fixture = base.clone();
                fixture.marker = marker;
                cases.push(("marker", fixture));
            }
            let mut fixture = base.clone();
            fixture.zero_value = 1;
            cases.push(("zero value", fixture));
        }
        let mut fixture = base.clone();
        fixture.angle_marker = 0;
        cases.push(("angle marker", fixture));
        for (name, fixture) in cases {
            assert!(
                fixture.decode().is_none(),
                "{name} unmarked={}",
                base.unmarked
            );
        }
    }
}

#[test]
fn a_negative_angle_states_its_magnitude_and_a_reversed_turn() {
    for base in [Fixture::marked(), Fixture::unmarked()] {
        let mut negative = base.clone();
        negative.angle = -1.5;
        let construction = negative.decode().expect("negative angle");
        assert_eq!(
            construction.travel,
            crate::records::feature::path_features::DesignRevolveTravel::Angle(
                cadmpeg_ir::scalar::PositiveAngle::new(1.5).unwrap()
            )
        );
        assert!(construction.reversed);
        let mut zero = base.clone();
        zero.angle = 0.0;
        assert!(zero.decode().is_none());
    }
}
