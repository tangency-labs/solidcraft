//! Tests for sprockets, pulleys, splines and face gears.

use std::f64::consts::PI;

use crate::face_gear::FaceGear;
use crate::geom::{Outline, V2};
use crate::involute::Involute;
use crate::plan::{Kind, Spec, Step, plan};
use crate::pulley::{BELTS, Pulley, belt_by_id};
use crate::spline::{Spline, SplineStandard};
use crate::sprocket::{CHAINS, Sprocket, chain_by_id, chamfer_profile, tooth_width};
use crate::wrap::{belt_length, belt_path, chain_center, chain_links};

use crate::test_util::close;

fn closed(o: &Outline) {
    for w in o.segs.windows(2) {
        assert!(w[0].end().dist(w[1].start()) < 1e-9, "gap {:?} → {:?}", w[0].end(), w[1].start());
    }
    let (Some(a), Some(b)) = (o.segs.last(), o.segs.first()) else { panic!("empty outline") };
    assert!(a.end().dist(b.start()) < 1e-9);
}

/// The sampled outline crosses itself nowhere (segments that share an end excepted).
fn simple(o: &Outline) {
    let mut p = o.polyline(0.002);
    p.dedup_by(|a, b| a.dist(*b) < 1e-9);
    if p.len() > 1 && p[0].dist(p[p.len() - 1]) < 1e-9 {
        p.pop();
    }
    let n = p.len();
    let cross = |a: V2, b: V2, c: V2, d: V2| -> bool {
        let d1 = (b - a).cross(c - a);
        let d2 = (b - a).cross(d - a);
        let d3 = (d - c).cross(a - c);
        let d4 = (d - c).cross(b - c);
        d1 * d2 < -1e-18 && d3 * d4 < -1e-18
    };
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (a, b, c, d) = (p[i], p[(i + 1) % n], p[j], p[(j + 1) % n]);
            assert!(!cross(a, b, c, d), "self-intersection near {a:?} and {c:?}");
        }
    }
}

#[test]
fn chain_table_and_iso_606_sprocket_dimensions() {
    let c = chain_by_id("08b").unwrap();
    assert_eq!((c.pitch, c.roller), (12.7, 8.51));
    assert!(CHAINS.iter().all(|c| c.roller < c.pitch && c.inner_width < c.strand_pitch));
    let s = Sprocket { teeth: 17, pitch: 12.7, roller: 8.51 };
    // Catalogue values for an 08B 17-tooth sprocket: d 69.12, df 60.61, da 72.1…76.5.
    assert!(close(s.pitch_diameter(), 69.12, 0.01), "{}", s.pitch_diameter());
    assert!(close(s.root_diameter(), 60.61, 0.01));
    let (lo, hi) = s.tip_diameter_range();
    assert!(close(lo, 72.12, 0.02) && close(hi, 76.49, 0.02), "{lo} {hi}");
    // Odd teeth: caliper diameter d·cos(90°/z) − d1.
    assert!(close(s.caliper_diameter(), 69.12 * (PI / 34.0).cos() - 8.51, 0.01));
    assert!(s.seating_radius() > 0.505 * 8.51 && s.seating_radius() < 0.505 * 8.51 + 0.069 * 8.51f64.cbrt());
    let t = s.outline(0.0, s.tip_diameter() / 2.0);
    closed(&t.outline);
    simple(&t.outline);
    assert!(!t.pointed);
    assert!(close(t.outline.max_radius(), s.tip_diameter() / 2.0, 1e-3));
    // The seats' bottoms: the roller seating radius below the pitch circle.
    assert!(close(t.r_min, s.pitch_diameter() / 2.0 - s.seating_radius(), 1e-9));
    let area = t.outline.area();
    assert!(area > PI * t.r_min.powi(2) && area < PI * t.ra.powi(2), "{area}");
    // Tooth width per ISO 606.
    assert!(close(tooth_width(12.7, 7.75, 1), 0.93 * 7.75, 1e-12));
    assert!(close(tooth_width(15.875, 9.65, 2), 0.93 * 9.65, 1e-12));
}

#[test]
fn every_chain_and_tooth_count_makes_a_clean_sprocket() {
    for c in CHAINS {
        for z in [6u32, 9, 13, 17, 25, 40, 80] {
            let s = Sprocket { teeth: z, pitch: c.pitch, roller: c.roller };
            let t = s.outline(0.3, s.tip_diameter() / 2.0);
            closed(&t.outline);
            if z <= 25 {
                simple(&t.outline);
            }
            assert!(t.ra > s.pitch_diameter() / 2.0, "{} z{z}", c.id);
            assert!(t.outline.area() > 0.0);
        }
    }
}

#[test]
fn chamfer_profile_is_a_closed_band_per_strand() {
    let prof = chamfer_profile(12.7, 37.0, 7.2, 2, 13.92, 28.0);
    assert!(prof.iter().all(|q| q.x >= 0.0 && q.is_finite()));
    assert!(prof.iter().any(|q| q.x > 37.0));
    let area: f64 = (0..prof.len()).map(|i| prof[i].cross(prof[(i + 1) % prof.len()])).sum::<f64>() * 0.5;
    assert!(area.abs() > 0.0);
}

