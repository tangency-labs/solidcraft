// SPDX-License-Identifier: Apache-2.0

use serde_json::{json, Value};

fn row(kind: &str, target: &str) -> Value {
    json!({
        "from": "root.f3d", "relativePath": target, "type": kind,
        "properties": [
            {"neutronRole": {"value": "role", "dataType": "STRING"}},
            {"neutronData": {"value": "data", "dataType": "STRING"}}
        ]
    })
}

fn table(rows: Vec<Value>) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "name": "RedirectionsStream", "schema-version": 0,
        "designs": [{"file-version": 1, "targetFileName": "root.f3d",
                     "displayName": "root", "lineageUrn": "urn:l", "versionUrn": "urn:v"}],
        "references": rows
    }))
    .unwrap()
}

#[test]
fn reference_rows_preserve_types_and_source_ordinals() {
    for (kinds, expected_xrefs, expected_derived) in [
        (vec!["XREF"], vec![0], vec![]),
        (vec!["DERIVED"], vec![], vec![0]),
        (
            vec!["DERIVED", "XREF", "DERIVED", "XREF"],
            vec![1, 3],
            vec![0, 2],
        ),
    ] {
        let bytes = table(kinds.iter().map(|kind| row(kind, "part.f3d")).collect());
        let parsed =
            crate::xref::parse(&cadmpeg_test_support::service_decode_context(), &bytes).unwrap();
        assert_eq!(
            parsed
                .references
                .iter()
                .map(|r| r.ordinal)
                .collect::<Vec<_>>(),
            expected_xrefs
        );
        assert_eq!(
            parsed
                .derived_references
                .iter()
                .map(|r| r.ordinal)
                .collect::<Vec<_>>(),
            expected_derived
        );
        for r in &parsed.derived_references {
            assert_eq!(r.id, format!("f3d:derived:reference#{}", r.ordinal));
            assert_eq!(r.from.as_str(), "root.f3d");
            assert_eq!(r.relative_path.as_str(), "part.f3d");
            assert_eq!(r.neutron_role.as_str(), "role");
            assert_eq!(r.neutron_data, "data");
        }
        assert_eq!(
            crate::xref::root_occurrences(&parsed).unwrap().len(),
            expected_xrefs.len()
        );
    }
}

#[test]
fn unknown_reference_type_refuses_the_whole_table() {
    for kinds in [
        vec!["UNKNOWN"],
        vec!["XREF", "UNKNOWN"],
        vec!["DERIVED", "UNKNOWN", "XREF"],
    ] {
        let bytes = table(kinds.iter().map(|kind| row(kind, "part.f3d")).collect());
        assert!(
            crate::xref::parse(&cadmpeg_test_support::service_decode_context(), &bytes).is_err()
        );
    }
}

#[test]
fn malformed_derived_rows_refuse_the_table() {
    let valid = row("DERIVED", "source.f3d");
    let mut cases = Vec::new();
    for field in ["from", "relativePath"] {
        let mut invalid = valid.clone();
        invalid[field] = json!("");
        cases.push(invalid);
    }
    for properties in [
        json!([]),
        json!([{"neutronRole":{"dataType":"STRING","value":"role"}}]),
        json!([{"neutronRole":{"dataType":"STRING","value":"role"}}, {"neutronRole":{"dataType":"STRING","value":"role"}}]),
        json!([{"neutronRole":{"dataType":"NUMBER","value":"role"}}, {"neutronData":{"dataType":"STRING","value":"data"}}]),
    ] {
        let mut invalid = valid.clone();
        invalid["properties"] = properties;
        cases.push(invalid);
    }
    for invalid in cases {
        let bytes = table(vec![row("XREF", "child.f3d"), invalid]);
        assert!(
            crate::xref::parse(&cadmpeg_test_support::service_decode_context(), &bytes).is_err()
        );
    }
}

#[test]
fn omitted_mixed_references_reserve_zero_local_root_ordinals() {
    for mixed in [false, true] {
        let mut rows = vec![row("XREF", "first.f3d"), row("XREF", "second.f3d")];
        if mixed {
            rows.push(row("DERIVED", "source.f3d"));
        }
        let mut parsed = crate::xref::parse(
            &cadmpeg_test_support::service_decode_context(),
            &table(rows),
        )
        .unwrap();
        assert_eq!(
            crate::xref::local_root_ordinal_start(&parsed),
            if mixed { 0 } else { 2 }
        );
        parsed.references[0].transform = Some(crate::xref::IDENTITY_ROWS.try_into().unwrap());
        assert_eq!(
            crate::xref::local_root_ordinal_start(&parsed),
            if mixed { 1 } else { 2 }
        );
        parsed.references[1].transform = Some(crate::xref::IDENTITY_ROWS.try_into().unwrap());
        assert_eq!(crate::xref::local_root_ordinal_start(&parsed), 2);
        parsed.references[0].transform = None;
        assert_eq!(crate::xref::local_root_ordinal_start(&parsed), 2);
    }
}
