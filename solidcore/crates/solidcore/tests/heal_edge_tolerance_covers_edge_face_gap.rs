//! The healer's tolerance fix (`Fix_TolSizes`) gives an edge a tolerance that covers its
//! edge/face gap.
//!
//! Upstream sets a fixed edge's tolerance to `gain * gap - def` (`SmTol::CalcEdgeZoneTol3d`),
//! which is under the gap itself for gaps below ten times the default. The check's edge/face
//! test (`SmBrep` Test #4) then refuses the edge it has just fixed. Two corpus files read with
//! healing show it: on `Fillet/sphere_plane.smb` the plane/sphere circle edge is 1.69e-5 from
//! the sphere with a tolerance of 1e-5. On `Solids/revolution_mold.smb` two edges have gaps of
//! 2.99e-5 and 9.25e-5, with tolerances of 2.29e-5 and 9.17e-5.
//!
//! The kept NURB of WR-493 also brings this out. The found sphere evaluates the file's own
//! UV trims, so they are no longer dropped and rebuilt with a grown tolerance.
//!
//! [`corpus_reads_change_only_the_covered_tolerances`] and
//! [`moved_fillet_rows_keep_main_geometry_and_mass`] replay the goldens rows the fix moved
//! against main 3a0f49b09's values (tables made by this file's functions on main).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::AttributeStore;
use solidcore::mass::enclosure::EnclosureStop;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::database_io::FileType;
use solidcore::topology::edgeuse::p1::max_edge_face_gap;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, Kernel};

/// Reads an upstream `.smb` test file (healing on, as `SmBrep::ReadFromFile`).
fn read(k: &mut Kernel, name: &str) -> Brep {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(name);
    let bytes = std::fs::read(&path).unwrap();
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
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b
}

/// Every loop edge use's edge/face gap is within its edge's tolerance, and the check
/// reports no edge/face gap. `named` are the edges (by index) the healer raised: each
/// has a use with a gap that is checked. `uses` is `(checked, all)`: exactly `checked`
/// of the B-rep's `all` edge uses have a gap (`max_edge_face_gap` is `Some`) and are
/// checked.
fn assert_gaps_covered(name: &str, named: &[u32], uses: (usize, usize)) {
    use solidcore::topology::EdgeId;
    let mut k = Kernel::default();
    let mut b = read(&mut k, name);
    let eus: Vec<_> = b.edgeuses().iter().map(|(id, _)| id).collect();
    let mut checked = 0;
    let mut checked_edges = Vec::new();
    for eu in eus {
        let Some(e) = b.edgeuse(eu).unwrap().edge else {
            continue;
        };
        let Some(gap) = max_edge_face_gap(&k, &mut b, eu, false).unwrap() else {
            continue;
        };
        let tol = b.edge(e).unwrap().tolerance;
        assert!(
            gap.gap_3d <= tol,
            "{name}: {eu:?} of {e:?}: gap {:e} over tolerance {tol:e}",
            gap.gap_3d
        );
        checked += 1;
        checked_edges.push(e);
    }
    assert_eq!(
        (checked, b.edgeuses().iter().count()),
        uses,
        "{name}: edge uses checked, of all"
    );
    for &i in named {
        let e = EdgeId::from_index(i);
        assert!(checked_edges.contains(&e), "{name}: {e:?} not checked");
    }
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let lines: Vec<_> = lines
        .iter()
        .filter(|l| l.contains("Edge/Surface gap"))
        .collect();
    assert!(lines.is_empty(), "{name}: {lines:?}");
}

/// Edge uses of `Fillet/sphere_plane.smb` with an edge/face gap (all of them), and all
/// its edge uses.
const USES_SPHERE_PLANE: (usize, usize) = (12, 12);
/// Edge uses of `Solids/revolution_mold.smb` with an edge/face gap (all of them: four
/// per edge, 4 x 88), and all its edge uses.
const USES_REVOLUTION_MOLD: (usize, usize) = (352, 352);

#[test]
fn sphere_plane_circle_edge_covers_its_gap() {
    assert_gaps_covered("Fillet/sphere_plane.smb", &[3], USES_SPHERE_PLANE);
}

#[test]
fn revolution_mold_edges_cover_their_gaps() {
    assert_gaps_covered(
        "Solids/revolution_mold.smb",
        &[26, 59, 60],
        USES_REVOLUTION_MOLD,
    );
}

/// The UV trim of the sphere's use of `Fillet/sphere_plane.smb`'s circle edge (edge 3),
/// built after the read: the file's trim fails `sm_CheckUVTrimCurve` and is dropped, so
/// `GetOrCreateUVTrimCurve` makes it. Returns the curve's interval ends and 65 points
/// along it, and the edge's tolerance.
fn sphere_plane_circle_trim(k: &Kernel, b: &mut Brep) -> (Vec<[u64; 2]>, f64) {
    use solidcore::topology::edgeuse::{p1 as eu1, p3 as eu3};
    use solidcore::topology::{EdgeId, FaceId};
    let e = EdgeId::from_index(3);
    let eus = b.edge(e).unwrap().edgeuses.clone();
    let eu = eus
        .into_iter()
        .find(|&eu| eu1::face(b, eu).unwrap() == Some(FaceId::from_index(0)))
        .expect("the sphere's use of the circle edge");
    eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)
        .unwrap()
        .status
        .unwrap();
    let trim = b.edgeuse(eu).unwrap().uv_trim_curve.clone().unwrap();
    let ivl = trim.natural_interval();
    let mut bits = vec![[ivl.min().to_bits(), ivl.max().to_bits()]];
    for i in 0..=64 {
        let p = trim
            .evaluate_point(k, ivl.evaluate(f64::from(i) / 64.0))
            .unwrap();
        bits.push([p.x.to_bits(), p.y.to_bits()]);
    }
    (bits, b.edge(e).unwrap().tolerance)
}

/// Raising an edge's tolerance to cover its gap does not loosen the UV trim fitted
/// later. On `Fillet/sphere_plane.smb` the healer raises the circle edge from the
/// default 1e-5 to 1.85e-5 (gain times its 1.69e-5 gap); the sphere's trim, made after
/// the read, is the trim made at the healer's tolerance without that raise (1e-5,
/// upstream's `CalcEdgeZoneTol3d` for the gap), bit for bit. A trim fitted at the raised
/// tolerance bounded a cap 3.9e-7 from the one the edge bounds, against 4.7e-8.
#[test]
fn sphere_plane_trim_fit_does_not_grow_with_the_gap_cover() {
    use solidcore::topology::edgeuse::p1 as eu1;
    use solidcore::topology::{EdgeId, FaceId};
    let mut k = Kernel::default();
    let mut b = read(&mut k, "Fillet/sphere_plane.smb");
    let e = EdgeId::from_index(3);
    let raised = b.edge(e).unwrap().tolerance;
    assert!(raised > 1.0e-5, "the circle edge is raised: {raised:e}");
    let (healed, tol) = sphere_plane_circle_trim(&k, &mut b);

    // the same read with the circle edge at the tolerance before the gap cover, and the
    // sphere's trim made again there
    let mut k0 = Kernel::default();
    let mut b0 = read(&mut k0, "Fillet/sphere_plane.smb");
    for eu in b0.edge(e).unwrap().edgeuses.clone() {
        if eu1::face(&b0, eu).unwrap() == Some(FaceId::from_index(0)) {
            eu1::set_uv_trim_curve(&mut b0, eu, eu1::UvTrimCurveArg::Null, 0.0, true, None)
                .unwrap();
        }
    }
    b0.edge_mut(e).unwrap().tolerance = 1.0e-5;
    let (unraised, tol0) = sphere_plane_circle_trim(&k0, &mut b0);
    // (both trims raise the edge to twice their distance to the sphere, the same)
    assert_eq!(tol.to_bits(), tol0.to_bits(), "{tol:e} {tol0:e}");
    assert_eq!(healed, unraised);
}

/// The fillet `blend.fillet_executive_p3` `source-file-corner_box-edge13` makes on
/// `Fillet/corner_box.smb` (edge 13, radius 0.1, circular section, G1, thumbweight 1),
/// with edge 13 at its tolerance as read (`lower` false) or at the tolerance before the
/// gap cover (`lower` true, the default 1e-5). Returns the vertex, edge and face counts
/// and the bits of the area, volume and centroid at the golden's accuracy (1e-4).
fn corner_box_edge13_fillet(lower: bool) -> ([usize; 3], [u64; 5]) {
    use solidcore::blend::fillet_executive::api::fillet_edges;
    use solidcore::mass::integrator::properties::brep_mass_properties_default;
    use solidcore::math::Vec3;
    use solidcore::topology::brep::p1 as bl;
    let mut k = Kernel::default();
    let mut b = read(&mut k, "Fillet/corner_box.smb");
    let e = bl::edges(&b, None).unwrap()[13];
    let raised = b.edge(e).unwrap().tolerance;
    // (the healer raised it to cover its 1.59e-5 edge/face gap)
    assert!(raised > 1.7e-5, "edge 13 is raised: {raised:e}");
    if lower {
        b.edge_mut(e).unwrap().tolerance = 1.0e-5;
    }
    fillet_edges(&mut k, &mut b, &[e], 0.1, 1, 1, 1.0).unwrap();
    let counts = [
        bl::vertices(&b, None).unwrap().len(),
        bl::edges(&b, None).unwrap().len(),
        bl::faces(&b, None).unwrap().len(),
    ];
    let m = brep_mass_properties_default(&k, &mut b, 1e-4, 1.0, Vec3::ZERO).unwrap();
    let bits = [
        m.properties.area.to_bits(),
        m.properties.volume.to_bits(),
        m.centroid.x.to_bits(),
        m.centroid.y.to_bits(),
        m.centroid.z.to_bits(),
    ];
    (counts, bits)
}

