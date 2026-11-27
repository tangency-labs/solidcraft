//! Regression test (tangency-labs/solidcraft#15282 review): an offset whose mated edge is
//! off one of its extended surfaces is a closed solid once `MateEdges` installs the
//! intersection in its place.
//!
//! `Fillet/tilt_cyl_on_box.smb` (a box with a tilted cylinder, filleted) offset on its
//! face 1 by 11.0492, in and out, convex edges extended, as the goldens case
//! `pe_Fillet_tilt_cyl_on_box_f1_d0_{in,out}_ext` does. Upstream's shortcut kept the old
//! offset edge because its ends match the intersection's, though it lies at least 0.3 off
//! one of the two surfaces in between (certified:
//! `offset::curve_on_surfaces::tests::tilt_cyl_old_offset_edge_is_off_its_plane_and_the_intersection_is_on_both`);
//! its result (1 region, 1 shell, 8 faces, 9 loops, 16 edges, 10 vertices) is no solid:
//! `V - E + F - (L - F) = 1`, which no closed surface gives. `MateEdges` now keeps an
//! offset edge only where it is proved on both surfaces, and installs the intersection
//! (proved on both) otherwise. That intersection is now the exact ellipse of the plane and
//! the tilted cylinder (WR-495, #21301), proved in closed form below.
//!
//! The result is one closed manifold solid: two regions (the infinite one and the solid's)
//! bounded by the two faceuse shells of one surface (8 faces, 9 loops, 15 edges, 10
//! vertices: Euler characteristic 2, genus 0), Level 2 check-clean.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::blend::fillet_section::station::Nurbs;
use solidcore::curve::curve::CurveKind;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::offset::curve_on_surfaces::curve_on_surfaces;
use solidcore::offset::offset_executive::p1::{OffsetExecutive, OffsetOperation};
use solidcore::offset::offset_executive::p2::SolidOffsetOptions;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2 as face_p2;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, vertex};
use solidcore::{Brep, EntityRef, Kernel};

fn tilt_cyl_on_box(k: &mut Kernel) -> Option<Brep> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Fillet/tilt_cyl_on_box.smb");
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
    // the goldens reader's identities (`own_reader::read_key_of`): typed, one per object
    let key = |e: EntityRef| {
        use EntityRef as E;
        let (type_code, kind, i) = match e {
            E::Brep(x) => (16011, 0u64, x.index()),
            E::Region(x) => (16012, 1, x.index()),
            E::Shell(x) => (16013, 2, x.index()),
            E::Faceuse(x) => (16022, 3, x.index()),
            E::Face(x) => (16002, 4, x.index()),
            E::Loopuse(x) => (16004, 5, x.index()),
            E::Loop(x) => (16003, 6, x.index()),
            E::Edgeuse(x) => (16005, 7, x.index()),
            E::Edge(x) => (16016, 8, x.index()),
            E::Vertexuse(x) => (16006, 9, x.index()),
            E::Vertex(x) => (16021, 10, x.index()),
        };
        ObjectRef {
            type_code,
            key: ObjectKey(((kind + 1) << 40) | u64::from(i)),
        }
    };
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 40,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

/// `DoSolidOffset` of the part on its face 1 by 11.0492 (`inset` in or out), convex edges
/// extended, results not merged, as the goldens case; `None` without the pinned upstream
/// checkout.
fn offset(inset: bool) -> Option<(Kernel, Brep)> {
    let mut k = Kernel::default();
    let mut part = tilt_cyl_on_box(&mut k)?;
    let face = part.face_list()[1];
    let mut exec =
        OffsetExecutive::new(OffsetOperation::SolidOffset, 11.0492, 0.001_339_890_721_639);
    exec.set_inset(inset);
    exec.set_extend_convex_edges(true);
    exec.set_merge_results(false);
    exec.set_shell_faces(&[face]);
    let mut notifications = Notifications::new(&mut part);
    let result = exec
        .do_solid_offset(
            &mut k,
            Some(&mut part),
            SolidOffsetOptions::default(),
            &mut notifications.context(),
        )
        .expect("offset");
    Some((k, result))
}

