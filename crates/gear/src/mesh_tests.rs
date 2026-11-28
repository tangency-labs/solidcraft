//! 2D meshing of every parallel-axis set, proved with the checker in `mesh2d`: over two tooth
//! pitches (a whole input turn for the cycloidal drive) no pair overlaps and the flanks stay in
//! contact (or a backlash apart). Run with `--nocapture` for the numbers of every case.

use std::f64::consts::{PI, TAU};

use crate::geom::{Region, V2};
use crate::involute::{
    Internal, Involute, external_flank, external_outline, internal_tip_interference, mesh_internal, mesh_phase, mesh_phase_internal,
};
use crate::mesh2d::{Member, Motion, Stat, probe, run};
use crate::plan::{Kind, Plan, PreviewGear, Spec, plan};

struct Row {
    label: String,
    /// The length the tolerances scale with (the module; the pin radius for a cycloidal drive).
    m: f64,
    stat: Stat,
    /// The flank gap the backlash asks for (mm, normal to the flanks).
    want_gap: f64,
    warnings: Vec<String>,
}

/// Driver angles per run (two tooth pitches).
const STEPS: usize = 40;
/// Each flank is a spline through as few points as keep it within `PROFILE_TOLERANCE` of the
/// exact curve, so two flanks in contact may cross by twice that, and a little for the corners
/// that run where a fillet (fitted more loosely) meets its flank (× m).
const DEPTH: f64 = 2.0 * crate::involute::PROFILE_TOLERANCE + 3e-4;
/// Largest overlap area (× m²).
const AREA: f64 = 1e-3;
/// How far the flanks may be from touching (beyond the backlash) at any angle (× m).
const GAP: f64 = 1.5e-3;

impl Row {
    fn show(&self) {
        let s = &self.stat;
        eprintln!(
            "{:<62} area {:>8.5} mm2  depth {:>7.5} m  gap {:>7.5}..{:>7.5} m (want {:.4}){}",
            self.label,
            s.area,
            s.depth / self.m,
            s.gap_min / self.m,
            s.gap_max / self.m,
            self.want_gap / self.m,
            if self.warnings.is_empty() { String::new() } else { format!("  ! {}", self.warnings.join(" | ")) }
        );
    }
    fn overlaps(&self) -> bool {
        self.stat.depth > DEPTH * self.m || self.stat.area > AREA * self.m * self.m
    }
    /// No overlap at any angle, and contact (a backlash apart at most) at every angle, unless
    /// the plan itself says the teeth lose contact.
    fn check(&self) {
        self.show();
        let (s, m) = (&self.stat, self.m);
        assert!(s.depth <= DEPTH * m, "{}: flanks cross by {} mm (at driver angle {})", self.label, s.depth, s.area_at);
        assert!(s.area <= AREA * m * m, "{}: overlap of {} mm² at driver angle {}", self.label, s.area, s.area_at);
        if !self.warnings.iter().any(|w| w.contains("lose contact")) {
            assert!(s.gap_max <= self.want_gap + GAP * m, "{}: flanks {} mm apart, {} wanted", self.label, s.gap_max, self.want_gap);
        }
        // Backlash is set on the pitch circle; along the flanks it comes out a little less.
        assert!(s.gap_min >= 0.8 * self.want_gap - GAP * m, "{}: flanks only {} mm apart, {} wanted", self.label, s.gap_min, self.want_gap);
    }
}

fn member(g: &PreviewGear, m: f64) -> Member {
    Member::of(g, 0.2 * m, 2e-5 * m)
}

fn fact(p: &Plan, label: &str) -> f64 {
    p.facts.iter().find(|f| f.label == label).unwrap_or_else(|| panic!("no fact {label}")).value
}

fn pair_spec(z1: u32, z2: u32, x1: f64, x2: f64) -> Spec {
    Spec { kind: Kind::Pair, teeth: z1, teeth2: z2, shift: x1, shift2: x2, ..Spec::default() }
}

fn pair_row(label: &str, s: &Spec) -> Row {
    let p = plan(s).unwrap();
    let (a, b) = (member(&p.preview[0], s.module), member(&p.preview[1], s.module));
    let stat = run(&a, &b, 0.0, 2.0 * TAU / f64::from(s.teeth), STEPS);
    let aw = fact(&p, "working pressure angle").to_radians();
    Row { label: format!("pair {label}"), m: s.module, stat, want_gap: s.backlash * aw.cos(), warnings: p.warnings }
}

