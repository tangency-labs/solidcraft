//! tangency-labs/solidcore#1036: two capped degree-1 skins sharing three section curves
//! (A through sections 0..4, B through 2..6), so their side faces coincide between z = 20
//! and z = 40. Upstream (C++ at `c9c5979` and v1.0.1 `3fd5b95`) returns an empty A − B and
//! all of A for A ∩ B, and fails A ∪ B (1001); the port does the same (WR-473).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::boolean::api_brep::{
    boolean_difference, boolean_intersection, boolean_union, boolean_with_options,
};
use solidcore::boolean::merge::MergeOptions;
use solidcore::curve::curve::Curve;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, face_area, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::p1;
use solidcore::topology::Brep;
use solidcore::Kernel;

fn section(i: usize) -> solidcore::Result<Curve> {
    let i = f64::from(u32::try_from(i).unwrap());
    let (a, b, z) = (20.0 * (1.0 - 0.04 * i), 12.0 * (1.0 - 0.03 * i), 10.0 * i);
    let mut pts: Vec<Point3> = (0..12)
        .map(|j| {
            let t = 2.0 * std::f64::consts::PI * f64::from(j) / 12.0;
            Point3::new(a * t.cos(), b * t.sin(), z)
        })
        .collect();
    pts.extend(pts.first().copied());
    BSplineCurve::interpolate_points(&pts, None, 3, None, None, true, ParamMethod::ChordLength)
        .map(Curve::from)
}

fn skin_range(k: &mut Kernel, from: usize, to: usize) -> solidcore::Result<Brep> {
    let cs = (from..to)
        .map(section)
        .collect::<solidcore::Result<Vec<Curve>>>()?;
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    create_skin_primitive(k, &vec![1; cs.len()], &refs, 1, true).map(|s| s.brep)
}

fn volume(k: &Kernel, b: &mut Brep) -> solidcore::Result<f64> {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs)
}

fn shape(k: &Kernel, r: solidcore::Result<Brep>) -> String {
    match r {
        Ok(mut b) => format!(
            "F/E/V {}/{}/{} V {:?} integrity {:?}",
            p1::faces(&b, None).unwrap().len(),
            p1::edges(&b, None).unwrap().len(),
            p1::vertices(&b, None).unwrap().len(),
            volume(k, &mut b),
            solidcore::topology::integrity::check(&b)
        ),
        Err(e) => format!("{e:?}"),
    }
}

/// Each face: its surface kind, area, loop count and the z values of its vertices.
fn faces(k: &Kernel, b: &mut Brep) -> Vec<String> {
    let mut out = Vec::new();
    for f in p1::faces(b, None).unwrap() {
        let kind: String = format!(
            "{:?}",
            b.face(f)
                .unwrap()
                .surface
                .as_ref()
                .map(solidcore::surface::surface::Surface::kind)
        )
        .chars()
        .take_while(|c| *c != '(')
        .collect();
        let area = face_area(k, b, f, 1e-4);
        let loops = solidcore::topology::face::p2::loops(b, f, false, None).map(|l| l.len());
        let mut zs: Vec<String> = solidcore::topology::face::p4::edges(b, f, None, false, None)
            .unwrap()
            .into_iter()
            .flat_map(|e| solidcore::topology::edge::vertices(b, e, None).unwrap())
            .flatten()
            .map(|v| format!("{:.3}", b.vertex(v).unwrap().point.z))
            .collect();
        zs.sort();
        zs.dedup();
        out.push(format!(
            "{f:?} {kind} area {area:?} loops {loops:?} vertex z {zs:?}"
        ));
    }
    out
}

