//! The numbers against textbook values (KHK Gear Technical Reference worked examples, which
//! follow ISO 21771) and against independent geometry, and the fitted tooth sides against the
//! exact involute.

use std::f64::consts::PI;

use crate::geom::{Seg, V2, inv, natural_spline};
use crate::involute::{BasicRack, Internal, Involute, external_flank, external_outline, mesh_external, mesh_internal};

use crate::test_util::close;

fn spur(z: u32, m: f64, x: f64) -> Involute {
    Involute { teeth: z, module: m, shift: x, ..Involute::default() }
}

#[test]
fn standard_spur_pair_matches_the_textbook() {
    // KHK table 4.1: m = 3, α = 20°, z = 12 and 24.
    let (g1, g2) = (spur(12, 3.0, 0.0), spur(24, 3.0, 0.0));
    for (g, d, db, da, df) in [(g1, 36.0, 33.8289, 42.0, 28.5), (g2, 72.0, 67.6579, 78.0, 64.5)] {
        assert!(close(2.0 * g.r(), d, 1e-9));
        assert!(close(2.0 * g.rb(), db, 1e-4), "{}", 2.0 * g.rb());
        assert!(close(2.0 * g.ra(), da, 1e-9));
        assert!(close(2.0 * g.rf(), df, 1e-9));
        assert!(close(g.thickness(), PI * 3.0 / 2.0, 1e-12));
        let f = external_flank(&g);
        assert!(close(2.0 * f.ra, da, 1e-9) && close(2.0 * f.rf, df, 1e-9));
    }
    let m = mesh_external(&g1, &g2, 10.0);
    assert!(close(m.center_distance, 54.0, 1e-9));
    assert!(close(m.contact_ratio, 1.5111, 2e-4), "{}", m.contact_ratio);
    assert!(close(m.clearance_1, 0.75, 1e-9) && close(m.clearance_2, 0.75, 1e-9));
}

#[test]
fn shifted_spur_pair_matches_the_textbook() {
    // KHK table 4.3: m = 3, α = 20°, z = 12 and 24, x = 0.6 and 0.36.
    let (g1, g2) = (spur(12, 3.0, 0.6), spur(24, 3.0, 0.36));
    let m = mesh_external(&g1, &g2, 10.0);
    assert!(close(m.working_pressure_angle.to_degrees(), 26.0886, 1e-4), "{}", m.working_pressure_angle.to_degrees());
    assert!(close(inv(m.working_pressure_angle), 0.034316, 1e-6));
    assert!(close(m.center_distance, 56.4999, 1e-4), "{}", m.center_distance);
    assert!(close(2.0 * g1.rf(), 32.1, 1e-9) && close(2.0 * g2.rf(), 66.66, 1e-9));
    assert!(close(g1.thickness(), 3.0 * (PI / 2.0 + 2.0 * 0.6 * 20f64.to_radians().tan()), 1e-12));
    // No backlash at the working circles: the two teeth fill the working pitch.
    let (rw1, rw2) = (m.center_distance * 12.0 / 36.0, m.center_distance * 24.0 / 36.0);
    assert!(close(g1.thickness_at(rw1) + g2.thickness_at(rw2), 2.0 * PI * rw1 / 12.0, 1e-9));
}

#[test]
fn span_measurement_matches_the_tables() {
    // Span over k teeth for m = 1, α = 20° (any gear handbook's table).
    for (z, k, w) in [(20, 3, 7.6604), (24, 3, 7.7165), (10, 2, 4.5683)] {
        let (kk, ww) = spur(z, 1.0, 0.0).span();
        assert_eq!(kk, k, "z {z}");
        assert!(close(ww, w, 1e-4), "z {z}: {ww}");
    }
    // KHK table 5.10: m = 3, z = 24, x = +0.4 measures over 4 teeth, 32.8266.
    let (k, w) = spur(24, 3.0, 0.4).span();
    assert_eq!(k, 4);
    assert!(close(w, 32.8266, 1e-4), "{w}");
    // Against geometry, helical too: (k − 1) base pitches and one base thickness, in the normal
    // plane of the base cylinder.
    for (z, beta, x) in [(17, 0.0f64, 0.0), (30, 15.0, 0.3), (55, 30.0, -0.2), (120, 22.0, 0.5), (8, 0.0, 0.5), (400, 40.0, 1.0)] {
        let g = Involute { teeth: z, module: 2.5, helix: beta.to_radians(), shift: x, ..Involute::default() };
        let (k, w) = g.span();
        let base_helix = (g.helix.sin() * g.pressure_angle.cos()).asin();
        let base_thickness = 2.0 * g.rb() * g.half_angle_at(g.rb());
        let expect = (f64::from(k - 1) * g.base_pitch() + base_thickness) * base_helix.cos();
        assert!(close(w, expect, 1e-9), "z {z}: {w} vs {expect}");
        // The anvils touch inside the tooth's height.
        let touch = (g.rb().powi(2) + (w * base_helix.cos() / 2.0).powi(2)).sqrt();
        assert!(touch > g.rb() && touch < g.ra(), "z {z}: touch {touch} outside {}..{}", g.rb(), g.ra());
    }
}

