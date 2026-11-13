//! #3093: `Defeature/extrusion.smb`'s add-by-faces extrusion (the `blend.feature_executive`
//! case `add_by_faces_extrusion`) built a closed solid or an open body depending on a few
//! ulps of an extended surface's domain.
//!
//! The extrusion face's base patch extends its neighbours Tangent by twice the hole's box,
//! 20. The extruded neighbour runs 0 to 10 along its one-degree direction, so the extension
//! mirrors the piece twice. The parameter asked, 20 to the last ulp past 20, left the second
//! mirror's reflection point 4e-15 below the knot at 10: that knot was mirrored onto the new
//! end as a sliver span the knot fixup merged, the end knot four times over at degree 1.
//! The surface cache could not decompose it, the merge of the extended faces stopped, the
//! patch did not fill the hole, and the base came back open (with success, until WR-656's
//! refusal became the default). The mirror now reflects at the knot itself.
//!
//! The model is the upstream test file, read by the Rust reader when the pinned upstream
//! checkout is present.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::feature_executive::{FeatureExecutive, RebuildBrepType};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::mass::enclosure::{enclose_volume, VolumeSign};
use solidcore::nurbs::nurbs_srf::decompose_srf;
use solidcore::surface::extend::Continuity;
use solidcore::surface::side::BoxSide;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