#[test]
fn pairs_mesh_plain_and_undercut() {
    for (z1, z2) in [(20, 32), (12, 40), (8, 25), (6, 13), (10, 10), (17, 60), (30, 120), (9, 11), (25, 25), (40, 13)] {
        pair_row(&format!("{z1}/{z2}"), &pair_spec(z1, z2, 0.0, 0.0)).check();
    }
    for (z1, z2, pa, m) in [(30, 40, 14.5, 1.0), (12, 20, 25.0, 3.0), (22, 35, 17.5, 0.5), (18, 27, 30.0, 4.0)] {
        pair_row(&format!("{z1}/{z2} pa {pa} m {m}"), &Spec { pressure_angle: pa, module: m, ..pair_spec(z1, z2, 0.0, 0.0) }).check();
    }
}

#[test]
fn pairs_mesh_shifted_and_at_a_centre_distance() {
    for (z1, z2, x1, x2) in [(12, 30, 0.4, 0.0), (10, 20, 0.5, 0.3), (20, 32, -0.2, 0.2), (15, 45, 0.3, -0.3), (8, 12, 0.5, 0.4), (14, 50, 0.0, 0.6)]
    {
        pair_row(&format!("{z1}/{z2} x {x1}/{x2}"), &pair_spec(z1, z2, x1, x2)).check();
    }
    for (z1, z2, x1, a) in [(20, 32, 0.0, 52.8), (20, 30, 0.2, 51.0), (14, 22, 0.0, 37.0), (20, 32, 0.0, 51.5), (11, 17, 0.3, 29.0)] {
        let s = Spec { center_distance: a, ..pair_spec(z1, z2, x1, 0.0) };
        pair_row(&format!("{z1}/{z2} x1 {x1} a {a}"), &s).check();
        let p = plan(&s).unwrap();
        assert!((fact(&p, "centre distance") - a).abs() < 1e-6);
        assert!(!p.warnings.iter().any(|w| w.contains("tip clearance")), "{:?}", p.warnings);
    }
}

#[test]
fn pairs_mesh_helical_and_with_backlash() {
    for (z1, z2, x1, helix) in [(20, 32, 0.0, 20.0), (12, 25, 0.3, 30.0), (9, 30, 0.0, -15.0), (16, 16, 0.0, 45.0)] {
        pair_row(&format!("{z1}/{z2} x1 {x1} helix {helix}"), &Spec { helix, ..pair_spec(z1, z2, x1, 0.0) }).check();
    }
    for (z1, z2, x1, b) in [(20, 32, 0.0, 0.1), (10, 24, 0.3, 0.1), (8, 25, 0.0, 0.1), (30, 31, 0.0, 0.05)] {
        pair_row(&format!("{z1}/{z2} x1 {x1} backlash {b}"), &Spec { backlash: b, ..pair_spec(z1, z2, x1, 0.0) }).check();
    }
    pair_row("20/32 helix 20 a 56 backlash 0.1", &Spec { helix: 20.0, center_distance: 56.0, backlash: 0.1, ..pair_spec(20, 32, 0.0, 0.0) }).check();
}

#[test]
fn undercut_pairs_that_lose_contact_say_so() {
    // Heavily undercut pinions have too little involute left to hand over from tooth to tooth.
    for (z1, z2) in [(6, 13), (9, 11)] {
        let r = pair_row(&format!("{z1}/{z2}"), &pair_spec(z1, z2, 0.0, 0.0));
        r.show();
        // (How wide the gap gets depends on which angles are probed; that there is one, on the
        // 6-tooth pinion, does not.)
        assert!(z1 != 6 || r.stat.gap_max > GAP * r.m, "{}", r.label);
        assert!(r.warnings.iter().any(|w| w.contains("lose contact")), "{:?}", r.warnings);
    }
    // A sound pair says nothing of the kind.
    assert!(pair_row("20/32", &pair_spec(20, 32, 0.0, 0.0)).warnings.is_empty());
}

