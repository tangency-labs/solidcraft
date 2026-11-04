// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::test_support::tokens_test::{push_tagged_i64, t_dbl};

pub(crate) fn generated_pcurve_block() -> Vec<u8> {
    generated_pcurve_block_with_points([[0.25, 0.5], [0.75, 1.5]])
}

pub(super) fn generated_planar_pcurve_block() -> Vec<u8> {
    generated_pcurve_block_with_points([[0.025, 0.05], [0.075, 0.15]])
}

fn generated_pcurve_block_with_points(points: [[f64; 2]; 2]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 1);
    push_tagged_i64(&mut b, 0x15, 0);
    push_tagged_i64(&mut b, 0x04, 2);
    for (k, m) in [(0.0, 1i64), (1.0, 1)] {
        t_dbl(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for [u, v] in points {
        t_dbl(&mut b, u);
        t_dbl(&mut b, v);
    }
    b
}

pub(super) fn generated_rational_pcurve_block() -> Vec<u8> {
    generated_rational_pcurve_block_with_points([[0.25, 0.5], [0.75, 1.5]])
}

pub(super) fn generated_planar_rational_pcurve_block() -> Vec<u8> {
    generated_rational_pcurve_block_with_points([[0.025, 0.05], [0.075, 0.15]])
}

fn generated_rational_pcurve_block_with_points(points: [[f64; 2]; 2]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x05nurbs");
    push_tagged_i64(&mut b, 0x04, 1);
    push_tagged_i64(&mut b, 0x15, 0);
    push_tagged_i64(&mut b, 0x04, 2);
    for (k, m) in [(0.0, 1i64), (1.0, 1)] {
        t_dbl(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for ([u, v], weight) in points.into_iter().zip([1.0, 0.5]) {
        t_dbl(&mut b, u);
        t_dbl(&mut b, v);
        t_dbl(&mut b, weight);
    }
    b
}

pub(crate) fn generated_curve_block() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 2);
    push_tagged_i64(&mut b, 0x15, 0);
    push_tagged_i64(&mut b, 0x04, 2);
    for (k, m) in [(0.0, 2i64), (1.0, 2)] {
        t_dbl(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for point in [[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]] {
        for coordinate in point {
            t_dbl(&mut b, coordinate);
        }
    }
    b
}

pub(crate) fn generated_surface_block() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 1);
    push_tagged_i64(&mut b, 0x04, 1);
    for _ in 0..4 {
        push_tagged_i64(&mut b, 0x15, 0);
    }
    push_tagged_i64(&mut b, 0x04, 2);
    push_tagged_i64(&mut b, 0x04, 2);
    for _ in 0..2 {
        for (k, m) in [(0.0, 1i64), (1.0, 1)] {
            t_dbl(&mut b, k);
            push_tagged_i64(&mut b, 0x04, m);
        }
    }
    for p in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ] {
        for c in p {
            t_dbl(&mut b, c);
        }
    }
    b
}

pub(super) fn generated_rational_surface_block() -> Vec<u8> {
    let mut block = generated_surface_block();
    block.splice(0..6, b"\x0d\x05nurbs".iter().copied());
    let non_rational = generated_surface_block();
    let control_start = non_rational.len() - 4 * 3 * 9;
    let rational_control_start = control_start + 1;
    for pole in (0..4).rev() {
        let at = rational_control_start + pole * 3 * 9 + 3 * 9;
        let weight = [1.0f64, 0.8, 1.2, 1.0][pole];
        let mut tagged = vec![0x06];
        tagged.extend_from_slice(&weight.to_le_bytes());
        block.splice(at..at, tagged);
    }
    block
}