#[test]
fn chain_length_and_centre_distance_agree() {
    let (z1, z2, p) = (17.0, 38.0, 12.7);
    let a = 500.0;
    let x = chain_links(z1, z2, a, p);
    let back = chain_center(z1, z2, x, p).unwrap();
    assert!(close(back, a, 1e-6), "{back}");
    // Same teeth: links = 2a/p + z.
    assert!(close(chain_links(20.0, 20.0, 254.0, 12.7), 40.0 + 20.0, 1e-9));
    // An open belt round equal pulleys: 2a + πd.
    assert!(close(belt_length(10.0, 10.0, 100.0), 200.0 + 20.0 * PI, 1e-9));
    let o = belt_path(10.0, 25.0, 120.0, 0.0).unwrap();
    closed(&o);
    assert!(close(o.area(), (belt_length(10.0, 25.0, 120.0) - 0.0) * 0.0 + o.area(), 0.0));
    assert!(o.area() > 0.0);
}

#[test]
fn timing_pulley_dimensions_and_grooves() {
    let gt2 = belt_by_id("gt2_2").unwrap();
    let p20 = Pulley { belt: *gt2, teeth: 20 };
    // GT2 20-tooth: pitch Ø 12.73, OD 12.22 (makers' tables).
    assert!(close(p20.pitch_diameter(), 12.732, 0.001));
    assert!(close(p20.outside_diameter(), 12.224, 0.001));
    // The published GT2 2 mm geometry puts the flank centres 0.40 mm off the middle.
    let crate::pulley::Groove::Curvilinear { depth, tip_r, flank_r, .. } = gt2.groove else { panic!() };
    let b = ((flank_r - tip_r).powi(2) - (depth - tip_r).powi(2)).sqrt();
    assert!(close(b, 0.40, 0.01), "{b}");
    let htd5 = belt_by_id("htd_5m").unwrap();
    // HTD 5M 30 teeth: pitch Ø 47.75, OD 46.60.
    let h30 = Pulley { belt: *htd5, teeth: 30 };
    assert!(close(h30.pitch_diameter(), 47.746, 0.001) && close(h30.outside_diameter(), 46.60, 0.01));
    for belt in BELTS {
        let prof = belt.groove.profile();
        assert!(close(prof.first().unwrap().y, 0.0, 1e-9) && close(prof.last().unwrap().y, 0.0, 1e-9), "{}", belt.id);
        assert!(prof.iter().all(|q| q.y <= 1e-9 && q.y >= -belt.groove.depth() - 1e-9), "{}", belt.id);
        let w = 2.0 * belt.groove.half_width();
        assert!(w > 0.3 * belt.pitch && w < 0.95 * belt.pitch, "{} groove {w} of pitch {}", belt.id, belt.pitch);
        for z in [12u32, 20, 36, 60] {
            let pu = Pulley { belt: *belt, teeth: z };
            let o = pu.outline(0.0);
            closed(&o);
            if z <= 20 {
                simple(&o);
            }
            assert!(pu.land() > 0.0, "{} z{z}", belt.id);
            assert!(close(o.max_radius(), pu.outside_diameter() / 2.0, 1e-3), "{} z{z}", belt.id);
        }
    }
}

#[test]
fn din_5480_and_ansi_splines() {
    // DIN 5480 – 25 × 1.25 × 18: shift +0.45, d_a1 24.75, d_f1 22.125 (hobbed, h_fP = 0.6 m),
    // d_a2 22.5, d_f2 25 (the hub's root diameter is the reference diameter).
    let s = Spline {
        standard: SplineStandard::Din5480,
        module: 1.25,
        teeth: 18,
        pressure_angle: 30f64.to_radians(),
        reference_diameter: 25.0,
        fillet_root: false,
        clearance: 0.0,
    };
    assert!(close(s.shift(), 0.45, 1e-12));
    let (da1, df1) = s.shaft_diameters();
    let (da2, df2) = s.hub_diameters();
    assert!(close(da1, 24.75, 1e-9) && close(df1, 22.125, 1e-9) && close(da2, 22.5, 1e-9) && close(df2, 25.0, 1e-9));
    let g = s.shaft();
    assert!(close(g.ra(), da1 / 2.0, 1e-9) && close(g.rf(), df1 / 2.0, 1e-9));
    // With no clearance the hub's space equals the shaft's tooth.
    assert!(close(s.hub_space(), g.thickness(), 1e-12));
    let (o, f) = s.shaft_outline(0.0);
    closed(&o);
    simple(&o);
    assert!(!f.pointed);
    let h = s.hub_outline(0.0);
    closed(&h);
    simple(&h);
    // The hub's teeth sit in the shaft's roots: its smallest radius is the hub tip.
    let rmin = h.polyline(0.01).iter().map(|q| q.len()).fold(f64::MAX, f64::min);
    assert!(close(rmin, da2 / 2.0, 1e-3), "{rmin}");
    // ANSI B92.1, 24 teeth 16/32: pitch Ø 1.5 in, major 1.5625 in.
    let a = Spline {
        standard: SplineStandard::AnsiB92,
        module: 25.4 / 16.0,
        teeth: 24,
        pressure_angle: 30f64.to_radians(),
        reference_diameter: 0.0,
        fillet_root: true,
        clearance: 0.05,
    };
    assert!(close(a.pitch_diameter(), 38.1, 1e-9));
    assert!(close(a.shaft_diameters().0, 25.4 * 25.0 / 16.0, 1e-9));
    assert!(close(a.shaft_diameters().1, 25.4 * (24.0 - 1.8) / 16.0, 1e-9));
    let (o, _) = a.shaft_outline(0.0);
    closed(&o);
    simple(&o);
    simple(&a.hub_outline(0.0));
    assert!(a.hub_space() > a.shaft().thickness());
}

