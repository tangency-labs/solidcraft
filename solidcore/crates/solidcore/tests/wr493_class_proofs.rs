//! Class proofs for the golden rows moved by retiring the WR-493 flag. A sphere or torus
//! found while reading a file now keeps the file's own NURB, and the file's UV trim curves
//! stay on it.
//!
//! Recognising a face's NURB as a sphere or torus used to replace that NURB by one rebuilt
//! from the fitted circles. The rebuilt NURB has another parametrisation, so the file's UV
//! trims no longer mapped onto their edges. `check_and_fix_uv_trim_curves` then dropped
//! them and the face measures were taken on trims rebuilt on the refit surface.
//!
//! - [`found_spheres_and_tori_keep_the_file_nurb_and_trims`] (structural identity): the
//!   found surface is the file's NURB bit for bit, so the file's trims keep their meaning
//!   on it. The parameter, UV, trim and written-file rows of these files now record the
//!   file's own geometry.
//! - [`ss_plane_sph_area_is_the_area_its_edges_bound`]: on the four `ss_plane_sph` parts the
//!   face area equals the area its edge loop bounds on the sphere. Gauss-Bonnet gives that
//!   area as a line integral, here converged to 1e-12. The old value was 2.8e-3 off.
//! - [`sphere_plane_area_is_closed_form`]: the band, cap and disk of
//!   `Fillet/sphere_plane.smb` in closed form; the area is closer to it than before.
//! - [`point_just_outside_the_sphere_pole_is_in_the_infinite_region`]: the
//!   `query.brep` `sphere_vertex3_off_region` point lies outside the sphere exactly.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::zone_tol3d;
use solidcore::intersect::point_class::PointClassType;
use solidcore::mass::integrator::properties::face_properties;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::brep::{BrepIdentity, BrepQuery};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::database_io::FileType;
use solidcore::topology::face::p2::PPF_AREA;
use solidcore::topology::part_file::{read_part_from_file, PartEnv};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{BrepId, FaceId};
use solidcore::{Brep, EntityRef, Kernel};

fn bytes(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(name);
    std::fs::read(path).unwrap()
}

/// Reads an upstream `.smb` test file (healing on, as `SmBrep::ReadFromFile`).
fn read(k: &mut Kernel, name: &str) -> Brep {
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = solidcore::topology::loopuse::notify_ref;
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes(name), FileType::Ascii, false, &mut env).unwrap();
    b
}

/// A distinct identity per topology entity of each B-rep of a part file.
fn part_key(brep: usize, e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    let brep = (brep as u64 + 1) << 36;
    ObjectRef {
        type_code,
        key: ObjectKey(brep | ((kind + 1) << 32) | u64::from(i)),
    }
}

/// Reads the B-reps of an upstream `.smp` part file.
fn read_part(k: &mut Kernel, name: &str) -> Vec<Brep> {
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let mut env = PartEnv {
        attributes: &mut attributes,
        key_of: &part_key,
        records: &mut records,
        next_key: 1 << 62,
    };
    read_part_from_file(k, bytes(name), FileType::Ascii, &mut env)
        .unwrap()
        .breps
}

/// The face area as `corpus.parity` measures it (per-face integration at 1e-6).
fn face_area(k: &Kernel, b: &mut Brep, f: FaceId) -> f64 {
    let bbox = solidcore::query::face::p1::calculate_bounding_box(k, &*b, f, false).unwrap();
    let s = bbox.size();
    let d = s.x.max(s.y).max(s.z);
    let p = face_properties(
        k,
        b,
        f,
        OrientType::Same,
        1e-6,
        bbox.mid(),
        d * d / 100.0,
        0.0,
        PPF_AREA,
    )
    .unwrap();
    p.area.abs()
}

/// The `SmBSplineSurface` part of a surface derived from it.
fn b_spline_part(s: &Surface) -> Option<&BSplineSurface> {
    match s.kind() {
        SurfaceKind::BSpline(b) => Some(b),
        SurfaceKind::Plane(p) => Some(p.base()),
        SurfaceKind::Sphere(sp) => Some(sp.base().base()),
        SurfaceKind::Torus(t) => Some(t.base().base()),
        SurfaceKind::SurfOfRevolution(sr) => Some(sr.base()),
        _ => None,
    }
}