fn planetary_spec(zs: u32, zp: u32, zr: u32, n: u32) -> Spec {
    Spec { kind: Kind::Planetary, teeth: zs, teeth2: zp, teeth3: zr, planets: n, ..Spec::default() }
}

/// Sun–planet and planet–ring rows, each over every planet.
fn planetary_rows(label: &str, s: &Spec) -> [Row; 2] {
    let p = plan(s).unwrap();
    let n = p.preview.len();
    let sun = member(&p.preview[0], s.module);
    let ring = member(&p.preview[n - 1], s.module);
    let t1 = 2.0 * TAU / f64::from(s.teeth);
    let (mut sp, mut pr): (Option<Stat>, Option<Stat>) = (None, None);
    for g in &p.preview[1..n - 1] {
        let planet = member(g, s.module);
        for (acc, st) in [(&mut sp, run(&sun, &planet, 0.0, t1, STEPS)), (&mut pr, run(&planet, &ring, 0.0, t1, STEPS))] {
            match acc {
                Some(a) => a.merge(&st),
                None => *acc = Some(st),
            }
        }
    }
    let at = (s.pressure_angle.to_radians().tan() / s.helix.to_radians().cos()).atan();
    let want = s.backlash * at.cos();
    [
        Row { label: format!("planetary {label} sun-planet"), m: s.module, stat: sp.unwrap(), want_gap: want, warnings: p.warnings.clone() },
        Row { label: format!("planetary {label} planet-ring"), m: s.module, stat: pr.unwrap(), want_gap: want, warnings: p.warnings },
    ]
}

#[test]
fn planetary_sets_mesh_at_every_planet() {
    for (zs, zp, zr, n) in [(18, 18, 54, 3), (20, 16, 52, 3), (12, 18, 48, 4), (24, 12, 48, 6), (21, 21, 63, 2), (17, 19, 55, 1)] {
        for r in planetary_rows(&format!("{zs}/{zp}/{zr} x{n}"), &planetary_spec(zs, zp, zr, n)) {
            r.check();
        }
    }
    for r in planetary_rows("18/18/54 x3 helix 15", &Spec { helix: 15.0, ..planetary_spec(18, 18, 54, 3) }) {
        r.check();
    }
}

#[test]
fn planetary_sets_mesh_with_uneven_spacing() {
    // (sun + ring) / planets not whole: the planets go to the nearest places that mesh.
    for (zs, zp, zr, n) in [(20, 15, 50, 3), (16, 20, 56, 5), (10, 25, 60, 3), (30, 15, 60, 4), (36, 12, 60, 5)] {
        for r in planetary_rows(&format!("{zs}/{zp}/{zr} x{n}"), &planetary_spec(zs, zp, zr, n)) {
            assert!(r.warnings.iter().any(|w| w.contains("spaced unevenly")));
            r.check();
        }
    }
    for r in planetary_rows("20/15/50 x4 backlash 0.1", &Spec { backlash: 0.1, ..planetary_spec(20, 15, 50, 4) }) {
        r.check();
    }
    for r in planetary_rows("16/20/56 x3 m 1 pa 25", &Spec { module: 1.0, pressure_angle: 25.0, ..planetary_spec(16, 20, 56, 3) }) {
        r.check();
    }
}

#[test]
fn planetary_ring_shifts_to_reach_the_planets() {
    // Shifted sun and planets, and ring tooth counts off sun + 2 × planet: the ring's own shift
    // brings it to the planets' centre distance.
    for (zs, zp, zr, n, x1, x2) in
        [(18, 18, 54, 3, 0.3, 0.2), (12, 18, 48, 4, 0.4, 0.0), (20, 16, 51, 1, 0.0, 0.0), (20, 16, 53, 1, 0.0, 0.0), (15, 15, 45, 3, 0.2, 0.3)]
    {
        let s = Spec { shift: x1, shift2: x2, ..planetary_spec(zs, zp, zr, n) };
        let p = plan(&s).unwrap();
        assert!(!p.warnings.iter().any(|w| w.contains("planets sit at")), "{:?}", p.warnings);
        assert!(fact(&p, "ring profile shift").abs() > 1e-6);
        for r in planetary_rows(&format!("{zs}/{zp}/{zr} x{n} x {x1}/{x2}"), &s) {
            r.check();
        }
    }
}