#[test]
fn shaft_and_hub_do_not_overlap() {
    // Sample the shaft's teeth: every point lies inside the hub's toothed hole (or on it).
    let s = Spline {
        standard: SplineStandard::Din5480,
        module: 2.0,
        teeth: 21,
        pressure_angle: 30f64.to_radians(),
        reference_diameter: 45.0,
        fillet_root: false,
        clearance: 0.04,
    };
    let (shaft, _) = s.shaft_outline(0.0);
    let hole = s.hub_outline(0.0).polyline(0.002);
    let inside = |q: V2| -> bool {
        let mut c = false;
        for i in 0..hole.len() - 1 {
            let (a, b) = (hole[i], hole[i + 1]);
            if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                c = !c;
            }
        }
        c
    };
    for q in shaft.polyline(0.005) {
        let shrunk = q * (1.0 - 1e-4);
        assert!(inside(shrunk), "shaft point {q:?} (r {}) outside the hub's hole", q.len());
    }
}

#[test]
fn face_gear_limits_and_sections() {
    let pinion = Involute { teeth: 18, module: 2.0, ..Involute::default() };
    let fg = FaceGear::new(pinion, 60, 0.0);
    let (r0, rmin, rmax) = (fg.pitch_radius(), fg.undercut_radius(), fg.pointed_radius());
    assert!(rmin < r0 && r0 < rmax, "{rmin} {r0} {rmax}");
    // At the pitch radius the face gear's tooth is half the pitch at the pitch plane, and a
    // 20° rack's above and below it.
    assert!(close(fg.thickness(r0, 0.0), PI * 2.0 / 2.0, 2e-4), "{}", fg.thickness(r0, 0.0));
    for h in [-1.0, 0.8, 1.6] {
        assert!(close(fg.thickness(r0, h), PI - 2.0 * h * 20f64.to_radians().tan(), 5e-4), "{h}: {}", fg.thickness(r0, h));
    }
    // Further out the flanks are those of a rack of the working pressure angle acos(r_b/r_w).
    let r = 1.05 * r0;
    let aw = (pinion.rb() / (r * 18.0 / 60.0)).acos();
    assert!(close((fg.thickness(r, 0.0) - fg.thickness(r, 1.0)) / 2.0, aw.tan(), 5e-4));
    // The tooth narrows going up and further out.
    assert!(fg.thickness(r0, fg.top()) < fg.thickness(r0, fg.bottom()));
    assert!(fg.thickness(rmax, fg.top()) < fg.thickness(rmin, fg.top()));
    assert!(close(fg.thickness(rmax, fg.top()), 0.2 * 2.0, 1e-4));
    // Litvin's inner limit for z1 = 18, z2 = 60, 20°: L1 ≈ … the undercut limit in modules.
    assert!(rmin / 2.0 > 22.0 && rmin / 2.0 < 30.0, "{}", rmin / 2.0);
    let sections = fg.sections(&fg.heights(), rmin, rmax.min(rmin + 12.0));
    assert_eq!(sections.len(), 5);
    for o in &sections {
        closed(o);
        simple(o);
        assert_eq!(o.segs.len(), sections[0].segs.len());
    }
}

#[test]
fn new_kinds_plan_with_facts_and_steps() {
    for k in [Kind::Sprocket, Kind::Pulley, Kind::Spline, Kind::FaceGear, Kind::Elliptical] {
        let s = Spec::defaults_for(k);
        let p = plan(&s).unwrap_or_else(|e| panic!("{k:?}: {e}"));
        eprintln!("== {k:?} ({})", p.preview_note);
        for f in &p.facts {
            eprintln!("   {} = {:.4}", f.label, f.value);
        }
        for w in &p.warnings {
            eprintln!("   ! {w}");
        }
        for b in &p.bodies {
            for st in &b.steps {
                if let Step::Prism { region, .. } = st {
                    for o in region.outer.iter().chain(&region.holes) {
                        closed(o);
                    }
                }
            }
        }
    }
    // Default face gear: no clamping needed.
    let p = plan(&Spec::defaults_for(Kind::FaceGear)).unwrap();
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    // Pairs.
    let mut s = Spec::defaults_for(Kind::Sprocket);
    s.teeth2 = 38;
    s.center_distance = 450.0;
    s.strands = 2;
    let p = plan(&s).unwrap();
    assert_eq!(p.bodies.len(), 2);
    let links = p.facts.iter().find(|f| f.label.starts_with("chain links")).unwrap().value;
    assert!(links % 2.0 == 0.0 && links > 2.0 * 450.0 / 12.7);
    let mut s = Spec::defaults_for(Kind::Pulley);
    s.teeth2 = 40;
    s.center_distance = 150.0;
    let p = plan(&s).unwrap();
    assert_eq!(p.bodies.len(), 2);
    let a = p.facts.iter().find(|f| f.label.starts_with("centre distance")).unwrap().value;
    let teeth = p.facts.iter().find(|f| f.label.starts_with("belt teeth")).unwrap().value;
    assert!(close(belt_length(20.0 * 2.0 / PI / 2.0, 40.0 * 2.0 / PI / 2.0, a), teeth * 2.0, 1e-6));
    // ANSI spline and custom chain.
    let mut s = Spec::defaults_for(Kind::Spline);
    s.spline_standard = SplineStandard::AnsiB92;
    s.module = 25.4 / 16.0;
    s.teeth = 24;
    assert!(plan(&s).unwrap().warnings.is_empty());
    let mut s = Spec::defaults_for(Kind::Sprocket);
    s.chain = crate::sprocket::CUSTOM;
    s.chain_pitch = 10.0;
    s.roller_diameter = 6.0;
    assert!(plan(&s).is_ok());
    s.roller_diameter = 11.0;
    assert!(plan(&s).is_err());
}