/// The NURB as bits: every control point `[x, y, z, w]` and both knot vectors.
type Bits = (Vec<[u64; 4]>, Vec<u64>, Vec<u64>);

fn bits(b: &BSplineSurface) -> Option<Bits> {
    let n = b.gw_nurb()?;
    let rows = n
        .rows()
        .flat_map(|r| {
            r.iter()
                .map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits(), p.w.to_bits()])
        })
        .collect();
    let knots = |k: &[f64]| k.iter().map(|x| x.to_bits()).collect();
    Some((rows, knots(n.u_knots()), knots(n.v_knots())))
}

/// A UV curve as bits: its natural interval and five points on it.
fn curve_bits(k: &Kernel, c: &solidcore::curve::curve::Curve) -> Vec<u64> {
    let iv = c.natural_interval();
    let mut out = vec![iv.min().to_bits(), iv.max().to_bits()];
    for f in [0.0, 0.17, 0.5, 0.83, 1.0] {
        let p = c
            .evaluate_point(k, iv.min() + (iv.max() - iv.min()) * f)
            .unwrap();
        out.extend([p.x.to_bits(), p.y.to_bits()]);
    }
    out
}

/// The face of a loop edge use.
fn face_of(b: &Brep, eu: &solidcore::topology::Edgeuse) -> Option<FaceId> {
    let solidcore::topology::EdgeuseOwner::Loopuse(Some(lu)) = eu.owner else {
        return None;
    };
    let l = b.loopuse(lu).ok()?.loop_?;
    Some(b.loop_(l).ok()?.face)
}

/// Every found sphere/torus face holds a NURB that is bitwise one of the file's surfaces,
/// so its UV parameters are the file's. Returns the number of found faces and of the UV
/// trims on them that are the file's own (same interval, same points bit for bit), so a
/// file without any cannot pass vacuously.
fn assert_found_faces_keep_file_nurbs(name: &str) -> (usize, usize) {
    let mut k = Kernel::default();
    let data = BrepData::read_from_file(&k, bytes(name), FileType::Ascii).unwrap();
    let file: Vec<Bits> = data
        .surfaces
        .iter()
        .filter_map(|s| b_spline_part(&s.object).and_then(bits))
        .collect();
    let file_trims: Vec<Vec<u64>> = data
        .uv_curves
        .iter()
        .map(|c| curve_bits(&k, &c.object))
        .collect();

    let b = read(&mut k, name);
    let mut found = Vec::new();
    for (fid, f) in b.faces().iter() {
        let Some(s) = f.surface.as_ref() else {
            continue;
        };
        if !matches!(s.kind(), SurfaceKind::Sphere(_) | SurfaceKind::Torus(_)) {
            continue;
        }
        found.push(fid);
        let nurb = b_spline_part(s)
            .and_then(bits)
            .expect("found surface keeps a NURB");
        assert!(
            file.contains(&nurb),
            "{name}: {fid:?}: found surface's NURB is not the file's"
        );
    }
    let mut trims = 0;
    for (_, eu) in b.edgeuses().iter() {
        if !face_of(&b, eu).is_some_and(|f| found.contains(&f)) {
            continue;
        }
        // (the healer still rebuilds a file trim that misses its edge, as on any surface)
        if eu
            .uv_trim_curve
            .as_ref()
            .is_some_and(|t| file_trims.contains(&curve_bits(&k, t)))
        {
            trims += 1;
        }
    }
    (found.len(), trims)
}

#[test]
fn found_spheres_and_tori_keep_the_file_nurb_and_trims() {
    let files = [
        "Fillet/box_w_slot.smb",
        "Fillet/sphere_diff_3cylinders.smb",
        "Fillet/sphere_plane.smb",
        "Solids/nurb_fillet.smb",
        "Solids/revolution_360.smb",
        "Solids/revolution_mold.smb",
        "Solids/sphere.smb",
        "sphere.smb",
        "surf_sphere150.smb",
    ];
    for name in files {
        let (found, trims) = assert_found_faces_keep_file_nurbs(name);
        assert!(
            found > 0 && trims > 0,
            "{name}: {found} spheres or tori found, {trims} trims"
        );
    }
}

