//! The difftest output of an [`AssertArray`] (keep in sync with
//! `harness/cpp/shim/sc_assert_reports.h`): `ok`, the count, then per report the
//! reporting type, list index, test index, assert type, ok flag, reporter / owner /
//! other identities, owner and other types, tolerance, value, 3D distance and the
//! label texts, then `last_add`. Identities are `0` for none, `1` for the checked root
//! object and `2, 3, ..` for other objects in order of first appearance.

use solidcore::geom::assert_array::{AssertArray, ObjectKey};

fn text(out: &mut Vec<f64>, s: Option<&str>) {
    out.push(s.map_or(0, str::len) as f64);
    if let Some(s) = s {
        out.extend(s.bytes().map(f64::from));
    }
}

fn id(seen: &mut Vec<ObjectKey>, k: Option<ObjectKey>, root: ObjectKey) -> f64 {
    let Some(k) = k else {
        return 0.0;
    };
    if k == root {
        return 1.0;
    }
    if let Some(i) = seen.iter().position(|s| *s == k) {
        return (i + 2) as f64;
    }
    seen.push(k);
    (seen.len() + 1) as f64
}

/// Appends `ok` and the reports of `reports`, naming `root` identity 1.
pub fn put_assert_reports(v: &mut Vec<f64>, ok: bool, reports: &AssertArray, root: ObjectKey) {
    let mut seen = Vec::new();
    v.push(f64::from(u8::from(ok)));
    v.push(reports.len() as f64);
    for r in reports.reports() {
        v.extend([
            r.reporting_type as f64,
            r.list_index as f64,
            r.test_index as f64,
            f64::from(r.assert_type.code()),
            f64::from(u8::from(r.ok)),
        ]);
        v.push(id(&mut seen, r.reporter, root));
        v.push(id(&mut seen, r.owner, root));
        v.push(id(&mut seen, r.other, root));
        v.extend([
            r.owner_type as f64,
            r.other_type as f64,
            r.tol3d,
            r.value,
            r.dist3d,
        ]);
        text(v, Some(r.name));
        text(v, r.message.as_deref());
        text(v, r.message_for_file);
        text(v, r.owner_type_string);
        text(v, r.other_type_string);
    }
    v.push(f64::from(u8::from(reports.last_add())));
}