/// One closed manifold solid: two regions (the infinite one and the solid's), two
/// faceuse shells (the two sides of its one surface), that surface of Euler characteristic
/// `V - E + F - (L - F) = 2` (genus 0), Level 2 clean.
fn assert_one_genus_0_solid(inset: bool) {
    let Some((mut k, mut b)) = offset(inset) else {
        return;
    };
    b.use_arena_naming().unwrap();
    assert!(brep_is_manifold_solid(&b).unwrap(), "inset {inset}");
    let regions = brep_p1::regions(&b, None).unwrap();
    let mut shells = std::collections::BTreeSet::new();
    for &r in &regions {
        shells.extend(b.region(r).unwrap().shells.iter().copied());
    }
    let faces = brep_p1::faces(&b, None).unwrap();
    let edges = brep_p1::edges(&b, None).unwrap();
    let vertices = brep_p1::vertices(&b, None).unwrap();
    let mut loops = 0usize;
    for &f in &faces {
        loops += solidcore::topology::face::p2::loops(&b, f, false, None)
            .unwrap()
            .len();
    }
    let (v, e, f, l) = (
        vertices.len() as i64,
        edges.len() as i64,
        faces.len() as i64,
        loops as i64,
    );
    assert_eq!(regions.len(), 2, "inset {inset}");
    assert_eq!(shells.len(), 2, "inset {inset}");
    assert_eq!(
        v - e + f - (l - f),
        2,
        "inset {inset}: V {v} E {e} F {f} L {l}"
    );
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "inset {inset}: {lines:?}");

    // every edge proved within its tolerance of both of its faces' surfaces (a seam edge's
    // two uses are on one face: its surface is checked for both)
    for &e in &edges {
        let curve = edge::curve(&b, e).unwrap().expect("edge curve");
        let ivl = edge::interval(&b, e).unwrap();
        let tol = edge::tolerance(&b, e).unwrap();
        let faces = edge::faces(&b, e, None).unwrap();
        assert!(
            matches!(faces.len(), 1 | 2),
            "inset {inset}: edge {e:?} faces {faces:?}"
        );
        let s0 = face_p2::surface(&b, faces[0]).unwrap().expect("surface");
        let s1 = face_p2::surface(&b, *faces.last().unwrap())
            .unwrap()
            .expect("surface");
        assert!(
            curve_on_surfaces(&k, curve, ivl, [s0, s1], tol).unwrap(),
            "inset {inset}: edge {e:?} over {ivl:?} not proved within {tol} of its faces"
        );
    }

    // the vertex box, as the goldens row records it
    let (lo, hi) = vertices.iter().fold(
        ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
        |(mut lo, mut hi), &v| {
            let p = vertex::point(&b, v).unwrap();
            for (i, c) in [p.x, p.y, p.z].into_iter().enumerate() {
                lo[i] = lo[i].min(c);
                hi[i] = hi[i].max(c);
            }
            (lo, hi)
        },
    );
    let golden = if inset {
        [
            -99.44260603027345,
            -58.78686719970706,
            -30.262270031738275,
            99.44260603027342,
            58.78686719970703,
            82.07627110765964,
        ]
    } else {
        [
            -121.54100603027345,
            -80.88526719970704,
            -52.36067003173828,
            121.54100603027345,
            80.88526719970704,
            103.9026032099467,
        ]
    };
    for (got, want) in lo.iter().chain(hi.iter()).zip(golden) {
        assert!(
            (got - want).abs() <= 1e-9 * got.abs().max(want.abs()),
            "inset {inset}: vertex box {lo:?} {hi:?}"
        );
    }
}

