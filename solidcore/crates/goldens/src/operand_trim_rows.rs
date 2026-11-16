//! Certificates of the golden rows the operand-trim copy moves (tests only;
//! `keep_operand_uv_trims` retired, #14246): `boolean::merge::p2`'s new faces keep their
//! operand faces' UV trims where the surface keeps its parameterization, so a result's edge
//! uses that had none now carry one.
//!
//! A row is certified as follows.
//! * **Replay.** The case's Rust side is run again. Its output, with every UV trim's
//!   samples and every UV trim owner blinded ([`blinded_digest`]), has the digest recorded
//!   for this branch, so the run is the row. With each listed moved value put back to main's
//!   (7d4445c0c) value, it has the digest of main's output blinded alike: every value but the
//!   trims and the listed ones is main's, bit for bit.
//! * **Soundness.** The result body passes `check_brep` level 2 and, a manifold solid, has a
//!   volume proved positive ([`validation::soundness`]).
//! * **Trims on edges.** Every trim the body holds lies on its edge within the edge's
//!   tolerance, and its edge within that of the trim's image
//!   ([`crate::trims_on_edges`], rounded interval arithmetic).
//! * **Moved values.** Each listed moved value is a coordinate of an edge curve's sample
//!   point or of a vertex, or a vertex's tolerance. A point, main's and this branch's, has its
//!   distance to each surface of the faces it lies on enclosed ([`crate::certify::nearest`]);
//!   the move is `closer` where this branch's upper end is below main's lower end on some
//!   face and above on none, `farther` the other way, `equal` where the enclosures meet. A
//!   tolerance must cover its vertex's distance to its edges' curve ends (enclosed in
//!   intervals); both values must, and the move is `closer` when this branch's is the
//!   smaller, `equal` when the two differ by at most that distance's upper end (each
//!   records the kernel default `1e-5` plus a measure of a gap that small), `farther`
//!   otherwise. An edge's tolerance is classed alike against the most of its uses' trims'
//!   certified bounds and its ends' distances to its vertices. A moved sample of a face's
//!   surface (a fixed point of its natural domain) is classed as an edge's sample, against
//!   that face alone.
//!
//! A `farther` move fails the test.
//! Run: `cargo test --profile goldens -p goldens --lib operand_trim_rows -- --nocapture`.

use solidcore::blend::fillet_section::certificate::curve_derivs_over;
use solidcore::blend::fillet_section::interval::{round_up, Ivl};
use solidcore::blend::fillet_section::series::V3;
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::p5::DropPointOptions;
use solidcore::topology::brep::validation;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{Brep, EdgeId, FaceId, VertexId};

use crate::certify::{nearest, Enclosure};
use crate::trims_on_edges::{certify_brep, curve_nurbs, norm_hi};
use crate::Output;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// A record's curve block (`topology.brep_mirror`'s `curve_samples`): `-1`, or the type,
/// the natural interval and five samples, each `0, x, y, z` or `1`. Its length at `i`.
fn curve_block(rec: &[f64], i: usize) -> usize {
    if rec[i] == -1.0 {
        return 1;
    }
    let mut j = i + 3;
    for _ in 0..5 {
        j += if rec[j] == 0.0 { 4 } else { 1 };
    }
    j - i
}

/// A record's surface block (`topology.brep_mirror`'s `surface_samples`): `-1`, or the
/// type, the natural UV domain and nine samples, each `0, x, y, z` or `1`. Its length at `i`.
fn surface_block(rec: &[f64], i: usize) -> usize {
    if rec[i] == -1.0 {
        return 1;
    }
    let mut j = i + 5;
    for _ in 0..9 {
        j += if rec[j] == 0.0 { 4 } else { 1 };
    }
    j - i
}