#[test]
fn over_pins_matches_the_textbook_and_the_pins_touch_the_flanks() {
    // KHK table 5.13: m = 1, z = 20, x = 0, pins of 1.7: 22.2941.
    let (m, _) = spur(20, 1.0, 0.0).over_pins(Some(1.7));
    assert!(close(m, 22.2941, 2e-4), "{m}");
    for (z, x, dp) in [(20, 0.0, 1.7), (21, 0.0, 1.728), (12, 0.5, 1.9), (45, -0.3, 1.728), (9, 0.4, 1.8)] {
        let g = spur(z, 1.0, x);
        let (m, dp) = g.over_pins(Some(dp));
        let zf = f64::from(z);
        let rm = if z % 2 == 0 { (m - dp) / 2.0 } else { (m - dp) / 2.0 / (PI / (2.0 * zf)).cos() };
        // The pin sits in the space beside tooth 0 (centred on +x): its centre half a pitch round.
        let c = V2::polar(rm, PI / zf);
        let mut nearest = f64::INFINITY;
        for i in 0..=20000 {
            let rr = g.rb() + (g.ra() - g.rb()) * f64::from(i) / 20000.0;
            nearest = nearest.min(c.dist(V2::polar(rr, g.half_angle_at(rr))));
        }
        assert!(close(nearest, dp / 2.0, 2e-6), "z {z}: pin {dp} sits {nearest} from the flank");
    }
}

#[test]
fn internal_mesh_has_no_backlash_at_the_working_circles() {
    for (z1, z2, x1, x2) in [(16, 24, 0.0, 0.516), (20, 60, 0.2, 0.0), (18, 54, 0.0, 0.0), (25, 80, -0.1, 0.3)] {
        let pinion = spur(z1, 3.0, x1);
        let ring = Internal { gear: spur(z2, 3.0, x2) };
        let m = mesh_internal(&pinion, &ring, 10.0);
        if x2 == 0.516 {
            // KHK table 4.5.
            assert!(close(inv(m.working_pressure_angle), 0.061857, 1e-6), "{}", inv(m.working_pressure_angle));
        }
        let (zf1, zf2) = (f64::from(z1), f64::from(z2));
        let (rw1, rw2) = (m.center_distance * zf1 / (zf2 - zf1), m.center_distance * zf2 / (zf2 - zf1));
        // The pinion's tooth fills the ring's space on the working circles.
        let space = 2.0 * rw2 * ring.space_half_at(rw2);
        assert!(close(pinion.thickness_at(rw1), space, 1e-9), "z {z1}/{z2}: tooth {} space {space}", pinion.thickness_at(rw1));
        if x1 == 0.0 && x2 == 0.0 {
            // The pinion's tip clears the ring's root by the basic rack's 0.25 module; the ring's
            // tip, which stops where a rack-cut pinion's involute does (see `Internal::r_tip`),
            // clears the pinion's root by that much more.
            let raised = ring.r_tip() - (ring.gear.r() - 3.0);
            assert!(raised > 0.0 && raised < 0.2 * 3.0, "{raised}");
            assert!(close(m.clearance_1, 0.75, 1e-9) && close(m.clearance_2, 0.75 + raised, 1e-9), "{} {}", m.clearance_1, m.clearance_2);
        }
    }
}