/// `Defeature/extrusion.smb`, read and arena-named as the goldens reader does.
fn extrusion(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Defeature/extrusion.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        eprintln!("unavailable upstream test file: {}", path.display());
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

/// Every extruded face extended Tangent on all sides by `d`: a knot vector a B-spline can
/// have (end knots at most `p + 1` times, inner ones at most `p`), which decomposes.
#[test]
fn an_extension_by_whole_mirrors_has_no_sliver_span() {
    let mut k = Kernel::default();
    let Some(b) = extrusion(&mut k) else { return };
    let mut extruded = 0;
    for f in bp1::faces(&b, None).unwrap() {
        let s = b.face(f).unwrap().surface.clone().unwrap();
        if !matches!(s.kind(), SurfaceKind::SurfOfExtrusion(_)) {
            continue;
        }
        extruded += 1;
        for d in [20.0, 30.0, 40.0] {
            let e = s
                .extended_sides(&k, &BoxSide::ALL, Continuity::Tangent, d)
                .unwrap()
                .surface;
            let SurfaceKind::BSpline(bs) = e.kind() else {
                panic!("d {d}: not a B-spline")
            };
            let net = bs.gw_nurb().unwrap();
            let (p, q) = net.degrees();
            for (knots, deg) in [(net.u_knots(), p), (net.v_knots(), q)] {
                let mut i = 0;
                while i < knots.len() {
                    let j = knots[i..].iter().take_while(|&&x| x == knots[i]).count();
                    let end = i == 0 || i + j == knots.len();
                    let most = if end { deg + 1 } else { deg };
                    assert!(
                        j <= most,
                        "d {d}: knot {} {j} times, degree {deg}",
                        knots[i]
                    );
                    i += j;
                }
            }
            decompose_srf(net).unwrap_or_else(|e| panic!("d {d}: {e:?}"));
        }
    }
    assert_eq!(extruded, 2);
}

/// The three features off: one closed solid of 13 faces, Level 2 clean, the same at the
/// default reach (twice the hole's box, 20) and on either side of it. (From 30 on the
/// patch is refused for another reason, not this one.)
#[test]
fn the_extrusion_defeatures_to_the_same_solid_at_any_reach() {
    let base = |dist: Option<f64>| {
        let mut k = Kernel::default();
        let mut b = extrusion(&mut k)?;
        let faces = bp1::faces(&b, None).unwrap();
        {
            let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
            for set in [&[10][..], &[6, 7, 8, 9], &[4]] {
                let fs: Vec<_> = set.iter().map(|&i| faces[i]).collect();
                ex.add_feature(
                    &mut k,
                    &fs,
                    RebuildBrepType::ExtendSurface,
                    RebuildBrepType::FromOtherBrep,
                )
                .unwrap()
                .unwrap();
            }
            if let Some(d) = dist {
                ex.set_base_ext_dist(d);
            }
            ex.do_defeature(&mut k)
                .unwrap_or_else(|e| panic!("reach {dist:?}: {e:?}"));
        }
        let s = validation::soundness(&mut k, &mut b).unwrap();
        assert!(s.manifold, "reach {dist:?}");
        assert!(s.check.lines().is_empty(), "reach {dist:?}: {:?}", s.check);
        assert_eq!(bp1::faces(&b, None).unwrap().len(), 13, "reach {dist:?}");
        positive(s.volume);
        Some(narrow(&k, &mut b))
    };
    let Some(v) = base(None) else { return };
    // the same solid at every reach: every volume in one enclosure is within `REL` of
    // every volume in the other
    for d in [10.0, 15.0, 19.999_999_999, 20.0, 25.0] {
        let w = base(Some(d)).unwrap();
        let apart = (w.1 - v.0).max(v.1 - w.0);
        assert!(apart <= REL * v.0, "reach {d}: {w:?} against {v:?}");
    }
}

/// Upstream's `my_test_multi_rebuilds` (case 2): the three features by their loop edges, each
/// rebuilt with planar caps. One feature's cap cannot be built. Upstream leaves its hole open
/// for the caller to put back with `RestoreFeatures`; here `do_defeature` puts it back itself
/// and names it, and the other two come off: the closed body upstream's restore makes.
#[test]
fn a_feature_whose_cap_cannot_be_built_is_kept_and_named() {
    let mut k = Kernel::default();
    let Some(mut b) = extrusion(&mut k) else {
        return;
    };
    let faces = bp1::faces(&b, None).unwrap();
    let edges = bp1::edges(&b, None).unwrap();
    let kept = {
        let mut ex = FeatureExecutive::new(&mut k, &mut b).unwrap();
        for (face, loop_edges) in [
            (10, &[8, 25][..]),
            (9, &[4, 5, 6, 7, 17, 19, 21, 23]),
            (4, &[2, 12, 13, 14]),
        ] {
            let es: Vec<_> = loop_edges.iter().map(|&i| edges[i]).collect();
            ex.add_feature_with_edges(
                &mut k,
                faces[face],
                &es,
                RebuildBrepType::PlanarCap,
                RebuildBrepType::PlanarCap,
            )
            .unwrap()
            .unwrap();
        }
        ex.do_defeature(&mut k).unwrap()
    };
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].open_loop.is_some());
    let s = validation::soundness(&mut k, &mut b).unwrap();
    assert!(s.manifold);
    assert!(s.check.lines().is_empty(), "{:?}", s.check);
    positive(s.volume);
    // every volume in the enclosure is within `REL` of the body's
    let (lo, hi) = narrow(&k, &mut b);
    let apart = (hi - VOLUME).max(VOLUME - lo);
    assert!(apart <= REL * lo, "[{lo}, {hi}]");
}

/// The volume of the body `a_feature_whose_cap_cannot_be_built_is_kept_and_named` makes: the
/// middle of its certified enclosure `[757.499_497_550_496_5, 757.499_497_564_175]` (width
/// 1.4e-8). The case first pinned `757.499_499_825_162`, the volume estimator's answer at
/// accuracy 1e-6 (`brep_volume_default`, still its answer on main); the enclosure proves that
/// 3.0e-9 of the volume too large, past the case's own relative bound [`REL`] (#15038).
const VOLUME: f64 = 757.499_497_557;

/// How far apart, relative to the volume, two volumes of the same solid may be: the bound
/// the case compared its volumes with before the volume became an enclosure (#15038).
const REL: f64 = 1e-9;