/// `topology.brep_mirror`'s `geometry` record with every edge use's UV trim block replaced
/// by `-1` (no trim).
pub(crate) fn blind_record(rec: &[f64]) -> Vec<f64> {
    let mut out = vec![rec[0], rec[1]];
    let mut i = 2;
    for _ in 0..rec[1] as usize {
        let n = curve_block(rec, i);
        out.extend_from_slice(&rec[i..i + n]);
        i += n;
        let uses = rec[i] as usize;
        out.push(rec[i]);
        i += 1;
        for _ in 0..uses {
            i += curve_block(rec, i);
            out.push(-1.0);
        }
    }
    out.extend_from_slice(&rec[i..]);
    out
}

/// The values of `out` with its `record` blinded ([`blind_record`]) and every UV trim owner
/// `-1`.
pub(crate) fn blinded_values(out: &Output) -> std::collections::BTreeMap<String, Vec<f64>> {
    let mut values = out.values.clone();
    if let Some(r) = values.get_mut("record") {
        *r = blind_record(r);
    }
    if let Some(o) = values.get_mut("uv_curve_owners") {
        for x in o.iter_mut() {
            *x = -1.0;
        }
    }
    values
}

/// The FNV-1a digest of `values` and `ints`: for each array in key order its key, a tag
/// (`0` values, `1` ints), its length and its elements' bits (a NaN as `f64::NAN`'s).
pub(crate) fn digest(
    values: &std::collections::BTreeMap<String, Vec<f64>>,
    ints: &std::collections::BTreeMap<String, Vec<i64>>,
) -> u64 {
    let mut h = FNV_OFFSET;
    for (k, a) in values {
        h = fnv(h, k.as_bytes());
        h = fnv(h, &[0]);
        h = fnv(h, &(a.len() as u64).to_le_bytes());
        for &x in a {
            let x = if x.is_nan() { f64::NAN } else { x };
            h = fnv(h, &x.to_bits().to_le_bytes());
        }
    }
    for (k, a) in ints {
        h = fnv(h, k.as_bytes());
        h = fnv(h, &[1]);
        h = fnv(h, &(a.len() as u64).to_le_bytes());
        for &x in a {
            h = fnv(h, &x.to_le_bytes());
        }
    }
    h
}

/// [`digest`] of `out` blinded ([`blinded_values`]) with each moved value's slot set to
/// `main` (`put_main`), each first checked to hold `now`.
fn blinded_digest(out: &Output, moved: &[Moved], put_main: bool) -> u64 {
    let mut values = blinded_values(out);
    for &(key, i, main, now) in moved {
        let a = values.get_mut(key).unwrap_or_else(|| panic!("no {key}"));
        assert!(
            a[i] == now || (a[i].is_nan() && now.is_nan()),
            "{key}[{i}] is {:e}, not {now:e}",
            a[i]
        );
        if put_main {
            a[i] = main;
        }
    }
    digest(&values, &out.ints)
}

/// A moved value: its array, its index in the blinded array, main's value and this
/// branch's.
pub(crate) type Moved = (&'static str, usize, f64, f64);

/// How a moved value compares with main's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    Closer,
    Equal,
    Farther,
}

/// Classes two enclosures of the same distance, main's `m` and this branch's `n`.
fn class_of(m: Enclosure, n: Enclosure) -> Class {
    if n.hi < m.lo {
        Class::Closer
    } else if n.lo > m.hi {
        Class::Farther
    } else {
        Class::Equal
    }
}