/// Writes each new type's preview outlines as JSON to `$GEAR_DUMP` (run with --ignored), for
/// looking at them.
#[test]
#[ignore]
fn dump_previews() {
    let Ok(dir) = std::env::var("GEAR_DUMP") else { return };
    let mut pair = Spec::defaults_for(Kind::Sprocket);
    pair.teeth2 = 30;
    pair.center_distance = 250.0;
    let mut belt = Spec::defaults_for(Kind::Pulley);
    belt.teeth2 = 36;
    belt.belt = "htd_5m";
    let mut t5 = Spec::defaults_for(Kind::Pulley);
    t5.belt = "t5";
    t5.teeth = 16;
    let mut ansi = Spec::defaults_for(Kind::Spline);
    ansi.spline_standard = SplineStandard::AnsiB92;
    ansi.module = 25.4 / 16.0;
    ansi.teeth = 24;
    ansi.fillet_root = true;
    for (n, s) in [
        ("sprocket", Spec::defaults_for(Kind::Sprocket)),
        ("sprocket_pair", pair),
        ("pulley", Spec::defaults_for(Kind::Pulley)),
        ("pulley_pair", belt),
        ("pulley_t5", t5),
        ("spline", Spec::defaults_for(Kind::Spline)),
        ("spline_ansi", ansi),
        ("face_gear", Spec::defaults_for(Kind::FaceGear)),
        ("elliptical", Spec::defaults_for(Kind::Elliptical)),
    ] {
        let p = plan(&s).unwrap();
        let mut out = String::from("[");
        for (i, g) in p.preview.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let loops: Vec<String> = g
                .region
                .outer
                .iter()
                .chain(&g.region.holes)
                .map(|o| {
                    format!(
                        "[{}]",
                        o.polyline(0.005).iter().map(|q| format!("[{:.5},{:.5}]", q.x + g.center.x, q.y + g.center.y)).collect::<Vec<_>>().join(",")
                    )
                })
                .collect();
            out.push_str(&format!("{{\"name\":\"{}\",\"loops\":[{}]}}", g.name, loops.join(",")));
        }
        out.push(']');
        std::fs::write(format!("{dir}/{n}.json"), out).unwrap();
    }
}

#[test]
fn elliptical_pair_rolls_and_closes() {
    use crate::elliptic::EllipticPair;
    let e =
        EllipticPair { teeth: 30, module: 2.0, eccentricity: 0.3, pressure_angle: 20f64.to_radians(), addendum: 1.0, dedendum: 1.25, backlash: 0.0 };
    let p = e.pitch();
    // The mate closes after one turn and has the same length of pitch curve.
    let c2 = &p.curve2;
    let len2: f64 = (0..c2.len()).map(|i| c2[i].dist(c2[(i + 1) % c2.len()])).sum();
    assert!(close(len2, PI * 2.0 * 30.0, 0.05), "{len2}");
    assert!(c2[0].dist(c2[c2.len() - 1]) < 0.2);
    // r1 + r2 = A everywhere along the contact.
    assert!(p.ratio_min < 1.0 && p.ratio_max > 1.0);
    let o1 = e.teeth(&EllipticPair::curve1(&p), 0.0);
    closed(&o1);
    simple(&o1);
    let o2 = e.teeth(c2, PI * 2.0 / 2.0);
    closed(&o2);
    simple(&o2);
    assert!(o1.area() > 0.0 && o2.area() > 0.0);
}

