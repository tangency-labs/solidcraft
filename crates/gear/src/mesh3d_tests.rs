//! The 3D sets mesh, proved with the checker in `mesh3d`: through two tooth pitches no surface
//! point of one member is inside the other and the flanks stay in contact (or a backlash
//! apart); half a pitch out of phase they collide. Run with `--nocapture` for every case's
//! numbers.

use crate::mesh3d::{Pair, Run, Stat, expected_gap as want_gap, motion};
use crate::plan::{Kind, Spec, plan};

/// Driver angles per run (two tooth pitches).
const STEPS: usize = 16;

pub struct Row {
    pub label: String,
    pub m: f64,
    pub backlash: f64,
    /// The gap the backlash asks for ([`want_gap`]).
    pub want_lo: f64,
    pub want_hi: f64,
    pub run: Run,
    /// Member 2 turned half a pitch out of phase.
    pub off: Stat,
    pub warnings: Vec<String>,
}

pub fn measure(label: &str, s: &Spec) -> Row {
    let p = plan(s).unwrap_or_else(|e| panic!("{label}: {e}"));
    assert_eq!(p.bodies.len(), 2, "{label}");
    let m = s.module;
    let (ratio, half, pitch) = motion(s);
    let mut pair = Pair::for_run(&p.bodies[0], &p.bodies[1], 0.03 * m, 0.1 * m, ratio, -pitch, 2.0 * pitch);
    let run = pair.run(ratio, 0.0, 0.0, 2.0 * pitch, STEPS);
    let off = pair.at(0.0, half);
    if std::env::var("MESH3D_WHERE").is_ok() {
        for (what, place, at) in [("deepest", run.place, run.at), ("nearest", run.gap_place, run.gap_at)] {
            pair.a.spin = at;
            pair.b.spin = ratio * at;
            let (la, lb) = (pair.a.local(place), pair.b.local(place));
            for e in [0.005, 0.02, 0.05] {
                let around: Vec<[f64; 3]> = (0..6)
                    .map(|i| {
                        let mut q = place;
                        q[i / 2] += if i % 2 == 0 { e } else { -e };
                        q
                    })
                    .collect();
                eprintln!(
                    "    +-{e}: in body 1 {:?} in body 2 {:?}",
                    around.iter().map(|q| pair.a.contains(*q) as u8).collect::<Vec<_>>(),
                    around.iter().map(|q| pair.b.contains(*q) as u8).collect::<Vec<_>>()
                );
            }
            eprintln!(
                "  {label}: {what} at driver {at:.4}: body 1 r {:.3} z {:.3}, body 2 r {:.3} z {:.3} (z ranges {:?} {:?})",
                la[0].hypot(la[1]),
                la[2],
                lb[0].hypot(lb[1]),
                lb[2],
                pair.a.z,
                pair.b.z
            );
        }
    }
    let (want_lo, want_hi) = want_gap(s);
    Row { label: label.to_string(), m, backlash: s.backlash, want_lo, want_hi, run, off, warnings: p.warnings }
}

/// A flank is a spline within `PROFILE_TOLERANCE` of its curve and the checker's surfaces are
/// facets of about a thirtieth of a module, so two flanks in contact may read as crossing by
/// this much (× m).
const DEPTH: f64 = 2.0e-3;
/// How far beyond the backlash the flanks may be from touching at any step (× m).
const GAP: f64 = 2.5e-3;

