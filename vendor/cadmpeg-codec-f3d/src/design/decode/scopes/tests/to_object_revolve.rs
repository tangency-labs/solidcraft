// SPDX-License-Identifier: Apache-2.0

use super::super::parameter_scope::parse_parameter_scope_with_type_guid;
use super::super::path_feature::exact_path_feature_construction;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::{
    DesignPathFeatureConstruction, DesignRevolveConstruction, DesignRevolveTargets,
    DesignRevolveTravel,
};
use crate::records::identity::Located;
use crate::records::parameters::DesignParameterOwner;
use crate::records::references::DesignClassTag;
use crate::test_support::{indexed_header, lp_utf16};
use cadmpeg_core::decode::u64_from_index;

const REVOLVE_TYPE_GUID: &str = "E3849A15-2FC6-42A0-AF3A-2F1D7273B406";
const SCOPE: u32 = 70;
const AXIS_GROUP: u32 = 83;
const FIRST_TARGET: u32 = 85;
const SECOND_TARGET: u32 = 87;
const OWNER: u32 = 95;
const FINAL_REFERENCE: u32 = OWNER + 2;
const STREAM: &str = "f3d:Design/BulkStream.dat";

/// An admitted to-object frame.
#[derive(Clone, Copy, Debug)]
enum Form {
    OneSided,
    OneSidedBoolean,
    TwoSided,
    UnmarkedTwoSidedBoolean,
}

impl Form {
    fn unmarked(self) -> bool {
        matches!(self, Self::UnmarkedTwoSidedBoolean)
    }

    fn two_sided(self) -> bool {
        matches!(self, Self::TwoSided | Self::UnmarkedTwoSidedBoolean)
    }

    fn boolean(self) -> bool {
        matches!(self, Self::OneSidedBoolean | Self::UnmarkedTwoSidedBoolean)
    }

    fn payload(self) -> usize {
        match self {
            Self::OneSided => 383,
            Self::OneSidedBoolean => 416,
            Self::TwoSided => 415,
            Self::UnmarkedTwoSidedBoolean => 433,
        }
    }
}

#[derive(Clone)]
struct Fixture {
    form: Form,
    operation: u32,
    zero_run: u8,
    marker: u8,
    zero_value: u32,
    extent_kind: u32,
    direction_kind: u8,
    project_axis: u8,
    solid: u8,
    zero_flags: [u8; 3],
    owner_value: f64,
    axis_reference: u32,
    side_two_slot: u8,
    final_reference: u32,
    swapped_targets: bool,
    extra_padding: usize,
    extra_references: bool,
}

impl Fixture {
    fn one_sided() -> Self {
        Self {
            form: Form::OneSided,
            operation: 4,
            zero_run: 0,
            marker: 1,
            zero_value: 0,
            extent_kind: 1,
            direction_kind: 0,
            project_axis: 0,
            solid: 1,
            zero_flags: [0; 3],
            owner_value: 0.0,
            axis_reference: AXIS_GROUP,
            side_two_slot: 0,
            final_reference: FINAL_REFERENCE,
            swapped_targets: false,
            extra_padding: 0,
            extra_references: false,
        }
    }

    fn one_sided_boolean() -> Self {
        Self {
            form: Form::OneSidedBoolean,
            operation: 1,
            ..Self::one_sided()
        }
    }

    fn two_sided() -> Self {
        Self {
            form: Form::TwoSided,
            ..Self::one_sided()
        }
    }

    fn unmarked_two_sided_boolean() -> Self {
        Self {
            form: Form::UnmarkedTwoSidedBoolean,
            operation: 1,
            ..Self::one_sided()
        }
    }

    /// Body group, profile group, axis group, one target group per side, the
    /// owner, and the final reference; each group is followed by its member.
    fn table(&self) -> Vec<u32> {
        let mut table = Vec::new();
        if self.form.boolean() {
            table.extend([79, 80]);
        }
        table.extend([81, 82, AXIS_GROUP, 84, FIRST_TARGET, 86]);
        if self.form.two_sided() {
            table.extend([SECOND_TARGET, 88]);
        }
        if self.extra_references {
            table.extend([89, 90]);
        }
        table.extend([OWNER, FINAL_REFERENCE]);
        table
    }

    /// The side-one target, and the side-two target of a two-sided scope.
    fn targets(&self) -> (u32, Option<u32>) {
        match (self.form.two_sided(), self.swapped_targets) {
            (false, _) => (FIRST_TARGET, None),
            (true, false) => (SECOND_TARGET, Some(FIRST_TARGET)),
            (true, true) => (FIRST_TARGET, Some(SECOND_TARGET)),
        }
    }