/// The fixture `tests/data/issue_15282_tilt_cyl_mate_pair.json`, which the unit test
/// `offset::curve_on_surfaces::tests::tilt_cyl_old_offset_edge_is_off_its_plane_and_the_intersection_is_on_both`
/// certifies, holds the outward offset's mated pair: its two surfaces (the tilted cylinder's
/// side, a rational quadratic by linear net, and the plane `z = 0`, a bilinear net) are one
/// result edge's faces', bit for bit. That edge's curve is no longer the fixture's marched
/// B-spline: `MateEdges` installs the exact intersection of the two surfaces (WR-495,
/// #21301), the ellipse proved here in closed form.
///
/// **Closed form.** The net's weight-1 points `P0`, `P2`, `P4`, `P6` of each `v` row are
/// the circle's quarter points, so its centre is `c = (P0 + P4) / 2` and its radius
/// `r = |P0 - c|`; the axis is `a = c1 - c0` between the two rows' centres. The plane
/// `z = 0` cuts it in the ellipse with centre `c0 - (c0.z / a.z) a`, minor radius `r` along
/// `a x z`, major radius `r |a| / |a.z|` along `(a.x, a.y, 0)`.
///
/// **Allowance.** Every coordinate, length and radius here is below `S = 256` in
/// magnitude. Each closed-form quantity takes at most 12 roundings of such operands (the
/// centre: two midpoints, a difference, a quotient, a product and a sum per coordinate;
/// the radii: three squares, two sums, a root, a product and a quotient), each within
/// `u S` (`u = 2^-53`), so within `12 u S` of the exact value of the net's data; the kernel
/// builds the ellipse from the same cylinder and plane in no more operations, so the two
/// agree within `32 u S` (directions, unit length: `32 u`). The measured differences are
/// below `1e-14` (`2^-46.5`), well inside `32 u S = 2^-40`.
///
/// **On both surfaces.** The ellipse is proved within `1e-6` of both nets over its whole
/// interval (`curve_on_surfaces`, an enclosure certificate), a thousandth of the edge's
/// tolerance. That tolerance is the offset's own, `0.001339890721639`: the B-rep
/// tolerance every edge of the inward offset carries and the floor of the outward one
/// (whose marched edges carry twice it), not raised by the exact mate.
#[test]
fn tilt_cyl_fixture_mate_is_the_exact_ellipse() {
    const OPERATION_TOLERANCE: f64 = 0.001_339_890_721_639;
    let Some((k, b)) = offset(false) else {
        return;
    };
    let data: serde_json::Value =
        serde_json::from_str(include_str!("data/issue_15282_tilt_cyl_mate_pair.json")).unwrap();
    let f64s = |v: &serde_json::Value| -> Vec<f64> { serde_json::from_value(v.clone()).unwrap() };
    let same_net = |s: &Surface, net: &serde_json::Value| match s.kind() {
        SurfaceKind::BSpline(bs) => Nurbs::of(bs).is_ok_and(|n| {
            n.p as u64 == net["p"].as_u64().unwrap()
                && n.q as u64 == net["q"].as_u64().unwrap()
                && n.u == f64s(&net["u"])
                && n.v == f64s(&net["v"])
                && n.pw == f64s(&net["pw"])
        }),
        _ => false,
    };
    let nets = data["s"].as_array().unwrap();
    let mated: Vec<_> = brep_p1::edges(&b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            let faces = edge::faces(&b, e, None).unwrap();
            nets.iter().all(|net| {
                faces.iter().any(|&f| {
                    same_net(
                        face_p2::surface(&b, f).unwrap().expect("surface"),
                        &net["net"],
                    )
                })
            })
        })
        .collect();
    assert_eq!(mated.len(), 1, "edges between the fixture's two surfaces");
    let e = mated[0];
    let curve = edge::curve(&b, e).unwrap().expect("edge curve");
    let CurveKind::Ellipse(el) = curve.kind() else {
        panic!("the mate is not an ellipse: {}", curve.type_code());
    };
    assert_eq!(edge::tolerance(&b, e).unwrap(), OPERATION_TOLERANCE);

    // the closed form from the fixture's nets
    let plane = f64s(&nets[1]["net"]["pw"]);
    assert!(
        plane.chunks(4).all(|p| p[2] == 0.0 && p[3] == 1.0),
        "the plane z = 0"
    );
    let cyl = f64s(&nets[0]["net"]["pw"]);
    let pt = |i: usize| {
        let w = cyl[4 * i + 3];
        [cyl[4 * i] / w, cyl[4 * i + 1] / w, cyl[4 * i + 2] / w]
    };
    let mid = |p: [f64; 3], q: [f64; 3]| {
        [
            0.5 * (p[0] + q[0]),
            0.5 * (p[1] + q[1]),
            0.5 * (p[2] + q[2]),
        ]
    };
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let sub = |p: [f64; 3], q: [f64; 3]| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
    // u-major, two v rows: the row v0's points are the even indices
    let (c0, c1) = (mid(pt(0), pt(8)), mid(pt(1), pt(9)));
    let a = sub(c1, c0);
    let r = len(sub(pt(0), c0));
    let s = -c0[2] / a[2];
    let centre = [c0[0] + s * a[0], c0[1] + s * a[1], c0[2] + s * a[2]];
    let major = r * len(a) / a[2].abs();
    let lp = (a[0] * a[0] + a[1] * a[1]).sqrt();
    let major_dir = [a[0] / lp, a[1] / lp, 0.0];

    let u = f64::EPSILON / 2.0;
    let (point_tol, dir_tol) = (32.0 * u * 256.0, 32.0 * u);
    let pos = el.position();
    let o = pos.origin();
    assert!(
        len(sub([o.x, o.y, o.z], centre)) <= point_tol,
        "centre {o:?} against {centre:?}"
    );
    // the major axis is the placement's x (radius at 0 degrees), the minor its y
    assert!(
        (el.x_radius() - major).abs() <= point_tol,
        "{} {major}",
        el.x_radius()
    );
    assert!(
        (el.y_radius() - r).abs() <= point_tol,
        "{} {r}",
        el.y_radius()
    );
    let x = pos.x_axis();
    let cross = [
        x.y * major_dir[2] - x.z * major_dir[1],
        x.z * major_dir[0] - x.x * major_dir[2],
        x.x * major_dir[1] - x.y * major_dir[0],
    ];
    assert!(
        len(cross) <= dir_tol,
        "major axis {x:?} against {major_dir:?}"
    );
    let y = pos.y_axis();
    assert!(
        (x.x * y.x + x.y * y.y + x.z * y.z).abs() <= dir_tol && y.z.abs() <= dir_tol,
        "minor axis {y:?}"
    );

    // on both of the fixture's surfaces, by certificate, over the edge's whole interval
    let faces = edge::faces(&b, e, None).unwrap();
    let s0 = face_p2::surface(&b, faces[0]).unwrap().expect("surface");
    let s1 = face_p2::surface(&b, faces[1]).unwrap().expect("surface");
    let ivl = edge::interval(&b, e).unwrap();
    assert!(curve_on_surfaces(&k, curve, ivl, [s0, s1], 1e-6).unwrap());
}

#[test]
fn tilt_cyl_on_box_offset_in_is_one_checked_solid() {
    assert_one_genus_0_solid(true);
}

#[test]
fn tilt_cyl_on_box_offset_out_is_one_checked_solid() {
    assert_one_genus_0_solid(false);
}
