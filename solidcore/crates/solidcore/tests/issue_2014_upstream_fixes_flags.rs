//! tangency-labs/solidcore#2014: [`UpstreamFixes::flags`] lists every opt-in fix by its
//! field name with its value, in declaration order, and [`UpstreamFixes::NAMES`] the names
//! alone, so a consumer that pins solidcore notices a new flag without parsing the derived
//! `Debug` (SolidCraft's `every_upstream_fix_is_on` did). Both are generated from the
//! struct definition, so a new field cannot be left out.
//!
//! The issue's input and expected values: `UpstreamFixes::default().flags()` yields one
//! entry per field, every one on but the held-back ones ([`UpstreamFixes::HELD_BACK`]: the
//! corrections are the kernel's default), and setting one flag makes exactly that entry
//! change. The count grows with every new flag, so the tests compare against
//! the struct itself (its size, one byte per `bool` field, and its derived `Debug`, which
//! only a test reads this way), never against a number.
#![allow(clippy::expect_used)] // test helpers

use solidcore::kernel::UpstreamFixes;
use solidcore::Kernel;

/// The `(field, value)` pairs the derived `Debug` prints, in its order: the struct's own
/// field list.
fn debug_fields(fixes: &UpstreamFixes) -> Vec<(String, bool)> {
    let text = format!("{fixes:?}");
    let body = text
        .strip_prefix("UpstreamFixes { ")
        .and_then(|t| t.strip_suffix(" }"))
        .expect("the derived `Debug` of a braced struct");
    body.split(", ")
        .map(|field| {
            let (name, value) = field.split_once(": ").expect("`name: value`");
            (name.to_owned(), value.parse().expect("a `bool` value"))
        })
        .collect()
}

/// `fixes.flags()` as owned pairs, to compare with [`debug_fields`].
fn listed(fixes: &UpstreamFixes) -> Vec<(String, bool)> {
    fixes
        .flags()
        .map(|(name, on)| (name.to_owned(), on))
        .collect()
}

/// The names of the flags that are on, in declaration order.
fn turned_on(fixes: &UpstreamFixes) -> Vec<&'static str> {
    fixes
        .flags()
        .filter(|&(_, on)| on)
        .map(|(name, _)| name)
        .collect()
}

/// The names of the flags that are off, in declaration order.
fn turned_off(fixes: &UpstreamFixes) -> Vec<&'static str> {
    fixes
        .flags()
        .filter(|&(_, on)| !on)
        .map(|(name, _)| name)
        .collect()
}

/// Every flag off: the starting point for setting one alone.
fn none_on() -> UpstreamFixes {
    let mut fixes = UpstreamFixes::all();
    for &name in UpstreamFixes::NAMES {
        *fixes.flag_mut(name).expect("a field name") = false;
    }
    fixes
}

#[test]
fn default_lists_every_field_on_but_the_held_back_in_declaration_order() {
    let fixes = Kernel::default().fixes;
    assert_eq!(fixes, UpstreamFixes::default());
    // Every field is a one-byte `bool` (the generating macro accepts no other type), so
    // the struct's size is its field count.
    let count = size_of::<UpstreamFixes>();
    assert!(count > 0);
    assert_eq!(UpstreamFixes::NAMES.len(), count);
    assert_eq!(fixes.flags().count(), count);
    // every correction on, but the held-back ones, in declaration order
    let held: Vec<&str> = UpstreamFixes::NAMES
        .iter()
        .copied()
        .filter(|n| UpstreamFixes::HELD_BACK.contains(n))
        .collect();
    assert_eq!(held.len(), UpstreamFixes::HELD_BACK.len());
    assert_eq!(turned_off(&fixes), held);
    // The same names in the same order as the struct's fields, each with its value.
    assert_eq!(listed(&fixes), debug_fields(&fixes));
    assert!(fixes
        .flags()
        .map(|(name, _)| name)
        .eq(UpstreamFixes::NAMES.iter().copied()));
}

#[test]
fn one_flag_on_is_exactly_that_entry() {
    let mut newest = none_on();
    newest.face_points_span_domain = true;
    assert_eq!(turned_on(&newest), ["face_points_span_domain"]);
    assert_eq!(listed(&newest), debug_fields(&newest));

    let mut first = none_on();
    first.plane_recognition_origin_after_axes = true;
    assert_eq!(turned_on(&first), ["plane_recognition_origin_after_axes"]);
    assert_eq!(listed(&first), debug_fields(&first));
}

/// `flag_mut` finds every field by its name, and only by it.
#[test]
fn flag_mut_sets_exactly_the_named_field() {
    for &name in UpstreamFixes::NAMES {
        let mut fixes = none_on();
        *fixes.flag_mut(name).expect("a field name") = true;
        assert_eq!(turned_on(&fixes), [name]);
        assert_eq!(fixes.flag_mut(name).copied(), Some(true));
    }
    let mut fixes = UpstreamFixes::default();
    assert!(fixes.flag_mut("no_such_fix").is_none());
    assert!(fixes.flag_mut("").is_none());
    assert_eq!(fixes, UpstreamFixes::default());
}
