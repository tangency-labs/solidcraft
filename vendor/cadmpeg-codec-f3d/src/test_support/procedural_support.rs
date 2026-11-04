// SPDX-License-Identifier: Apache-2.0

use super::smbh_curves_test::synthetic_geometry_with_procedural_curve_smbh;
use super::tokens_test::{
    push_native_enum, t_dbl, t_end, t_ident, t_long, t_pos, t_ref, t_subident, t_vec,
};

pub(crate) fn revolution_intersection(
    name: &str,
    referenced: bool,
    side: usize,
    has_pcurve: bool,
) -> Vec<u8> {
    let mut bytes = synthetic_geometry_with_procedural_curve_smbh();
    let start = cadmpeg_asm::asm_header::record_stream_start(&bytes).unwrap();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&bytes).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let old = records
        .iter()
        .find(|record| record.head() == "intcurve")
        .unwrap();
    let mut curve = Vec::new();
    t_subident(&mut curve, "intcurve");
    t_ident(&mut curve, "curve");
    t_ref(&mut curve, -1);
    t_long(&mut curve, -1);
    t_ref(&mut curve, -1);
    curve.push(0x0f);
    t_ident(&mut curve, "int_int_cur");
    t_long(&mut curve, 23917);
    push_native_enum(&mut curve, 0);
    curve.extend(line_block(
        [
            [13.0 * angle().cos(), 13.0 * angle().sin(), 7.0],
            [13.0 * angle().cos(), 13.0 * angle().sin(), 11.0],
        ],
        [0.0, 1.0],
    ));
    t_dbl(&mut curve, 0.0);
    for slot in 0..2 {
        if slot != side {
            t_ident(&mut curve, "null_surface");
            continue;
        }
        t_ident(&mut curve, "spline");
        curve.push(0x0b);
        curve.push(0x0f);
        t_ident(&mut curve, name);
        curve.extend(line_block(
            [[13.0, 0.0, 7.0], [13.0, 0.0, 11.0]],
            [7.0, 11.0],
        ));
        t_pos(&mut curve, [0.0, 0.0, 0.0]);
        t_vec(&mut curve, [0.0, 0.0, 1.0]);
        curve.extend(revolution_cache());
        t_dbl(&mut curve, 0.0);
        curve.push(0x10);
        curve.extend([0x0b; 4]);
    }
    for slot in 0..2 {
        if slot == side && has_pcurve {
            t_ident(&mut curve, "nubs");
            t_long(&mut curve, 1);
            push_native_enum(&mut curve, 0);
            t_long(&mut curve, 2);
            for parameter in [0.0, 1.0] {
                t_dbl(&mut curve, parameter);
                t_long(&mut curve, 1);
            }
            for u in [7.0, 11.0] {
                t_dbl(&mut curve, u);
                t_dbl(&mut curve, angle());
            }
        } else {
            t_ident(&mut curve, "nullbs");
        }
    }
    curve.extend([0x0b; 2]);
    for _ in 0..4 {
        t_long(&mut curve, 0);
    }
    curve.push(0x10);
    t_end(&mut curve);
    if referenced {
        let marker = curve
            .windows(name.len())
            .position(|window| window == name.as_bytes())
            .unwrap();
        let scope_start = marker - 3;
        let scope_end = curve[marker..]
            .iter()
            .position(|byte| *byte == 0x10)
            .unwrap()
            + marker
            + 1;
        let definition = curve[scope_start..scope_end].to_vec();
        let mut reference = vec![0x0f];
        t_ident(&mut reference, "ref");
        t_long(&mut reference, 1);
        reference.push(0x10);
        curve.splice(scope_start..scope_end, reference);
        let mut support = Vec::new();
        t_subident(&mut support, "spline");
        t_ident(&mut support, "surface");
        t_ref(&mut support, -1);
        t_long(&mut support, -1);
        t_ref(&mut support, -1);
        support.extend(definition);
        t_end(&mut support);
        bytes.splice(
            old.offset..old.offset + old.len,
            curve.into_iter().chain(support),
        );
    } else {
        bytes.splice(old.offset..old.offset + old.len, curve);
    }
    bytes
}

fn angle() -> f64 {
    std::f64::consts::FRAC_PI_6
}

fn line_block(points: [[f64; 3]; 2], domain: [f64; 2]) -> Vec<u8> {
    let mut bytes = Vec::new();
    t_ident(&mut bytes, "nubs");
    t_long(&mut bytes, 1);
    push_native_enum(&mut bytes, 0);
    t_long(&mut bytes, 2);
    for parameter in domain {
        t_dbl(&mut bytes, parameter);
        t_long(&mut bytes, 1);
    }
    for point in points {
        for value in point {
            t_dbl(&mut bytes, value);
        }
    }
    bytes
}

fn revolution_cache() -> Vec<u8> {
    let mut bytes = Vec::new();
    t_ident(&mut bytes, "nurbs");
    t_long(&mut bytes, 1);
    t_long(&mut bytes, 2);
    for _ in 0..4 {
        push_native_enum(&mut bytes, 0);
    }
    t_long(&mut bytes, 2);
    t_long(&mut bytes, 2);
    for (domain, multiplicity) in [([7.0, 11.0], 1), ([0.0, std::f64::consts::FRAC_PI_2], 2)] {
        for parameter in domain {
            t_dbl(&mut bytes, parameter);
            t_long(&mut bytes, multiplicity);
        }
    }
    for (x, y, weight) in [
        (13.0, 0.0, 1.0),
        (13.0, 13.0, std::f64::consts::FRAC_1_SQRT_2),
        (0.0, 13.0, 1.0),
    ] {
        for z in [7.0, 11.0] {
            for value in [x, y, z, weight] {
                t_dbl(&mut bytes, value);
            }
        }
    }
    bytes
}