/// Raising an edge's tolerance to cover its gap does not loosen the fillet along it. On
/// `Fillet/corner_box.smb` the healer raises edge 13 from the default 1e-5 to 1.745e-5
/// (gain times its 1.59e-5 gap). The fillet solver approximates at `TOL_FACTOR` times the
/// edge's fit tolerance, which is the 1e-5 the edge had before the raise, so the round is
/// the round made with the edge at 1e-5, bit for bit. Approximated at 4 times the raised
/// tolerance, the round's volume moved 2.3e-9 relative.
#[test]
fn corner_box_fillet_does_not_loosen_with_the_gap_cover() {
    let (counts, bits) = corner_box_edge13_fillet(false);
    let (counts0, bits0) = corner_box_edge13_fillet(true);
    assert_eq!(counts, [12, 18, 8]);
    assert_eq!(counts, counts0);
    let values = bits.map(f64::from_bits);
    let values0 = bits0.map(f64::from_bits);
    assert_eq!(bits, bits0, "{values:?} {values0:?}");
}

// ---- the corpus replay: every file the moved goldens read -------------------------------

/// The `corpus.parity` runner's object keys (its first B-rep).
fn corpus_key(e: solidcore::EntityRef) -> solidcore::geom::object::ObjectRef {
    use solidcore::geom::attribute::ObjectKey;
    use solidcore::EntityRef;
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
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
    solidcore::geom::object::ObjectRef {
        type_code,
        key: ObjectKey((1u64 << 36) | ((kind + 1) << 32) | u64::from(i)),
    }
}

/// Reads a corpus file as `corpus.parity` does (`rebuild` false), or with the UV trims
/// rebuilt (`bRebuildUVTrimCurves`, `topology.face_p3`'s `_rebuild` rows).
fn read_corpus(k: &mut Kernel, name: &str, rebuild: bool) -> Brep {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles")
        .join(name);
    let bytes = std::fs::read(&path).unwrap();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &corpus_key,
        records: &mut records,
        next_key: 1 << 62,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, rebuild, &mut env).unwrap();
    b
}

/// FNV-1a over the bits of the values fed to it.
struct Digest(u64);