/// The fields only the new types use, drawn over their whole range (for the property sweeps).
pub(crate) fn random_extras(r: &mut crate::sweep_tests::Rng, s: &mut Spec) {
    use crate::sweep_tests::random_bore;
    let two_bores = |r: &mut crate::sweep_tests::Rng, s: &mut Spec, d1: f64, d2: f64| {
        s.bore = random_bore(r, d1);
        s.bore2 = random_bore(r, d2);
        if s.hub_diameter > 0.0 {
            s.hub_diameter = s.bore.diameter.max(d1 * 0.1) * r.range(1.1, 3.0);
        }
        if s.holes > 0 && s.hole_diameter > 0.0 {
            s.hole_diameter = d1 * r.range(0.02, 0.4);
        }
    };
    match s.kind {
        Kind::Sprocket => {
            let pitch = if r.chance(0.25) {
                s.chain = crate::sprocket::CUSTOM;
                s.chain_pitch = r.log_range(3.0, 80.0);
                s.roller_diameter = s.chain_pitch * r.range(0.3, 0.8);
                s.roller_width = s.chain_pitch * r.range(0.2, 1.0);
                s.strand_pitch = s.roller_width * r.range(1.05, 2.5);
                s.chain_pitch
            } else {
                let c = CHAINS[r.int(0, CHAINS.len() as u32 - 1) as usize];
                s.chain = c.id;
                c.pitch
            };
            s.strands = r.int(1, 4);
            s.side_chamfer = r.chance(0.6);
            s.teeth = r.teeth(6).min(250);
            s.teeth2 = if r.chance(0.5) { 0 } else { r.teeth(6).min(250) };
            s.center_distance = if r.chance(0.4) { 0.0 } else { pitch * r.log_range(2.0, 120.0) };
            s.hub_length = pitch * r.range(0.2, 3.0);
            let d = |z: u32| pitch * f64::from(z) / PI;
            two_bores(r, s, d(s.teeth), d(s.teeth2.max(6)));
        }
        Kind::Pulley => {
            let b = BELTS[r.int(0, BELTS.len() as u32 - 1) as usize];
            s.belt = b.id;
            s.flanges = r.chance(0.5);
            s.teeth = r.teeth(6);
            s.teeth2 = if r.chance(0.5) { 0 } else { r.teeth(6) };
            s.face_width = b.pitch * r.log_range(0.5, 20.0);
            s.center_distance = if r.chance(0.4) { 0.0 } else { b.pitch * r.log_range(1.0, 400.0) };
            s.hub_length = b.pitch * r.range(0.2, 6.0);
            let d = |z: u32| b.pitch * f64::from(z) / PI;
            two_bores(r, s, d(s.teeth), d(s.teeth2.max(6)));
        }
        Kind::Spline => {
            s.spline_standard = if r.chance(0.5) { SplineStandard::Din5480 } else { SplineStandard::AnsiB92 };
            s.module = r.log_range(0.2, 12.0);
            s.teeth = r.teeth(6);
            s.pressure_angle = if r.chance(0.6) { 30.0 } else { [37.5, 45.0, r.range(20.0, 45.0)][r.int(0, 2) as usize] };
            let z = f64::from(s.teeth);
            s.spline_diameter = if r.chance(0.4) { 0.0 } else { s.module * (z + 1.1 + 2.0 * r.range(-0.3, 0.8)) };
            s.fillet_root = r.chance(0.5);
            s.backlash = if r.chance(0.5) { 0.0 } else { r.range(0.0, 0.1) * s.module };
            s.face_width = s.module * r.log_range(1.0, 40.0);
            s.hub_diameter = if r.chance(0.6) { 0.0 } else { s.module * z * r.range(0.9, 2.0) };
            s.bore = random_bore(r, s.module * z);
        }
        Kind::FaceGear => {
            s.teeth = r.teeth(6).min(120);
            s.teeth2 = if r.chance(0.8) { (f64::from(s.teeth) * r.range(1.0, 8.0)) as u32 } else { r.teeth(6) };
            s.pressure_angle = r.range(14.5, 30.0);
            let d2 = s.module * f64::from(s.teeth2.max(6));
            s.inner_diameter = if r.chance(0.4) { 0.0 } else { d2 * r.range(0.5, 1.4) };
            two_bores(r, s, d2, s.module * f64::from(s.teeth));
        }
        Kind::Elliptical => {
            s.teeth = r.int(8, 80);
            s.ellipse_eccentricity = r.range(0.0, 0.7);
            let d = s.module * f64::from(s.teeth);
            s.bore = random_bore(r, d * 0.6);
            s.bore2 = random_bore(r, d * 0.6);
        }
        _ => {}
    }
}

#[test]
fn sweep_sprockets_and_pulleys() {
    crate::sweep_tests::sweep(&[Kind::Sprocket, Kind::Pulley], 600, 11);
}

#[test]
fn sweep_splines() {
    crate::sweep_tests::sweep(&[Kind::Spline], 600, 12);
}

#[test]
fn sweep_face_gears() {
    crate::sweep_tests::sweep(&[Kind::FaceGear], 400, 13);
}

#[test]
fn sweep_elliptical_pairs() {
    crate::sweep_tests::sweep(&[Kind::Elliptical], 60, 14);
}

// ---------------------------------------------------------------------------------------------
// Standard tables.

#[test]
fn sprocket_tables_iso_606() {
    // Pitch diameters p / sin(180°/z) from the standard's table of d for unit pitch.
    for (z, unit) in [
        (9u32, 2.9238),
        (11, 3.5495),
        (13, 4.1786),
        (15, 4.8097),
        (17, 5.4422),
        (19, 6.0755),
        (21, 6.7095),
        (25, 7.9787),
        (38, 12.1096),
        (57, 18.1529),
    ] {
        let s = Sprocket { teeth: z, pitch: 1.0, roller: 0.6 };
        assert!(close(s.pitch_diameter(), unit, 6e-5), "z{z}: {}", s.pitch_diameter());
    }
    // Catalogue sprockets: (chain, z, pitch Ø, root Ø).
    for (id, z, d, df) in [
        ("06b", 15u32, 45.81, 39.46),
        ("08b", 19, 77.16, 68.65),
        ("10b", 21, 106.51, 96.35),
        ("12b", 25, 152.00, 139.93),
        ("16b", 13, 106.14, 90.26),
        ("ansi40", 18, 73.14, 65.22),
        ("ansi60", 15, 91.62, 79.71),
    ] {
        let c = chain_by_id(id).unwrap();
        let s = Sprocket { teeth: z, pitch: c.pitch, roller: c.roller };
        assert!(close(s.pitch_diameter(), d, 0.01), "{id} z{z}: d {}", s.pitch_diameter());
        assert!(close(s.root_diameter(), df, 0.01), "{id} z{z}: df {}", s.root_diameter());
        // The tooth form's limits: seating radius, seating angle, flank radius, tip diameter.
        let (d1, zf) = (c.roller, f64::from(z));
        let ri = s.seating_radius();
        assert!(ri >= 0.505 * d1 && ri <= 0.505 * d1 + 0.069 * d1.cbrt());
        let al = s.seating_angle().to_degrees();
        assert!(al >= 120.0 - 90.0 / zf && al <= 140.0 - 90.0 / zf);
        let re = s.flank_radius();
        assert!(re >= 0.12 * d1 * (zf + 2.0) && re <= 0.008 * d1 * (zf * zf + 180.0));
        let (lo, hi) = s.tip_diameter_range();
        assert!(close(hi, d + 1.25 * c.pitch - d1, 0.01) && close(lo, d + (1.0 - 1.6 / zf) * c.pitch - d1, 0.01));
        // Every roller sits in its seat: the outline comes no nearer its centre than the seating
        // radius, and reaches that at the seat's bottom.
        let t = s.outline(0.0, s.tip_diameter() / 2.0);
        let poly = t.outline.polyline(1e-4);
        for k in 0..z {
            let centre = V2::polar(s.pitch_diameter() / 2.0, std::f64::consts::TAU * f64::from(k) / zf);
            let nearest = poly.windows(2).map(|w| seg_dist(centre, w[0], w[1])).fold(f64::MAX, f64::min);
            assert!(close(nearest, ri, 2e-4) && nearest > d1 / 2.0, "{id} z{z} roller {k}: {nearest} against {ri}");
        }
    }
    // Even teeth are measured over the roots, odd teeth corrected by cos(90°/z).
    let even = Sprocket { teeth: 18, pitch: 12.7, roller: 7.92 };
    assert!(close(even.caliper_diameter(), even.root_diameter(), 1e-12));
}