/// The body's volume enclosed to a quarter of [`REL`] of itself, as `(lo, hi)`: narrow
/// enough that two enclosures of one solid are within `REL` of each other end to end, so the
/// comparison is the relative bound on numbers, proved, and not an overlap of enclosures of
/// any width.
fn narrow(k: &Kernel, b: &mut Brep) -> (f64, f64) {
    let rough = enclose_volume(k, b, 1.0).unwrap();
    assert!(rough.lo > 0.0, "{rough:?}");
    let e = enclose_volume(k, b, 0.25 * REL * rough.lo).unwrap();
    assert!(
        e.width() <= 0.25 * REL * rough.lo,
        "not enclosed narrowly enough: {e:?}"
    );
    (e.lo, e.hi)
}

/// A volume proved positive, as its enclosure's ends.
fn positive(v: Option<VolumeSign>) -> (f64, f64) {
    match v {
        Some(VolumeSign::Positive(e)) => (e.lo, e.hi),
        other => panic!("a volume not proved positive: {other:?}"),
    }
}

/// The body's counts and volume, to compare a base with its state before a call.
fn state(k: &mut Kernel, b: &mut Brep) -> ([usize; 3], (f64, f64)) {
    let counts = [
        bp1::faces(b, None).unwrap().len(),
        bp1::edges(b, None).unwrap().len(),
        bp1::vertices(b, None).unwrap().len(),
    ];
    let v = positive(validation::soundness(k, b).unwrap().volume);
    (counts, v)
}

/// The executive with the three features of `my_test_multi_rebuilds` (case 2) added.
fn three_features<'b>(k: &mut Kernel, b: &'b mut Brep) -> FeatureExecutive<'b> {
    let faces = bp1::faces(b, None).unwrap();
    let edges = bp1::edges(b, None).unwrap();
    let mut ex = FeatureExecutive::new(k, b).unwrap();
    for (face, loop_edges) in [
        (10, &[8, 25][..]),
        (9, &[4, 5, 6, 7, 17, 19, 21, 23]),
        (4, &[2, 12, 13, 14]),
    ] {
        let es: Vec<_> = loop_edges.iter().map(|&i| edges[i]).collect();
        ex.add_feature_with_edges(
            k,
            faces[face],
            &es,
            RebuildBrepType::PlanarCap,
            RebuildBrepType::PlanarCap,
        )
        .unwrap()
        .unwrap();
    }
    ex
}

/// (#3179 review, `CodeRabbit`) A feature that cannot be partitioned (`defeature_ready` false)
/// is not counted as taken off, though `build_patches` marks it rebuilt before refusing it:
/// it is kept, it has nothing to put back, so the call is refused and the base is as it was.
#[test]
fn a_feature_that_cannot_be_partitioned_refuses_the_call_and_leaves_the_base() {
    let mut k = Kernel::default();
    let Some(mut b) = extrusion(&mut k) else {
        return;
    };
    let before = state(&mut k, &mut b);
    let r = {
        let mut ex = three_features(&mut k, &mut b);
        ex.features_mut()[1].defeature_ready = false;
        ex.do_defeature(&mut k)
    };
    // a status answer: an `Upstream`, or the typed blend failure that replaced it (#13790)
    assert!(
        r.as_ref()
            .err()
            .and_then(solidcore::error::upstream_status)
            .is_some(),
        "{r:?}"
    );
    assert_eq!(state(&mut k, &mut b), before);
}

/// (#3179 review) A cancelled call puts the base back as it was, as a refusal does.
#[test]
fn a_cancelled_defeature_leaves_the_base() {
    let mut k = Kernel::default();
    let Some(mut b) = extrusion(&mut k) else {
        return;
    };
    let before = state(&mut k, &mut b);
    let token = solidcore::kernel::CancelToken::new();
    let r = {
        let mut ex = three_features(&mut k, &mut b);
        token.cancel();
        k.set_cancel_token(token.clone());
        ex.do_defeature(&mut k)
    };
    k.cancel = None;
    assert!(
        matches!(r, Err(solidcore::SolidError::Cancelled { .. })),
        "{r:?}"
    );
    assert_eq!(state(&mut k, &mut b), before);
}