#[test]
fn contact_ratio_matches_the_path_of_contact() {
    // Walk the line of action: contact lasts while the contact point is inside both tip circles.
    for (z1, z2, x1, x2, beta) in [(20, 40, 0.0, 0.0, 0.0f64), (12, 24, 0.6, 0.36, 0.0), (17, 51, 0.3, -0.1, 20.0), (30, 31, 0.0, 0.0, 0.0)] {
        let g1 = Involute { teeth: z1, module: 2.0, shift: x1, helix: beta.to_radians(), ..Involute::default() };
        let g2 = Involute { teeth: z2, module: 2.0, shift: x2, helix: -beta.to_radians(), ..Involute::default() };
        let m = mesh_external(&g1, &g2, 15.0);
        let (a, aw) = (m.center_distance, m.working_pressure_angle);
        let (rb1, rb2) = (g1.rb(), g2.rb());
        // Along the line of action from gear 1's tangent point (s = 0) to gear 2's (s = a·sin αw).
        let n = 400_000;
        let total = a * aw.sin();
        let mut inside = 0usize;
        for i in 0..=n {
            let s = total * i as f64 / n as f64;
            let (r1, r2) = ((rb1 * rb1 + s * s).sqrt(), (rb2 * rb2 + (total - s).powi(2)).sqrt());
            if r1 <= g1.ra() && r2 <= g2.ra() {
                inside += 1;
            }
        }
        let walked = total * inside as f64 / n as f64 / g1.base_pitch();
        assert!(close(m.contact_ratio, walked, 1e-4), "z {z1}/{z2}: {} vs {walked}", m.contact_ratio);
    }
}

/// The largest distance between the sketch's spline through a tooth side's fit points and the
/// exact involute, over the involute part of the flank; and the fillet's and flank's point counts.
fn flank_deviation(g: &Involute) -> (f64, usize, usize) {
    let f = external_flank(g);
    let pts: Vec<V2> = f.flank.iter().map(|(r, h)| V2::polar(*r, *h)).collect();
    let mut curve = Vec::new();
    natural_spline(&pts, 40, &mut curve);
    let rb = g.rb();
    let mut worst: f64 = 0.0;
    for q in &curve {
        let rr = q.len();
        if rr <= rb {
            continue;
        }
        // Off the involute round the circle, turned to the involute's normal.
        let off = (q.angle() - g.half_angle_at(rr)) * rr * (rb / rr);
        worst = worst.max(off.abs());
    }
    (worst, f.fillet.len(), f.flank.len())
}

#[test]
fn fitted_flanks_stay_within_a_micron_per_module_of_the_involute() {
    let mut most_points = 0;
    for z in [6, 9, 12, 17, 20, 30, 50, 100, 200, 400] {
        for alpha in [14.5f64, 20.0, 25.0, 30.0] {
            for x in [-0.4, 0.0, 0.5, 1.0] {
                for beta in [0.0f64, 25.0] {
                    let g = Involute {
                        teeth: z,
                        module: 1.0,
                        pressure_angle: alpha.to_radians(),
                        helix: beta.to_radians(),
                        shift: x,
                        ..Involute::default()
                    };
                    let f = external_flank(&g);
                    if f.defect().is_some() || f.flank.len() < 2 {
                        continue;
                    }
                    let (dev, nf, nk) = flank_deviation(&g);
                    assert!(dev < 1e-3, "z {z} α {alpha} x {x} β {beta}: {dev:.2e} with {nk} points");
                    most_points = most_points.max(nf.max(nk));
                }
            }
        }
    }
    let (dev, nf, nk) = flank_deviation(&spur(20, 1.0, 0.0));
    eprintln!("fit z20: fillet {nf} flank {nk} points, deviation {dev:.2e} modules; most points on a side {most_points}");
    let (dev, nf, nk) = flank_deviation(&spur(400, 1.0, 0.0));
    eprintln!("fit z400: fillet {nf} flank {nk} points, deviation {dev:.2e} modules");
}

#[test]
fn stub_racks_keep_their_proportions() {
    let g = Involute { teeth: 30, module: 2.0, rack: BasicRack::standard("stub").unwrap(), ..Involute::default() };
    assert!(close(g.ra() - g.r(), 1.6, 1e-12) && close(g.r() - g.rf(), 2.0, 1e-12));
    let (o, _) = external_outline(&g, 0.0);
    let tips = o.segs.iter().filter(|s| matches!(s, Seg::Arc { a, .. } if close(a.len(), g.ra(), 1e-9))).count();
    assert_eq!(tips, 30);
}