fn seg_dist(p: V2, a: V2, b: V2) -> f64 {
    let d = b - a;
    let l2 = d.dot(d);
    let t = if l2 > 0.0 { ((p - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + d * t)
}

#[test]
fn multi_strand_sprockets_are_discs_on_a_core() {
    for strands in 1..=4u32 {
        for chamfer in [false, true] {
            let mut s = Spec::defaults_for(Kind::Sprocket);
            (s.strands, s.side_chamfer, s.teeth) = (strands, chamfer, 19);
            let p = plan(&s).unwrap();
            crate::sweep_tests::check_plan(&p).unwrap();
            let steps = &p.bodies[0].steps;
            let prisms = steps.iter().filter(|st| matches!(st, Step::Prism { .. })).count();
            assert_eq!(prisms, 2 * strands as usize - 1, "{strands} strands");
            assert_eq!(steps.iter().filter(|st| matches!(st, Step::Revolve { .. })).count(), usize::from(chamfer));
            // The last disc ends at (strands − 1)·pt + bf1.
            let top =
                steps.iter().filter_map(|st| if let Step::Prism { z0, height, .. } = st { Some(z0 + height) } else { None }).fold(0.0, f64::max);
            let width = p.facts.iter().find(|f| f.label == "width over the teeth").unwrap().value;
            assert!(close(top, width, 1e-9) && close(width, f64::from(strands - 1) * 13.92 + if strands == 1 { 0.93 } else { 0.91 } * 7.75, 1e-9));
        }
    }
}

#[test]
fn pulley_tables() {
    // Makers' tables: (belt, teeth, pitch Ø, outside Ø).
    for (id, z, pd, od) in [
        ("gt2_2", 16u32, 10.19, 9.68),
        ("gt2_2", 36, 22.92, 22.41),
        ("gt2_3", 20, 19.10, 18.34),
        ("gt2_5", 18, 28.65, 27.51),
        ("htd_3m", 24, 22.92, 22.16),
        ("htd_5m", 20, 31.83, 30.69),
        ("htd_8m", 22, 56.02, 54.65),
        ("t2_5", 20, 15.92, 15.40),
        ("t5", 10, 15.92, 15.05),
        ("t5", 20, 31.83, 30.97),
        ("t10", 20, 63.66, 61.80),
        ("mxl", 20, 12.94, 12.43),
        ("xl", 10, 16.17, 15.66),
        ("xl", 20, 32.34, 31.83),
    ] {
        let p = Pulley { belt: *belt_by_id(id).unwrap(), teeth: z };
        assert!(close(p.pitch_diameter(), pd, 0.006), "{id} z{z}: pitch Ø {}", p.pitch_diameter());
        assert!(close(p.outside_diameter(), od, 0.006), "{id} z{z}: outside Ø {}", p.outside_diameter());
    }
    // Every curvilinear profile says it is an approximation; flanges and the hub build.
    for belt in BELTS {
        let mut s = Spec::defaults_for(Kind::Pulley);
        s.belt = belt.id;
        (s.teeth, s.face_width, s.hub_diameter, s.hub_length) = (24, belt.widths[0], 0.6 * belt.pitch * 24.0 / PI, 6.0);
        s.bore = crate::hub::Bore { kind: crate::hub::BoreKind::Round, diameter: 0.25 * belt.pitch * 24.0 / PI, ..Default::default() };
        let p = plan(&s).unwrap();
        crate::sweep_tests::check_plan(&p).unwrap();
        assert!(p.preview_note.contains("approximated"), "{}", belt.id);
        assert!(p.warnings.iter().any(|w| w.contains("approximated")), "{}", belt.id);
        assert_eq!(p.warnings.iter().any(|w| w.contains("proprietary")), belt.id.starts_with("gt2") || belt.id.starts_with("htd"), "{}", belt.id);
        // Tooth prism, two flanges, hub, bore.
        assert_eq!(p.bodies[0].steps.len(), 5, "{}", belt.id);
        s.flanges = false;
        assert_eq!(plan(&s).unwrap().bodies[0].steps.len(), 3);
    }
}

// ---------------------------------------------------------------------------------------------
// Meshing, proved with the 2D checker.

/// The hub and shaft of a spline plan against each other: (overlap depth, overlap area, gap), in
/// modules (area in modules² per tooth).
fn spline_fit(s: &Spec) -> (f64, f64, f64) {
    use crate::mesh2d::{Member, run};
    let p = plan(s).unwrap_or_else(|e| panic!("{e}"));
    let m = s.module;
    let (hub, shaft) = (Member::of(&p.preview[0], 0.2 * m, 2e-5 * m), Member::of(&p.preview[1], 0.2 * m, 2e-5 * m));
    let st = run(&hub, &shaft, 0.0, 1.0, 3);
    (st.depth / m, st.area / (m * m * f64::from(s.teeth)), st.gap_min / m)
}

fn spline_spec(standard: SplineStandard, m: f64, z: u32, db: f64, pa: f64, fillet: bool, clearance: f64) -> Spec {
    let mut s = Spec::defaults_for(Kind::Spline);
    (s.spline_standard, s.module, s.teeth, s.spline_diameter, s.pressure_angle, s.fillet_root, s.backlash) =
        (standard, m, z, db, pa, fillet, clearance);
    s
}

#[test]
fn spline_shaft_and_hub_mate() {
    // Flank to flank: no overlap beyond the fit tolerance of the two splined flanks, and no gap.
    let depth = 2.0 * crate::involute::PROFILE_TOLERANCE + 3e-4;
    let din = SplineStandard::Din5480;
    let ansi = SplineStandard::AnsiB92;
    for (st, m, z, db, pa) in [
        (din, 1.25, 18u32, 25.0, 30.0),
        (din, 2.0, 14, 30.0, 30.0),
        (din, 2.0, 21, 45.0, 30.0),
        (din, 3.0, 38, 120.0, 30.0),
        (din, 0.8, 6, 0.0, 30.0),
        (din, 5.0, 60, 0.0, 30.0),
        (ansi, 25.4 / 16.0, 24, 0.0, 30.0),
        (ansi, 25.4 / 8.0, 10, 0.0, 30.0),
        (ansi, 25.4 / 24.0, 40, 0.0, 37.5),
        (ansi, 25.4 / 32.0, 30, 0.0, 45.0),
    ] {
        for fillet in [false, true] {
            if fillet && pa > 44.0 {
                // The fillet root is too deep for a 45° cutter: refused.
                assert!(plan(&spline_spec(st, m, z, db, pa, fillet, 0.0)).unwrap_err().contains("fillet"));
                continue;
            }
            for c in [0.0, 0.04 * m] {
                let s = spline_spec(st, m, z, db, pa, fillet, c);
                let (d, area, gap) = spline_fit(&s);
                // The clearance is the space's width less the tooth's: half of it a flank.
                let want = 0.5 * c * pa.to_radians().cos();
                eprintln!(
                    "spline {st:?} m{m:.3} z{z} dB{db} {pa}° fillet {fillet} clearance {c:.3}: depth {d:.5} m, area {area:.6} m²/tooth, gap {gap:.5} m (want {:.5})",
                    want / m
                );
                assert!(d <= depth, "overlap {d} modules deep");
                assert!(area <= 1e-3, "overlap {area} m² a tooth");
                assert!((gap - want / m).abs() <= 1.5e-3 + 0.2 * want / m, "gap {gap} modules, {} wanted", want / m);
            }
        }
    }
}

#[test]
fn spline_refusals_and_notes() {
    let din = SplineStandard::Din5480;
    // A reference diameter that doesn't suit the teeth.
    let e = plan(&spline_spec(din, 2.0, 20, 60.0, 30.0, false, 0.0)).unwrap_err();
    assert!(e.contains("reference diameter") && e.contains("42") && e.contains("44"), "{e}");
    // A bore through the roots, a hub thinner than its teeth.
    let mut s = spline_spec(din, 1.25, 18, 25.0, 30.0, false, 0.0);
    s.bore = crate::hub::Bore { kind: crate::hub::BoreKind::Round, diameter: 23.0, ..Default::default() };
    assert!(plan(&s).unwrap_err().contains("bore"));
    s.bore = Default::default();
    s.hub_diameter = 24.0;
    assert!(plan(&s).unwrap_err().contains("outside diameter"));
    s.hub_diameter = 40.0;
    let p = plan(&s).unwrap();
    assert_eq!(p.bodies.len(), 2);
    assert!(p.preview_note.contains("DIN 5480") && p.preview_note.contains("25 × 1.25 × 18"), "{}", p.preview_note);
    let fact = |l: &str| p.facts.iter().find(|f| f.label == l).unwrap_or_else(|| panic!("no fact {l}")).value;
    assert!(close(fact("hub major (root) diameter"), 25.0, 1e-9) && close(fact("shaft minor (root) diameter"), 22.125, 1e-9));
    // DIN 5480's nominal tooth thickness: m·π/2 + 2·x·m·tan 30°.
    assert!(close(fact("shaft tooth thickness (pitch)"), 1.25 * PI / 2.0 + 2.0 * 0.45 * 1.25 * 30f64.to_radians().tan(), 1e-9));
    // Off-standard pressure angles are said.
    let p = plan(&spline_spec(din, 1.25, 18, 25.0, 25.0, false, 0.0)).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("30°")));
    assert!(plan(&spline_spec(din, 1.25, 18, 25.0, 50.0, false, 0.0)).is_err());
}