/// Probe: the three Booleans and the operands, at the default (upstream) behaviour.
#[test]
#[ignore = "diagnostic probe: panics with its report on purpose; #1036 (WR-473) reproduces at c9c5979 and v1.0.1: A - B empty, A n B = A, A u B 1001"]
fn issue_1036_probe() {
    let mut k = Kernel::default();
    let mut report = Vec::new();
    let a = skin_range(&mut k, 0, 5);
    report.push(format!("A: {}", shape(&k, a)));
    let b = skin_range(&mut k, 2, 7);
    report.push(format!("B: {}", shape(&k, b)));
    for (name, op) in [
        ("A - B (want 14051.84)", 0),
        ("A n B (want 12084.49)", 1),
        ("A u B (want 36398.22)", 2),
    ] {
        let (a, b) = (
            skin_range(&mut k, 0, 5).unwrap(),
            skin_range(&mut k, 2, 7).unwrap(),
        );
        let r = match op {
            0 => boolean_difference(&mut k, a, b),
            1 => boolean_intersection(&mut k, a, b),
            _ => boolean_union(&mut k, a, b),
        };
        report.push(format!("{name}: {}", shape(&k, r)));
    }
    // A imprinted with A n B and nothing else (`m_bImprinting`), and the operands' faces
    for (name, op, imprint) in [
        (
            "A imprinted (difference)",
            BooleanOperation::Difference,
            true,
        ),
        (
            "A - B without post-process",
            BooleanOperation::Difference,
            false,
        ),
        (
            "A n B without post-process",
            BooleanOperation::Intersection,
            false,
        ),
    ] {
        let (a, b) = (
            skin_range(&mut k, 0, 5).unwrap(),
            skin_range(&mut k, 2, 7).unwrap(),
        );
        let options = MergeOptions::new(false, imprint, false, false, false);
        match boolean_with_options(&mut k, a, b, op, options) {
            Ok(mut out) => {
                report.push(format!(
                    "{name} integrity: {:?}",
                    solidcore::topology::integrity::check(&out.result)
                ));
                let fs = faces(&k, &mut out.result);
                report.push(format!("{name}: {} faces {fs:#?}", fs.len()));
            }
            Err(e) => report.push(format!("{name}: {e:?}")),
        }
    }
    for (name, from, to) in [("A", 0, 5), ("B", 2, 7)] {
        let mut x = skin_range(&mut k, from, to).unwrap();
        report.push(format!("{name} faces {:#?}", faces(&k, &mut x)));
    }
    // The same with each opt-in that touches this path: WR-305 (an interval along a face
    // edge classifies to the edge) for the slivers on A's top cap, and WR-304 (the
    // clean-up re-checks edges a face merge leaves inside one face). Neither changes
    // the results (run of 2026-10-27).
    for fix in ["classify_edge_interval", "boolean_cleanup_removes_slits"] {
        let mut k = Kernel::default();
        match fix {
            "classify_edge_interval" => k.fixes.classify_edge_interval = true,
            _ => k.fixes.boolean_cleanup_removes_slits = true,
        }
        for (name, op) in [("A - B", 0), ("A n B", 1), ("A u B", 2)] {
            let (a, b) = (
                skin_range(&mut k, 0, 5).unwrap(),
                skin_range(&mut k, 2, 7).unwrap(),
            );
            let r = match op {
                0 => boolean_difference(&mut k, a, b),
                1 => boolean_intersection(&mut k, a, b),
                _ => boolean_union(&mut k, a, b),
            };
            report.push(format!("[{fix}] {name}: {}", shape(&k, r)));
        }
        let (a, b) = (
            skin_range(&mut k, 0, 5).unwrap(),
            skin_range(&mut k, 2, 7).unwrap(),
        );
        let options = MergeOptions::new(false, true, false, false, false);
        match boolean_with_options(&mut k, a, b, BooleanOperation::Difference, options) {
            Ok(mut out) => {
                let fs = faces(&k, &mut out.result);
                report.push(format!("[{fix}] A imprinted: {} faces {fs:#?}", fs.len()));
            }
            Err(e) => report.push(format!("[{fix}] A imprinted: {e:?}")),
        }
    }
    panic!("{report:#?}");
}
