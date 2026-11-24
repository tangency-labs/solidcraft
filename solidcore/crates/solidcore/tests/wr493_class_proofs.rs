//! Class proofs for the golden rows moved by retiring the WR-493 flag. A sphere or torus
//! found while reading a file now keeps the file's own NURB, and the file's UV trim curves
//! stay on it.
//!
//! Recognising a face's NURB as a sphere or torus used to replace that NURB by one rebuilt
//! from the fitted circles. The rebuilt NURB has another parametrisation, so the file's UV
//! trims no longer mapped onto their edges. `check_and_fix_uv_trim_curves` then dropped
//! them and the face measures were taken on trims rebuilt on the refit surface.
//!
//! - `found_spheres_and_tori_keep_the_file_nurb_and_trims` (structural identity): the
//!   found surface is the file's NURB bit for bit, so the file's trims keep their meaning
//!   on it. The parameter, UV, trim and written-file rows of these files now record the
//!   file's own geometry.
//! - `ss_plane_sph_area_is_the_area_its_edges_bound`: on the four `ss_plane_sph` parts the
//!   face area equals the area its edge loop bounds on the sphere. Gauss-Bonnet gives that
//!   area as a line integral, here converged to 1e-12. The old value was 2.8e-3 off.
//! - `sphere_plane_area_is_closed_form`: the band, cap and disk of
//!   `Fillet/sphere_plane.smb` in closed form; the area is closer to it than before.
//! - `box_w_slot_sphere_corners_are_the_area_their_trims_bound`: each sphere corner of
//!   `Fillet/box_w_slot.smb` now measures the area its own trims bound (Gauss-Bonnet and
//!   Green in UV agree to 1e-11), to 2e-9; the old areas were 5.5e-9 to 2.9e-8 off.
//! - `box_w_slot_volume_and_mass_are_those_their_trims_bound`: integrated tightly, each
//!   corner's volume and moments are those its trims bound (Stokes and Green agree), so
//!   the geometry is exact. The `corpus.parity` volume is the trims-bound reference to
//!   the bit (old 9.8e-11 off). The `blend.fillet_executive_p3` mass row is closer in
//!   every component but cz, which moves by one ulp through the corners' z moment (EQUAL,
//!   within the row's accuracy, to the certified enclosure): the integrator ends
//!   its trim pieces where a trim crosses an interior v knot line of its surface on every
//!   face, not only on a banded one (without that, cy, Pyz and Pxy were farther).
//! - `point_just_outside_the_sphere_pole_is_in_the_infinite_region`: the
//!   `query.brep` `sphere_vertex3_off_region` point lies outside the sphere exactly.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::p2::LINE_TYPE;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::zone_tol3d;
use solidcore::intersect::point_class::PointClassType;
use solidcore::mass::enclosure::{enclose_face_area_relative, enclose_volume};
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