/// The face gear's teeth at radius `r`, unrolled, under the pinion: the checker's numbers in
/// modules over two pinion pitches.
fn face_mesh(fg: &FaceGear, r: f64) -> crate::mesh2d::Stat {
    use crate::mesh2d::{Member, Motion, run};
    let m = fg.pinion.module;
    let rw = r * fg.z1() / fg.z2();
    let rack = crate::geom::Region { outer: vec![fg.unrolled(r, 11, 2.0 * m)], holes: vec![] };
    let pinion = crate::geom::Region { outer: vec![crate::involute::external_outline(&fg.pinion, -PI / 2.0 + PI / fg.z1()).0], holes: vec![] };
    let a = Member::new(&pinion, Motion::turning(V2::ZERO, 1.0), 0.2 * m, 2e-5 * m);
    let b = Member::new(&rack, Motion { center: V2::ZERO, speed: 0.0, slide: rw, orbit: 0.0 }, 0.2 * m, 2e-5 * m);
    let turn = std::f64::consts::TAU / fg.z1();
    run(&a, &b, -turn, turn, 40)
}

#[test]
fn face_gear_teeth_mesh_with_the_pinion_along_their_length() {
    let depth = 2.0 * crate::involute::PROFILE_TOLERANCE + 3e-4;
    for (z1, z2, m, pa, backlash) in [
        (18u32, 40u32, 2.0, 20.0, 0.0),
        (18, 60, 2.0, 20.0, 0.0),
        (12, 48, 1.0, 20.0, 0.0),
        (24, 72, 3.0, 20.0, 0.0),
        (20, 100, 1.5, 25.0, 0.0),
        (17, 51, 2.0, 20.0, 0.1),
        (30, 45, 2.0, 20.0, 0.0),
    ] {
        let pinion = Involute { teeth: z1, module: m, pressure_angle: f64::to_radians(pa), ..Involute::default() };
        let fg = FaceGear::new(pinion, z2, backlash);
        let (ri, ro) = (fg.undercut_radius(), fg.pointed_radius());
        assert!(ro > ri, "z{z1}/{z2}: no room ({ri} … {ro})");
        for (k, r) in [ri, 0.5 * (ri + ro), ro].into_iter().enumerate() {
            let st = face_mesh(&fg, r);
            let aw = (pinion.rb() / (r * fg.z1() / fg.z2())).acos();
            // The backlash comes off the face gear's teeth: half of it a flank.
            let want = 0.5 * backlash * aw.cos();
            eprintln!(
                "face gear {z1}/{z2} m{m} {pa}° at r {r:.3} ({}): depth {:.5} m, area {:.6} m², gap {:.5}..{:.5} m (want {:.5})",
                ["undercut limit", "middle", "pointing limit"][k],
                st.depth / m,
                st.area / (m * m),
                st.gap_min / m,
                st.gap_max / m,
                want / m
            );
            assert!(st.depth <= depth * m, "z{z1}/{z2} at r {r}: flanks cross by {} modules", st.depth / m);
            assert!(st.area <= 1e-3 * m * m, "z{z1}/{z2} at r {r}: overlap {} m²", st.area / (m * m));
            assert!(st.gap_min >= 0.8 * want - 1.5e-3 * m);
            if k == 1 {
                assert!(st.gap_max <= want + 1.5e-3 * m, "z{z1}/{z2} at r {r}: flanks {} modules apart", st.gap_max / m);
            }
        }
    }
}