/// Gauss-Legendre nodes and weights on [-1, 1] (Newton on the Legendre recurrence).
fn gauss(n: usize) -> Vec<(f64, f64)> {
    (1..=n)
        .map(|i| {
            let nf = n as f64;
            let mut x = (std::f64::consts::PI * (i as f64 - 0.25) / (nf + 0.5)).cos();
            let mut dp = 1.0;
            for _ in 0..100 {
                let (mut p0, mut p1) = (1.0, x);
                for m in 2..=n {
                    let mf = m as f64;
                    (p0, p1) = (p1, ((2.0 * mf - 1.0) * x * p1 - (mf - 1.0) * p0) / mf);
                }
                dp = nf * (x * p1 - p0) / (x * x - 1.0);
                let dx = p1 / dp;
                x -= dx;
                if dx.abs() < 1e-16 {
                    break;
                }
            }
            (x, 2.0 / ((1.0 - x * x) * dp * dp))
        })
        .collect()
}

/// The area on the sphere (`centre`, `radius`) of the side of the closed edge loop of `b`
/// that holds the direction `inside` from the centre. By Gauss-Bonnet in spherical
/// coordinates about that direction it is `r^2 * |sum of (1 - cos theta) d phi|` along the
/// loop; each edge's knot span is integrated with `n` Gauss points.
fn loop_area(k: &Kernel, b: &Brep, centre: Vec3, radius: f64, inside: Vec3, n: usize) -> f64 {
    let e3 = inside * (1.0 / inside.length());
    let seed = if e3.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let e1 = seed.cross(e3);
    let e1 = e1 * (1.0 / e1.length());
    let e2 = e3.cross(e1);
    // the edges, each with its direction along the loop (a single closed chain)
    let mut edges: Vec<_> = b.edges().iter().map(|(_, e)| e).collect();
    let mut chain = vec![(edges.remove(0), true)];
    while !edges.is_empty() {
        let (last, fwd) = *chain.last().unwrap();
        let iv = last.interval;
        let c = last.curve.as_ref().unwrap();
        let end = c
            .evaluate_point(k, if fwd { iv.max() } else { iv.min() })
            .unwrap();
        let at = |e: &&solidcore::topology::Edge, t: f64| {
            (e.curve.as_ref().unwrap().evaluate_point(k, t).unwrap() - end).length()
        };
        let i = edges
            .iter()
            .position(|e| at(e, e.interval.min()) < 1e-6 || at(e, e.interval.max()) < 1e-6)
            .expect("closed chain");
        let e = edges.remove(i);
        let fwd = at(&e, e.interval.min()) < 1e-6;
        chain.push((e, fwd));
    }
    let g = gauss(n);
    let mut total = 0.0;
    for (e, fwd) in chain {
        let c = e.curve.as_ref().unwrap();
        let (knots, _) = c.knots(k, Some(&e.interval)).unwrap();
        let sign = if fwd { 1.0 } else { -1.0 };
        for w in knots.windows(2) {
            let (a, bb) = (w[0], w[1]);
            for &(x, wt) in &g {
                let t = a + (bb - a) * (x + 1.0) / 2.0;
                let d = c.evaluate(k, t, 1, false, false).unwrap();
                let q = d[0] - centre;
                let (qx, qy, qz) = (q.dot(e1), q.dot(e2), q.dot(e3));
                let (dx, dy) = (d[1].dot(e1), d[1].dot(e2));
                let cos_t = qz / q.length();
                let dphi = (qx * dy - qy * dx) / (qx * qx + qy * qy);
                total += sign * wt * (bb - a) / 2.0 * (1.0 - cos_t) * dphi;
            }
        }
    }
    radius * radius * total.abs()
}