/// The surfaces of the faces of `e`'s uses.
fn edge_faces(b: &Brep, e: EdgeId) -> Vec<FaceId> {
    let mut out = Vec::new();
    for &eu in &b.edge(e).unwrap().edgeuses {
        if let Some(f) = eu1::face(b, eu).unwrap() {
            if !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

/// The edges ending at `v`.
fn vertex_edges(b: &Brep, v: VertexId) -> Vec<EdgeId> {
    solidcore::topology::brep::p1::edges(b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| b.edge(e).unwrap().vertices.contains(&v))
        .collect()
}

/// The enclosure of the distance from `p` to `f`'s surface, around a drop's foot.
fn to_face(k: &Kernel, b: &Brep, f: FaceId, p: Point3) -> Enclosure {
    let surface = b.face(f).unwrap().surface.as_ref().unwrap();
    let measured = crate::certify::measured(surface);
    let domain = measured.natural_uv_domain_with_nurb(k).unwrap();
    let g = measured
        .drop_point::<NoObject>(k, p, &domain, &DropPointOptions::default())
        .unwrap();
    nearest(k, surface, None, p, g.result_uv)
}

/// Classes a moved point against the faces it lies on: `closer` on one face and `farther`
/// on none, `farther` on one, else `equal`.
fn point_class(k: &Kernel, b: &Brep, faces: &[FaceId], main: Point3, now: Point3) -> Class {
    let mut class = Class::Equal;
    for &f in faces {
        let (m, n) = (to_face(k, b, f, main), to_face(k, b, f, now));
        let c = class_of(m, n);
        eprintln!(
            "    face {}: main [{:e}, {:e}], now [{:e}, {:e}]: {c:?}",
            f.index(),
            m.lo,
            m.hi,
            n.lo,
            n.hi
        );
        match c {
            Class::Farther => return Class::Farther,
            Class::Closer => class = Class::Closer,
            Class::Equal => {}
        }
    }
    class
}

/// An upper bound on the distance from `v`'s point to the nearer end of each of its edges'
/// curves, the ends enclosed in intervals: the most over the edges.
fn vertex_need(b: &Brep, v: VertexId) -> f64 {
    let at = b.vertex(v).unwrap().point;
    let mut need = 0.0_f64;
    for e in vertex_edges(b, v) {
        let rec = b.edge(e).unwrap();
        let cn = curve_nurbs(rec.curve.as_ref().unwrap()).unwrap();
        let mut nearest_end = f64::INFINITY;
        for s in [rec.interval.min(), rec.interval.max()] {
            let c = curve_derivs_over(&cn, Ivl::from(s), 0, None).unwrap()[0];
            let d = norm_hi(V3 {
                x: c.x - Ivl::from(at.x),
                y: c.y - Ivl::from(at.y),
                z: c.z - Ivl::from(at.z),
            });
            nearest_end = nearest_end.min(d);
        }
        need = need.max(nearest_end);
    }
    round_up(need)
}

/// Where a moved value of `topology.brep_mirror`'s record or `graph`'s geometry lies.
enum Site {
    /// Coordinate `c` of sample `s` of edge `e`'s curve.
    EdgeSample { e: usize, s: usize, c: usize },
    /// Coordinate `c` (3: the tolerance) of the vertex numbered `v` in the geometry record.
    Vertex { v: usize, c: usize },
    /// Coordinate `c` of sample `s` of face `f`'s surface.
    FaceSample { f: usize, s: usize, c: usize },
    /// The tolerance of the edge numbered `e` in the geometry record (`graph`'s walk order).
    EdgeTolerance { e: usize },
}

/// The site of blinded record index `i`.
fn record_site(rec: &[f64], i: usize) -> Site {
    let mut j = 2;
    for e in 0..rec[1] as usize {
        let n = curve_block(rec, j);
        if i < j + n {
            let mut at = j + 3;
            for s in 0..5 {
                if rec[at] == 0.0 {
                    if i > at && i < at + 4 {
                        return Site::EdgeSample {
                            e,
                            s,
                            c: i - at - 1,
                        };
                    }
                    at += 4;
                } else {
                    at += 1;
                }
            }
            panic!("record[{i}] is not a sample coordinate of edge {e}");
        }
        j += n;
        let uses = rec[j] as usize;
        j += 1 + uses;
    }
    let faces = rec[j] as usize;
    j += 1;
    for f in 0..faces {
        let n = surface_block(rec, j);
        if i < j + n {
            let mut at = j + 5;
            for s in 0..9 {
                if rec[at] == 0.0 {
                    if i > at && i < at + 4 {
                        return Site::FaceSample {
                            f,
                            s,
                            c: i - at - 1,
                        };
                    }
                    at += 4;
                } else {
                    at += 1;
                }
            }
            panic!("record[{i}] is not a sample coordinate of face {f}");
        }
        j += n;
    }
    panic!("record[{i}] is past the faces");
}

/// The site of geometry index `i` (`graph`'s layout: vertex uses' UV, vertices' point and
/// tolerance, faces, edges).
fn geometry_site(ints: &[i64], i: usize) -> Site {
    let (nf, ne, nvu, nv) = (
        ints[3] as usize,
        ints[7] as usize,
        ints[8] as usize,
        ints[9] as usize,
    );
    assert!(i >= 2 * nvu, "geometry[{i}] is a vertex use's");
    let r = i - 2 * nvu;
    if r < 4 * nv {
        return Site::Vertex { v: r / 4, c: r % 4 };
    }
    let r = r - 4 * nv - 5 * nf;
    assert!(
        r < 3 * ne && r % 3 == 2,
        "geometry[{i}] is not a vertex's or an edge's tolerance"
    );
    Site::EdgeTolerance { e: r / 3 }
}

/// An upper bound on how far `e`'s curve lies from its uses' trims' images (each trim
/// certified, [`certify_brep`]) and its ends from its vertices: the most of these.
fn edge_need(b: &Brep, certs: &[crate::trims_on_edges::TrimCertificate], e: EdgeId) -> f64 {
    let mut need = 0.0_f64;
    for c in certs.iter().filter(|c| c.edge == e) {
        need = need.max(c.bound);
    }
    let rec = b.edge(e).unwrap();
    let cn = curve_nurbs(rec.curve.as_ref().unwrap()).unwrap();
    for v in &rec.vertices {
        let at = b.vertex(*v).unwrap().point;
        let mut nearest_end = f64::INFINITY;
        for s in [rec.interval.min(), rec.interval.max()] {
            let c = curve_derivs_over(&cn, Ivl::from(s), 0, None).unwrap()[0];
            nearest_end = nearest_end.min(norm_hi(V3 {
                x: c.x - Ivl::from(at.x),
                y: c.y - Ivl::from(at.y),
                z: c.z - Ivl::from(at.z),
            }));
        }
        need = need.max(nearest_end);
    }
    round_up(need)
}

/// Certifies one row of a unit whose output is `topology.brep_mirror`'s record and `graph`
/// (see the module docs): `out` the case's output, `b` its result body.
pub(crate) fn certify_row(
    name: &str,
    out: &Output,
    b: &mut Brep,
    main_digest: u64,
    new_digest: u64,
    moved: &[Moved],
) -> Vec<Class> {
    let mut k = Kernel::default();
    // replay
    assert_eq!(
        blinded_digest(out, moved, false),
        new_digest,
        "{name}: not the row"
    );
    assert_eq!(
        blinded_digest(out, moved, true),
        main_digest,
        "{name}: more than the listed values moved"
    );
    // soundness
    let s = validation::soundness(&mut k, b).unwrap();
    if !s.sound() {
        eprintln!("UNSOUND {name}: {:?}", s.check.lines());
    }
    // moved values
    let record = out
        .values
        .get("record")
        .map(|r| blind_record(r))
        .unwrap_or_default();
    let geometry = out.values.get("geometry").cloned().unwrap_or_default();
    let graph = out.ints.get("graph").cloned().unwrap_or_default();
    let edges = solidcore::topology::brep::p1::edges(b, None).unwrap();
    let faces = solidcore::topology::brep::p1::faces(b, None).unwrap();
    let walked_edges = crate::units::boolean_merge_p1::Canon::reachable(b)
        .edges
        .items;
    let certs = certify_brep(&k, b).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut classes = Vec::new();
    // a point is classed once, all its moved coordinates put back together
    let mut done: Vec<(&str, usize)> = Vec::new();
    // the edges a moved value lies on or ends
    let mut touched: Vec<EdgeId> = Vec::new();
    for &(key, i, _, _) in moved {
        let site = match key {
            "record" => record_site(&record, i),
            "geometry" => geometry_site(&graph, i),
            _ => panic!("{name}: a moved {key} value"),
        };
        let main_of = |arr: &str, j: usize, now: f64| {
            moved
                .iter()
                .find(|m| m.0 == arr && m.1 == j)
                .map_or(now, |m| m.2)
        };
        match site {
            Site::EdgeSample { e, s, c } => {
                let start = i - c - 1;
                if done.contains(&("record", start)) {
                    continue;
                }
                done.push(("record", start));
                let now = Point3::new(record[start + 1], record[start + 2], record[start + 3]);
                let main = Point3::new(
                    main_of("record", start + 1, now.x),
                    main_of("record", start + 2, now.y),
                    main_of("record", start + 3, now.z),
                );
                eprintln!("  edge {e} sample {s}: main {main:?}, now {now:?}");
                touched.push(edges[e]);
                let faces = edge_faces(b, edges[e]);
                classes.push(point_class(&k, b, &faces, main, now));
            }
            Site::FaceSample { f, s, c } => {
                // a sample of the face's own surface at a fixed point of its natural domain:
                // main's and this branch's distance to the surface, enclosed
                let start = i - c - 1;
                if done.contains(&("record", start)) {
                    continue;
                }
                done.push(("record", start));
                let now = Point3::new(record[start + 1], record[start + 2], record[start + 3]);
                let main = Point3::new(
                    main_of("record", start + 1, now.x),
                    main_of("record", start + 2, now.y),
                    main_of("record", start + 3, now.z),
                );
                eprintln!("  face {f} sample {s}: main {main:?}, now {now:?}");
                classes.push(point_class(&k, b, &faces[f..=f], main, now));
            }
            Site::EdgeTolerance { e } => {
                let eid = walked_edges[e];
                touched.push(eid);
                let (m, n) = (main_of("geometry", i, geometry[i]), geometry[i]);
                assert_eq!(b.edge(eid).unwrap().tolerance, n, "{name}: edge {e}");
                let need = edge_need(b, &certs, eid);
                assert!(
                    m >= need && n >= need,
                    "{name}: edge {e} tolerance {m:e} / {n:e} under {need:e}"
                );
                let class = if n < m {
                    Class::Closer
                } else if n - m <= need {
                    Class::Equal
                } else {
                    Class::Farther
                };
                eprintln!(
                    "  edge {e} tolerance: main {m:e}, now {n:e}, need at most {need:e}: {class:?}"
                );
                classes.push(class);
            }
            Site::Vertex { v, c } => {
                let start = i - c;
                let now = Point3::new(geometry[start], geometry[start + 1], geometry[start + 2]);
                let vid = b
                    .vertices()
                    .iter()
                    .find(|(_, r)| r.point == now)
                    .map_or_else(|| panic!("{name}: no vertex at {now:?}"), |(id, _)| id);
                touched.extend(vertex_edges(b, vid));
                if c == 3 {
                    let (m, n) = (main_of("geometry", i, geometry[i]), geometry[i]);
                    let need = vertex_need(b, vid);
                    assert!(
                        m >= need && n >= need,
                        "{name}: vertex {v} tolerance {m:e} / {n:e} under {need:e}"
                    );
                    let class = if n < m {
                        Class::Closer
                    } else if n - m <= need {
                        Class::Equal
                    } else {
                        Class::Farther
                    };
                    eprintln!("  vertex {v} tolerance: main {m:e}, now {n:e}, need at most {need:e}: {class:?}");
                    classes.push(class);
                    continue;
                }
                if done.contains(&("geometry", start)) {
                    continue;
                }
                done.push(("geometry", start));
                let main = Point3::new(
                    main_of("geometry", start, now.x),
                    main_of("geometry", start + 1, now.y),
                    main_of("geometry", start + 2, now.z),
                );
                eprintln!("  vertex {v}: main {main:?}, now {now:?}");
                let mut faces = Vec::new();
                for e in vertex_edges(b, vid) {
                    for f in edge_faces(b, e) {
                        if !faces.contains(&f) {
                            faces.push(f);
                        }
                    }
                }
                classes.push(point_class(&k, b, &faces, main, now));
            }
        }
    }
    // trims on the touched edges: every other value is the main's (the digests), its trims
    // read the same in the record
    let mut held = 0;
    let mut untouched_off = 0;
    for c in &certs {
        if touched.contains(&c.edge) {
            assert!(c.holds(), "{name}: {c:?}");
            eprintln!(
                "  trim of {:?} on {:?}: bound {:e} (at midpoints {:e}) of {:e}, {} pieces, {}, \
                 {:e} past its surface's knots",
                c.edgeuse,
                c.edge,
                c.bound,
                c.sampled,
                c.tolerance,
                c.pieces,
                if c.reversed { "reversed" } else { "forward" },
                c.past
            );
            held += 1;
        } else if !c.holds() {
            untouched_off += 1;
        }
    }
    eprintln!(
        "{name}: {} trims certified, {held} on touched edges held, {untouched_off} off its \
         edge's tolerance on an untouched edge (as the main's)",
        certs.len()
    );
    for c in &classes {
        assert_ne!(*c, Class::Farther, "{name}: a moved value is farther");
    }
    classes
}

mod local_operation {
    use super::{certify_row, Class};
    use crate::units::topology_brep_local_operation::{result_breps, Runner};
    use crate::UnitDiff;

    struct Row {
        case: &'static str,
        main: u64,
        new: u64,
        moved: &'static [super::Moved],
    }

    include!("operand_trim_rows/local_operation.rs");

    /// Every `topology.brep_local_operation` row the copy moves (67): certified as the
    /// module docs say; none farther.
    #[test]
    fn local_operation_rows_are_certified() {
        let (mut closer, mut equal) = (0, 0);
        for row in ROWS {
            let case = crate::case::regression_case(&Runner, row.case);
            let out = Runner.run_rust(&case).unwrap();
            let (_, mut bodies) = result_breps(&case).unwrap();
            let (_, b) = &mut bodies[0];
            for c in certify_row(row.case, &out, b, row.main, row.new, row.moved) {
                match c {
                    Class::Closer => closer += 1,
                    Class::Equal => equal += 1,
                    Class::Farther => unreachable!(),
                }
            }
        }
        eprintln!(
            "{} rows: moved values closer {closer}, equal {equal}",
            ROWS.len()
        );
        assert_eq!(ROWS.len(), 67);
    }
}

/// The rows of `boolean.merge_p4` and `topology.brep_p7` the copy moves: their outputs write
/// whole bodies (`topology.brep_p10`'s snapshot, `p7`'s destination graph), where the move is
/// a UV trim block that main left empty. A row is certified by
/// * **replay**: the case's Rust side, run again, has the output digest ([`digest`]) recorded
///   for this branch, so the run is the row;
/// * **only trims moved**: run again with every edge use's trim cleared on each body the
///   unit writes ([`crate::trim_hook`]) just before it is written, its output has the digest
///   main's (7d4445c0c) code has run alike, so every value but the trim blocks is main's,
///   bit for bit;
/// * **trims on edges**: every trim each written body holds lies on its edge within the
///   edge's tolerance ([`crate::trims_on_edges`], rounded interval arithmetic), so each
///   added trim is the edge's own.
mod trim_blocks {
    use super::digest;
    use crate::trim_hook::with_hook;
    use crate::trims_on_edges::{certify_brep, TrimCertificate};
    use crate::UnitDiff;
    use solidcore::kernel::Kernel;
    use solidcore::topology::Brep;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Row {
        case: &'static str,
        new: u64,
        main: u64,
    }

    fn clear_trims(b: &mut Brep) {
        let ids: Vec<_> = b.edgeuses().ids().collect();
        for eu in ids {
            b.edgeuse_mut(eu).unwrap().uv_trim_curve = None;
        }
    }

    /// A scratch copy of `b` where each edge use of a face that holds no trim, nor its mate,
    /// gets the one `create_uv_trim_curve` drops from its edge (as a caller asking for it
    /// makes it): the copy and the number made.
    fn remade(k: &Kernel, b: &Brep) -> (Brep, usize) {
        use solidcore::topology::edgeuse::{p1 as eu1, p3 as eu3};
        let mut s = b.clone_standing_for_self().unwrap();
        let ids: Vec<_> = s.edgeuses().ids().collect();
        let mut made = 0;
        for eu in ids {
            if eu1::face(&s, eu).unwrap().is_none()
                || eu1::uv_trim_curve(&s, eu).unwrap().0.is_some()
            {
                continue;
            }
            let c = eu3::create_uv_trim_curve(k, &mut s, eu, false, None).unwrap();
            s.edgeuse_mut(eu).unwrap().uv_trim_curve = Some(c.curve);
            made += 1;
        }
        (s, made)
    }

    /// Certifies `rows` of `runner`'s unit (see the module docs): the trims certified and
    /// the trims made. With `remake`, each written body's missing trims are first made on a
    /// scratch copy ([`remade`]) and certified with the rest.
    fn certify(runner: &dyn UnitDiff, rows: &[Row], remake: bool) -> (usize, usize) {
        let mut trims = 0;
        let made_all: Rc<RefCell<usize>> = Rc::default();
        for row in rows {
            let case = crate::case::regression_case(runner, row.case);
            let certs: Rc<RefCell<Vec<TrimCertificate>>> = Rc::default();
            let sink = Rc::clone(&certs);
            let made_sink = Rc::clone(&made_all);
            let name = row.case;
            let out = with_hook(
                Box::new(move |b: &mut Brep| {
                    let k = Kernel::default();
                    let c = if remake {
                        let (s, made) = remade(&k, b);
                        eprintln!("{name}: {made} trims made");
                        *made_sink.borrow_mut() += made;
                        certify_brep(&k, &s)
                    } else {
                        certify_brep(&k, b)
                    }
                    .unwrap_or_else(|e| panic!("{name}: {e}"));
                    sink.borrow_mut().extend(c);
                }),
                || runner.run_rust(&case),
            )
            .unwrap();
            assert_eq!(
                digest(&out.values, &out.ints),
                row.new,
                "{name}: not the row"
            );
            let blind = with_hook(Box::new(clear_trims), || runner.run_rust(&case)).unwrap();
            assert_eq!(
                digest(&blind.values, &blind.ints),
                row.main,
                "{name}: more than the trims moved"
            );
            let certs = certs.borrow();
            assert!(!certs.is_empty(), "{name}: no trims");
            for c in certs.iter() {
                assert!(c.holds(), "{name}: {c:?}");
            }
            eprintln!("{name}: {} trims certified on their edges", certs.len());
            trims += certs.len();
        }
        let made = *made_all.borrow();
        (trims, made)
    }

    /// The 15 `boolean.merge_p4` rows.
    #[test]
    fn merge_p4_rows_are_certified() {
        let (trims, _) = certify(&crate::units::boolean_merge_p4::Runner, MERGE_P4, false);
        eprintln!("{} rows, {trims} trims", MERGE_P4.len());
        assert_eq!(MERGE_P4.len(), 15);
    }

    /// The 9 `topology.brep_p7` rows.
    #[test]
    fn brep_p7_rows_are_certified() {
        let (trims, _) = certify(&crate::units::topology_brep_p7::Runner, BREP_P7, false);
        eprintln!("{} rows, {trims} trims", BREP_P7.len());
        assert_eq!(BREP_P7.len(), 9);
    }

    /// The `topology.brep_p10` subdivide cases the trim drop moves (two rows, plus two that
    /// move below the comparison tolerance): their faces' surfaces changed kind in the
    /// separating cut, so this branch keeps no trim on them (`brep::p9`'s
    /// `drop_face_trims`), where main kept the old ones. Nothing but the trim blocks moved,
    /// and the trims made again from the edges where a caller asks for them lie on their
    /// edges.
    #[test]
    fn brep_p10_rows_are_certified() {
        let (trims, made) = certify(&crate::units::topology_brep_p10::Runner, BREP_P10, true);
        eprintln!("{} cases, {trims} trims, {made} made again", BREP_P10.len());
        assert_eq!(BREP_P10.len(), 4);
        // the dropped trims were made again and certified (the two rows; the other two
        // cases keep every trim)
        assert!(made > 0, "no dropped trim was made again");
    }

    include!("operand_trim_rows/trim_blocks.rs");
}