fn involute_of(s: &Spec, teeth: u32, shift: f64, helix: f64) -> Involute {
    Involute {
        teeth,
        module: s.module,
        pressure_angle: s.pressure_angle.to_radians(),
        helix: helix.to_radians(),
        shift,
        rack: s.rack,
        backlash: s.backlash,
        tip_shortening: 0.0,
        virtual_teeth: 0.0,
    }
}

fn gear_region(g: &Involute, phase: f64) -> Region {
    Region { outer: vec![external_outline(g, phase).0], holes: vec![] }
}

/// The `internal` set's ring with a pinion put in it; also whether the tip check flags the pair.
fn internal_row(zr: u32, zp: u32, xr: f64, xp: f64, helix: f64, backlash: f64) -> (Row, bool) {
    let s = Spec { kind: Kind::Internal, teeth: zr, shift: xr, helix, backlash, ..Spec::default() };
    let p = plan(&s).unwrap();
    let ring_gear = Internal { gear: involute_of(&s, zr, xr, helix) };
    let pinion = involute_of(&s, zp, xp, helix);
    let mesh = mesh_internal(&pinion, &ring_gear, s.face_width);
    // The pinion a little off the x axis, a tooth on the line of centres; the plan's ring has
    // its tooth 0 on +x, so it is turned to suit.
    let dir = 0.3;
    let ratio = f64::from(zp) / f64::from(zr);
    let turned = mesh_phase_internal(zp, dir, zr, dir);
    let region = Region { outer: p.preview[0].region.outer.clone(), holes: p.preview[0].region.holes.iter().map(|o| o.rotated(turned)).collect() };
    let ring = Member::new(&region, Motion::turning(V2::ZERO, ratio), 0.2 * s.module, 2e-5 * s.module);
    let pin = Member::new(&gear_region(&pinion, dir), Motion::turning(V2::polar(mesh.center_distance, dir), 1.0), 0.2 * s.module, 2e-5 * s.module);
    let stat = run(&pin, &ring, 0.0, 2.0 * TAU / f64::from(zp), STEPS);
    let flagged = internal_tip_interference(&pinion, external_flank(&pinion).ra, &ring_gear, &mesh);
    let row = Row {
        label: format!("internal {zr} pinion {zp} x {xr}/{xp} helix {helix} backlash {backlash}"),
        m: s.module,
        stat,
        want_gap: backlash * mesh.working_pressure_angle.cos(),
        warnings: p.warnings,
    };
    (row, flagged)
}

#[test]
fn internal_gears_mesh_with_a_pinion() {
    for (zr, zp, xr, xp, helix, b) in [
        (60, 20, 0.0, 0.0, 0.0, 0.0),
        (40, 18, 0.0, 0.0, 0.0, 0.0),
        (80, 30, 0.3, 0.3, 0.0, 0.0),
        (60, 25, 0.2, 0.0, 0.0, 0.0),
        (72, 18, 0.0, 0.4, 0.0, 0.0),
        (60, 20, 0.0, 0.0, 20.0, 0.0),
        (60, 20, 0.0, 0.0, 0.0, 0.1),
        (100, 40, 0.0, 0.0, 0.0, 0.0),
        (48, 24, 0.0, 0.0, 0.0, 0.0),
    ] {
        let (row, flagged) = internal_row(zr, zp, xr, xp, helix, b);
        assert!(!flagged, "{}", row.label);
        row.check();
    }
}

#[test]
fn internal_gears_say_which_pinions_are_too_big() {
    for zr in [36, 60, 90] {
        let p = plan(&Spec { kind: Kind::Internal, teeth: zr, ..Spec::default() }).unwrap();
        let largest = fact(&p, "largest pinion (no tip interference)") as u32;
        assert!(largest >= 3 && largest < zr, "{zr}: {largest}");
        // The largest that fits meshes cleanly; a few teeth more and the tips collide going in
        // and out of mesh, which the check sees coming.
        let (row, flagged) = internal_row(zr, largest, 0.0, 0.0, 0.0, 0.0);
        assert!(!flagged);
        row.check();
        let (row, flagged) = internal_row(zr, largest + 3, 0.0, 0.0, 0.0, 0.0);
        row.show();
        assert!(flagged && row.overlaps(), "{}", row.label);
    }
}