/// `corpus.parity` `ss_plane_sph1..4`: the area moved from 25.21534708166 (refit sphere,
/// trims rebuilt) to 25.1445667451; the face's edge loop bounds 25.1445668449 on the sphere.
#[test]
fn ss_plane_sph_area_is_the_area_its_edges_bound() {
    for name in [
        "ss_plane_sph1.smp",
        "ss_plane_sph2.smp",
        "ss_plane_sph3.smp",
        "ss_plane_sph4.smp",
    ] {
        let mut k = Kernel::default();
        let mut breps = read_part(&mut k, name);
        let b = &mut breps[0];
        let faces = bl::faces(b, None).unwrap();
        assert_eq!(faces.len(), 1, "{name}");
        let f = faces[0];
        let s = b.face(f).unwrap().surface.as_ref().unwrap();
        let SurfaceKind::Sphere(sp) = s.kind() else {
            panic!("{name}: sphere not found")
        };
        let (centre, radius) = (sp.base().position().origin(), sp.radius());
        assert!(
            (centre - Vec3::new(10.0, 2.0, 2.0)).length() < 1e-12 && (radius - 2.0).abs() < 1e-12
        );
        // a point well inside the face: the middle of its UV domain
        let dom = b.face(f).unwrap().uv_domain;
        let mid = s.evaluate_point(&k, dom.evaluate(0.5, 0.5)).unwrap();
        let cert = loop_area(&k, b, centre, radius, mid - centre, 16);
        let cert2 = loop_area(&k, b, centre, radius, mid - centre, 32);
        assert!(
            (cert - cert2).abs() <= 1e-12 * cert,
            "{name}: quadrature not converged: {cert} {cert2}"
        );
        let area = face_area(&k, b, f);
        assert!(
            (area - cert).abs() <= 1e-6 * cert,
            "{name}: area {area}, edges bound {cert}"
        );
        let old = 25.21534708166;
        assert!((old - cert).abs() > 1e3 * (area - cert).abs(), "{name}");
    }
}

/// `corpus.parity` `Fillet__sphere_plane.smb`: a sphere of radius 5 between z = +-h
/// (h = 4.98097..., the file's pole vertices) less its cap beyond the plane x = -4, plus
/// the disk of radius 3 in that plane. The area moved from 309.82222399413644 to
/// 309.8222080702443; the circle edge lies 1.7e-5 off the sphere, so it can be no closer
/// than that scale.
#[test]
fn sphere_plane_area_is_closed_form() {
    use std::f64::consts::PI;
    let h = 4.980_973_490_458_730_2;
    let exact = 2.0 * PI * 5.0 * 2.0 * h - 2.0 * PI * 5.0 * 1.0 + PI * 9.0;
    let mut k = Kernel::default();
    let mut b = read(&mut k, "Fillet/sphere_plane.smb");
    let faces = bl::faces(&b, None).unwrap();
    let area: f64 = faces.iter().map(|&f| face_area(&k, &mut b, f)).sum();
    let old = 309.822_223_994_136_44;
    assert!(
        (area - exact).abs() < (old - exact).abs(),
        "area {area}, closed form {exact}"
    );
    assert!(
        (area - exact).abs() <= 1e-5,
        "area {area}, closed form {exact}"
    );
}

/// `query.brep` `sphere_vertex3_off_region`: the vertex at the pole (0, 0, 5) of the sphere
/// of radius 5 about the origin, moved by (1e-7, -1e-7, 1e-7), is at distance
/// `sqrt(2e-14 + (5 + 1e-7)^2) > 5` from the centre: outside, in the infinite region (the
/// row moved from region 1, the inside, to region 0).
#[test]
fn point_just_outside_the_sphere_pole_is_in_the_infinite_region() {
    let mut k = Kernel::default();
    let mut b = read(&mut k, "sphere.smb");
    let pole = b
        .vertices()
        .iter()
        .map(|(_, v)| v.point)
        .find(|p| (*p - Vec3::new(0.0, 0.0, 5.0)).length() < 1e-12)
        .expect("pole vertex");
    let p = Vec3::new(pole.x + 1e-7, pole.y - 1e-7, pole.z + 1e-7);
    assert!(p.length() > 5.0);
    let query = BrepQuery::new(BrepIdentity::arena(BrepId::from_index(0)));
    let tol = zone_tol3d(&k, 1e-6);
    let pc = query.point_3d_classify(&k, &mut b, p, tol, false).unwrap();
    assert_eq!(pc.point_class(), PointClassType::Region, "{pc:?}");
    let Some(EntityRef::Region(r)) = pc.object() else {
        panic!("{pc:?}")
    };
    assert_eq!(b.infinite_region(), Some(r));
}