    /// The scope frame. The kind is the localized `Révolution`, and zero
    /// padding brings the token-independent frame to its admitted length.
    fn bytes(&self) -> Vec<u8> {
        fn marked(bytes: &mut Vec<u8>, record: u32) {
            bytes.push(1);
            bytes.extend_from_slice(&record.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"331", SCOPE);
        bytes.push(self.zero_run);
        if self.form.unmarked() {
            bytes.extend_from_slice(&[0; 9]);
        } else {
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
        marked(&mut bytes, OWNER);
        marked(&mut bytes, self.axis_reference);
        let (side_one, side_two) = self.targets();
        match side_two {
            Some(record) => marked(&mut bytes, record),
            None => bytes.push(self.side_two_slot),
        }
        marked(&mut bytes, self.final_reference);
        marked(&mut bytes, side_one);
        let table = self.table();
        let padding =
            self.form.payload() + self.extra_padding - bytes.len() - 90 - 11 * table.len();
        bytes.extend(std::iter::repeat_n(0, padding));
        bytes.extend_from_slice(&u32::try_from(table.len()).unwrap().to_le_bytes());
        for &record in &table {
            marked(&mut bytes, record);
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
            "id": format!("{STREAM}:design-parameter-owner#{OWNER}"),
            "byte_offset": 2000,
            "frame_length": 104,
            "class_tag": "351",
            "record_index": OWNER,
            "scope_record_index": SCOPE,
            "local_ordinal": 0,
            "evaluated_value": self.owner_value,
            "evaluated_value_offset": 2040,
            "parameter_record_index": OWNER - 1,
            "owned_ordinal": 3,
            "variant": 0,
            "companion_record_index": OWNER + 1,
        }))
        .unwrap();
        match exact_path_feature_construction(&bytes, &records, &scope, &[owner])? {
            DesignPathFeatureConstruction::Revolve(construction) => Some(construction),
            _ => None,
        }
    }
}

fn targets(construction: &DesignRevolveConstruction) -> DesignRevolveTargets {
    match construction.travel {
        DesignRevolveTravel::ToTargets(targets) => targets,
        DesignRevolveTravel::Angle(_) => panic!("expected a to-object travel"),
    }
}

fn located(value: u32, offset: usize) -> Located<u32> {
    Located {
        value,
        offset: u64_from_index(offset),
    }
}

#[test]
fn a_one_sided_scope_reads_its_target_owner_and_solid_flag() {
    let construction = Fixture::one_sided().decode().expect("one-sided scope");
    assert_eq!(construction.operation, DesignExtrudeOperation::NewBody);
    assert_eq!(construction.solid, Some(true));
    assert_eq!(construction.angle_record_index, OWNER);
    assert!(!construction.reversed);
    assert_eq!(
        targets(&construction),
        DesignRevolveTargets {
            side_one: located(FIRST_TARGET, 74),
            side_two: None,
        }
    );

    let mut surface = Fixture::one_sided();
    surface.solid = 0;
    assert_eq!(surface.decode().map(|c| c.solid), Some(Some(false)));
    let mut projected = Fixture::one_sided();
    projected.project_axis = 1;
    assert!(projected.decode().is_some());

    let boolean = Fixture::one_sided_boolean()
        .decode()
        .expect("one-sided Boolean scope");
    assert_eq!(boolean.operation, DesignExtrudeOperation::Join);
    assert_eq!(targets(&boolean).side_one.value, FIRST_TARGET);
}

#[test]
fn a_two_sided_scope_reads_its_side_two_target_before_its_side_one_target() {
    let construction = Fixture::two_sided().decode().expect("two-sided scope");
    assert_eq!(construction.operation, DesignExtrudeOperation::NewBody);
    assert_eq!(
        targets(&construction),
        DesignRevolveTargets {
            side_one: located(SECOND_TARGET, 84),
            side_two: Some(located(FIRST_TARGET, 62)),
        }
    );

    let construction = Fixture::unmarked_two_sided_boolean()
        .decode()
        .expect("unmarked two-sided Boolean scope");
    assert_eq!(construction.operation, DesignExtrudeOperation::Join);
    assert_eq!(construction.solid, Some(true));
    assert_eq!(
        targets(&construction),
        DesignRevolveTargets {
            side_one: located(SECOND_TARGET, 80),
            side_two: Some(located(FIRST_TARGET, 58)),
        }
    );
}

#[test]
fn every_prologue_field_and_reference_slot_refuses_another_value() {
    let bases = [
        Fixture::one_sided(),
        Fixture::one_sided_boolean(),
        Fixture::two_sided(),
        Fixture::unmarked_two_sided_boolean(),
    ];
    for base in bases {
        let mut cases: Vec<(&str, Fixture)> = Vec::new();
        let mut case = |name, edit: fn(&mut Fixture)| {
            let mut fixture = base.clone();
            edit(&mut fixture);
            cases.push((name, fixture));
        };
        case("extent kind", |f| f.extent_kind = 2);
        case("direction kind", |f| f.direction_kind = 1);
        case("project axis", |f| f.project_axis = 2);
        case("solid flag", |f| f.solid = 2);
        case("zero flags", |f| f.zero_flags = [0, 1, 0]);
        case("nonzero owner", |f| f.owner_value = 1.0);
        case("axis reference", |f| f.axis_reference = 99);
        case("final reference", |f| f.final_reference = OWNER);
        case("frame length", |f| f.extra_padding = 1);
        case("reference count", |f| f.extra_references = true);
        case("operation", |f| {
            f.operation = if f.operation == 4 { 1 } else { 4 }
        });
        if base.form.unmarked() {
            case("zero run", |f| f.zero_run = 1);
        } else {
            case("marker", |f| f.marker = 0);
            case("zero value", |f| f.zero_value = 1);
        }
        if base.form.two_sided() {
            case("target order", |f| f.swapped_targets = true);
        } else {
            case("side-two slot", |f| f.side_two_slot = 2);
        }
        for (name, fixture) in cases {
            assert!(fixture.decode().is_none(), "{name}: {:?}", base.form);
        }
    }
}