impl Digest {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn u64(&mut self, v: u64) {
        for byte in v.to_le_bytes() {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }
}

/// Feeds an edge use's UV trim: its interval and 17 points along it, each with the
/// face's surface point there (`u64::MAX` for no trim, an error, or no surface).
fn digest_trim(d: &mut Digest, k: &Kernel, b: &Brep, eu: solidcore::topology::EdgeuseId) {
    use solidcore::math::Point2;
    use solidcore::topology::edgeuse::p1 as eu1;
    let Some(trim) = b.edgeuse(eu).unwrap().uv_trim_curve.clone() else {
        d.u64(u64::MAX);
        return;
    };
    let ivl = trim.natural_interval();
    d.f64(ivl.min());
    d.f64(ivl.max());
    let surface = eu1::face(b, eu)
        .unwrap()
        .and_then(|f| b.face(f).unwrap().surface.clone());
    for i in 0..=16 {
        let Ok(p) = trim.evaluate_point(k, ivl.evaluate(f64::from(i) / 16.0)) else {
            d.u64(u64::MAX);
            continue;
        };
        d.f64(p.x);
        d.f64(p.y);
        match surface
            .as_ref()
            .map(|s| s.evaluate_point(k, Point2::new(p.x, p.y)))
        {
            Some(Ok(q)) => [q.x, q.y, q.z].into_iter().for_each(|c| d.f64(c)),
            _ => d.u64(u64::MAX),
        }
    }
}

/// Feeds an edge's curve: its interval and 17 points along it.
fn digest_curve(d: &mut Digest, k: &Kernel, b: &Brep, e: solidcore::topology::EdgeId) {
    let r = b.edge(e).unwrap();
    let Some(c) = r.curve.as_ref() else {
        d.u64(u64::MAX);
        return;
    };
    let ivl = r.interval;
    d.f64(ivl.min());
    d.f64(ivl.max());
    for i in 0..=16 {
        match c.evaluate_point(k, ivl.evaluate(f64::from(i) / 16.0)) {
            Ok(p) => [p.x, p.y, p.z].into_iter().for_each(|v| d.f64(v)),
            Err(_) => d.u64(u64::MAX),
        }
    }
}

/// The largest edge/face gap of an edge's uses (0 with none).
fn edge_gap(k: &Kernel, b: &mut Brep, e: solidcore::topology::EdgeId) -> f64 {
    let mut gap = 0.0f64;
    for eu in b.edge(e).unwrap().edgeuses.clone() {
        if let Some(g) = max_edge_face_gap(k, b, eu, false).unwrap() {
            gap = gap.max(g.gap_3d);
        }
    }
    gap
}

/// The entries of `table` for `name`.
fn entries(table: &[(&str, &'static [(u32, u64)])], name: &str) -> &'static [(u32, u64)] {
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(&[], |(_, t)| *t)
}

/// Replays one read of `name` against main 3a0f49b09, recording every disagreement in
/// `failures`:
/// - each edge in `raised` (index, main's tolerance bits) had main's tolerance under its
///   edge/face gap `g`, and now has `zone_tol3d(gain * g)`, which covers `g`;
/// - each vertex in `moved` (index, main's bits) rose above main's tolerance to its
///   largest edge tolerance (the healer's vertex step, `CalcVertexZoneTol3d`, with the
///   raised edges as input);
/// - every edge covers its gap.
///
/// Returns the digest of the read with main's tolerances put back on those entries:
/// face, edge and vertex tolerances, edge/face gaps, edge curves and the file's UV trims
/// with the surface points along them. With `lazy`, also the digest of the UV trims
/// `GetOrCreateUVTrimCurve` makes afterwards on the read as it is (raised).
fn replay(
    name: &str,
    rebuild: bool,
    raised: &[(u32, u64)],
    moved: &[(u32, u64)],
    lazy: bool,
    failures: &mut Vec<String>,
) -> (u64, u64) {
    use solidcore::geom::tol::{gap_gain, zone_tol3d};
    use solidcore::topology::edgeuse::p3 as eu3;
    use solidcore::topology::{vertex, EdgeId, VertexId};
    let mut k = Kernel::default();
    let mut b = read_corpus(&mut k, name, rebuild);
    let gain = gap_gain(&k);
    let mut d = Digest::new();
    for (_, f) in b.faces().iter() {
        d.f64(f.tolerance);
    }
    let edges: Vec<EdgeId> = b.edges().iter().map(|(id, _)| id).collect();
    let mut seen = 0;
    for &e in &edges {
        let tol = b.edge(e).unwrap().tolerance;
        let g = edge_gap(&k, &mut b, e);
        if tol < g {
            failures.push(format!(
                "{name}: {e:?} tolerance {tol:e} under its gap {g:e}"
            ));
        }
        match raised.iter().find(|&&(i, _)| i == e.index()) {
            Some(&(_, main)) => {
                seen += 1;
                let main = f64::from_bits(main);
                let cover = zone_tol3d(&k, gain * g);
                if !(main < g && tol.to_bits() == cover.to_bits()) {
                    failures.push(format!(
                        "{name}: {e:?} main {main:e}, gap {g:e}, now {tol:e}, cover {cover:e}"
                    ));
                }
                d.f64(main);
            }
            None => d.f64(tol),
        }
        d.f64(g);
        digest_curve(&mut d, &k, &b, e);
    }
    if seen != raised.len() {
        failures.push(format!("{name}: {seen} of {} raised edges", raised.len()));
    }
    let vertices: Vec<VertexId> = b.vertices().iter().map(|(id, _)| id).collect();
    let mut seen = 0;
    for &v in &vertices {
        let tol = b.vertex(v).unwrap().tolerance;
        match moved.iter().find(|&&(i, _)| i == v.index()) {
            Some(&(_, main)) => {
                seen += 1;
                let main = f64::from_bits(main);
                let largest = vertex::edges(&b, v, None)
                    .unwrap()
                    .into_iter()
                    .map(|e| b.edge(e).unwrap().tolerance)
                    .fold(0.0f64, f64::max);
                if !(main < tol && tol.to_bits() == largest.to_bits()) {
                    failures.push(format!(
                        "{name}: {v:?} main {main:e}, now {tol:e}, largest edge {largest:e}"
                    ));
                }
                d.f64(main);
            }
            None => d.f64(tol),
        }
    }
    if seen != moved.len() {
        failures.push(format!("{name}: {seen} of {} moved vertices", moved.len()));
    }
    let eus: Vec<_> = b.edgeuses().iter().map(|(id, _)| id).collect();
    for &eu in &eus {
        digest_trim(&mut d, &k, &b, eu);
    }
    let mut trims = Digest::new();
    if lazy {
        for &eu in &eus {
            if b.edgeuse(eu).unwrap().edge.is_none() {
                continue;
            }
            let made = eu3::get_or_create_uv_trim_curve(&k, &mut b, eu, false, false, true)
                .map(|r| r.status.is_ok());
            trims.u64(u64::from(matches!(made, Ok(true))));
            digest_trim(&mut trims, &k, &b, eu);
        }
    }
    (d.0, trims.0)
}

/// Tolerances to set: (entity index, bits).
type Tolerances<'a> = &'a [(u32, u64)];

/// `corpus.parity`'s area and volume of a corpus file (its `measure`, at accuracy 1e-6),
/// with the entries of `lower` (edges, vertices: index, bits) set back to those
/// tolerances first.
fn corpus_mass(name: &str, lower: Option<(Tolerances<'_>, Tolerances<'_>)>) -> (f64, f64) {
    use solidcore::geom::attribute::ObjectKey;
    use solidcore::mass::integrator::properties::{
        brep_volume, face_properties, PropertyAttributes,
    };
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::brep::p1 as bl;
    use solidcore::topology::brep::p2::{is_manifold_solid, is_sheet};
    use solidcore::topology::face::p2::{self as fl, PPF_AREA};
    use solidcore::topology::{EdgeId, VertexId};
    let mut k = Kernel::default();
    let mut b = read_corpus(&mut k, name, false);
    if let Some((edges, vertices)) = lower {
        for &(i, t) in edges {
            b.edge_mut(EdgeId::from_index(i)).unwrap().tolerance = f64::from_bits(t);
        }
        for &(i, t) in vertices {
            b.vertex_mut(VertexId::from_index(i)).unwrap().tolerance = f64::from_bits(t);
        }
    }
    // the runner's order: counts, kind and shells, then the area and the volume
    let faces = bl::faces(&b, None).unwrap();
    for f in &faces {
        fl::loops(&b, *f, false, None).unwrap();
    }
    let solid = is_manifold_solid(&b).unwrap();
    if !solid {
        is_sheet(&mut k, &b).unwrap();
    }
    bl::shells(&mut k, &b, None).unwrap();
    let mut area = 0.0;
    for f in &faces {
        let bbox = solidcore::query::face::p1::calculate_bounding_box(&k, &b, *f, false).unwrap();
        let s = bbox.size();
        let d = s.x.max(s.y).max(s.z);
        let p = face_properties(
            &k,
            &mut b,
            *f,
            OrientType::Same,
            1e-6,
            bbox.mid(),
            d * d / 100.0,
            0.0,
            PPF_AREA,
        )
        .unwrap();
        area += p.area.abs();
    }
    let mut volume = f64::NAN;
    if solid {
        let store = AttributeStore::new();
        let key = |_| ObjectKey(0);
        let attributes = PropertyAttributes {
            store: &store,
            key_of: &key,
        };
        volume = brep_volume(&k, &mut b, 1e-6, &attributes).unwrap();
    }
    (area, volume)
}

/// The upstream test files the moved goldens rows read (`topology.brep_p10` `read_*`,
/// `topology.face_p3` and `topology.edgeuse_p3` `own_reader_*`, `heal.stitch_p1`/`_p2`,
/// `blend.fillet_removal`, `blend.feature_executive`, `blend.fillet_executive_p3`, and the
/// 26 `corpus.parity` rows).
///
/// Main 3a0f49b09's values per file, made by this file's functions on main: the read's
/// digest, the lazily made UV trims' digest, the rebuilding read's digest, and the
/// bits of `corpus.parity`'s area and volume.
const MAIN: [(&str, [u64; 5]); 28] = [
    (
        "Fillet/blade_hub_1.smb",
        [
            0xda2e0b563fa35862,
            0x410172437ab22187,
            0x41949589187dc360,
            0x40c8bf351a4d4317,
            0x40cb1472e2daad8a,
        ],
    ),
    (
        "Fillet/blade_hub_2.smb",
        [
            0x4830aab7f1df9f45,
            0x8ce78a827602a435,
            0x9d95007e0e59fd13,
            0x40c6f745308c0c73,
            0x40c837c714dc148a,
        ],
    ),
    (
        "Fillet/blade_hub_3.smb",
        [
            0x239ea37fe6e8415a,
            0x3a137639560123dc,
            0x19b6267d7bab4376,
            0x40c94ce71acf47de,
            0x40c96aa73761feba,
        ],
    ),
    (
        "Fillet/bottle.smb",
        [
            0x569b0994d89a8785,
            0x20f9e5358be089e4,
            0x56d4f90e1a00266f,
            0x40408f20e135de78,
            0x40262233c6fff91f,
        ],
    ),
    (
        "Fillet/box_extrusion2.smb",
        [
            0xb5359aa30f9be919,
            0xa92d78f82fd63550,
            0x73843118350f3240,
            0x409f327abf0704e1,
            0x40adb03a50d6ac5b,
        ],
    ),
    (
        "Fillet/case_ext.smb",
        [
            0x144323840f389bc5,
            0xbbbd7dd4306bf8db,
            0xef701738a4b8846c,
            0x408895fdf62968f7,
            0x7ff8000000000000,
        ],
    ),
    (
        "Fillet/case_int.smb",
        [
            0x9e30445d19b63b9,
            0xce34e7dca4d3659d,
            0xcdd53dc07ba274c0,
            0x4078b5841d201b24,
            0x7ff8000000000000,
        ],
    ),
    (
        "Fillet/cliff_1.smb",
        [
            0xda2e0b563fa35862,
            0x410172437ab22187,
            0x41949589187dc360,
            0x40c8bf351a4d4317,
            0x40cb1472e2daad8a,
        ],
    ),
    (
        "Fillet/cliff_2.smb",
        [
            0x4830aab7f1df9f45,
            0x8ce78a827602a435,
            0x9d95007e0e59fd13,
            0x40c6f745308c0c73,
            0x40c837c714dc148a,
        ],
    ),
    (
        "Fillet/corner_box.smb",
        [
            0x699606f650437b1a,
            0xc073140317deae8e,
            0x47a83b95cc5e0446,
            0x4076c3ed2f3a2428,
            0x407c82bd88190ff0,
        ],
    ),
    (
        "Fillet/cyl_cyl_2191.smb",
        [
            0x828b24a619a790a1,
            0xee88c4bf1729e813,
            0xac041466db068115,
            0x4094358b547eda4d,
            0x40a751772349d47b,
        ],
    ),
    (
        "Fillet/cyl_cyl_shell.smb",
        [
            0xad51b1c9877aaf32,
            0x923c99c62ac0ee91,
            0xd47a38435c9b143e,
            0x4093cebda7a891ea,
            0x7ff8000000000000,
        ],
    ),
    (
        "Fillet/nurb_box1.smb",
        [
            0xdcab9ac6acfa5ef0,
            0x31b192a69dd3a411,
            0x50773cc6f6b54207,
            0x40992d1da43ecaaa,
            0x40adb597b83279dd,
        ],
    ),
    (
        "Fillet/open_shell.smb",
        [
            0x782000078cc156ac,
            0x3c0be9627c8fa0c0,
            0x8111b6b11b09433a,
            0x40c223453310b0b4,
            0x7ff8000000000000,
        ],
    ),
    (
        "Fillet/shell_fillet2.smb",
        [
            0xc883abda5a4672f7,
            0xbef2111c0d616bcc,
            0xe9468e03f3395efe,
            0x405664215d2967d8,
            0x7ff8000000000000,
        ],
    ),
    (
        "Fillet/sphere_dif_box.smb",
        [
            0x8f07277297df6078,
            0xcd50069fef891ecf,
            0x67b61d59d84afc0d,
            0x4085e633bce84e1d,
            0x7ff8000000000000,
        ],
    ),
    (
        "Fillet/sphere_plane.smb",
        [
            0xb3a46ba2e4d3817b,
            0x894d776ca5ba209d,
            0xfea47f8f2838bbe2,
            0x40735d27cb8cc7eb,
            0x7ff8000000000000,
        ],
    ),
    (
        "Fillet/test1.smb",
        [
            0xa7a5c3c9426e5f1b,
            0x1f46996340f01656,
            0x2732c82601be613b,
            0x40821911bbfe80e6,
            0x40895b346dbc001b,
        ],
    ),
    (
        "Fillet/test8.smb",
        [
            0x2da444e8b44697e7,
            0xd8145f250145f285,
            0xdef35e04b22f01d7,
            0x408276d5e8c514e0,
            0x7ff8000000000000,
        ],
    ),
    (
        "Fillet/torii_fillet.smb",
        [
            0x8bd41358f6bbd5b2,
            0x98ba90ffef54b4ca,
            0xa397712e89e2cc3e,
            0x409a06f0bf7e58e7,
            0x40aec2e2904b54c5,
        ],
    ),
    (
        "Fillet/torii_unioned.smb",
        [
            0x8f8d666dbaaa5216,
            0x25216f9f503e46ec,
            0x6b6ac489310cc557,
            0x40b0edf5e26557ab,
            0x40c164a29f25822d,
        ],
    ),
    (
        "Fillet/vase1.smb",
        [
            0x6c2688581ef72501,
            0x934916761febf75d,
            0x629b7badfc0138a6,
            0x40ff8cfbe4b56c96,
            0x4130d86087906c9b,
        ],
    ),
    (
        "Fillet/vase2.smb",
        [
            0x693763053bd85a82,
            0xf711c3cab635d99c,
            0x8b060805692b6a3f,
            0x41001932fe19de87,
            0x4130d95b0f6cce4f,
        ],
    ),
    (
        "Fillet/vase3.smb",
        [
            0x184b72dd9f60fe08,
            0x68482310d17f07be,
            0xbfc5158946937f01,
            0x40ff28e1cf641cd3,
            0x4130d6e013e89aee,
        ],
    ),
    (
        "Shell/ShellBrep_983.smb",
        [
            0x5ecbece2adaae2dd,
            0xe712bce78fa09f8,
            0x1294ca4cd17be661,
            0x40875cb11c471695,
            0x4091346faf421ea4,
        ],
    ),
    (
        "Solids/fillet_ts.smb",
        [
            0x4a518a2b79a61d37,
            0xdc7bcb6913cc1499,
            0x902514eb874e694,
            0x4082ea0519f2d264,
            0x7ff8000000000000,
        ],
    ),
    (
        "Solids/revolution2.smb",
        [
            0xc244bc1a2ccee347,
            0x14f0ee52985b5a13,
            0x6fbf2b74c3839734,
            0x40a3f9c095be4e22,
            0x40a865973227fce6,
        ],
    ),
    (
        "Solids/revolution_mold.smb",
        [
            0x462faca60ad813f0,
            0x88a9396a602fe93c,
            0x692f780f5f318bf3,
            0x40d2a6a844e34477,
            0x40f8a3124b2291b7,
        ],
    ),
];

/// The healer's gap cover changes, on the 28 files the moved goldens rows read, exactly
/// the tolerances it means to and nothing the mass or the trims see:
/// - 206 edges whose tolerance on main was under their edge/face gap `g` now have
///   `zone_tol3d(gain * g)` (3 more on the rebuilding read); every other edge keeps
///   main's tolerance bit for bit, and every edge covers its gap;
/// - 296 vertices rise to their largest edge tolerance (1 on the rebuilding read), the
///   healer's vertex step on the raised edges; every other vertex keeps main's;
/// - face tolerances, edge/face gaps, edge curves, the file's UV trims (and the rebuilt
///   ones), and every UV trim `GetOrCreateUVTrimCurve` makes after the read, are main's
///   bit for bit: trims fit at the edge's fit tolerance (`edgeuse::p3::fit_tolerance`),
///   not at the raised one;
/// - `corpus.parity`'s areas are main's bit for bit; its volumes are main's bit for bit
///   with the moved tolerances set back to main's, and as read differ only on
///   `Fillet/corner_box.smb` and `Fillet/test1.smb`, by 1 ulp: the volume integrates
///   about the middle of the B-rep's box, which is grown by the edge and vertex
///   tolerances (`CalculateBoundingBox`, `bExpandByZoneTol3d`). Each pair is classified
///   against a certified enclosure `[lo, hi]` of the exact volume
///   (`mass::enclosure::enclose_volume`, which reads no edge tolerance), asked
///   `1e-9 * |V|` wide, the goldens' relative tolerance. On `test1` it reaches that width
///   and both values lie below `lo`, so the larger, this branch's, is closer to the exact
///   volume. On `corner_box` it stops at its cell budget, wider than asked, holding both
///   values: neither can be told farther.
#[test]
fn corpus_reads_change_only_the_covered_tolerances() {
    let mut failures = Vec::new();
    let mut raised = 0;
    let mut moved = 0;
    let mut moved_volumes = Vec::new();
    let mut volume_classes = Vec::new();
    for (name, [read_main, lazy_main, rebuild_main, area_main, volume_main]) in MAIN {
        let edges = entries(RAISED_EDGES, name);
        let vertices = entries(MOVED_VERTICES, name);
        raised += edges.len();
        moved += vertices.len();
        let (read_digest, lazy_digest) = replay(name, false, edges, vertices, true, &mut failures);
        let rebuild_edges = entries(REBUILD_RAISED_EDGES, name);
        let rebuild_vertices = entries(REBUILD_MOVED_VERTICES, name);
        let (rebuild_digest, _) = replay(
            name,
            true,
            rebuild_edges,
            rebuild_vertices,
            false,
            &mut failures,
        );
        for (what, now, main) in [
            ("read", read_digest, read_main),
            ("lazy trims", lazy_digest, lazy_main),
            ("rebuilding read", rebuild_digest, rebuild_main),
        ] {
            if now != main {
                failures.push(format!("{name}: {what} digest {now:#x}, main {main:#x}"));
            }
        }
        let (area, volume) = corpus_mass(name, None);
        let (area0, volume0) = corpus_mass(name, Some((edges, vertices)));
        if area.to_bits() != area_main || area0.to_bits() != area_main {
            failures.push(format!(
                "{name}: area {:#x} / lowered {:#x}, main {area_main:#x}",
                area.to_bits(),
                area0.to_bits()
            ));
        }
        if volume0.to_bits() != volume_main {
            failures.push(format!(
                "{name}: lowered volume {:#x}, main {volume_main:#x}",
                volume0.to_bits()
            ));
        }
        if volume.to_bits() != volume_main {
            // (classified against a certified enclosure of the exact volume, asked
            // 1e-9 * |V| wide, the goldens' relative tolerance)
            let main = f64::from_bits(volume_main);
            let mut k = Kernel::default();
            let mut b = read_corpus(&mut k, name, false);
            let asked = 1e-9 * main.abs();
            let e = solidcore::mass::enclosure::enclose_volume(&k, &mut b, asked).unwrap();
            let class = if e.lo <= volume.min(main) && volume.max(main) <= e.hi {
                "both enclosed"
            } else if volume.max(main) < e.lo {
                if volume > main {
                    "closer"
                } else {
                    "farther"
                }
            } else if volume.min(main) > e.hi {
                if volume < main {
                    "closer"
                } else {
                    "farther"
                }
            } else {
                "straddles"
            };
            volume_classes.push((name, class, e.lo, e.hi, asked, e.stop));
            moved_volumes.push(name);
        }
    }
    assert_eq!((raised, moved), (206, 296));
    assert_eq!(
        moved_volumes,
        ["Fillet/corner_box.smb", "Fillet/test1.smb"],
        "{failures:#?}"
    );
    let classes: Vec<_> = volume_classes.iter().map(|&(n, c, ..)| (n, c)).collect();
    assert_eq!(
        classes,
        [
            ("Fillet/corner_box.smb", "both enclosed"),
            ("Fillet/test1.smb", "closer")
        ],
        "{volume_classes:?}"
    );
    // (the doc's claims on the widths, against the asked width rather than as printed
    // digits, which move with any change to the enclosure's cells: `test1`'s enclosure is
    // as narrow as asked, so its "closer" is decided within the goldens' relative
    // tolerance; `corner_box`'s spent its cell budget wider than asked, so "both
    // enclosed" is all it can say)
    let widths: Vec<_> = volume_classes
        .iter()
        .map(|&(n, _, lo, hi, asked, stop)| (n, stop, hi - lo <= asked))
        .collect();
    assert_eq!(
        widths,
        [
            ("Fillet/corner_box.smb", EnclosureStop::Budget, false),
            ("Fillet/test1.smb", EnclosureStop::Reached, true)
        ],
        "{volume_classes:?}"
    );
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Edges whose tolerance the read raises: per file, (edge index, main's tolerance bits).
const RAISED_EDGES: &[(&str, &[(u32, u64)])] = &[
    (
        "Fillet/blade_hub_1.smb",
        &[
            (7, 0x3f01692145479eec),
            (8, 0x3eff66a320d8900e),
            (10, 0x3ee4f8b588e368f1),
            (12, 0x3f0605b5bb5b6f28),
            (14, 0x3f05dd7bc5ff2249),
            (19, 0x3ee78c3f0adde333),
            (23, 0x3ef790d277f4413a),
            (25, 0x3ee4f8b588e368f1),
            (26, 0x3efd080a6560090c),
            (28, 0x3efea239655549e0),
            (38, 0x3ef36b20585d0708),
            (40, 0x3ef8317461789916),
            (41, 0x3ee60ce2b1a06437),
            (43, 0x3ef117adfa6f1844),
            (44, 0x3ee4f8b588e368f1),
            (46, 0x3ee4f8b588e368f1),
            (50, 0x3ef8ca044abf3700),
            (51, 0x3ef1ac7d364d4b7a),
            (62, 0x3ef1ca6e08a3bf12),
            (63, 0x3efddc3d1dc7a4e2),
            (65, 0x3ee4f8b588e368f1),
            (66, 0x3f019ac7168d8b08),
            (77, 0x3ef7cef674fe7de8),
            (78, 0x3f058ab2a27e852a),
            (80, 0x3f0681559af75019),
            (88, 0x3f0593c631192351),
            (90, 0x3f055dbe9982525a),
            (94, 0x3efc5e6802072c02),
            (96, 0x3efc8873887a9c26),
            (97, 0x3ee4f8b588e368f1),
            (99, 0x3ee73455c085ee45),
            (109, 0x3efd7c57b79f3440),
            (111, 0x3efd66865e51b1e6),
        ],
    ),
    (
        "Fillet/blade_hub_2.smb",
        &[
            (6, 0x3ef88c6ebc09888c),
            (8, 0x3ee65f938d3b1fe5),
            (10, 0x3ef5a5706b083a88),
            (11, 0x3ef9c86a69ae2668),
            (12, 0x3f047175a95ecac1),
            (14, 0x3ee4f8b588e368f1),
            (16, 0x3f04146c84867910),
            (18, 0x3f035086b680755b),
            (23, 0x3efca7200b006e6a),
            (25, 0x3ee4f8b588e368f1),
            (27, 0x3efcca5a664286c6),
            (29, 0x3ee4f8b588e368f1),
            (30, 0x3efeb7550dab1372),
            (32, 0x3f0094f2d986b7c7),
            (42, 0x3ef31abd76112496),
            (44, 0x3ef8394a57e771d6),
            (45, 0x3ee6893fc834eacf),
            (47, 0x3efb81caef036832),
            (48, 0x3ee4f8b588e368f1),
            (57, 0x3eec483f0446c9d9),
            (58, 0x3eee36bdb356112f),
            (69, 0x3ef1af8d94767428),
            (70, 0x3efc56449f562902),
            (72, 0x3ee4f8b588e368f1),
            (73, 0x3ef4ab31dff2773e),
        ],
    ),
    (
        "Fillet/blade_hub_3.smb",
        &[
            (2, 0x3efe072649c12f8a),
            (4, 0x3ef1c82c62215478),
            (6, 0x3f018be1ea26ed0c),
            (7, 0x3f02d056320226af),
            (8, 0x3eff11aac0388dec),
            (10, 0x3ee4f8b588e368f1),
            (12, 0x3f03e4b41cfd7adf),
            (14, 0x3f003b350e31db57),
            (22, 0x3eecde04b7296715),
            (25, 0x3ee5d2657da57e53),
            (26, 0x3ef598890406da58),
            (28, 0x3efecd714a9875de),
            (29, 0x3efc37895c10696e),
            (40, 0x3ef65a7ca3529bd4),
            (41, 0x3efd3dd339049e7c),
            (43, 0x3ee8ed7cfe1d49bb),
            (46, 0x3ef076601fff34f8),
            (48, 0x3ee4f8b588e368f1),
            (50, 0x3ef26e8e08bc5ad6),
            (51, 0x3ef1e0e391c37636),
            (62, 0x3f02638eabe6072f),
            (63, 0x3f08c696afe9165e),
            (65, 0x3ee4f8b588e368f1),
            (66, 0x3eeb22288b52bb8d),
            (74, 0x3f03bec470741e09),
            (75, 0x3efa421f505b82e8),
            (76, 0x3f00d260b71298fc),
            (77, 0x3ef041a02bf383c0),
            (78, 0x3f063b36f2d5c362),
            (80, 0x3f0185e471b3583a),
            (88, 0x3f05d464ef083cb0),
            (90, 0x3f05b8a3e1f41a2d),
            (100, 0x3ee5864fc3cb858b),
            (102, 0x3ee56e2629cafe7f),
            (103, 0x3efe0d1812102dc6),
            (106, 0x3ee4f8b588e368f1),
            (108, 0x3ee4f8b588e368f1),
            (118, 0x3efbc005fab09f56),
            (120, 0x3efbae8c8906048e),
        ],
    ),
    (
        "Fillet/bottle.smb",
        &[
            (25, 0x3ee942308b035f45),
            (28, 0x3eea1c3f376a8589),
            (33, 0x3ee8ecd2aa0b69db),
            (34, 0x3f1085293e8cbe1c),
            (35, 0x3f0de04ffc8eb432),
            (37, 0x3f171094c43241a1),
        ],
    ),
    ("Fillet/box_extrusion2.smb", &[(14, 0x3f0ca9167773c8bc)]),
    ("Fillet/case_ext.smb", &[(0, 0x3f0da07639b836bc)]),
    ("Fillet/case_int.smb", &[(0, 0x3f0da07639b836bc)]),
    (
        "Fillet/cliff_1.smb",
        &[
            (7, 0x3f01692145479eec),
            (8, 0x3eff66a320d8900e),
            (10, 0x3ee4f8b588e368f1),
            (12, 0x3f0605b5bb5b6f28),
            (14, 0x3f05dd7bc5ff2249),
            (19, 0x3ee78c3f0adde333),
            (23, 0x3ef790d277f4413a),
            (25, 0x3ee4f8b588e368f1),
            (26, 0x3efd080a6560090c),
            (28, 0x3efea239655549e0),
            (38, 0x3ef36b20585d0708),
            (40, 0x3ef8317461789916),
            (41, 0x3ee60ce2b1a06437),
            (43, 0x3ef117adfa6f1844),
            (44, 0x3ee4f8b588e368f1),
            (46, 0x3ee4f8b588e368f1),
            (50, 0x3ef8ca044abf3700),
            (51, 0x3ef1ac7d364d4b7a),
            (62, 0x3ef1ca6e08a3bf12),
            (63, 0x3efddc3d1dc7a4e2),
            (65, 0x3ee4f8b588e368f1),
            (66, 0x3f019ac7168d8b08),
            (77, 0x3ef7cef674fe7de8),
            (78, 0x3f058ab2a27e852a),
            (80, 0x3f0681559af75019),
            (88, 0x3f0593c631192351),
            (90, 0x3f055dbe9982525a),
            (94, 0x3efc5e6802072c02),
            (96, 0x3efc8873887a9c26),
            (97, 0x3ee4f8b588e368f1),
            (99, 0x3ee73455c085ee45),
            (109, 0x3efd7c57b79f3440),
            (111, 0x3efd66865e51b1e6),
        ],
    ),
    (
        "Fillet/cliff_2.smb",
        &[
            (6, 0x3ef88c6ebc09888c),
            (8, 0x3ee65f938d3b1fe5),
            (10, 0x3ef5a5706b083a88),
            (11, 0x3ef9c86a69ae2668),
            (12, 0x3f047175a95ecac1),
            (14, 0x3ee4f8b588e368f1),
            (16, 0x3f04146c84867910),
            (18, 0x3f035086b680755b),
            (23, 0x3efca7200b006e6a),
            (25, 0x3ee4f8b588e368f1),
            (27, 0x3efcca5a664286c6),
            (29, 0x3ee4f8b588e368f1),
            (30, 0x3efeb7550dab1372),
            (32, 0x3f0094f2d986b7c7),
            (42, 0x3ef31abd76112496),
            (44, 0x3ef8394a57e771d6),
            (45, 0x3ee6893fc834eacf),
            (47, 0x3efb81caef036832),
            (48, 0x3ee4f8b588e368f1),
            (57, 0x3eec483f0446c9d9),
            (58, 0x3eee36bdb356112f),
            (69, 0x3ef1af8d94767428),
            (70, 0x3efc56449f562902),
            (72, 0x3ee4f8b588e368f1),
            (73, 0x3ef4ab31dff2773e),
        ],
    ),
    ("Fillet/corner_box.smb", &[(13, 0x3ee4f8b588e368f1)]),
    (
        "Fillet/cyl_cyl_2191.smb",
        &[
            (21, 0x3f02e3c62f3a545e),
            (24, 0x3f18009652da3e1c),
            (25, 0x3f1800939acc16fe),
            (26, 0x3f02e3c41770d8f7),
        ],
    ),
    (
        "Fillet/cyl_cyl_shell.smb",
        &[(2, 0x3f052ee05447869a), (5, 0x3f052ee053c08ddd)],
    ),
    (
        "Fillet/nurb_box1.smb",
        &[(3, 0x3ef32e82f3a7cafc), (6, 0x3ee4f8b588e368f1)],
    ),
    (
        "Fillet/open_shell.smb",
        &[
            (1, 0x3ee4f8b588e368f1),
            (3, 0x3ee4f8b588e368f1),
            (5, 0x3f0986654a325208),
            (16, 0x3ef8563816b64cf6),
            (17, 0x3ef8563816b64cf6),
        ],
    ),
    (
        "Fillet/shell_fillet2.smb",
        &[(0, 0x3f18df9e5002dfaf), (6, 0x3f0f9767421575a6)],
    ),
    (
        "Fillet/sphere_dif_box.smb",
        &[(11, 0x3eed24a7bbe76acd), (13, 0x3ee4f8b588e368f1)],
    ),
    ("Fillet/sphere_plane.smb", &[(3, 0x3ee4f8b588e368f1)]),
    (
        "Fillet/test1.smb",
        &[(1, 0x3eed72027411c483), (2, 0x3eed72027411c483)],
    ),
    ("Fillet/test8.smb", &[(3, 0x3f14fbe76bcf5772)]),
    (
        "Fillet/torii_fillet.smb",
        &[(22, 0x3f0e017dbe460a6c), (24, 0x3f0e017dbe4628b4)],
    ),
    ("Fillet/torii_unioned.smb", &[(2, 0x3f038db30168b3cb)]),
    (
        "Fillet/vase1.smb",
        &[(30, 0x3f167b4dcc885faf), (40, 0x3f167b4dcc99f949)],
    ),
    ("Fillet/vase2.smb", &[(14, 0x3f11b6ec6ab05faf)]),
    (
        "Fillet/vase3.smb",
        &[(24, 0x3f10950b4d0d2c7c), (34, 0x3f10950b4e905faf)],
    ),
    (
        "Shell/ShellBrep_983.smb",
        &[(28, 0x3efc4a686d5b51a0), (29, 0x3efc52064d7b5ea8)],
    ),
    (
        "Solids/fillet_ts.smb",
        &[
            (23, 0x3ee4f8b588e368f1),
            (29, 0x3ee4f8b588e368f1),
            (31, 0x3ee545dfec262fad),
            (33, 0x3ee7488868b30d3b),
        ],
    ),
    (
        "Solids/revolution2.smb",
        &[
            (12, 0x3f00aeb58aac7a28),
            (14, 0x3f01b55a6117155f),
            (17, 0x3ef68559fbd69dbe),
        ],
    ),
    (
        "Solids/revolution_mold.smb",
        &[
            (26, 0x3f180b6f6975736c),
            (59, 0x3ef7faa193029ca8),
            (60, 0x3ef7faa193961784),
        ],
    ),
];
/// Vertices whose tolerance the read moves: per file, (vertex index, main's bits).
const MOVED_VERTICES: &[(&str, &[(u32, u64)])] = &[
    (
        "Fillet/blade_hub_1.smb",
        &[
            (4, 0x3eff66a320d8900e),
            (5, 0x3eff66a320d8900e),
            (6, 0x3ee4f8b588e368f1),
            (7, 0x3ee4f8b588e368f1),
            (8, 0x3f0605b5bb5b6f28),
            (9, 0x3f0605b5bb5b6f28),
            (10, 0x3f05dd7bc5ff2249),
            (11, 0x3f05dd7bc5ff2249),
            (12, 0x3ee78c3f0adde333),
            (13, 0x3ee4f8b588e368f1),
            (14, 0x3efd080a6560090c),
            (15, 0x3efea239655549e0),
            (17, 0x3ef790d277f4413a),
            (18, 0x3ef790d277f4413a),
            (19, 0x3ef8ca044abf3700),
            (20, 0x3efd080a6560090c),
            (21, 0x3efea239655549e0),
            (26, 0x3ef36b20585d0708),
            (27, 0x3ef8317461789916),
            (28, 0x3ef36b20585d0708),
            (29, 0x3ef8317461789916),
            (30, 0x3ee60ce2b1a06437),
            (31, 0x3ef117adfa6f1844),
            (32, 0x3ee4f8b588e368f1),
            (33, 0x3ee4f8b588e368f1),
            (36, 0x3ef8ca044abf3700),
            (37, 0x3ef1ac7d364d4b7a),
            (42, 0x3ef1ca6e08a3bf12),
            (43, 0x3efddc3d1dc7a4e2),
            (44, 0x3ef1ca6e08a3bf12),
            (45, 0x3f019ac7168d8b08),
            (46, 0x3ee4f8b588e368f1),
            (47, 0x3f019ac7168d8b08),
            (52, 0x3f058ab2a27e852a),
            (53, 0x3f058ab2a27e852a),
            (54, 0x3f0681559af75019),
            (55, 0x3f0681559af75019),
            (58, 0x3f0593c631192351),
            (59, 0x3f055dbe9982525a),
            (60, 0x3f0593c631192351),
            (61, 0x3f055dbe9982525a),
            (62, 0x3efc5e6802072c02),
            (63, 0x3efc8873887a9c26),
            (64, 0x3efc5e6802072c02),
            (65, 0x3efc8873887a9c26),
            (66, 0x3ee4f8b588e368f1),
            (67, 0x3ee73455c085ee45),
            (72, 0x3efd7c57b79f3440),
            (73, 0x3efd66865e51b1e6),
            (74, 0x3efd7c57b79f3440),
            (75, 0x3efd66865e51b1e6),
        ],
    ),
    (
        "Fillet/blade_hub_2.smb",
        &[
            (4, 0x3ef88c6ebc09888c),
            (5, 0x3ef9c86a69ae2668),
            (6, 0x3ef5a5706b083a88),
            (7, 0x3ef9c86a69ae2668),
            (8, 0x3f047175a95ecac1),
            (9, 0x3f047175a95ecac1),
            (10, 0x3ee4f8b588e368f1),
            (11, 0x3ee4f8b588e368f1),
            (12, 0x3f04146c84867910),
            (13, 0x3f04146c84867910),
            (14, 0x3f035086b680755b),
            (15, 0x3f035086b680755b),
            (16, 0x3efca7200b006e6a),
            (17, 0x3ee4f8b588e368f1),
            (18, 0x3efeb7550dab1372),
            (19, 0x3f0094f2d986b7c7),
            (20, 0x3ee4f8b588e368f1),
            (21, 0x3efcca5a664286c6),
            (22, 0x3efcca5a664286c6),
            (23, 0x3ee4f8b588e368f1),
            (24, 0x3efeb7550dab1372),
            (25, 0x3f0094f2d986b7c7),
            (30, 0x3ef31abd76112496),
            (31, 0x3ef8394a57e771d6),
            (32, 0x3ef31abd76112496),
            (33, 0x3efb81caef036832),
            (34, 0x3ee6893fc834eacf),
            (35, 0x3efb81caef036832),
            (36, 0x3ee4f8b588e368f1),
            (40, 0x3eec483f0446c9d9),
            (41, 0x3eee36bdb356112f),
            (42, 0x3eec483f0446c9d9),
            (43, 0x3eee36bdb356112f),
            (48, 0x3ef1af8d94767428),
            (49, 0x3efc56449f562902),
            (50, 0x3ef1af8d94767428),
            (51, 0x3efc56449f562902),
            (52, 0x3ee4f8b588e368f1),
            (53, 0x3ef4ab31dff2773e),
        ],
    ),
    (
        "Fillet/blade_hub_3.smb",
        &[
            (0, 0x3efe072649c12f8a),
            (1, 0x3f02d056320226af),
            (2, 0x3f018be1ea26ed0c),
            (3, 0x3f02d056320226af),
            (4, 0x3eff11aac0388dec),
            (5, 0x3eff11aac0388dec),
            (6, 0x3ee4f8b588e368f1),
            (7, 0x3ee4f8b588e368f1),
            (8, 0x3f03e4b41cfd7adf),
            (9, 0x3f03e4b41cfd7adf),
            (10, 0x3f003b350e31db57),
            (11, 0x3f003b350e31db57),
            (14, 0x3eecde04b7296715),
            (16, 0x3eecde04b7296715),
            (17, 0x3ef598890406da58),
            (18, 0x3efecd714a9875de),
            (19, 0x3efc37895c10696e),
            (20, 0x3efecd714a9875de),
            (21, 0x3efc37895c10696e),
            (26, 0x3ef65a7ca3529bd4),
            (27, 0x3efd3dd339049e7c),
            (28, 0x3ef65a7ca3529bd4),
            (29, 0x3efd3dd339049e7c),
            (30, 0x3ee8ed7cfe1d49bb),
            (32, 0x3ef076601fff34f8),
            (33, 0x3ef1e0e391c37636),
            (34, 0x3ef26e8e08bc5ad6),
            (35, 0x3ee4f8b588e368f1),
            (36, 0x3ef26e8e08bc5ad6),
            (37, 0x3ef1e0e391c37636),
            (42, 0x3f02638eabe6072f),
            (43, 0x3f08c696afe9165e),
            (44, 0x3f02638eabe6072f),
            (45, 0x3f08c696afe9165e),
            (46, 0x3ee4f8b588e368f1),
            (47, 0x3eeb22288b52bb8d),
            (48, 0x3f03bec470741e09),
            (49, 0x3f03bec470741e09),
            (50, 0x3f00d260b71298fc),
            (51, 0x3f00d260b71298fc),
            (52, 0x3f063b36f2d5c362),
            (53, 0x3f063b36f2d5c362),
            (54, 0x3f0185e471b3583a),
            (55, 0x3f0185e471b3583a),
            (58, 0x3f05d464ef083cb0),
            (59, 0x3f05b8a3e1f41a2d),
            (60, 0x3f05d464ef083cb0),
            (61, 0x3f05b8a3e1f41a2d),
            (66, 0x3ee5864fc3cb858b),
            (67, 0x3ee56e2629cafe7f),
            (68, 0x3efe0d1812102dc6),
            (69, 0x3ee56e2629cafe7f),
            (70, 0x3efe0d1812102dc6),
            (71, 0x3ee4f8b588e368f1),
            (72, 0x3ee4f8b588e368f1),
            (73, 0x3ee4f8b588e368f1),
            (78, 0x3efbc005fab09f56),
            (79, 0x3efbae8c8906048e),
            (80, 0x3efbc005fab09f56),
            (81, 0x3efbae8c8906048e),
        ],
    ),
    (
        "Fillet/bottle.smb",
        &[
            (17, 0x3ee942308b035f45),
            (18, 0x3f0de04ffc8eb432),
            (19, 0x3f1085293e8cbe1c),
            (22, 0x3f1085293e8cbe1c),
        ],
    ),
    ("Fillet/box_extrusion2.smb", &[(10, 0x3f0ca9167773c8bc)]),
    ("Fillet/case_ext.smb", &[(0, 0x3f0da07639b836bc)]),
    ("Fillet/case_int.smb", &[(0, 0x3f0da07639b836bc)]),
    (
        "Fillet/cliff_1.smb",
        &[
            (4, 0x3eff66a320d8900e),
            (5, 0x3eff66a320d8900e),
            (6, 0x3ee4f8b588e368f1),
            (7, 0x3ee4f8b588e368f1),
            (8, 0x3f0605b5bb5b6f28),
            (9, 0x3f0605b5bb5b6f28),
            (10, 0x3f05dd7bc5ff2249),
            (11, 0x3f05dd7bc5ff2249),
            (12, 0x3ee78c3f0adde333),
            (13, 0x3ee4f8b588e368f1),
            (14, 0x3efd080a6560090c),
            (15, 0x3efea239655549e0),
            (17, 0x3ef790d277f4413a),
            (18, 0x3ef790d277f4413a),
            (19, 0x3ef8ca044abf3700),
            (20, 0x3efd080a6560090c),
            (21, 0x3efea239655549e0),
            (26, 0x3ef36b20585d0708),
            (27, 0x3ef8317461789916),
            (28, 0x3ef36b20585d0708),
            (29, 0x3ef8317461789916),
            (30, 0x3ee60ce2b1a06437),
            (31, 0x3ef117adfa6f1844),
            (32, 0x3ee4f8b588e368f1),
            (33, 0x3ee4f8b588e368f1),
            (36, 0x3ef8ca044abf3700),
            (37, 0x3ef1ac7d364d4b7a),
            (42, 0x3ef1ca6e08a3bf12),
            (43, 0x3efddc3d1dc7a4e2),
            (44, 0x3ef1ca6e08a3bf12),
            (45, 0x3f019ac7168d8b08),
            (46, 0x3ee4f8b588e368f1),
            (47, 0x3f019ac7168d8b08),
            (52, 0x3f058ab2a27e852a),
            (53, 0x3f058ab2a27e852a),
            (54, 0x3f0681559af75019),
            (55, 0x3f0681559af75019),
            (58, 0x3f0593c631192351),
            (59, 0x3f055dbe9982525a),
            (60, 0x3f0593c631192351),
            (61, 0x3f055dbe9982525a),
            (62, 0x3efc5e6802072c02),
            (63, 0x3efc8873887a9c26),
            (64, 0x3efc5e6802072c02),
            (65, 0x3efc8873887a9c26),
            (66, 0x3ee4f8b588e368f1),
            (67, 0x3ee73455c085ee45),
            (72, 0x3efd7c57b79f3440),
            (73, 0x3efd66865e51b1e6),
            (74, 0x3efd7c57b79f3440),
            (75, 0x3efd66865e51b1e6),
        ],
    ),
    (
        "Fillet/cliff_2.smb",
        &[
            (4, 0x3ef88c6ebc09888c),
            (5, 0x3ef9c86a69ae2668),
            (6, 0x3ef5a5706b083a88),
            (7, 0x3ef9c86a69ae2668),
            (8, 0x3f047175a95ecac1),
            (9, 0x3f047175a95ecac1),
            (10, 0x3ee4f8b588e368f1),
            (11, 0x3ee4f8b588e368f1),
            (12, 0x3f04146c84867910),
            (13, 0x3f04146c84867910),
            (14, 0x3f035086b680755b),
            (15, 0x3f035086b680755b),
            (16, 0x3efca7200b006e6a),
            (17, 0x3ee4f8b588e368f1),
            (18, 0x3efeb7550dab1372),
            (19, 0x3f0094f2d986b7c7),
            (20, 0x3ee4f8b588e368f1),
            (21, 0x3efcca5a664286c6),
            (22, 0x3efcca5a664286c6),
            (23, 0x3ee4f8b588e368f1),
            (24, 0x3efeb7550dab1372),
            (25, 0x3f0094f2d986b7c7),
            (30, 0x3ef31abd76112496),
            (31, 0x3ef8394a57e771d6),
            (32, 0x3ef31abd76112496),
            (33, 0x3efb81caef036832),
            (34, 0x3ee6893fc834eacf),
            (35, 0x3efb81caef036832),
            (36, 0x3ee4f8b588e368f1),
            (40, 0x3eec483f0446c9d9),
            (41, 0x3eee36bdb356112f),
            (42, 0x3eec483f0446c9d9),
            (43, 0x3eee36bdb356112f),
            (48, 0x3ef1af8d94767428),
            (49, 0x3efc56449f562902),
            (50, 0x3ef1af8d94767428),
            (51, 0x3efc56449f562902),
            (52, 0x3ee4f8b588e368f1),
            (53, 0x3ef4ab31dff2773e),
        ],
    ),
    (
        "Fillet/corner_box.smb",
        &[(7, 0x3ee4f8b588e368f1), (9, 0x3ee4f8b588e368f1)],
    ),
    (
        "Fillet/cyl_cyl_2191.smb",
        &[
            (10, 0x3f02e3c62f3a545e),
            (11, 0x3f18009652da3e1c),
            (12, 0x3f18009652da3e1c),
            (13, 0x3f02e3c62f3a545e),
            (14, 0x3f1800939acc16fe),
            (15, 0x3f1800939acc16fe),
            (16, 0x3f02e3c41770d8f7),
            (17, 0x3f02e3c41770d8f7),
        ],
    ),
    (
        "Fillet/cyl_cyl_shell.smb",
        &[(0, 0x3f052ee05447869a), (2, 0x3f052ee05447869a)],
    ),
    (
        "Fillet/open_shell.smb",
        &[
            (2, 0x3ee4f8b588e368f1),
            (4, 0x3ef8563816b64cf6),
            (5, 0x3f0986654a325208),
            (6, 0x3f0986654a325208),
            (12, 0x3ef8563816b64cf6),
            (14, 0x3ef8563816b64cf6),
        ],
    ),
    (
        "Fillet/shell_fillet2.smb",
        &[(0, 0x3f18df9e5002dfaf), (1, 0x3f18df9e5002dfaf)],
    ),
    ("Fillet/sphere_plane.smb", &[(2, 0x3ee4f8b588e368f1)]),
    (
        "Fillet/test1.smb",
        &[(0, 0x3eed72027411c483), (1, 0x3eed72027411c483)],
    ),
    ("Fillet/test8.smb", &[(2, 0x3f14fbe76bcf5772)]),
    (
        "Fillet/torii_fillet.smb",
        &[
            (10, 0x3f0e017dbe460a6c),
            (17, 0x3f0e017dbe460a6c),
            (18, 0x3f0e017dbe4628b4),
            (19, 0x3f0e017dbe4628b4),
        ],
    ),
    ("Fillet/torii_unioned.smb", &[(1, 0x3f038db30168b3cb)]),
    (
        "Shell/ShellBrep_983.smb",
        &[
            (8, 0x3efc4a686d5b51a0),
            (9, 0x3efc52064d7b5ea8),
            (18, 0x3efc4a686d5b51a0),
            (19, 0x3efc52064d7b5ea8),
        ],
    ),
    (
        "Solids/fillet_ts.smb",
        &[
            (6, 0x3ee7488868b30d3b),
            (9, 0x3ee545dfec262fad),
            (10, 0x3ee7488868b30d3b),
            (17, 0x3ee545dfec262fad),
            (23, 0x3ee4f8b588e368f1),
            (24, 0x3ee4f8b588e368f1),
            (26, 0x3ee4f8b588e368f1),
            (29, 0x3ee4f8b588e368f1),
        ],
    ),
    (
        "Solids/revolution2.smb",
        &[
            (10, 0x3f00aeb58aac7a28),
            (11, 0x3f00aeb58aac7a28),
            (12, 0x3f01b55a6117155f),
            (13, 0x3f01b55a6117155f),
        ],
    ),
    (
        "Solids/revolution_mold.smb",
        &[
            (43, 0x3ef7faa193961784),
            (44, 0x3ef7faa193029ca8),
            (45, 0x3ef7faa193961784),
            (46, 0x3ef7faa193029ca8),
        ],
    ),
];
/// [`RAISED_EDGES`] of the read that rebuilds the UV trims.
const REBUILD_RAISED_EDGES: &[(&str, &[(u32, u64)])] = &[
    ("Fillet/open_shell.smb", &[(17, 0x3efa4fff2e7e0c33)]),
    (
        "Fillet/vase3.smb",
        &[(24, 0x3f10950b4d0d2c7c), (34, 0x3f10950b4e905faf)],
    ),
];
/// [`MOVED_VERTICES`] of the read that rebuilds the UV trims.
const REBUILD_MOVED_VERTICES: &[(&str, &[(u32, u64)])] =
    &[("Fillet/open_shell.smb", &[(12, 0x3efa4fff2e7e0c33)])];

// ---- the fillets of `blend.fillet_executive_p3`'s moved rows ----------------------------

/// The `blend.fillet_executive_p3` rows whose result moved (all but
/// `piecewise-file-corner_box-edge0`, which merges pieces): file, edge (B-rep list
/// index), radius; circular section, G1, thumbweight 1.
const FILLETS: [(&str, usize, f64); 29] = [
    ("Fillet/corner_box.smb", 0, 0.1),
    ("Fillet/corner_box.smb", 4, 0.1),
    ("Fillet/corner_box.smb", 8, 0.1),
    ("Fillet/corner_box.smb", 9, 0.1),
    ("Fillet/case_ext.smb", 1, 0.5),
    ("Fillet/case_int.smb", 1, 0.5),
    ("Fillet/open_shell.smb", 0, 0.5),
    ("Fillet/open_shell.smb", 5, 0.5),
    ("Fillet/shell_fillet2.smb", 1, 0.5),
    ("Fillet/bottle.smb", 0, 0.5),
    ("Fillet/bottle.smb", 2, 0.5),
    ("Fillet/bottle.smb", 10, 0.5),
    ("Fillet/bottle.smb", 13, 0.5),
    ("Fillet/bottle.smb", 7, 0.1),
    ("Fillet/bottle.smb", 8, 0.1),
    ("Fillet/bottle.smb", 30, 0.1),
    ("Fillet/bottle.smb", 32, 0.1),
    ("Fillet/bottle.smb", 27, 2.0),
    ("Fillet/bottle.smb", 29, 2.0),
    ("Fillet/bottle.smb", 30, 2.0),
    ("Fillet/bottle.smb", 32, 2.0),
    ("Fillet/bottle.smb", 4, 0.5),
    ("Fillet/bottle.smb", 6, 0.5),
    ("Fillet/bottle.smb", 8, 0.5),
    ("Fillet/bottle.smb", 16, 0.5),
    ("Fillet/bottle.smb", 19, 0.5),
    ("Fillet/bottle.smb", 27, 0.5),
    ("Fillet/bottle.smb", 29, 0.1),
    ("Fillet/bottle.smb", 29, 1.0),
];

/// One fillet of [`FILLETS`] on the read: the digest of its status and its result's
/// geometry (counts, face tolerances, vertex points, edge curves, and every edge use's UV
/// trim with the surface points along it; not the edge and vertex tolerances), and the
/// bits of the result's mass properties at the golden's accuracy (1e-4, origin 0), or
/// the error's status.
fn fillet_result(name: &str, edge: usize, radius: f64) -> (u64, Vec<u64>) {
    use solidcore::blend::fillet_executive::api::fillet_edges;
    use solidcore::mass::integrator::properties::brep_mass_properties_default;
    use solidcore::math::Vec3;
    use solidcore::topology::brep::p1 as bl;
    let mut k = Kernel::default();
    let mut b = read_corpus(&mut k, name, false);
    let e = bl::edges(&b, None).unwrap()[edge];
    // (a refused fillet: its status and the body it leaves are recorded)
    let status = match fillet_edges(&mut k, &mut b, &[e], radius, 1, 1, 1.0) {
        Ok(()) => 0,
        Err(error) => error.sm_status().unwrap_or(-1),
    };
    let mut d = Digest::new();
    d.u64(u64::from(status.unsigned_abs()));
    let faces = bl::faces(&b, None).unwrap();
    let edges = bl::edges(&b, None).unwrap();
    let vertices = bl::vertices(&b, None).unwrap();
    for n in [faces.len(), edges.len(), vertices.len()] {
        d.u64(n as u64);
    }
    for f in faces {
        d.f64(b.face(f).unwrap().tolerance);
    }
    for v in vertices {
        let p = b.vertex(v).unwrap().point;
        for c in [p.x, p.y, p.z] {
            d.f64(c);
        }
    }
    for e in edges {
        digest_curve(&mut d, &k, &b, e);
        for eu in b.edge(e).unwrap().edgeuses.clone() {
            digest_trim(&mut d, &k, &b, eu);
        }
    }
    let mass = match brep_mass_properties_default(&k, &mut b, 1e-4, 1.0, Vec3::ZERO) {
        Ok(m) => {
            let mut bits = vec![
                m.properties.area.to_bits(),
                m.properties.volume.to_bits(),
                m.properties.mass.to_bits(),
            ];
            for v in [m.centroid, m.inertia, m.products] {
                bits.extend([v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]);
            }
            bits
        }
        Err(error) => vec![u64::from(error.sm_status().unwrap_or(0).unsigned_abs())],
    };
    (d.0, mass)
}

/// Main 3a0f49b09's [`fillet_result`] digests per [`FILLETS`] entry, made by this file's
/// functions on main; entry 7 (`open_shell` e5) is main bcfb4b98e's, after #18402 moved
/// that fillet's result (the other 28 are unchanged there). Entry 14 (`bottle` e8 r0.1) is #18401's:
/// that PR moves this fillet's digest and mass in their last bits, and its tree before
/// #18230 makes the same digest and mass bit for bit. The 19 equal digests (`0x30d463838d7fca03`) are bottle
/// fillets: main refuses each (status 1001) and leaves the read body, so all digest the
/// same body and status.
const FILLET_MAIN: [u64; 29] = [
    0x24f613f551b1b3ab,
    0x241377ca06fbe761,
    0xc9494b45a0f312d7,
    0x7c0ce81dfdec54b0,
    0xc0013e51f46a74a,
    0x357c96c0893f37a2,
    0x2dd7a7af4ca5bb1e,
    0x51f6abb34e90bff2,
    0xb716929faf389955,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x7442b43a428ea1ca,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
    0x30d463838d7fca03,
];

/// Main 3a0f49b09's mass bits for the rows whose mass moved in the expectations:
/// `source-file-corner_box-edge4`, `-edge8`, `-edge9` and
/// `stubs4-delete-edge-bottle-e8-r0.1` ([`FILLETS`] 1, 2, 3 and 14). Entry 14 is #18401's (see
/// [`FILLET_MAIN`]).
const FILLET_MAIN_MASS: [(usize, [u64; 12]); 4] = [
    (
        1,
        [
            0x4076bfbccb19102a,
            0x407c8288ca848c80,
            0x407c8288ca848c80,
            0xc023e4f278d29639,
            0xc0241052cfc69609,
            0x400bb474093d6d56,
            0x40eaa4527185e0b9,
            0x40ebd653fa8df30d,
            0x40f7abf16eb9815a,
            0xc0cf08d416325adf,
            0xc0ced0087d0fee1f,
            0x40e6333a0d76af85,
        ],
    ),
    (
        2,
        [
            0x4076bfbccb19102a,
            0x407c8288ca848c80,
            0x407c8288ca848c80,
            0xc023e51b0129d1ec,
            0xc0241052cfc69609,
            0x400bb4daf832b522,
            0x40eaa466811c6f18,
            0x40ebd6c16a9f8408,
            0x40f7ac1e1ef702a8,
            0xc0cf0946b86a1ab9,
            0xc0ced0b92bef7338,
            0x40e63367314308c1,
        ],
    ),
    (
        3,
        [
            0x4076c10e1e1ce654,
            0x407c829973ddde71,
            0x407c829973ddde71,
            0xc023e5148bf8d37b,
            0xc0241050602f226e,
            0x400bb48477d8cc2a,
            0x40eaa4610aefcd5c,
            0x40ebd6b63d441862,
            0x40f7ac2386495ac6,
            0xc0cf08f0f659bc40,
            0xc0ced0968b1cdcf1,
            0x40e6336bc4833118,
        ],
    ),
    (
        14,
        [
            0x40408bcce093b407,
            0x402621896e50fcb0,
            0x402621896e50fcb0,
            0x3f10d1c99a55926e,
            0x3f13efa09958129b,
            0x4000a10fe9da9881,
            0x4050316571691566,
            0x4050a9c3826e8ec1,
            0x40151aaab5c59a6f,
            0x3f6aed515da75eee,
            0x3f66b6a18cf50c20,
            0xbf3c98659ef8970b,
        ],
    ),
];

/// The fillets behind `blend.fillet_executive_p3`'s moved rows make main's geometry and
/// mass bit for bit: the rows moved only in the edge and vertex tolerances the result
/// carries from the read. The four rows whose mass moved in the expectations
/// (`source-file-corner_box-edge4`, `-edge8`, `-edge9`,
/// `stubs4-delete-edge-bottle-e8-r0.1`) have main's mass bit for bit: main's committed rows were older than main's own output, and the bless
/// rewrote the whole row when the result's tolerances moved.
#[test]
fn moved_fillet_rows_keep_main_geometry_and_mass() {
    let mut failures = Vec::new();
    for (i, &(name, edge, radius)) in FILLETS.iter().enumerate() {
        let (digest, mass) = fillet_result(name, edge, radius);
        if digest != FILLET_MAIN[i] {
            failures.push(format!(
                "{i} {name} e{edge} r{radius}: digest {digest:#x}, main {:#x}",
                FILLET_MAIN[i]
            ));
        }
        if let Some((_, main)) = FILLET_MAIN_MASS.iter().find(|(j, _)| *j == i) {
            if mass != main {
                failures.push(format!("{i} {name}: mass {mass:#x?}, main {main:#x?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