/// The `rack` set's rack with a pinion rolling on it.
fn rack_row(zp: u32, xp: f64, xr: f64, helix: f64, backlash: f64) -> Row {
    let s = Spec { kind: Kind::Rack, shift: xr, helix, backlash, rack_teeth: 10, ..Spec::default() };
    let p = plan(&s).unwrap();
    let pinion = involute_of(&s, zp, xp, -helix);
    let pitch = PI * pinion.mt();
    assert!((fact(&p, "pitch") - pitch).abs() < 1e-9);
    // Over the space between teeth 4 and 5, a tooth pointing down into it; the profile shifts
    // lift the pinion off the line y = 0. The rack slides at the pinion's pitch circle speed.
    let centre = V2::new(5.0 * pitch, pinion.r() + (xp + xr) * s.module);
    let rack =
        Member::new(&p.preview[0].region, Motion { center: V2::ZERO, speed: 0.0, slide: pinion.r(), orbit: 0.0 }, 0.2 * s.module, 2e-5 * s.module);
    let pin = Member::new(&gear_region(&pinion, -PI / 2.0), Motion::turning(centre, 1.0), 0.2 * s.module, 2e-5 * s.module);
    let turn = TAU / f64::from(zp);
    let stat = run(&pin, &rack, -turn, turn, STEPS);
    Row {
        label: format!("rack pinion {zp} x {xp} rack x {xr} helix {helix} backlash {backlash}"),
        m: s.module,
        stat,
        want_gap: backlash * pinion.alpha_t().cos(),
        warnings: p.warnings,
    }
}

#[test]
fn racks_mesh_with_a_pinion() {
    for (zp, xp, xr, helix, b) in [
        (20, 0.0, 0.0, 0.0, 0.0),
        (12, 0.0, 0.0, 0.0, 0.0),
        (8, 0.0, 0.0, 0.0, 0.0),
        (10, 0.4, 0.0, 0.0, 0.0),
        (20, 0.0, 0.3, 0.0, 0.0),
        (15, 0.2, -0.2, 0.0, 0.0),
        (18, 0.0, 0.0, 20.0, 0.0),
        (20, 0.0, 0.0, 0.0, 0.1),
        (40, -0.3, 0.0, 0.0, 0.0),
    ] {
        rack_row(zp, xp, xr, helix, b).check();
    }
}

fn cycloidal_row(z1: u32, z2: u32, m: f64, backlash: f64) -> Row {
    let s = Spec { teeth: z1, teeth2: z2, module: m, backlash, ..Spec::defaults_for(Kind::Cycloidal) };
    let p = plan(&s).unwrap();
    let (a, b) = (member(&p.preview[0], m), member(&p.preview[1], m));
    let stat = run(&a, &b, 0.0, 2.0 * TAU / f64::from(z1), STEPS);
    Row { label: format!("cycloidal {z1}/{z2} m {m} backlash {backlash}"), m, stat, want_gap: backlash, warnings: p.warnings }
}

#[test]
fn cycloidal_pairs_mesh() {
    for (z1, z2, m, b) in [
        (60, 8, 1.0, 0.0),
        (30, 10, 1.0, 0.0),
        (40, 12, 2.0, 0.0),
        (64, 8, 0.5, 0.0),
        (20, 20, 1.0, 0.0),
        (96, 12, 1.0, 0.0),
        (30, 6, 1.0, 0.0),
        (8, 60, 1.0, 0.0),
        (12, 7, 1.0, 0.0),
        (60, 8, 1.0, 0.05),
    ] {
        cycloidal_row(z1, z2, m, b).check();
    }
}