/// The area `corpus.parity` records for `b`: its runner's calls in its order (the counts,
/// loops, kind and shells, then each face's [`face_area`] summed in face order), so the
/// value is the committed row's bit for bit.
fn parity_area(k: &mut Kernel, b: &mut Brep) -> f64 {
    let faces = bl::faces(b, None).unwrap();
    bl::edges(b, None).unwrap();
    bl::vertices(b, None).unwrap();
    for f in &faces {
        solidcore::topology::face::p2::loops(b, *f, false, None).unwrap();
    }
    if !solidcore::topology::brep::p2::is_manifold_solid(b).unwrap() {
        solidcore::topology::brep::p2::is_sheet(k, b).unwrap();
    }
    bl::shells(k, b, None).unwrap();
    let mut area = 0.0;
    for f in &faces {
        area += face_area(k, b, *f);
    }
    area
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

/// A UV curve as bits: its natural interval, for a B-spline or a line its kind and whole
/// definition, and five points on it as evaluated.
fn curve_bits(k: &Kernel, c: &solidcore::curve::curve::Curve) -> Vec<u64> {
    use solidcore::curve::curve::CurveKind;
    let iv = c.natural_interval();
    let mut out = vec![iv.min().to_bits(), iv.max().to_bits()];
    // a B-spline or line: its whole definition (degree, knots and control points in use)
    // and its kind, so a line and the B-spline of its definition are told apart
    let base = match c.kind() {
        CurveKind::BSpline(b) => {
            out.push(1);
            Some(std::borrow::Cow::Borrowed(b))
        }
        CurveKind::Line(l) => {
            out.push(2);
            l.base_with_nurb().ok()
        }
        _ => None,
    };
    if let Some(nurb) = base.as_ref().and_then(|b| b.gw_nurb()) {
        let (n, p) = (nurb.n(), nurb.degree());
        out.push(p as u64);
        out.extend(nurb.knots()[..n + p + 2].iter().map(|x| x.to_bits()));
        for q in &nurb.cpts()[..=n] {
            out.extend([q.x, q.y, q.z, q.w].map(f64::to_bits));
        }
    }
    // and, whatever the kind, five points as evaluated bit for bit
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

/// Every found sphere/torus face holds a NURB that is bitwise its own record's surface in
/// the file, so its UV parameters are the file's. Returns the number of found faces and of
/// the UV trims on them that are their own edge-use record's as the read makes it (the
/// same kind, definition bit for bit — degree, knots and control points — and five
/// evaluated points), so a file without any cannot pass vacuously; the others are listed. Record i makes the primary edge use at
/// arena index 2i (which carries the record's trim) and its mate at 2i + 1 (which
/// carries none, asserted).
fn assert_found_faces_keep_file_nurbs(name: &str) -> (usize, usize, Vec<u32>) {
    let mut k = Kernel::default();
    let data = BrepData::read_from_file(&k, bytes(name), FileType::Ascii).unwrap();
    // each record's trim as the read makes it: the read's analytics (`MakeUVCurve` with
    // `add_analytics`) turn a B-spline that is a line into that line, on every face
    let file_trims: Vec<Vec<u64>> = data
        .uv_curves
        .iter()
        .map(|c| {
            let c = &c.object;
            let line = if c.is_analytic() {
                None
            } else {
                c.create_analytic_curve(&k, &c.natural_interval())
                    .unwrap()
                    .filter(|a| a.is_kind_of(LINE_TYPE))
            };
            curve_bits(&k, line.as_ref().unwrap_or(c))
        })
        .collect();

    let b = read(&mut k, name);
    // the read keeps the file's record order: face i is record i
    assert_eq!(b.faces().iter().count(), data.faces.len(), "{name}: faces");
    // each edge use record makes its use (arena 2i, which carries the record's trim) and
    // the mate (arena 2i + 1, which carries none)
    assert_eq!(
        b.edgeuses().iter().count(),
        2 * data.edgeuses.len(),
        "{name}: edge uses"
    );
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
        let record = &data.faces[fid.index() as usize];
        let source = data
            .surfaces
            .get(record.surface as usize)
            .and_then(|s| b_spline_part(&s.object).and_then(bits));
        assert_eq!(
            Some(nurb),
            source,
            "{name}: {fid:?}: found surface's NURB is not its own record's"
        );
    }
    let mut trims = 0;
    let mut remade = Vec::new();
    for (id, eu) in b.edgeuses().iter() {
        if !face_of(&b, eu).is_some_and(|f| found.contains(&f)) {
            continue;
        }
        let i = id.index() as usize;
        if i % 2 == 1 {
            // a mate: the read puts no trim on it
            assert!(eu.uv_trim_curve.is_none(), "{name}: mate {i} has a trim");
            continue;
        }
        let record = data.edgeuses.get(i / 2).expect("record");
        let own = usize::try_from(record.uv_curve)
            .ok()
            .and_then(|u| file_trims.get(u));
        match (own, eu.uv_trim_curve.as_ref()) {
            (Some(own), Some(t)) if *own == curve_bits(&k, t) => trims += 1,
            (Some(_), _) => remade.push(id.index()),
            (None, _) => panic!("{name}: edge use record {} has no trim", i / 2),
        }
    }
    (found.len(), trims, remade)
}

#[test]
fn found_spheres_and_tori_keep_the_file_nurb_and_trims() {
    // every primary edge use on a found face keeps its own record's UV trim, as the read
    // makes it (the earlier lists [10, 26], [4] and [58, 68] were lines the read's
    // analytics made from those very records, whose NURBs the line rebuilds: compared with
    // the read's line of the record, they are the record's)
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
    let mut failures = Vec::new();
    for name in files {
        let (found, trims, remade) = assert_found_faces_keep_file_nurbs(name);
        if found == 0 || trims == 0 || !remade.is_empty() {
            failures.push(format!(
                "{name}: {found} found, {trims} file trims, remade {remade:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
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
    // the chain closes: the last edge ends where the first starts
    {
        let end_of = |(e, fwd): (&solidcore::topology::Edge, bool), at_end: bool| {
            let iv = e.interval;
            let t = if fwd == at_end { iv.max() } else { iv.min() };
            e.curve.as_ref().unwrap().evaluate_point(k, t).unwrap()
        };
        let (first, last) = (chain[0], *chain.last().unwrap());
        let gap = (end_of(last, true) - end_of(first, false)).length();
        assert!(gap < 1e-6, "the loop does not close: {gap:e}");
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

/// `corpus.parity` `ss_plane_sph1..4`: the sphere face's row moved from main's committed
/// 25.215347285779345 (sph1, sph4) and 25.215347285779387 (sph2, sph3) (refit sphere, trims
/// rebuilt) to 25.14456696301113 (sph1, sph4) and 25.144566963011123 (sph2, sph3).
/// - PROVEN: the row is its face's area. The face's area is certified
///   ([`enclose_face_area_relative`], 1e-10 of itself wide, about 2.5e-9) and the row is in
///   the enclosure. The enclosure is of this branch's face, so it ranks nothing by itself
///   (main's rows, more than 1e3 widths outside it, measure main's face).
/// - CLOSER, ARGUED: the reference is the area the face's edge loop bounds on the sphere
///   (Gauss-Bonnet along the edges, by quadrature: n = 16 agrees with n = 32 to 1e-12,
///   not certified). The row is 4.7e-9 relative off it (the trims' gap to their edges),
///   main's rows 2.8e-3 relative, more than 1e3 times farther. Cross-check: the area the
///   face's UV trims bound (Green's theorem in UV) is the row to 1e-12.
///
/// The plane face's row is 25, the area of its square of side 5, as main's committed row.
#[test]
fn ss_plane_sph_area_is_the_area_its_edges_bound() {
    let mut rows = Vec::new();
    for (name, row) in [
        ("ss_plane_sph1.smp", 25.144_566_963_011_13),
        ("ss_plane_sph2.smp", 25.144_566_963_011_123),
        ("ss_plane_sph3.smp", 25.144_566_963_011_123),
        ("ss_plane_sph4.smp", 25.144_566_963_011_13),
    ] {
        let mut k = Kernel::default();
        let mut breps = read_part(&mut k, name);
        // the committed row, measured as the runner does, B-rep by B-rep
        let area = parity_area(&mut k, &mut breps[0]);
        if area.to_bits() != f64::to_bits(row) {
            rows.push(format!("{name}: area {area}, row {row}"));
        }
        let plane = parity_area(&mut k, &mut breps[1]);
        assert_eq!(plane, 25.0, "{name}: plane {plane}");
        let b = &mut breps[0];
        let faces = bl::faces(b, None).unwrap();
        assert_eq!(faces.len(), 1, "{name}");
        let f = faces[0];
        // the certified area, on a fresh read (the enclosure makes any missing trims)
        let mut fresh = read_part(&mut k, name);
        let e = enclose_face_area_relative(&k, &mut fresh[0], &[f], 1e-10).unwrap();
        let width = e.hi - e.lo;
        assert!(
            e.lo <= area && area <= e.hi,
            "{name}: row {area} off [{}, {}]",
            e.lo,
            e.hi
        );
        for old in [25.215_347_285_779_345, 25.215_347_285_779_387] {
            assert!(old - e.hi > 1e3 * width, "{name}: old {old} not ranked");
        }
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
        // the Green-boundary cross-check: the face's own UV trims bound the row's area (to
        // 1e-12; Green in UV, an independent line integral), and the edge loop's area
        // within the trims' 9.2e-4 gap to their edges (4.7e-9 relative here)
        let green = sphere_face_trim_area(&k, b, f, 64, centre).green;
        assert!(
            (green - area).abs() <= 1e-12 * green,
            "{name}: Green {green}, row {area}"
        );
        assert!(
            (green - cert).abs() <= 1e-8 * cert,
            "{name}: Green {green}, Gauss-Bonnet {cert}"
        );
        assert!(
            (area - cert).abs() <= 1e-6 * cert,
            "{name}: row {area}, edges bound {cert}"
        );
        // main's committed rows (CLOSER, ARGUED: `cert` is a quadrature)
        for old in [25.215_347_285_779_345, 25.215_347_285_779_387] {
            assert!((old - cert).abs() > 1e3 * (area - cert).abs(), "{name}");
        }
        // the plane: its square of side 5, as main's committed row
        let square = plane_polygon_area(&breps[1]);
        assert_eq!(square, 25.0, "{name}: plane square {square}");
        assert_eq!(plane, square, "{name}: plane {plane}");
    }
    assert!(rows.is_empty(), "{rows:#?}");
}

/// The area of the planar face of `b`, a polygon of straight edges, from its vertices
/// (the shoelace formula in the plane's own axes, here axis-aligned).
fn plane_polygon_area(b: &Brep) -> f64 {
    let mut pts: Vec<[f64; 3]> = b
        .vertices()
        .iter()
        .map(|(_, v)| v.point.to_array())
        .collect();
    for (_, e) in b.edges().iter() {
        let c = e.curve.as_ref().unwrap();
        assert!(
            matches!(c.kind(), solidcore::curve::curve::CurveKind::Line(_)),
            "the plane's edges are lines"
        );
    }
    // the plane's normal axis: the coordinate all vertices share
    let n = (0..3)
        .find(|&i| pts.iter().all(|p| p[i] == pts[0][i]))
        .expect("an axis-aligned plane");
    let (a, c) = ((n + 1) % 3, (n + 2) % 3);
    let m = pts.len() as f64;
    let (ca, cc) = (
        pts.iter().map(|p| p[a]).sum::<f64>() / m,
        pts.iter().map(|p| p[c]).sum::<f64>() / m,
    );
    pts.sort_by(|p, q| {
        let ap = (p[c] - cc).atan2(p[a] - ca);
        let aq = (q[c] - cc).atan2(q[a] - ca);
        ap.total_cmp(&aq)
    });
    let mut twice = 0.0;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        twice += p[a] * q[c] - q[a] * p[c];
    }
    twice.abs() / 2.0
}

/// `corpus.parity` `Fillet__sphere_plane.smb`: a sphere of radius 5 between z = +-h
/// (h = 4.98097..., the height of the file's two vertices on the sphere) less its cap beyond the plane x = -4, plus
/// the disk of radius 3 in that plane. The committed row moved from main's 309.8222156047131
/// (1.4e-5 off the closed form; its round trip's 309.8221903199704, 1.2e-5 off) to
/// 309.8222012093527 (6.4e-7 off, the round trip's alike).
#[test]
fn sphere_plane_area_is_closed_form() {
    use std::f64::consts::PI;
    let h = 4.980_973_490_458_730_2;
    let exact = 2.0 * PI * 5.0 * 2.0 * h - 2.0 * PI * 5.0 * 1.0 + PI * 9.0;
    let mut k = Kernel::default();
    let mut b = read(&mut k, "Fillet/sphere_plane.smb");
    // the file's two vertices on the sphere at z = +-h (each a seam end, off the axis)
    for z in [h, -h] {
        assert!(
            b.vertices()
                .iter()
                .any(|(_, x)| x.point.z == z && (x.point.length() - 5.0).abs() < 1e-14),
            "no vertex on the sphere at z = {z}"
        );
    }
    let row = 309.822_201_209_352_7;
    let area = parity_area(&mut k, &mut b);
    assert_eq!(area.to_bits(), f64::to_bits(row), "area {area}");
    for old in [309.822_215_604_713_1, 309.822_190_319_970_4] {
        assert!(
            (row - exact).abs() < (old - exact).abs(),
            "row {row}, main's {old}, closed form {exact}"
        );
    }
    assert!(
        (row - exact).abs() <= 1e-6,
        "row {row}, closed form {exact}"
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

/// The cuts of `[a, b]` where the UV curve `t` crosses one of the knot lines `uk` (in u)
/// or `vk` (in v): the surface is a different polynomial patch on each side, so a Gauss
/// rule only converges on pieces between them. Sign changes are found on 512 steps and
/// bisected to the last bit. Two crossings of one line inside one step are not seen: the
/// rule then converges more slowly on that piece, which the n = 24 against n = 32
/// comparison of the callers shows; these integrals are a cross-check, the areas are
/// proven by `enclose_face_area_relative`.
fn knot_cuts(
    k: &Kernel,
    t: &solidcore::curve::curve::Curve,
    a: f64,
    b: f64,
    uk: &[f64],
    vk: &[f64],
) -> Vec<f64> {
    let mut cuts = vec![a, b];
    let m = 512_u32;
    for (knots, in_u) in [(uk, true), (vk, false)] {
        for &kn in knots {
            let f = |x: f64| {
                let p = t.evaluate_point(k, x).unwrap();
                if in_u {
                    p.x - kn
                } else {
                    p.y - kn
                }
            };
            for i in 0..m {
                let x0 = a + (b - a) * f64::from(i) / f64::from(m);
                let x1 = a + (b - a) * f64::from(i + 1) / f64::from(m);
                // a crossing on a step's point is a cut there (the ends are cuts already)
                if i > 0 && f(x0) == 0.0 {
                    cuts.push(x0);
                }
                if f(x0) * f(x1) < 0.0 {
                    let (mut lo, mut hi) = (x0, x1);
                    for _ in 0..80 {
                        let mid = 0.5 * (lo + hi);
                        if f(lo) * f(mid) <= 0.0 {
                            hi = mid;
                        } else {
                            lo = mid;
                        }
                    }
                    cuts.push(0.5 * (lo + hi));
                }
            }
        }
    }
    cuts.sort_by(|x, y| x.partial_cmp(y).unwrap());
    cuts.dedup();
    cuts
}

/// The integrator's volume group integrands at `x` (relative to the origin) for the area
/// vector `n`: the volume `z n_z`, then the static moments, the second moments, the
/// products (yz, zx, xy) and the axial moments, each times `z n_z` (as the integrator's
/// closure strips take them, `mass::integrator::properties::closure_integrands`).
fn volume_integrands(x: Vec3, n: Vec3) -> [f64; 13] {
    let vd = x.z * n.z;
    [
        vd,
        x.x * vd,
        x.y * vd,
        x.z * vd / 2.0,
        (x.x * x.x) * vd,
        (x.y * x.y) * vd,
        (x.z * x.z / 3.0) * vd,
        (x.y * x.z) * vd / 2.0,
        (x.z * x.x) * vd / 2.0,
        (x.x * x.y) * vd,
        (x.y * x.y + x.z * x.z / 3.0) * vd,
        (x.x * x.x + x.z * x.z / 3.0) * vd,
        (x.x * x.x + x.y * x.y) * vd,
    ]
}

/// The integrals of a sphere face over its own trims (see [`sphere_face_trim_area`]).
struct TrimIntegrals {
    /// The area by Gauss-Bonnet: `r^2 * |sum of (1 - cos theta) d phi|` along the trims'
    /// images about the direction of the face's middle.
    gauss_bonnet: f64,
    /// The area by Green in UV: `|sum of G(u, v) dv|`, `G(u, v)` the integral of
    /// `|S_u x S_v|` from the domain's `u0` to `u` (itself split at the u knots).
    green: f64,
    /// The volume integral `integral of z n_z dA` (relative to the origin, `n` out of the
    /// sphere) by line integrals alone: on the sphere `x . n = r + (c - origin) . n`, so
    /// with `a = (-y z, x z, 0) / 3`, whose curl is `z e_z - x / 3`, it is
    /// `(r A + (c - origin) . S) / 3 + sum of a . dx`. `A` is the Gauss-Bonnet area and
    /// `S` the vector area `1/2 * sum of (x - c) x dx` (Stokes); both line sums run round
    /// the loop the way that makes the Gauss-Bonnet sum positive (anticlockwise about the
    /// face's middle seen from outside the sphere).
    stokes_volume: f64,
    /// The volume group of [`volume_integrands`] by Green in UV as `green`, with the
    /// integrand's `S_u x S_v` in place of its length, taken out of the sphere (the sign of
    /// [`Self::stokes_volume`]).
    green_volume: [f64; 13],
}

/// The area of the sphere face `f` bounded by its own UV trims, two ways, with `n` Gauss
/// points on every piece between trim knots and surface knot crossings:
/// - Gauss-Bonnet along the trims' images, `r^2 * |sum of (1 - cos theta) d phi|` about the
///   direction of the face's middle;
/// - Green in UV, `|sum of G(u, v) dv|` with `G(u, v)` the integral of `|S_u x S_v|` from
///   the domain's `u0` to `u` (itself split at the u knots).
///
/// Also the face's share of the volume group (see [`TrimIntegrals`]) about `origin`, on
/// the sphere's outward side.
fn sphere_face_trim_area(k: &Kernel, b: &Brep, f: FaceId, n: usize, origin: Vec3) -> TrimIntegrals {
    use solidcore::math::Point2;
    let face = b.face(f).unwrap();
    let s = face.surface.as_ref().unwrap();
    let SurfaceKind::Sphere(sp) = s.kind() else {
        panic!("{f:?} is not a sphere")
    };
    let (centre, radius) = (sp.base().position().origin(), sp.radius());
    let nurb = sp.base().base().gw_nurb().unwrap();
    let (uk, vk) = (nurb.u_knots().to_vec(), nurb.v_knots().to_vec());
    // the trims of the face, each over its edge's interval
    let mut pieces = Vec::new();
    for (_, eu) in b.edgeuses().iter() {
        if face_of(b, eu) != Some(f) {
            continue;
        }
        let Some(t) = eu.uv_trim_curve.as_ref() else {
            continue;
        };
        pieces.push((t, b.edge(eu.edge.unwrap()).unwrap().interval));
    }
    assert!(pieces.len() >= 3, "{f:?}: {} trims", pieces.len());
    let img = |t: &solidcore::curve::curve::Curve, x: f64| {
        let uv = t.evaluate_point(k, x).unwrap();
        s.evaluate_point(k, Point2::new(uv.x, uv.y)).unwrap()
    };
    // the pole of the spherical coordinates: the direction of the face's middle
    let mid = s
        .evaluate_point(k, face.uv_domain.evaluate(0.5, 0.5))
        .unwrap();
    let e3 = (mid - centre) * (1.0 / (mid - centre).length());
    let seed = if e3.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let e1 = seed.cross(e3);
    let e1 = e1 * (1.0 / e1.length());
    let e2 = e3.cross(e1);
    // one closed chain, each piece with its sense along it
    let mut chain = vec![(pieces.remove(0), 1.0)];
    while !pieces.is_empty() {
        let ((lt, liv), sense) = *chain.last().unwrap();
        let end = img(lt, if sense > 0.0 { liv.max() } else { liv.min() });
        let gap = |(t, iv): &(
            &solidcore::curve::curve::Curve,
            solidcore::geom::extent1d::Extent1d,
        ),
                   at_min: bool| {
            (img(t, if at_min { iv.min() } else { iv.max() }) - end).length()
        };
        let i = pieces
            .iter()
            .position(|p| gap(p, true) < 1e-4 || gap(p, false) < 1e-4)
            .expect("closed chain");
        let p = pieces.remove(i);
        let sense = if gap(&p, true) < gap(&p, false) {
            1.0
        } else {
            -1.0
        };
        chain.push((p, sense));
    }
    // the chain closes: the last trim's image ends where the first's starts (a missing
    // side, which neither area integral need see, leaves it open)
    {
        let ((ft, fiv), fs) = chain[0];
        let ((lt, liv), ls) = *chain.last().unwrap();
        let start = img(ft, if fs > 0.0 { fiv.min() } else { fiv.max() });
        let end = img(lt, if ls > 0.0 { liv.max() } else { liv.min() });
        assert!(
            (end - start).length() < 1e-4,
            "{f:?}: the trims do not close"
        );
    }
    let g = gauss(n);
    let u0 = face.uv_domain.min().x;
    let (mut gb, mut green) = (0.0, 0.0);
    let mut va = Vec3::new(0.0, 0.0, 0.0);
    let mut al = 0.0;
    let mut volume = [0.0; 13];
    for ((t, iv), sense) in &chain {
        let (knots, _) = t.knots(k, Some(iv)).unwrap();
        for w in knots.windows(2) {
            for c in knot_cuts(k, t, w[0], w[1], &uk, &vk).windows(2) {
                let h = (c[1] - c[0]) / 2.0;
                for &(x, wt) in &g {
                    let d = t
                        .evaluate(k, c[0] + h * (x + 1.0), 1, false, false)
                        .unwrap();
                    let (u, v) = (d[0].x, d[0].y);
                    let (p, su, sv) = s
                        .evaluate_1st_derivatives(k, Point2::new(u, v), false, false)
                        .unwrap();
                    let dp = su * d[1].x + sv * d[1].y;
                    let q = p - centre;
                    let (qx, qy, qz) = (q.dot(e1), q.dot(e2), q.dot(e3));
                    let dphi = (qx * dp.dot(e2) - qy * dp.dot(e1)) / (qx * qx + qy * qy);
                    gb += sense * wt * h * (1.0 - qz / q.length()) * dphi;
                    va += q.cross(dp) * (0.5 * sense * wt * h);
                    let o = p - origin;
                    al += sense * wt * h * (-o.y * o.z * dp.x + o.x * o.z * dp.y) / 3.0;
                    // G(u, v): u0 to u along v, split at the u knots between them
                    let mut br = vec![u0, u];
                    br.extend(uk.iter().filter(|&&kn| kn > u0.min(u) && kn < u0.max(u)));
                    br.sort_by(|x, y| x.partial_cmp(y).unwrap());
                    let mut gu = 0.0;
                    let mut gv = [0.0; 13];
                    for s2 in br.windows(2) {
                        let h2 = (s2[1] - s2[0]) / 2.0;
                        for &(x2, wt2) in &g {
                            let (p2, a1, a2) = s
                                .evaluate_1st_derivatives(
                                    k,
                                    Point2::new(s2[0] + h2 * (x2 + 1.0), v),
                                    false,
                                    false,
                                )
                                .unwrap();
                            gu += wt2 * h2 * a1.cross(a2).length();
                            for (o, val) in gv
                                .iter_mut()
                                .zip(volume_integrands(p2 - origin, a1.cross(a2)))
                            {
                                *o += wt2 * h2 * val;
                            }
                        }
                    }
                    if u < u0 {
                        gu = -gu;
                        for o in &mut gv {
                            *o = -*o;
                        }
                    }
                    green += sense * wt * h * gu * d[1].y;
                    for (o, val) in volume.iter_mut().zip(gv) {
                        *o += sense * wt * h * val * d[1].y;
                    }
                }
            }
        }
    }
    let way = if gb < 0.0 { -1.0 } else { 1.0 };
    let (area, vector_area, a_line) = (radius * radius * gb.abs(), va * way, al * way);
    let stokes_volume = (radius * area + (centre - origin).dot(vector_area)) / 3.0 + a_line;
    // Green's sum has the sign of the UV loop's way round and of S_u x S_v: take the
    // outward one, that of the volume by Stokes
    let out = if (volume[0] < 0.0) == (stokes_volume < 0.0) {
        1.0
    } else {
        -1.0
    };
    TrimIntegrals {
        gauss_bonnet: area,
        green: green.abs(),
        stokes_volume,
        green_volume: volume.map(|x| x * out),
    }
}

/// `corpus.parity` `Fillet__box_w_slot.smb` area (5.914389913639413 to 5.914390008524208)
/// and the `blend.fillet_executive_p3` `source-file-box_w_slot-all-edges` area (the
/// fillet is refused, so the body is the file as read). Only the eight sphere corner faces
/// (radius 0.06) move: they now keep the file's NURB and trims. Each face's area bounded by
/// its own trims, by Gauss-Bonnet and by Green in UV (two independent line integrals,
/// converged at n = 24 against n = 32), is matched by the integrator to 2e-9 relative; the
/// old per-face areas, taken on a refit sphere with rebuilt trims, were 5.5e-9 to 2.9e-8
/// off. On the sum the new area is 2e-11 off the trims' area, the old one 9.5e-8.
///
/// PROVEN against the stored geometry (both rows): the area of the body as read, the file's
/// NURB and the file's trims, is certified ([`enclose_face_area_relative`] over every face,
/// 1e-10 of itself wide, about 5.9e-10); both new rows are in the enclosure and both old
/// rows more than 100 widths outside it. The enclosure is of this body's domain: main
/// measured another one (the refit sphere with rebuilt trims), so this proves the new rows
/// are the file's area and the old ones are not, which ranks them only with the file's
/// trims taken as the face's boundary. The quadrature references above (n = 24 against 32,
/// no remainder bound) are cross-checks, not the proof.
#[test]
fn box_w_slot_sphere_corners_are_the_area_their_trims_bound() {
    // the old per-face areas (refit sphere, rebuilt trims), faces 18..=25
    let old = [
        4.539_699_685_693_518e-3,
        4.539_699_673_323_199e-3,
        4.539_699_674_678_868e-3,
        5.552_842_101_372_212e-3,
        5.552_842_101_462_258e-3,
        5.552_842_101_694_424e-3,
        4.539_699_683_930_832e-3,
        5.552_842_101_449_415e-3,
    ];
    let name = "Fillet/box_w_slot.smb";
    let mut k = Kernel::default();
    let b = read(&mut k, name);
    // measured on a second read: making the missing trims of other faces may touch the body
    let mut m = read(&mut k, name);
    let faces = bl::faces(&b, None).unwrap();
    let (mut total, mut reference, mut old_total) = (0.0, 0.0, 0.0);
    let mut spheres = 0;
    for &f in &faces {
        let area = face_area(&k, &mut m, f);
        total += area;
        if !matches!(
            b.face(f).unwrap().surface.as_ref().unwrap().kind(),
            SurfaceKind::Sphere(_)
        ) {
            reference += area;
            old_total += area;
            continue;
        }
        let (t24, t32) = (
            sphere_face_trim_area(&k, &b, f, 24, Vec3::ZERO),
            sphere_face_trim_area(&k, &b, f, 32, Vec3::ZERO),
        );
        let (gb24, green24, gb32, green32) =
            (t24.gauss_bonnet, t24.green, t32.gauss_bonnet, t32.green);
        assert!((gb24 - gb32).abs() <= 1e-12 * gb32, "{f:?}: {gb24} {gb32}");
        assert!(
            (green24 - green32).abs() <= 1e-12 * green32,
            "{f:?}: {green24} {green32}"
        );
        assert!(
            (gb32 - green32).abs() <= 1e-11 * gb32,
            "{f:?}: Gauss-Bonnet {gb32}, Green {green32}"
        );
        assert!(
            (area - gb32).abs() <= 2e-9 * gb32,
            "{f:?}: area {area}, trims bound {gb32}"
        );
        let was = old[spheres];
        assert!(
            (was - gb32).abs() > 100.0 * (area - gb32).abs(),
            "{f:?}: old {was}, new {area}, trims bound {gb32}"
        );
        reference += gb32;
        old_total += was;
        spheres += 1;
    }
    assert_eq!(spheres, 8);
    // the corpus row, replayed
    assert!(
        (total - 5.914_390_008_524_208).abs() <= 1e-15 * total,
        "{total}"
    );
    assert!((total - reference).abs() <= 5e-11, "{total} {reference}");
    let old_row = 5.914_389_913_639_413;
    assert!((old_total - old_row).abs() <= 1e-9 * old_row, "{old_total}");
    assert!(
        (old_row - reference).abs() > 1e3 * (total - reference).abs(),
        "old {old_row}, new {total}, trims bound {reference}"
    );
    // the certified area: corpus.parity's row and fillet_executive_p3's (`brep_mass_properties`
    // at 1e-4), new in, old out
    let mut eb = read(&mut k, name);
    let e = enclose_face_area_relative(&k, &mut eb, &faces, 1e-10).unwrap();
    let width = e.hi - e.lo;
    for (new, old) in [
        (total, old_row),
        (5.914_390_008_549_578, 5.914_389_912_975_9),
    ] {
        assert!(
            e.lo <= new && new <= e.hi,
            "new {new} off [{}, {}]",
            e.lo,
            e.hi
        );
        assert!(e.lo - old > 100.0 * width, "old {old} not ranked");
    }
}

/// The integrator's share of the sphere face `f` in the volume group and area, about
/// `origin`, for the faceuse bounding the solid, and the same over the face's own trims
/// (`n = 32`, after checking it against `n = 24` and the Stokes volume against Green's).
/// Returns `(integrator, trims)`, each `[area, volume group (13)]`.
fn sphere_face_shares(
    k: &Kernel,
    b: &Brep,
    m: &mut Brep,
    f: FaceId,
    accuracy: f64,
    origin: Vec3,
    estimated: f64,
) -> ([f64; 14], [f64; 14]) {
    use solidcore::topology::face::p2::PPF_ALL;
    let face = b.face(f).unwrap();
    let up = face.faceuse.unwrap();
    let mate = b.faceuse(up).unwrap().mate.unwrap();
    let fu = if solidcore::topology::edgeuse::p1::faceuse_region(b, up).unwrap()
        == b.infinite_region()
    {
        mate
    } else {
        up
    };
    let p = face_properties(
        k,
        m,
        f,
        b.faceuse(fu).unwrap().orientation,
        accuracy,
        origin,
        estimated,
        0.0,
        PPF_ALL,
    )
    .unwrap();
    let mut integ = [0.0; 14];
    integ[0] = p.area.abs();
    integ[1] = p.volume;
    for (i, v) in p.moments[4..].iter().enumerate() {
        integ[2 + 3 * i] = v.x;
        integ[3 + 3 * i] = v.y;
        integ[4 + 3 * i] = v.z;
    }
    let (t24, t32) = (
        sphere_face_trim_area(k, b, f, 24, origin),
        sphere_face_trim_area(k, b, f, 32, origin),
    );
    // The trims' integrals have converged, and the volume by Stokes is the volume by Green.
    let scale = t32.gauss_bonnet.abs().max(1.0);
    assert!((t32.gauss_bonnet - t24.gauss_bonnet).abs() <= 1e-14 * scale);
    for (a, b) in t32.green_volume.iter().zip(t24.green_volume) {
        assert!((a - b).abs() <= 1e-14 * scale, "{f:?}: n=32 {a} n=24 {b}");
    }
    assert!(
        // the Stokes form cancels terms of order |c - origin| |S| ~ 1 (measured 8.8e-14)
        (t32.stokes_volume - t32.green_volume[0]).abs() <= 2e-13 * scale,
        "{f:?}: Stokes {} Green {}",
        t32.stokes_volume,
        t32.green_volume[0]
    );
    let mut trims = [0.0; 14];
    trims[0] = t32.gauss_bonnet;
    trims[1..].copy_from_slice(&t32.green_volume);
    (integ, trims)
}

/// `Fillet/box_w_slot.smb`: the `corpus.parity` volume and the
/// `blend.fillet_executive_p3` `source-file-box_w_slot-all-edges` mass properties (the
/// fillet is refused, so the body is the file as read). Only the eight sphere corners move.
/// The reference is each row with the corners' integrated shares replaced by their
/// shares over their own trims ([`sphere_face_trim_area`]: line integrals on the file's own
/// NURB and trims); the rest of each row (the other faces and the edge gap closures) is
/// the integrator's on both sides.
///
/// The ranking against that reference is ARGUED, not proved: its line integrals converge
/// (n = 24 against 32, Stokes against Green) but carry no remainder bound. A certified
/// bound is out of reach for the ranking: the rows differ by 1.4e-10 of the volume, and
/// the volume enclosure ([`enclose_volume`]) spends its cell budget at 2.2e-4 of it wide.
/// What is PROVEN is EQUAL (WIDE): both volume rows lie in that certified enclosure, which
/// is at most 2.3e-4 of the volume wide (asserted here); the mass rows are held EQUAL to the
/// body's certified enclosure by `goldens/tests/wr493_moved_rows_are_classified`.
#[test]
fn box_w_slot_volume_and_mass_are_those_their_trims_bound() {
    use solidcore::mass::integrator::properties::{
        brep_mass_properties_default, brep_volume_default,
    };
    use solidcore::query::brep::{
        calculate_bounding_box, calculate_tight_bounding_box, BoundingBoxOptions,
    };
    let name = "Fillet/box_w_slot.smb";
    let mut k = Kernel::default();
    let b = read(&mut k, name);
    let spheres: Vec<FaceId> = bl::faces(&b, None)
        .unwrap()
        .into_iter()
        .filter(|&f| {
            matches!(
                b.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::Sphere(_)
            )
        })
        .collect();
    assert_eq!(spheres.len(), 8);
    let size = |e: solidcore::geom::extent3d::Extent3d| {
        let s = e.size();
        let d = s.x.max(s.y).max(s.z);
        d * d / 100.0
    };

    // corpus.parity: brep_volume at 1e-6 about the box's middle
    let mut v = read(&mut k, name);
    let bbox = calculate_bounding_box(&k, &mut v, BoundingBoxOptions::default()).unwrap();
    let row = brep_volume_default(&k, &mut v, 1e-6).unwrap();
    assert_eq!(row, 0.680_748_106_606_711_7);
    let mut m = read(&mut k, name);
    let mut delta = 0.0;
    for &f in &spheres {
        let (integ, trims) = sphere_face_shares(&k, &b, &mut m, f, 1e-6, bbox.mid(), size(bbox));
        delta += integ[1] - trims[1];
    }
    let reference = row - delta;
    let old_row = 0.680_748_106_510_291_7;
    // EQUAL, certified: both rows in the volume's enclosure
    let mut ev = read(&mut k, name);
    let e = enclose_volume(&k, &mut ev, 1e-5 * row).unwrap();
    eprintln!(
        "{name}: volume enclosure [{}, {}], {:e} of the volume wide",
        e.lo,
        e.hi,
        (e.hi - e.lo) / row
    );
    // WIDE: the budget stops the enclosure at 2.2146644063900247e-4 of the volume (a
    // tighter one holding both rows passes too)
    let width = (e.hi - e.lo) / row;
    eprintln!("{name}: volume enclosure {width:e} of the volume wide");
    assert!(width < 2.3e-4, "volume enclosure {width:e} wide");
    for r in [row, old_row] {
        assert!(
            e.lo <= r && r <= e.hi,
            "volume {r} off [{}, {}]",
            e.lo,
            e.hi
        );
    }
    // ARGUED CLOSER: the new row is the reference to the bit, the old row 9.8e-11 off (the
    // Stokes/Green agreement above, 8 * 2e-13, is a cross-check of the reference, not a
    // bound on its error).
    assert!((row - reference).abs() * 10.0 < (old_row - reference).abs());
    assert!((row - reference).abs() <= 1e-11 * reference.abs());

    // The geometry is exact: integrated tightly, each corner's shares are those its trims
    // bound, so what remains of a row's distance from its reference is the quadrature at
    // the row's own accuracy.
    let mut m = read(&mut k, name);
    for &f in &spheres {
        let (integ, trims) = sphere_face_shares(&k, &b, &mut m, f, 1e-12, Vec3::ZERO, size(bbox));
        for (i, (a, r)) in integ.iter().zip(trims).enumerate() {
            assert!(
                (a - r).abs() <= 1e-14,
                "{f:?} share {i}: integrated {a} trims {r}"
            );
        }
    }

    // fillet_executive_p3: brep_mass_properties at 1e-4, density 1, about the origin
    let mut w = read(&mut k, name);
    let tight = calculate_tight_bounding_box(&k, &mut w).unwrap();
    let mid = tight.mid();
    let mp = brep_mass_properties_default(&k, &mut w, 1e-4, 1.0, Vec3::ZERO).unwrap();
    let flat = |area: f64, volume: f64, c: Vec3, i: Vec3, p: Vec3| {
        [
            area, volume, volume, c.x, c.y, c.z, i.x, i.y, i.z, p.x, p.y, p.z,
        ]
    };
    let new_row = [
        5.914390008549578,
        0.6807481066067114,
        0.6807481066067114,
        1.9999999999999853,
        -0.21949084150622125,
        0.19830878452084488,
        0.31582474680321304,
        2.796768143646739,
        3.0415735197486984,
        -0.029630896962770808,
        0.26999665917208526,
        -0.298835949545732,
    ];
    let old_row = [
        5.9143899129759,
        0.6807481065534877,
        0.6807481065534877,
        1.999999999999982,
        -0.21949084146911574,
        0.19830878452084486,
        0.3158247467306677,
        2.796768143424426,
        3.041573519459353,
        -0.02963089695544498,
        0.2699966591509754,
        -0.2988359494718497,
    ];
    assert_eq!(
        flat(
            mp.properties.area,
            mp.properties.volume,
            mp.centroid,
            mp.inertia,
            mp.products
        ),
        new_row
    );
    // the row's precise properties about `mid`, the corners swapped for their trims' shares
    let mut total = [0.0; 14];
    total[0] = mp.properties.area;
    total[1] = mp.properties.volume;
    for (i, v) in mp.properties.moments[4..].iter().enumerate() {
        total[2 + 3 * i] = v.x;
        total[3 + 3 * i] = v.y;
        total[4 + 3 * i] = v.z;
    }
    let mut m = read(&mut k, name);
    for &f in &spheres {
        let (integ, trims) = sphere_face_shares(&k, &b, &mut m, f, 1e-4, mid, size(tight));
        for ((t, a), r) in total.iter_mut().zip(integ).zip(trims) {
            *t -= a - r;
        }
    }
    // `brep_mass_properties`' moments about the origin, from those about `mid`
    let (volume, s) = (total[1], Vec3::new(total[2], total[3], total[4]));
    let (m6, m7) = (
        Vec3::new(total[8], total[9], total[10]),
        Vec3::new(total[11], total[12], total[13]),
    );
    let d = mid;
    let comp = |v: Vec3, i: usize| match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    };
    let shift = |a: usize, b: usize| {
        comp(d, a) * comp(s, b) + comp(d, b) * comp(s, a) + comp(d, a) * comp(d, b) * volume
    };
    let inertia = m7
        + Vec3::new(
            shift(1, 1) + shift(2, 2),
            shift(0, 0) + shift(2, 2),
            shift(0, 0) + shift(1, 1),
        );
    let products = m6 + Vec3::new(shift(1, 2), shift(2, 0), shift(0, 1));
    let reference = flat(
        total[0],
        volume,
        mid + s * (1.0 / volume),
        inertia,
        products,
    );
    for i in 0..12 {
        let (new_off, old_off) = (new_row[i] - reference[i], old_row[i] - reference[i]);
        // every component is within 1e-9 relative of the trims-bound reference
        assert!(
            new_off.abs() <= 1e-9 * reference[i].abs(),
            "mass[{i}]: new {} reference {}",
            new_row[i],
            reference[i]
        );
        // every other component CLOSER: the corners' trims cross their NURB's interior v
        // knot lines, and the integrator now ends its trim pieces there on every face
        // (before, only on a banded face, so cy, Pyz and Pxy were 7.3e-10, 7.1e-10
        // and 7.1e-10 relative off the reference against the old row's 1.7e-10, 2.5e-10
        // and 2.5e-10)
        if i == 5 {
            // cz moves by one ulp: `brep_mass_properties` takes cz = mid.z + S.z / mass,
            // and this run's S.z (the corners' z moment, summed in another order) gives
            // this run's cz over main's mass as over its own, so the ulp is S.z's, not the
            // volume's. One ulp is below this reference's own rounding, so cz is not ranked
            // here; `goldens/tests/wr493_moved_rows_are_classified` holds it EQUAL, within
            // the row's accuracy, to the body's certified enclosure (not WIDE)
            let sz = mp.properties.moments[4].z;
            assert_eq!(new_row[i].to_bits(), old_row[i].to_bits() + 1);
            assert_eq!((mid.z + sz / old_row[2]).to_bits(), new_row[i].to_bits());
            assert_eq!(
                (mid.z + sz / mp.properties.mass).to_bits(),
                new_row[i].to_bits()
            );
            continue;
        }
        assert!(
            new_off.abs() < old_off.abs(),
            "mass[{i}]: new off {new_off:e} old off {old_off:e}"
        );
    }
}
