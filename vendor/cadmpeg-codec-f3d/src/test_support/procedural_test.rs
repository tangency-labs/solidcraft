// SPDX-License-Identifier: Apache-2.0
//! Procedural-surface payload helpers for synthetic SMBH tests.
#![allow(clippy::unwrap_used)]

use crate::test_support::smbh_blocks_test::{generated_curve_block, generated_surface_block};
use crate::test_support::tokens_test::{push_tagged_i64, t_dbl, t_ident, t_long};

pub(crate) fn push_optional_value_quartet(surface: &mut Vec<u8>) {
    for value in [1.0, 0.0, 1.0, 0.0] {
        surface.push(0x0a);
        t_dbl(surface, value);
    }
}

pub(crate) fn push_revision_cl_scale(surface: &mut Vec<u8>, with_path: bool) {
    // One member: type, curve, endpoints, support, pcurve, flags, subdata.
    t_long(surface, 1);
    t_long(surface, 1);
    surface.extend_from_slice(&generated_curve_block());
    surface.push(0x0a);
    t_dbl(surface, 0.0);
    surface.push(0x0a);
    t_dbl(surface, 1.0);
    t_ident(surface, "null_surface");
    t_ident(surface, "nullbs");
    surface.push(0x0b);
    t_long(surface, -1);
    // Subdata type 213 with one row and one column: leading pair plus
    // `column_count + 1` trailing pairs in the revision encoding.
    t_long(surface, 213);
    t_long(surface, 1);
    t_long(surface, 1);
    for value in [0.0, 1.0, -0.5, 0.25, 0.75, 0.75] {
        t_dbl(surface, value);
    }
    surface.push(0x0b);
    if with_path {
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
    } else {
        t_ident(surface, "null_curve");
    }
    t_long(surface, 0);
    t_long(surface, -1);
}

/// A form-2 `par_int_cur` whose uv pcurve runs from `first` to `second`.
pub(crate) fn generated_form_two_par_int_cur(first: [f64; 2], second: [f64; 2]) -> Vec<u8> {
    let mut scope = vec![0x0f];
    t_ident(&mut scope, "par_int_cur");
    push_tagged_i64(&mut scope, 0x04, 1);
    push_tagged_i64(&mut scope, 0x15, 2);
    for bound in [0.0, 1.0] {
        scope.push(0x0a);
        t_dbl(&mut scope, bound);
    }
    push_tagged_i64(&mut scope, 0x15, 0);
    t_ident(&mut scope, "spline");
    scope.push(0x0b);
    scope.push(0x0f);
    t_ident(&mut scope, "exact_spl_sur");
    scope.extend_from_slice(&generated_surface_block());
    scope.push(0x10);
    scope.extend_from_slice(&[0x0b; 4]);
    t_ident(&mut scope, "null_surface");
    scope.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut scope, 0x04, 1);
    push_tagged_i64(&mut scope, 0x15, 0);
    push_tagged_i64(&mut scope, 0x04, 2);
    for (knot, multiplicity) in [(0.0, 1i64), (1.0, 1)] {
        t_dbl(&mut scope, knot);
        push_tagged_i64(&mut scope, 0x04, multiplicity);
    }
    for [u, v] in [first, second] {
        t_dbl(&mut scope, u);
        t_dbl(&mut scope, v);
    }
    t_ident(&mut scope, "nullbs");
    push_tagged_i64(&mut scope, 0x04, 0);
    scope.push(0x10);
    scope
}
