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
//! [`corpus_reads_cover_every_edge_face_gap`] checks the cover on every file the moved
//! goldens rows of the gap cover read.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::attribute::AttributeStore;
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
/// built after the read once the caller has dropped the file's trim, so
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
    // the sphere's trim made again after the read (the read keeps the file's own trim,
    // WR-493)
    for eu in b.edge(e).unwrap().edgeuses.clone() {
        if eu1::face(&b, eu).unwrap() == Some(FaceId::from_index(0)) {
            eu1::set_uv_trim_curve(&mut b, eu, eu1::UvTrimCurveArg::Null, 0.0, true, None).unwrap();
        }
    }
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

// ---- corpus reads ------------------------------------------------------------------------

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
/// rebuilt (`bRebuildUVTrimCurves`, `topology.face_p3`'s `_rebuild` rows), with the
/// tolerances the read's kept-trim cover raised after the healer put back as the healer
/// left them ([`undo_trim_cover`]): the body the healer's gap cover leaves.
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
    undo_trim_cover(&mut b);
    b
}

/// Puts back every edge and vertex tolerance the read's kept-trim cover raised
/// ([`Brep::trim_cover_raises`]) to the value the read had made before it, checking each
/// still holds the raised value.
fn undo_trim_cover(b: &mut Brep) {
    let raises = b.trim_cover_raises().clone();
    for (e, (before, after)) in raises.edges {
        let r = b.edge_mut(e).unwrap();
        assert_eq!(r.tolerance.to_bits(), after.to_bits(), "{e:?}");
        r.tolerance = before;
    }
    for (v, (before, after)) in raises.vertices {
        let r = b.vertex_mut(v).unwrap();
        assert_eq!(r.tolerance.to_bits(), after.to_bits(), "{v:?}");
        r.tolerance = before;
    }
}

/// The 28 files the goldens rows the healer's gap cover moved read.
const COVER_READS: [&str; 28] = [
    "Fillet/blade_hub_1.smb",
    "Fillet/blade_hub_2.smb",
    "Fillet/blade_hub_3.smb",
    "Fillet/bottle.smb",
    "Fillet/box_extrusion2.smb",
    "Fillet/case_ext.smb",
    "Fillet/case_int.smb",
    "Fillet/cliff_1.smb",
    "Fillet/cliff_2.smb",
    "Fillet/corner_box.smb",
    "Fillet/cyl_cyl_2191.smb",
    "Fillet/cyl_cyl_shell.smb",
    "Fillet/nurb_box1.smb",
    "Fillet/open_shell.smb",
    "Fillet/shell_fillet2.smb",
    "Fillet/sphere_dif_box.smb",
    "Fillet/sphere_plane.smb",
    "Fillet/test1.smb",
    "Fillet/test8.smb",
    "Fillet/torii_fillet.smb",
    "Fillet/torii_unioned.smb",
    "Fillet/vase1.smb",
    "Fillet/vase2.smb",
    "Fillet/vase3.smb",
    "Shell/ShellBrep_983.smb",
    "Solids/fillet_ts.smb",
    "Solids/revolution2.smb",
    "Solids/revolution_mold.smb",
];

