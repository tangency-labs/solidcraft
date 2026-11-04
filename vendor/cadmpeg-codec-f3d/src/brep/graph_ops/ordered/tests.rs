// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;

#[test]
fn long_identity_selection_preserves_source_order_without_sorting_keys() {
    let prefix = "x".repeat(512);
    let rows = (0..2_000)
        .rev()
        .map(|index| format!("{prefix}#{index:04}"))
        .collect::<Vec<_>>();
    let reachable = rows
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 3 != 0)
        .map(|(_, id)| id.clone())
        .collect::<HashSet<_>>();
    let expected = rows
        .iter()
        .enumerate()
        .filter(|(index, _)| index % 3 != 0)
        .map(|(_, id)| id.clone())
        .collect::<Vec<_>>();
    let retained = super::select_rows(rows, &reachable, String::as_str);
    assert_eq!(retained, expected);
}