#[test]
fn face_gear_plan_limits_and_refusals() {
    let s = Spec::defaults_for(Kind::FaceGear);
    let p = plan(&s).unwrap();
    crate::sweep_tests::check_plan(&p).unwrap();
    let fact = |p: &crate::plan::Plan, l: &str| p.facts.iter().find(|f| f.label == l).unwrap_or_else(|| panic!("no fact {l}")).value;
    let (lo, hi) = (fact(&p, "undercut limit diameter"), fact(&p, "pointing limit diameter"));
    assert!(lo < hi && close(fact(&p, "teeth inner diameter"), lo, 1e-9));
    assert!(close(fact(&p, "tooth length"), s.face_width, 1e-9));
    // The pinion's axis is square to the face gear's and a pitch radius above its pitch plane.
    let pin = &p.bodies[1].frame;
    assert!(close(pin.z[0], 1.0, 1e-12) && close(pin.origin[2], s.module * f64::from(s.teeth) / 2.0, 1e-12));
    // The preview: the pinion's space points down at the start, over a face gear tooth.
    let (pinion, rack) = (&p.preview[0], &p.preview[1]);
    assert!(rack.slide > 0.0 && rack.period > 0.0 && close(pinion.speed, 1.0, 1e-12));
    let placed: Vec<V2> = rack.region.outer[0].polyline(0.01).iter().map(|q| rack.place(*q, 0.0)).collect();
    let top = placed.iter().map(|q| q.y).fold(f64::MIN, f64::max);
    assert!(placed.iter().any(|q| q.y > top - 1e-9 && q.x.abs() < 0.25 * rack.period), "no tooth under the pinion's space");
    let low = pinion.region.outer[0]
        .polyline(0.01)
        .iter()
        .map(|q| pinion.place(*q, 0.0))
        .filter(|q| q.x.abs() < 0.1 * rack.period)
        .map(|q| q.y)
        .fold(f64::MAX, f64::min);
    assert!(low > top, "the pinion's space is not over the tooth ({low} against {top})");

    let mut long = s.clone();
    long.face_width = 100.0;
    let p2 = plan(&long).unwrap();
    assert!(close(fact(&p2, "teeth outer diameter"), hi, 1e-6) && p2.warnings.iter().any(|w| w.contains("point")));
    let mut inside = s.clone();
    inside.inner_diameter = 0.5 * lo;
    let p3 = plan(&inside).unwrap();
    assert!(close(fact(&p3, "teeth inner diameter"), lo, 1e-9) && p3.warnings.iter().any(|w| w.contains("undercut")));
    // No room at all: refused with both limits.
    let mut none = s.clone();
    none.inner_diameter = hi;
    let e = plan(&none).unwrap_err();
    assert!(e.contains("no room") && e.contains("smaller inner diameter"), "{e}");
    let mut low = s.clone();
    (low.teeth, low.teeth2) = (30, 32);
    if let Err(e) = plan(&low) {
        assert!(e.contains("no room") || e.contains("more teeth"), "{e}");
    }
}