/// On each file of [`COVER_READS`], read plainly and with the UV trims rebuilt (both with
/// the kept-trim cover's raises put back, [`read_corpus`]), every edge's tolerance covers
/// the largest edge/face gap of its uses, measured afresh (`force_calc`), never read from
/// a use's cache (#20939).
#[test]
fn corpus_reads_cover_every_edge_face_gap() {
    let mut failures = Vec::new();
    let mut edges = 0;
    for name in COVER_READS {
        for rebuild in [false, true] {
            let mut k = Kernel::default();
            let mut b = read_corpus(&mut k, name, rebuild);
            let ids: Vec<_> = b.edges().iter().map(|(id, _)| id).collect();
            for e in ids {
                let tol = b.edge(e).unwrap().tolerance;
                let mut gap = 0.0f64;
                for eu in b.edge(e).unwrap().edgeuses.clone() {
                    if let Some(g) = max_edge_face_gap(&k, &mut b, eu, true).unwrap() {
                        gap = gap.max(g.gap_3d);
                    }
                }
                if tol < gap {
                    failures.push(format!(
                        "{name} (rebuild {rebuild}): {e:?} tolerance {tol:e} under its gap {gap:e}"
                    ));
                }
                edges += 1;
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(edges > 0);
}

/// #20939 on a corpus file: the rebuilding read of `Fillet/open_shell.smb` rebuilds edge
/// 17's UV trims after the heal, and the rebuilt trim moves the edge's interval from
/// `[0, 1]` to `[3.9e-6, 1]` (`create_uv_trim_curve`'s interval recovery, through
/// `edge::set_interval`). The edge/face gap and the start vertex's vertex/edge gap
/// cached over the old interval no longer hold: each use's gap as read equals the gap
/// measured afresh. (Before #20939 the cache kept the edge's 3.02e-5, measured over
/// `[0, 1]`, against a gap of 1.25e-5 over the edge's interval, and vertex 14's 3.02e-5
/// against 3.18e-5.)
#[test]
fn open_shell_rebuilt_trim_leaves_no_stale_gap() {
    use solidcore::topology::{edge, vertexuse, EdgeId, VertexId};
    let name = "Fillet/open_shell.smb";
    let mut k = Kernel::default();
    let mut b = read_corpus(&mut k, name, true);
    let e = EdgeId::from_index(17);
    let ivl = b.edge(e).unwrap().interval;
    assert!(
        ivl.min() > 0.0 && ivl.min() < 1e-5,
        "{name}: e17 interval {ivl:?}"
    );
    let plain = {
        let mut k = Kernel::default();
        let b = read_corpus(&mut k, name, false);
        b.edge(e).unwrap().interval
    };
    assert_eq!(
        (plain.min(), plain.max()),
        (0.0, 1.0),
        "{name}: e17 as healed"
    );
    let mut checked = 0;
    for eu in edge::edgeuses(&b, e, None, None).unwrap() {
        let read = max_edge_face_gap(&k, &mut b, eu, false)
            .unwrap()
            .unwrap()
            .gap_3d;
        let fresh = max_edge_face_gap(&k, &mut b, eu, true)
            .unwrap()
            .unwrap()
            .gap_3d;
        assert_eq!(
            read.to_bits(),
            fresh.to_bits(),
            "{name}: {eu:?} gap {read:e}, measured {fresh:e}"
        );
        checked += 1;
    }
    let v = VertexId::from_index(14);
    for vu in solidcore::topology::vertex::vertexuses(&b, v).unwrap() {
        let read = vertexuse::vertex_edge_gap(&k, &mut b, vu, false).unwrap();
        let fresh = vertexuse::vertex_edge_gap(&k, &mut b, vu, true).unwrap();
        let read_face = vertexuse::vertex_face_gap(&k, &mut b, vu, false).unwrap();
        let fresh_face = vertexuse::vertex_face_gap(&k, &mut b, vu, true).unwrap();
        for (what, read, fresh) in [
            (
                "vertex/edge",
                read.map(|g| g.gap_3d),
                fresh.map(|g| g.gap_3d),
            ),
            (
                "vertex/face",
                read_face.map(|g| g.gap_3d),
                fresh_face.map(|g| g.gap_3d),
            ),
        ] {
            assert_eq!(
                read.map(f64::to_bits),
                fresh.map(f64::to_bits),
                "{name}: {vu:?} {what} gap {read:?}, measured {fresh:?}"
            );
            checked += usize::from(read.is_some());
        }
    }
    assert!(checked > 2, "{name}: {checked} gaps checked");
}

/// The end points of the two edges of the `bottle` e8 r0.1 fillet's result whose tolerance
/// #18162's lazy UV trim raise lowers: its vertices 26, 38 and 27 (each edge curve ends
/// within 1e-4 of two of them; they are at least 0.03 apart).
const BOTTLE_E8_MOVED_ENDS: [[f64; 3]; 3] = [
    [
        -7.61836356003545e-7,
        -6.412140332612071e-1,
        3.937830788008653,
    ],
    [
        -9.695404181326284e-1,
        -5.125539440709455e-1,
        3.9377557534353658,
    ],
    [
        -9.995634517364296e-1,
        -5.038566395810731e-1,
        3.937831742378171,
    ],
];

/// The `bottle` e8 r0.1 fillet (`blend.fillet_executive_p3`
/// `stubs4-delete-edge-bottle-e8-r0.1`) under #18162's lazy UV trim raise: the result's
/// edges 42 and 47 keep 4e-5, their end vertices 26, 27 and 38 follow (4.07e-5, 4e-5,
/// 4e-5), and edge use 190's trim (face 16, a B-spline) is refit at that tolerance. Proved
/// here, on the result:
/// - the two edges (found by their end points, [`BOTTLE_E8_MOVED_ENDS`], not by arena id)
///   are at 4e-5 and their trim on face 16 (a B-spline) is within it: `trim_image_gap_bound`,
///   a proved bound on the distance between the trim's image and the edge curve, is at most
///   the tolerance (edge use 190's, the refit one, is 1.45e-5; edge use 170's 3.75e-6); their
///   other trim, on the procedural blend, has no exact net to enclose (goldens'
///   `certify::trim_tolerance` keeps those edges OPEN, `blend-surface`): that trim at 4e-5
///   is the result's one unproved remainder;
/// - the three end vertices (26, 38 and 27, at 4e-5, 4e-5 and 4.07e-5) each cover the end
///   of every edge curve that ends there, the distance's rounding (`gamma(8)` of the
///   coordinates' size, times sqrt 3) counted against the tolerance;
/// - Level 2 reports nothing: vertex 22's two vertex/face gaps (4.71e-5) are covered by
///   #17739's vertex step, which raises the read's vertex 22 to cover twice its gap;
/// - the mass properties are each within the case's accuracy (1e-4 of its scale: the area,
///   the volume, `R` the box's half diagonal for a centroid coordinate, `V R^2` for a
///   moment, or the value's own where larger) of the exact property: of every point of a
///   certified enclosure of it (`mass::enclosure::enclose_mass_properties`), so of the
///   exact value wherever it lies in it.
#[test]
fn bottle_e8_fillet_moved_by_the_lazy_raise_stays_within_its_proved_bounds() {
    use solidcore::blend::fillet_executive::api::fillet_edges;
    use solidcore::mass::integrator::properties::brep_mass_properties_default;
    use solidcore::math::Vec3;
    use solidcore::nurbs::math::gamma;
    use solidcore::query::brep::{calculate_bounding_box, BoundingBoxOptions};
    use solidcore::query::curve_surface_gap::trim_image_gap_bound_up_to;
    use solidcore::surface::surface::p2::SurfaceKind;
    use solidcore::topology::brep::p1 as bl;
    use solidcore::topology::edgeuse::p1 as eu1;
    const ACCURACY: f64 = 1e-4;
    let mut k = Kernel::default();
    let mut b = read_corpus(&mut k, "Fillet/bottle.smb", false);
    let e = bl::edges(&b, None).unwrap()[8];
    fillet_edges(&mut k, &mut b, &[e], 0.1, 1, 1, 1.0).unwrap();
    let counts = [
        bl::faces(&b, None).unwrap().len(),
        bl::edges(&b, None).unwrap().len(),
        bl::vertices(&b, None).unwrap().len(),
    ];
    assert_eq!(counts, [18, 47, 31]);

    let mut failures = Vec::new();
    let (mut moved, mut bounded, mut blend) = (0, 0, 0);
    for e in bl::edges(&b, None).unwrap() {
        let rec = b.edge(e).unwrap();
        let (tol, ivl) = (rec.tolerance, rec.interval);
        let curve = rec.curve.clone().unwrap();
        let at_moved_end = |t: f64| {
            let p = curve.evaluate_point(&k, t).unwrap();
            BOTTLE_E8_MOVED_ENDS
                .iter()
                .any(|&[x, y, z]| p.distance(Vec3::new(x, y, z)) <= 1e-4)
        };
        if !(at_moved_end(ivl.min()) && at_moved_end(ivl.max())) {
            continue;
        }
        moved += 1;
        assert_eq!(tol, 4e-5, "{e:?}");
        for eu in rec.edgeuses.clone() {
            let Some(trim) = b.edgeuse(eu).unwrap().uv_trim_curve.clone() else {
                continue;
            };
            let face = eu1::face(&b, eu).unwrap().unwrap();
            let surface = b.face(face).unwrap().surface.as_ref().unwrap();
            let is_blend = matches!(surface.kind(), SurfaceKind::ProceduralBlend(_));
            // (as the raise bounds it: an owner-less copy, the edge's interval)
            let mut surface = surface.clone_for_evaluation().unwrap();
            let _ = surface.set_owner(None);
            let range = [ivl.min(), ivl.max()];
            match trim_image_gap_bound_up_to(&k, &surface, &curve, &trim, range, 1e-3 * tol, tol)
                .unwrap()
            {
                Some(bound) if bound <= tol => bounded += 1,
                None if is_blend => blend += 1,
                other => failures.push(format!("{e:?} {eu:?}: {other:?} against {tol:e}")),
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    assert_eq!(
        [moved, bounded, blend],
        [2, 2, 2],
        "moved edges, bounded and blend trims"
    );

    // the end vertices the raise lowers (26, 38 and 27): each still covers the end of every
    // edge curve that ends there, each distance's rounding counted against it
    let mut lowered = Vec::new();
    for v in bl::vertices(&b, None).unwrap() {
        let rec = b.vertex(v).unwrap();
        let (at, tol) = (rec.point, rec.tolerance);
        if !BOTTLE_E8_MOVED_ENDS
            .iter()
            .any(|&[x, y, z]| at.distance(Vec3::new(x, y, z)) <= 1e-4)
        {
            continue;
        }
        lowered.push(tol);
        for e in solidcore::topology::vertex::edges(&b, v, None).unwrap() {
            let rec = b.edge(e).unwrap();
            let curve = rec.curve.as_ref().unwrap();
            let (v0, v1) = solidcore::topology::edge::start_end_vertices(&b, e).unwrap();
            for (end, t) in [(v0, rec.interval.min()), (v1, rec.interval.max())] {
                if end != Some(v) {
                    continue;
                }
                let p = curve.evaluate_point(&k, t).unwrap();
                let size = at.max_dimension().max(p.max_dimension());
                let far = at.distance(p) + gamma(8) * size * 3.0_f64.sqrt();
                assert!(far <= tol, "{v:?} {e:?}: {far:e} against {tol:e}");
            }
        }
    }
    lowered.sort_by(f64::total_cmp);
    // (the third moved 6.1e-12 relative, 4.0730163673638e-5 to 4.07301636733882e-5, when
    // the offset's extension became the default; each still covers every edge end above,
    // its rounding counted)
    assert_eq!(
        lowered,
        [4e-5, 4e-5, 4.07301636733882e-5],
        "the lowered vertices"
    );

    // (vertex 22's two vertex/face gaps, offset 4.7135459e-5, are covered by #17739's
    // healer, which covers twice that gap, so the check is clean)
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:#?}");

    let m = brep_mass_properties_default(&k, &mut b, ACCURACY, 1.0, Vec3::ZERO).unwrap();
    let mut now = vec![m.properties.area, m.properties.volume, m.properties.mass];
    for v in [m.centroid, m.inertia, m.products] {
        now.extend([v.x, v.y, v.z]);
    }
    let r = 0.5
        * calculate_bounding_box(&k, &mut b, BoundingBoxOptions::default())
            .unwrap()
            .size()
            .length();
    let x =
        solidcore::mass::enclosure::enclose_mass_properties(&k, &mut b, 0.1 * ACCURACY).unwrap();
    let vr2 = x.volume.hi * r * r;
    let band = |i: usize| match i {
        0 => (x.area, x.area.hi),
        1 | 2 => (x.volume, x.volume.hi),
        3..=5 => (x.centroid[i - 3], r),
        6..=8 => (x.inertia[i - 6], vr2),
        _ => (x.products[i - 9], vr2),
    };
    for (i, &value) in now.iter().enumerate() {
        let (band, scale) = band(i);
        // (`f64::max` skips a NaN, and an infinite value makes `accuracy` infinite: every
        // input of the comparison must be finite first)
        assert!(
            band.lo.is_finite() && band.hi.is_finite() && scale.is_finite(),
            "{i}: enclosure [{:e}, {:e}] at scale {scale:e} is not finite",
            band.lo,
            band.hi
        );
        assert!(value.is_finite(), "{i}: {value:e} is not finite");
        let accuracy = ACCURACY * scale.max(value.abs());
        // (the farthest the exact value can be)
        let far = (value - band.lo).max(band.hi - value);
        assert!(
            far <= accuracy,
            "{i}: {value:e} is up to {far:e} off [{:e}, {:e}] (accuracy {accuracy:e})",
            band.lo,
            band.hi
        );
    }
}