impl Row {
    pub fn show(&self) {
        let r = &self.run;
        eprintln!(
            "{:<44} pen {:>8.5} m  gap {:>8.5}..{:>8.5} m (backlash {:.3} mm, wants {:.4}..{:.4} mm)  vol {:>7.4} mm3 | half pitch off: pen {:>6.3} mm vol {:>8.2} mm3{}",
            self.label,
            r.penetration / self.m,
            r.gap_min / self.m,
            r.gap_max / self.m,
            self.backlash,
            self.want_lo,
            self.want_hi,
            r.volume,
            self.off.penetration,
            self.off.volume,
            if self.warnings.is_empty() { String::new() } else { format!("  ! {}", self.warnings.join(" | ")) }
        );
    }
    /// No penetration at any step, contact (a backlash apart at most) at every step, and a
    /// collision half a pitch out of phase.
    pub fn check(&self) {
        self.show();
        let (r, m) = (&self.run, self.m);
        assert!(r.penetration <= DEPTH * m, "{}: flanks cross by {} mm at driver angle {}", self.label, r.penetration, r.at);
        assert!(r.gap_max <= self.want_hi + GAP * m, "{}: flanks {} mm apart (the backlash asks for {})", self.label, r.gap_max, self.want_hi);
        assert!(r.gap_min >= self.want_lo - GAP * m, "{}: flanks only {} mm apart (the backlash asks for {})", self.label, r.gap_min, self.want_lo);
        assert!(
            self.off.penetration > 0.1 * m && self.off.volume > 0.02 * m * m * m,
            "{}: half a pitch out of phase should collide: {:?}",
            self.label,
            self.off
        );
    }
}

fn d(k: Kind) -> Spec {
    Spec::defaults_for(k)
}

fn bevels() -> Vec<(&'static str, Spec)> {
    vec![
        ("bevel default 20:30", d(Kind::Bevel)),
        ("bevel mitre 24:24", Spec { teeth: 24, teeth2: 24, ..d(Kind::Bevel) }),
        ("bevel 60deg 16:40", Spec { teeth: 16, teeth2: 40, shaft_angle: 60.0, ..d(Kind::Bevel) }),
        ("bevel 120deg 18:27 backlash", Spec { teeth: 18, teeth2: 27, shaft_angle: 120.0, backlash: 0.1, ..d(Kind::Bevel) }),
        ("bevel 13:31 m3 backlash", Spec { teeth: 13, teeth2: 31, module: 3.0, backlash: 0.1, face_width: 15.0, ..d(Kind::Bevel) }),
    ]
}

fn spirals() -> Vec<(&'static str, Spec)> {
    vec![
        ("spiral default 17:34 35deg", d(Kind::SpiralBevel)),
        ("spiral left 20deg 15:31", Spec { spiral_angle: -20.0, teeth: 15, teeth2: 31, ..d(Kind::SpiralBevel) }),
        ("spiral 60deg shaft 19:28", Spec { shaft_angle: 60.0, teeth: 19, teeth2: 28, ..d(Kind::SpiralBevel) }),
        (
            "spiral 120deg shaft 20:29 backlash",
            Spec { shaft_angle: 120.0, backlash: 0.1, spiral_angle: 30.0, teeth: 20, teeth2: 29, ..d(Kind::SpiralBevel) },
        ),
        ("spiral mitre 45deg 21:21 backlash", Spec { teeth: 21, teeth2: 21, spiral_angle: 45.0, backlash: 0.1, ..d(Kind::SpiralBevel) }),
    ]
}

fn crossed() -> Vec<(&'static str, Spec)> {
    vec![
        ("crossed default 15:25 45/45", d(Kind::CrossedHelical)),
        ("crossed 30/60 18:23", Spec { helix: 30.0, teeth: 18, teeth2: 23, ..d(Kind::CrossedHelical) }),
        ("crossed 60deg shaft 17:29", Spec { shaft_angle: 60.0, helix: 20.0, teeth: 17, teeth2: 29, ..d(Kind::CrossedHelical) }),
        (
            "crossed 120deg shaft 16:24 backlash",
            Spec { shaft_angle: 120.0, helix: 50.0, backlash: 0.1, teeth: 16, teeth2: 24, ..d(Kind::CrossedHelical) },
        ),
        (
            "crossed opposite hands 20:20 backlash",
            Spec { shaft_angle: 30.0, helix: 50.0, teeth: 20, teeth2: 20, backlash: 0.1, ..d(Kind::CrossedHelical) },
        ),
    ]
}

fn worms() -> Vec<(&'static str, Spec)> {
    vec![
        ("worm default 1:30", d(Kind::Worm)),
        ("worm 2-start z41", Spec { starts: 2, teeth: 41, ..d(Kind::Worm) }),
        ("worm 4-start q8 left", Spec { starts: 4, teeth: 36, worm_diameter: 16.0, helix: -1.0, ..d(Kind::Worm) }),
        ("worm left m3 z25 backlash", Spec { helix: -1.0, backlash: 0.1, teeth: 25, module: 3.0, ..d(Kind::Worm) }),
        ("worm 3-start 25deg shift backlash", Spec { starts: 3, backlash: 0.1, shift: 0.3, pressure_angle: 25.0, ..d(Kind::Worm) }),
    ]
}