/// The disc against the pins through a whole input turn, moved as the preview says: its centre
/// goes round at the input's speed while it turns back one lobe.
fn drive_row(pins: u32, pin_circle: f64, pin_diameter: f64, e: f64, backlash: f64) -> Row {
    let s = Spec { pins, pin_circle, pin_diameter, eccentricity: e, backlash, ..Spec::defaults_for(Kind::CycloidalDrive) };
    let p = plan(&s).unwrap();
    let m = pin_diameter / 2.0;
    let (disc, ring) = (&p.preview[0], &p.preview[1]);
    assert_eq!((disc.orbit, disc.speed, ring.speed), (1.0, -1.0 / (f64::from(pins) - 1.0), 0.0));
    let stat = run(&member(disc, m), &member(ring, m), 0.0, TAU, 240);
    Row {
        label: format!("drive {pins} pins d{pin_diameter} on d{pin_circle} e {e} backlash {backlash}"),
        m,
        stat,
        want_gap: backlash / 2.0,
        warnings: p.warnings,
    }
}

#[test]
fn cycloidal_drive_discs_roll_in_the_pins() {
    for (n, pc, pd, e, b) in [
        (12, 60.0, 6.0, 1.5, 0.0),
        (10, 50.0, 5.0, 1.0, 0.0),
        (20, 80.0, 5.0, 1.5, 0.0),
        (30, 100.0, 4.0, 1.0, 0.0),
        (8, 40.0, 6.0, 2.0, 0.0),
        (11, 60.0, 8.0, 2.0, 0.0),
        (12, 60.0, 6.0, 1.5, 0.1),
        (40, 120.0, 4.0, 0.8, 0.0),
    ] {
        let r = drive_row(n, pc, pd, e, b);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        r.check();
    }
}

#[test]
fn cycloidal_drive_refuses_pins_that_undercut_the_lobes() {
    // Pins bigger than the tightest curve of their path on the disc: the offset profile loops.
    let s = Spec { pins: 10, pin_circle: 50.0, pin_diameter: 12.0, eccentricity: 2.2, ..Spec::defaults_for(Kind::CycloidalDrive) };
    let e = plan(&s).unwrap_err();
    assert!(e.contains("too big"), "{e}");
}

#[test]
fn preview_place_is_the_motion_the_checker_proves() {
    // The window draws with `PreviewGear::place`: the same points as the checker's motion.
    let p = plan(&Spec::defaults_for(Kind::CycloidalDrive)).unwrap();
    let g = &p.preview[0];
    let q = V2::new(3.0, 4.0);
    for t in [0.0, 0.7, 2.9, 5.5] {
        let want = g.center.rot(t) + q.rot(g.speed * t);
        assert!(g.place(q, t).dist(want) < 1e-12);
    }
    let p = plan(&Spec::defaults_for(Kind::Pair)).unwrap();
    let g = &p.preview[1];
    assert!(g.place(q, 0.4).dist(g.center + q.rot(g.speed * 0.4)) < 1e-12);
}

#[test]
fn mesh_phase_holds_in_every_direction() {
    // Gear 2 put anywhere round gear 1, gear 1 turned anyhow: they mesh as on the x axis.
    let (g1, g2) = (Involute { teeth: 13, ..Involute::default() }, Involute { teeth: 22, ..Involute::default() });
    let a = g1.r() + g2.r();
    for (dir, phase1) in [(0.0, 0.0), (1.1, 0.0), (2.5, 0.37), (-2.0, -1.3), (4.0, 2.2)] {
        let phase2 = mesh_phase(13, phase1, 22, dir);
        let m1 = Member::new(&gear_region(&g1, phase1), Motion::turning(V2::ZERO, 1.0), 0.4, 4e-5);
        let m2 = Member::new(&gear_region(&g2, phase2), Motion::turning(V2::polar(a, dir), -13.0 / 22.0), 0.4, 4e-5);
        let stat = run(&m1, &m2, 0.0, TAU / 13.0, 12);
        assert!(stat.depth < 2e-3 && stat.gap_max < 3e-3, "dir {dir} phase {phase1}: {stat:?}");
    }
    // And the checker does see teeth that are out of step.
    let m1 = Member::new(&gear_region(&g1, 0.0), Motion::turning(V2::ZERO, 1.0), 0.4, 4e-5);
    let m2 = Member::new(&gear_region(&g2, mesh_phase(13, 0.0, 22, 0.0) + 0.05), Motion::turning(V2::new(a, 0.0), -13.0 / 22.0), 0.4, 4e-5);
    let stat = probe(&m1, &m2, 0.0);
    assert!(stat.depth > 0.05 && stat.area > 0.1, "{stat:?}");
}
