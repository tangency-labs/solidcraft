// SPDX-License-Identifier: Apache-2.0

use super::{active_row, is_root_record};
use crate::records::configuration::DesignConfigurationActiveRow;
use crate::test_support::configuration_table::root_record;

const ROW: &str = "da23188c-56aa-41fd-a6e8-7b526e965da5";

#[test]
fn configuration_root_names_its_table_and_active_row() {
    let record = root_record(*b"296", 349, "1", ROW);
    assert!(is_root_record(&record));
    assert_eq!(
        active_row(&record),
        Some(DesignConfigurationActiveRow {
            record_index: 349,
            table_id: "1".into(),
            row_id: ROW.into(),
        })
    );
}

#[test]
fn configuration_root_key_selects_the_record() {
    let mut record = root_record(*b"296", 349, "1", ROW);
    record[19] = b'd';
    assert!(!is_root_record(&record));
    let mut record = root_record(*b"296", 349, "1", ROW);
    record[15] = 24;
    assert!(!is_root_record(&record));
}

#[test]
fn configuration_root_outside_its_form_names_no_row() {
    let valid = root_record(*b"296", 349, "1", ROW);
    let end = valid.len();
    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
    for (name, at, byte) in [
        ("zero u32", 11, 1),
        ("first zero-run byte", 42, 1),
        ("last zero-run byte", 52, 1),
        ("constant one", 53, 2),
        ("trailing zero", end - 4, 1),
    ] {
        let mut record = valid.clone();
        record[at] = byte;
        cases.push((name, record));
    }
    let mut record = valid.clone();
    record.push(0);
    cases.push(("trailing byte", record));
    let mut record = valid.clone();
    record.truncate(end - 4);
    cases.push(("missing trailing zero", record));
    cases.push(("empty table id", root_record(*b"296", 349, "", ROW)));
    cases.push(("empty row id", root_record(*b"296", 349, "1", "")));
    for (name, record) in cases {
        assert!(is_root_record(&record), "{name}");
        assert_eq!(active_row(&record), None, "{name}");
    }
}
