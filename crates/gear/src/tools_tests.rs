//! The window's helpers: the live mesh verdict ([`crate::mesh2d::check_mesh`]), its cost, and
//! what it says about sound and broken sets.

use std::time::Instant;

use crate::geom::V2;
use crate::mesh2d::{Verdict, check_mesh, worst};
use crate::plan::{Kind, Plan, Spec, plan};

fn planetary(zs: u32, zp: u32, zr: u32, n: u32) -> Spec {
    Spec { kind: Kind::Planetary, teeth: zs, teeth2: zp, teeth3: zr, planets: n, ..Spec::default() }
}

/// The fastest of a few runs, in milliseconds (the machine may be busy building something).
fn fastest_ms(p: &Plan) -> f64 {
    (0..7)
        .map(|_| {
            let t = Instant::now();
            std::hint::black_box(check_mesh(std::hint::black_box(p)));
            t.elapsed().as_secs_f64() * 1e3
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn plans_list_their_meshes() {
    let p = plan(&Spec::defaults_for(Kind::Pair)).unwrap();
    assert_eq!(p.meshes.iter().map(|m| (m.a, m.b, m.internal)).collect::<Vec<_>>(), [(0, 1, false)]);
    let p = plan(&planetary(18, 18, 54, 3)).unwrap();
    assert_eq!(
        p.meshes.iter().map(|m| (m.a, m.b, m.internal)).collect::<Vec<_>>(),
        [(0, 1, false), (1, 4, true), (0, 2, false), (2, 4, true), (0, 3, false), (3, 4, true)]
    );
    assert_eq!((p.preview[0].name.as_str(), p.preview[4].name.as_str()), ("sun", "ring"));
    let p = plan(&Spec::defaults_for(Kind::Cycloidal)).unwrap();
    assert_eq!(p.meshes.len(), 1);
    let p = plan(&Spec::defaults_for(Kind::CycloidalDrive)).unwrap();
    assert_eq!(p.meshes.iter().map(|m| (m.a, m.b, m.internal)).collect::<Vec<_>>(), [(0, 1, true)]);
    // A lone cycloidal wheel, single gears and the sets on other axes have none.
    assert!(plan(&Spec { teeth2: 0, ..Spec::defaults_for(Kind::Cycloidal) }).unwrap().meshes.is_empty());
    for kind in [Kind::Spur, Kind::Helical, Kind::Internal, Kind::Rack] {
        let p = plan(&Spec::defaults_for(kind)).unwrap();
        assert!(p.meshes.is_empty() && check_mesh(&p).is_empty(), "{kind:?}");
    }
    // Sets on other axes are checked in 3D (see `mesh3d_tests` for their verdicts).
    for kind in [Kind::Bevel, Kind::SpiralBevel, Kind::CrossedHelical, Kind::Worm] {
        let p = plan(&Spec::defaults_for(kind)).unwrap();
        assert!(p.meshes.is_empty() && p.meshes3d.len() == 1, "{kind:?}");
    }
    // Building only one member still checks the whole mesh.
    let p = plan(&Spec { only: 2, ..Spec::defaults_for(Kind::Pair) }).unwrap();
    assert_eq!(worst(&check_mesh(&p)), Some(Verdict::Ok));
}

#[test]
fn sound_sets_check_ok() {
    let mut specs = vec![
        Spec::defaults_for(Kind::Pair),
        Spec { teeth: 12, teeth2: 40, ..Spec::defaults_for(Kind::Pair) },
        Spec { teeth: 10, teeth2: 20, shift: 0.5, shift2: 0.3, ..Spec::defaults_for(Kind::Pair) },
        Spec { teeth: 20, teeth2: 32, center_distance: 52.8, ..Spec::defaults_for(Kind::Pair) },
        Spec { teeth: 12, teeth2: 25, shift: 0.3, helix: 30.0, ..Spec::defaults_for(Kind::Pair) },
        Spec { backlash: 0.1, ..Spec::defaults_for(Kind::Pair) },
        Spec { backlash: 0.3, module: 3.0, pressure_angle: 25.0, ..Spec::defaults_for(Kind::Pair) },
        Spec { module: 0.3, teeth: 30, teeth2: 31, ..Spec::defaults_for(Kind::Pair) },
        Spec { module: 8.0, teeth: 17, teeth2: 120, ..Spec::defaults_for(Kind::Pair) },
        planetary(18, 18, 54, 3),
        planetary(24, 12, 48, 6),
        planetary(20, 15, 50, 3),
        Spec { shift: 0.3, shift2: 0.2, ..planetary(18, 18, 54, 3) },
        Spec { backlash: 0.1, ..planetary(20, 15, 50, 4) },
        Spec::defaults_for(Kind::Cycloidal),
        Spec { teeth: 30, teeth2: 10, backlash: 0.05, ..Spec::defaults_for(Kind::Cycloidal) },
        Spec::defaults_for(Kind::CycloidalDrive),
        Spec { pins: 30, pin_circle: 100.0, pin_diameter: 4.0, eccentricity: 1.0, ..Spec::defaults_for(Kind::CycloidalDrive) },
        Spec { backlash: 0.1, ..Spec::defaults_for(Kind::CycloidalDrive) },
    ];
    for s in specs.drain(..) {
        let p = plan(&s).unwrap();
        let r = check_mesh(&p);
        assert_eq!(r.len(), p.meshes.len());
        for x in &r {
            eprintln!(
                "{:?} {}/{}/{}: {}  (depth {:.5}, gap {:.5}..{:.5})",
                s.kind, s.teeth, s.teeth2, s.teeth3, x.message, x.penetration_mm, x.gap_min_mm, x.gap_max_mm
            );
            assert_eq!(x.verdict, Verdict::Ok, "{:?} {}/{}: {}", s.kind, s.teeth, s.teeth2, x.message);
            assert!(x.gap_max_mm >= x.gap_min_mm && x.gap_min_mm >= 0.0 && x.penetration_mm >= 0.0);
            // Backlash shows as the gap (a little less along the flanks than on the pitch circle).
            if s.backlash > 0.0 && s.kind != Kind::CycloidalDrive {
                assert!(x.gap_min_mm > 0.6 * s.backlash && x.gap_max_mm < 1.1 * s.backlash, "{}", x.message);
            }
        }
    }
}

#[test]
fn broken_meshes_are_named() {
    // Gear 2 turned out of step: overlap.
    let mut p = plan(&Spec::defaults_for(Kind::Pair)).unwrap();
    let turned = p.preview[1].region.outer[0].rotated(0.04);
    p.preview[1].region.outer[0] = turned;
    let r = check_mesh(&p);
    assert_eq!(r[0].verdict, Verdict::Interferes, "{}", r[0].message);
    assert!(r[0].penetration_mm > 0.05 && r[0].message.contains("gear 1") && r[0].message.contains("overlap"), "{}", r[0].message);
    assert_eq!(worst(&r), Some(Verdict::Interferes));
    // Pulled apart by 0.3 mm: no overlap, no contact.
    let mut p = plan(&Spec::defaults_for(Kind::Pair)).unwrap();
    p.preview[1].center = p.preview[1].center + V2::new(0.3, 0.0);
    let r = check_mesh(&p);
    assert_eq!(r[0].verdict, Verdict::NoContact, "{}", r[0].message);
    assert!(r[0].gap_min_mm > 0.05 && r[0].penetration_mm == 0.0, "{:?}", r[0]);
    // Pushed together by 0.3 mm: the teeth wedge.
    let mut p = plan(&Spec::defaults_for(Kind::Pair)).unwrap();
    p.preview[1].center = p.preview[1].center - V2::new(0.3, 0.0);
    assert_eq!(check_mesh(&p)[0].verdict, Verdict::Interferes);
    // A heavily undercut pinion hands over badly, as its plan warns (the gap that opens is a few
    // thousandths of a module: under what this coarse check calls lost contact), but nothing cuts.
    let p = plan(&Spec { teeth: 6, teeth2: 13, ..Spec::defaults_for(Kind::Pair) }).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("lose contact")));
    assert_ne!(check_mesh(&p)[0].verdict, Verdict::Interferes);
    // One planet of three knocked out of place: its two meshes fail, the others hold.
    let mut p = plan(&planetary(18, 18, 54, 3)).unwrap();
    let turned = p.preview[2].region.outer[0].rotated(0.06);
    p.preview[2].region.outer[0] = turned;
    let r = check_mesh(&p);
    let bad: Vec<(usize, usize)> = r.iter().filter(|x| x.verdict == Verdict::Interferes).map(|x| (x.a, x.b)).collect();
    assert_eq!(bad, [(0, 2), (2, 4)]);
    assert!(r.iter().filter(|x| x.a != 2 && x.b != 2).all(|x| x.verdict == Verdict::Ok));
    // A planet too big for its ring (ring − planet too few teeth): the tips collide.
    let p = plan(&planetary(12, 30, 36, 1));
    if let Ok(p) = p {
        assert!(check_mesh(&p).iter().any(|x| x.verdict == Verdict::Interferes), "{:?}", p.warnings);
    }
    // Indices out of range are skipped, not a panic.
    let mut p = plan(&Spec::defaults_for(Kind::Pair)).unwrap();
    p.meshes[0].b = 7;
    assert!(check_mesh(&p).is_empty());
}

#[test]
#[ignore = "timing: flaky while other tests load the machine"]
fn the_check_is_fast_enough_to_run_on_every_edit() {
    let pair = plan(&Spec::defaults_for(Kind::Pair)).unwrap();
    let six = plan(&planetary(24, 12, 48, 6)).unwrap();
    assert_eq!(six.meshes.len(), 12);
    let (t_pair, t_six) = (fastest_ms(&pair), fastest_ms(&six));
    let drive = fastest_ms(&plan(&Spec::defaults_for(Kind::CycloidalDrive)).unwrap());
    let big = fastest_ms(&plan(&Spec { teeth: 60, teeth2: 200, ..Spec::defaults_for(Kind::Pair) }).unwrap());
    eprintln!("check_mesh: default pair {t_pair:.2} ms, six-planet set {t_six:.2} ms, cycloidal drive {drive:.2} ms, 60/200 pair {big:.2} ms");
    // The budget is for a release build (debug builds are several times slower).
    if !cfg!(debug_assertions) {
        assert!(t_pair < 30.0, "default pair took {t_pair:.1} ms");
        assert!(t_six < 150.0, "six-planet set took {t_six:.1} ms");
    }
}