fn all(sets: Vec<(&'static str, Spec)>) {
    let rows: Vec<Row> = sets.iter().map(|(n, s)| measure(n, s)).collect();
    for r in &rows {
        r.show();
    }
    for r in &rows {
        r.check();
    }
}

#[test]
fn straight_bevels_mesh() {
    all(bevels());
}

#[test]
fn spiral_bevels_mesh() {
    all(spirals());
}

#[test]
fn crossed_helicals_mesh() {
    all(crossed());
}

#[test]
fn worms_mesh() {
    all(worms());
}

#[test]
#[ignore]
fn probe() {
    for (label, s) in [
        ("x 120 j0", Spec { shaft_angle: 120.0, helix: 50.0, teeth: 16, teeth2: 24, ..d(Kind::CrossedHelical) }),
        ("x 120 j.1", Spec { shaft_angle: 120.0, helix: 50.0, backlash: 0.1, teeth: 16, teeth2: 24, ..d(Kind::CrossedHelical) }),
        ("x 120 60/60 j.1", Spec { shaft_angle: 120.0, helix: 60.0, backlash: 0.1, teeth: 16, teeth2: 24, ..d(Kind::CrossedHelical) }),
        ("x 90 45 j.1", Spec { backlash: 0.1, ..d(Kind::CrossedHelical) }),
        ("x 90 30/60 j.1", Spec { helix: 30.0, backlash: 0.1, teeth: 18, teeth2: 23, ..d(Kind::CrossedHelical) }),
        (
            "x 90 30/60 j.1 stub",
            Spec {
                helix: 30.0,
                backlash: 0.1,
                teeth: 18,
                teeth2: 23,
                rack: crate::involute::BasicRack { addendum: 0.8, dedendum: 1.0, root_radius: 0.3 },
                ..d(Kind::CrossedHelical)
            },
        ),
    ] {
        measure(label, &s).show();
    }
}

/// The window's verdict (`check_mesh`) on the 3D sets: quick, Ok for sound sets (also when one
/// member is built), and interference for a member turned half a pitch.
#[test]
fn window_check_covers_3d_sets() {
    use crate::mesh2d::{Verdict, check_mesh};
    for (label, s) in [
        ("bevel", d(Kind::Bevel)),
        ("spiral bevel", d(Kind::SpiralBevel)),
        ("crossed helical", d(Kind::CrossedHelical)),
        ("hypoid", d(Kind::Hypoid)),
        ("worm", d(Kind::Worm)),
        ("worm backlash", Spec { backlash: 0.1, ..d(Kind::Worm) }),
        ("spiral bevel backlash, gear 2 only", Spec { backlash: 0.1, only: 2, ..d(Kind::SpiralBevel) }),
    ] {
        let mut p = plan(&s).unwrap();
        let t = std::time::Instant::now();
        let r = check_mesh(&p);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        eprintln!("{label:<36} {ms:6.1} ms  {}", r.iter().map(|x| x.message.clone()).collect::<Vec<_>>().join(" | "));
        assert_eq!(r.len(), 1, "{label}");
        assert_eq!(r[0].verdict, Verdict::Ok, "{label}: {}", r[0].message);
        // (The hypoid's bodies are the largest: a 41-tooth ring gear lofted on a flat cone and
        // a pinion through 41 sections.)
        let most = if label == "hypoid" { 1600.0 } else { 1000.0 };
        crate::test_util::assert_quick(label, ms, most);
        // Member 2 turned half its pitch about its own axis.
        let (_, half, _) = crate::mesh3d::motion(&s);
        let f = match &mut p.meshes3d[0].bodies {
            Some(b) => &mut b[1].frame,
            None => &mut p.bodies[1].frame,
        };
        let y = f.y();
        f.x = [0, 1, 2].map(|i| f.x[i] * half.cos() + y[i] * half.sin());
        let r = check_mesh(&p);
        assert_eq!(r[0].verdict, Verdict::Interferes, "{label} half a pitch out: {}", r[0].message);
    }
}

/// A big worm wheel's sections (its flanks nearly straight) still loft within the tolerance in
/// a few sections, and the plan stays quick.
#[test]
fn big_worm_wheels_loft_in_few_sections() {
    for z in [30, 120, 400] {
        let p = plan(&Spec { teeth: z, ..d(Kind::Worm) }).unwrap();
        let n = p.facts.iter().find(|f| f.label == "wheel loft sections").map_or(0.0, |f| f.value);
        assert!(n <= 11.0, "z{z}: {n} sections");
    }
}

/// Every kind at its defaults reads as meshing in the window's check (a red "Interferes" on a
/// default set is a false alarm or a real fault, either way a bug).
#[test]
fn window_check_passes_every_default_set() {
    use crate::mesh2d::{Verdict, check_mesh};
    let mut bad = Vec::new();
    for k in Kind::ALL.iter().copied() {
        let Ok(p) = plan(&d(k)) else { continue };
        for r in check_mesh(&p) {
            eprintln!("{k:?}: {:?} {}", r.verdict, r.message);
            if r.verdict != Verdict::Ok {
                bad.push(format!("{k:?}: {}", r.message));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The face gear's window verdict (its pinion against the face gear's teeth unrolled as a rack
/// at mid length, as the preview draws them): sound sets mesh, and the rack slid half a pitch
/// interferes. (The check once met the rack without the preview's half-pitch start and read
/// every face gear as interfering by 0.5 mm.)
#[test]
fn face_gear_window_check_meets_the_rack_as_drawn() {
    use crate::mesh2d::{Verdict, check_mesh};
    for (label, s) in [
        ("face gear default", d(Kind::FaceGear)),
        ("face gear 20:60 m1.5", Spec { teeth: 20, teeth2: 60, module: 1.5, ..d(Kind::FaceGear) }),
        ("face gear backlash", Spec { backlash: 0.1, ..d(Kind::FaceGear) }),
    ] {
        let mut p = plan(&s).unwrap_or_else(|e| panic!("{label}: {e}"));
        let r = check_mesh(&p);
        assert_eq!(r.len(), 1, "{label}");
        assert_eq!(r[0].verdict, Verdict::Ok, "{label}: {}", r[0].message);
        // The rack half a period further on: a tooth where the pinion has one.
        let g = &mut p.preview[1];
        let half = 0.5 * g.period;
        g.center.x += half;
        let r = check_mesh(&p);
        assert_eq!(r[0].verdict, Verdict::Interferes, "{label} half a pitch out: {}", r[0].message);
    }
}

/// Each row's farthest point found without making every row is the one of all the material
/// points: bodies with prisms, tapers, a ring's taper and wall, trims and revolves.
#[test]
fn the_farthest_point_per_row_is_that_of_every_material_point() {
    use crate::mesh3d::Solid;
    let ring = Spec {
        bevel_planet_mode: crate::plan::BevelPlanetMode::Ring,
        shaft_angle: 45.0,
        teeth: 20,
        teeth2: 10,
        planets: 3,
        ..Spec::defaults_for(Kind::BevelPlanetary)
    };
    for s in [d(Kind::Pair), d(Kind::Helical), d(Kind::Bevel), d(Kind::Planetary), ring] {
        let p = plan(&s).unwrap();
        for b in &p.bodies {
            let solid = Solid::new(b, 0.3);
            let mut all: std::collections::BTreeMap<u64, f64> = std::collections::BTreeMap::new();
            for q in solid.material_points(0.2) {
                let r = all.entry(q[2].to_bits()).or_insert(0.0);
                *r = r.max(q[0].hypot(q[1]));
            }
            let rows = solid.farthest_per_row(0.2);
            assert_eq!(rows.len(), all.len(), "{}: one point per z", b.name);
            for q in rows {
                let want = all[&q[2].to_bits()];
                assert!((q[0].hypot(q[1]) - want).abs() <= 1e-12 * want.max(1.0), "{} at z {}: {} against {want}", b.name, q[2], q[0].hypot(q[1]));
            }
        }
    }
}
