//! Certificates of the golden rows the operand-trim copy moves (tests only;
//! `keep_operand_uv_trims` retired, #14246): `boolean::merge::p2`'s new faces keep their
//! operand faces' UV trims where the surface keeps its parameterization, so a result's edge
//! uses that had none now carry one.
//!
//! A row is certified as follows.
//! * **Replay.** The case's Rust side is run again. Its output has the digest recorded for
//!   this branch ([`digest`]), so the run is the row. With each listed moved value put back
//!   to main's (7d4445c0c) value, it has the digest of main's output: every value but the
//!   listed ones is main's, bit for bit.
//! * **Soundness.** The result body's `check_brep` level 2 reports no line main's same body
//!   does not ([`level2_within_main`]) ([`validation::soundness`]), but for a line certified
//!   a false positive of its check ([`face34`]: test #34 on main's own geometry at a
//!   tolerance no larger than main's, coincident only inside the end contact of the edges'
//!   transversal meeting at their shared vertex).
//! * **Trims on edges.** Every trim the body holds on an edge a moved value touches lies on
//!   its edge within the edge's tolerance, and its edge within that of the trim's image
//!   ([`crate::trims_on_edges`], rounded interval arithmetic). A trim on another edge that
//!   does not fails.
//! * **Moved values.** Each listed moved value is a coordinate of an edge curve's sample
//!   point or of a vertex, or a vertex's tolerance. A point, main's and this branch's, has its
//!   distance to each surface of the faces it lies on enclosed
//!   ([`crate::certify::nearest_with_rounding`]); the move is `closer` where this branch's
//!   upper end is below main's lower end on some face and `farther` on none; `equal` where
//!   the enclosures meet and this branch's upper end is at most main's plus the rounding the
//!   two upper ends carry (each its evaluation's slack and `γ_8` of its distance); `farther`
//!   otherwise ([`class_of`]: most lower ends here are 0, where meeting alone proves
//!   nothing). A tolerance must cover its vertex's distance to its edges' curve ends and to
//!   the end images of their uses' trims (enclosed in intervals), and its edges' needs (below: the
//!   ball holds each edge's tolerance tube where it ends); both values must, and the move
//!   is `closer` when this branch's is the smaller, `equal` when the two differ by at most that distance's upper end (each
//!   records the kernel default `1e-5` plus a measure of a gap that small), `farther`
//!   otherwise. An edge's tolerance is classed alike against the most of its uses' trims'
//!   certified bounds and its ends' distances to its vertices. A moved sample of a face's
//!   surface (a fixed point of its natural domain) is classed as an edge's sample, against
//!   that face alone.
//!
//! A `farther` move fails the test.
//!
//! `topology.brep_local_operation`'s bodies carry the whole-body part alone, on the current
//! kernel with no moved value (`local_operation::local_operation_bodies_are_certified`):
//! every UV trim proved within its edge's tolerance, and level 2 nothing but the input
//! model's own lines.
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

use crate::certify::{nearest_with_rounding, Enclosure};
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

/// Classes two enclosures of the same distance, main's `m` and this branch's `n`, whose upper
/// ends carry `rounding` between them (the sum of what each adds to its computed distance:
/// [`nearest_with_rounding`]). `closer` where this branch's upper end is below main's lower
/// end; `equal` where the two meet and this branch's upper end is at most main's plus that
/// rounding, so the two certified bounds agree to the precision they are computed at;
/// `farther` otherwise: farther proved (this branch's lower end above main's upper end), or
/// this branch's bound above main's by more than their rounding, so it is not shown no
/// farther (where a lower end is 0, meeting alone proves nothing). An unbounded upper
/// end is `equal` only to an unbounded main's; a NaN is `farther`.
fn class_of(m: Enclosure, n: Enclosure, rounding: f64) -> Class {
    if n.hi < m.lo {
        Class::Closer
    } else if n.lo <= m.hi
        && (n.hi <= m.hi || (rounding.is_finite() && n.hi <= round_up(m.hi + rounding)))
    {
        Class::Equal
    } else {
        Class::Farther
    }
}

/// Enclosures that meet are not `equal` when this branch's upper end is far above main's
/// (#19318): main's distance at most 1e-12, this branch's at most 1e-9, both lower ends 0.
#[test]
fn meeting_enclosures_with_a_far_higher_upper_end_are_not_equal() {
    let m = Enclosure { lo: 0.0, hi: 1e-12 };
    let n = Enclosure { lo: 0.0, hi: 1e-9 };
    assert_eq!(class_of(m, n, 2e-15), Class::Farther);
    // within the rounding the two ends carry, and below main's: equal
    let within = Enclosure {
        lo: 0.0,
        hi: 1e-12 + 1.5e-15,
    };
    assert_eq!(class_of(m, within, 2e-15), Class::Equal);
    assert_eq!(
        class_of(m, Enclosure { lo: 0.0, hi: 5e-13 }, 0.0),
        Class::Equal
    );
    // the bound proved below main's lower end: closer; above its upper end: farther
    let m = Enclosure { lo: 1e-9, hi: 2e-9 };
    assert_eq!(
        class_of(m, Enclosure { lo: 0.0, hi: 5e-10 }, 0.0),
        Class::Closer
    );
    assert_eq!(
        class_of(m, Enclosure { lo: 3e-9, hi: 4e-9 }, 1.0),
        Class::Farther
    );
    // unbounded: equal only to an unbounded main's; NaN refuses
    let inf = Enclosure {
        lo: 0.0,
        hi: f64::INFINITY,
    };
    assert_eq!(class_of(m, inf, f64::INFINITY), Class::Farther);
    assert_eq!(class_of(inf, inf, f64::INFINITY), Class::Equal);
    let nan = Enclosure {
        lo: f64::NAN,
        hi: f64::NAN,
    };
    assert_eq!(class_of(m, nan, 0.0), Class::Farther);
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

/// `e`'s start and end vertex, read through its uses (`GetVertices`): a body an operation
/// returns without `Brep::refresh_cached_topology` (a feature body) can hold a stale
/// `Edge::vertices` pair naming a vertex no longer in the arena.
fn edge_ends(b: &Brep, e: EdgeId) -> Vec<VertexId> {
    let (s, t) = solidcore::topology::edge::start_end_vertices(b, e).unwrap();
    s.into_iter().chain(t).collect()
}

/// The edges ending at `v`.
fn vertex_edges(b: &Brep, v: VertexId) -> Vec<EdgeId> {
    solidcore::topology::brep::p1::edges(b, None)
        .unwrap()
        .into_iter()
        .filter(|&e| edge_ends(b, e).contains(&v))
        .collect()
}

/// The enclosure of the distance from `p` to `f`'s surface, around a drop's foot, and the
/// rounding its upper end carries ([`nearest_with_rounding`]).
fn to_face(k: &Kernel, b: &Brep, f: FaceId, p: Point3) -> (Enclosure, f64) {
    let surface = b.face(f).unwrap().surface.as_ref().unwrap();
    let measured = crate::certify::measured(surface);
    let domain = measured.natural_uv_domain_with_nurb(k).unwrap();
    let g = measured
        .drop_point::<NoObject>(k, p, &domain, &DropPointOptions::default())
        .unwrap();
    nearest_with_rounding(k, surface, None, p, g.result_uv)
}

/// Classes a moved point against the faces it lies on: `closer` on one face and `farther`
/// on none, `farther` on one, else `equal` ([`class_of`], with the rounding of both upper
/// ends).
fn point_class(k: &Kernel, b: &Brep, faces: &[FaceId], main: Point3, now: Point3) -> Class {
    let mut class = Class::Equal;
    for &f in faces {
        let ((m, rm), (n, rn)) = (to_face(k, b, f, main), to_face(k, b, f, now));
        let c = class_of(m, n, rm + rn);
        eprintln!(
            "    face {}: main [{:e}, {:e}], now [{:e}, {:e}], rounding {:e}: {c:?}",
            f.index(),
            m.lo,
            m.hi,
            n.lo,
            n.hi,
            rm + rn
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
/// curves and of each of its edges' uses' trims' images (the vertex's ball holds the ends
/// of both: [`crate::trims_on_edges::trim_end_images`]), the ends enclosed in intervals,
/// and on each of its edges' [`edge_need`] (the ball holds the edge's tolerance tube where
/// it ends, as the kernel keeps a vertex's tolerance at least its edges'): the most of these.
fn vertex_need(b: &Brep, certs: &[crate::trims_on_edges::TrimCertificate], v: VertexId) -> f64 {
    let k = Kernel::default();
    let at = b.vertex(v).unwrap().point;
    let to_v = |c: V3<Ivl>| {
        norm_hi(V3 {
            x: c.x - Ivl::from(at.x),
            y: c.y - Ivl::from(at.y),
            z: c.z - Ivl::from(at.z),
        })
    };
    let mut need = 0.0_f64;
    for e in vertex_edges(b, v) {
        let rec = b.edge(e).unwrap();
        let cn = curve_nurbs(rec.curve.as_ref().unwrap()).unwrap();
        let mut nearest_end = f64::INFINITY;
        for s in [rec.interval.min(), rec.interval.max()] {
            let c = curve_derivs_over(&cn, Ivl::from(s), 0, None).unwrap()[0];
            nearest_end = nearest_end.min(to_v(c));
        }
        need = need.max(nearest_end);
        for (eu, r) in b.edgeuses().iter() {
            if r.edge == Some(e) && r.uv_trim_curve.is_some() {
                let [p0, p1] = crate::trims_on_edges::trim_end_images(&k, b, eu).unwrap();
                need = need.max(to_v(p0).min(to_v(p1)));
            }
        }
        need = need.max(edge_need(b, certs, e));
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
    /// Bound `c` (`u0, v0, u1, v1`) of the UV domain of the face numbered `f` in the
    /// geometry record (`graph`'s walk order).
    FaceDomain { f: usize, c: usize },
    /// A bound of the natural UV domain of face `f`'s surface in the record.
    RecordDomain { f: usize },
}

/// The site of record index `i`: the edge count at `head`, each edge use taking `slot`
/// values (`1` a blinded trim, `0` a record written without trims).
fn record_site_in(rec: &[f64], i: usize, head: usize, slot: usize) -> Site {
    let mut j = head + 1;
    for e in 0..rec[head] as usize {
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
        j += 1 + slot * uses;
    }
    let faces = rec[j] as usize;
    j += 1;
    for f in 0..faces {
        let n = surface_block(rec, j);
        if i < j + n {
            if i > j && i < j + 5 {
                return Site::RecordDomain { f };
            }
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
    let r = r - 4 * nv;
    if r < 5 * nf {
        assert!(r % 5 < 4, "geometry[{i}] is a face's tolerance");
        return Site::FaceDomain { f: r / 5, c: r % 5 };
    }
    let r = r - 5 * nf;
    assert!(
        r < 3 * ne && r % 3 == 2,
        "geometry[{i}] is not a vertex's or an edge's tolerance"
    );
    Site::EdgeTolerance { e: r / 3 }
}

/// The class of a tolerance moved from main's `m` to `n`, both against `need`, the bound
/// the tolerance must cover (`n >= need` already asserted): lowered is closer; raised by at
/// most `need` is equal (within what the trims need); raised from under its need to at most
/// twice it is closer (main's under the bound, this branch's covering it with no more slack
/// than the need itself); any other raise is farther.
fn tolerance_class(m: f64, n: f64, need: f64) -> Class {
    if n < m || (m < need && n - need <= need) {
        Class::Closer
    } else if m >= need && n - m <= need {
        Class::Equal
    } else {
        Class::Farther
    }
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
    for v in edge_ends(b, e) {
        let at = b.vertex(v).unwrap().point;
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

/// [`digest`] of `out` as it is (no trims blinded: a record written without them) with each
/// moved value's slot set to `main` (`put_main`), each first checked to hold `now`.
pub(crate) fn plain_digest(out: &Output, moved: &[Moved], put_main: bool) -> u64 {
    let mut values = out.values.clone();
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

include!("operand_trim_rows/level2_main.rs");

/// `MAIN_LEVEL2`'s lines for `case`'s body `prefix` (none when it is not listed).
fn main_level2(case: &str, prefix: &str) -> &'static [&'static str] {
    MAIN_LEVEL2
        .iter()
        .find(|(c, p, _)| *c == case && *p == prefix)
        .map_or(&[], |e| e.2)
}

/// `line` with each parenthesised figure group (`(offset=…, tolerance=…)`) masked: the
/// test, its entity kind and its message, without the measured figures.
fn masked(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('(') {
        let Some(close) = rest[open..].find(')').map(|c| open + c) else {
            break;
        };
        out.push_str(&rest[..open]);
        let group = &rest[open..=close];
        out.push_str(if group.contains('=') { "(..)" } else { group });
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

mod face34;
use face34::FALSE_POSITIVES;

/// `check_brep` level 2 of a row's body, `lines`, against main's for the same body
/// (`MAIN_LEVEL2`, keyed by case and body prefix; a body not listed had none): every line is
/// one of main's, each of main's taken once, so no test fails that did not on main, nor more
/// often. A line that is not main's verbatim may take a main line of the same test whose
/// figures differ ([`masked`]): a warning on a trim (a 2D curve's cusp offset) whose trim
/// this branch copies instead of re-dropping. A line neither takes must be, verbatim, one of
/// `operand`'s, the input model's level 2 before the operation (each taken once): the
/// warning of an operand trim this branch copies where main re-dropped it.
fn level2_within_main(
    name: &str,
    prefix: &str,
    lines: &[String],
    operand: &[String],
    certified: &[&str],
) {
    let mut left: Vec<&str> = main_level2(name, prefix).to_vec();
    let mut unmatched = Vec::new();
    for l in lines {
        if certified.contains(&l.as_str()) {
            continue;
        }
        match left.iter().position(|m| m == l) {
            Some(i) => {
                left.remove(i);
            }
            None => unmatched.push(l),
        }
    }
    let mut copied: Vec<&String> = operand.iter().collect();
    for l in unmatched {
        let m = masked(l);
        if let Some(i) = left.iter().position(|x| masked(x) == m) {
            eprintln!("  level 2, figures moved: main {:?}, now {l:?}", left[i]);
            left.remove(i);
            continue;
        }
        // a warning of a trim copied from the operand: the input model reports it, figures
        // and all, before the operation
        let i = copied
            .iter()
            .position(|x| *x == l)
            .unwrap_or_else(|| panic!("{name} {prefix}: check_brep level 2 line not on main: {l}"));
        eprintln!("  level 2, the input model's own line (a copied trim): {l:?}");
        copied.remove(i);
    }
    eprintln!(
        "  level 2: {} lines, all main's ({} of main's not repeated)",
        lines.len(),
        left.len()
    );
}

/// Classes the moved values of one body of a row whose digests are already replayed:
/// `out`'s arrays `{prefix}record` (`topology.brep_mirror`'s geometry record; with
/// `blinded` a status first and each trim one slot, else written without trims) and
/// `{prefix}geometry` / `{prefix}graph` (`graph`'s), of the body `b` (see the module docs).
/// The body's `check_brep` level 2 lines must be main's ([`level2_within_main`]); when `b`
/// is the export's copy (its faces' `loops` and loops' `edges` dropped, so the lists'
/// owners no longer hold), the check runs on `raw`, the body as the operation returned it.
pub(crate) fn certify_body(
    name: &str,
    out: &Output,
    prefix: &str,
    b: &mut Brep,
    raw: Option<&mut Brep>,
    operand: &[String],
    moved: &[Moved],
    blinded: bool,
) -> Vec<Class> {
    let mut k = Kernel::default();
    // soundness
    let body: &mut Brep = match raw {
        Some(r) => r,
        None => &mut *b,
    };
    let s = validation::soundness(&mut k, body).unwrap();
    let lines = s.check.lines();
    // lines certified false positives of their check ([`face34`])
    let mut certified: Vec<&str> = Vec::new();
    for fp in FALSE_POSITIVES {
        if fp.0 == name && fp.1 == prefix {
            assert!(
                lines.iter().any(|l| l == fp.2),
                "{name} {prefix}: certified line gone, remove its FALSE_POSITIVES entry: {}",
                fp.2
            );
            let (pairs, worst) = face34::certify(&k, body, fp)
                .unwrap_or_else(|e| panic!("{name} {prefix}: {} not certified: {e}", fp.3));
            eprintln!(
                "  level 2: {} pair(s) certified a false positive ({}): main's geometry, no \
                 looser, ranges inside the shared vertex's transversal end contact (at most \
                 {worst:.3} of its reach): {}",
                pairs, fp.3, fp.2
            );
            certified.push(fp.2);
        }
    }
    level2_within_main(name, prefix, &lines, operand, &certified);
    // moved values
    let (rk, gk) = (format!("{prefix}record"), format!("{prefix}geometry"));
    let (head, slot) = if blinded { (1, 1) } else { (0, 0) };
    let record = out
        .values
        .get(&rk)
        .map(|r| if blinded { blind_record(r) } else { r.clone() })
        .unwrap_or_default();
    let geometry = out.values.get(&gk).cloned().unwrap_or_default();
    let graph = out
        .ints
        .get(&format!("{prefix}graph"))
        .cloned()
        .unwrap_or_default();
    let edges = solidcore::topology::brep::p1::edges(b, None).unwrap();
    let faces = solidcore::topology::brep::p1::faces(b, None).unwrap();
    let walked = crate::units::boolean_merge_p1::Canon::reachable(b);
    let walked_edges = walked.edges.items;
    let walked_faces = walked.faces.items;
    let certs = certify_brep(&k, b).unwrap_or_else(|e| panic!("{name}: {e}"));
    // the faces whose domain moved, each classed with its moved samples
    let mut domain_faces: Vec<FaceId> = Vec::new();
    let mut sample_class: Vec<(FaceId, Class)> = Vec::new();
    // this body's moved values, keyed `record` / `geometry`
    let moved: Vec<Moved> = moved
        .iter()
        .filter_map(|&(key, i, m, n)| {
            if key == rk {
                Some(("record", i, m, n))
            } else if key == gk {
                Some(("geometry", i, m, n))
            } else {
                None
            }
        })
        .collect();
    let moved = moved.as_slice();
    let mut classes = Vec::new();
    // a point is classed once, all its moved coordinates put back together
    let mut done: Vec<(&str, usize)> = Vec::new();
    // the edges a moved value lies on or ends
    let mut touched: Vec<EdgeId> = Vec::new();
    for &(key, i, _, _) in moved {
        let site = match key {
            "record" => record_site_in(&record, i, head, slot),
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
                let class = point_class(&k, b, &faces[f..=f], main, now);
                sample_class.push((faces[f], class));
                classes.push(class);
            }
            Site::FaceDomain { f, c } => {
                let (m, n) = (main_of("geometry", i, geometry[i]), geometry[i]);
                let fid = walked_faces[f];
                let d = b.face(fid).unwrap().uv_domain;
                let at = [d.min().x, d.min().y, d.max().x, d.max().y];
                assert_eq!(at[c], n, "{name}: face {f} domain {c}");
                eprintln!("  face {} domain {c}: main {m:e}, now {n:e}", fid.index());
                domain_faces.push(fid);
            }
            Site::RecordDomain { f } => {
                eprintln!("  face {f} record domain: index {i}");
                domain_faces.push(faces[f]);
            }
            Site::EdgeTolerance { e } => {
                let eid = walked_edges[e];
                touched.push(eid);
                let (m, n) = (main_of("geometry", i, geometry[i]), geometry[i]);
                assert_eq!(b.edge(eid).unwrap().tolerance, n, "{name}: edge {e}");
                let need = edge_need(b, &certs, eid);
                assert!(
                    n >= need,
                    "{name}: edge {e} tolerance {m:e} / {n:e} under {need:e}"
                );
                let class = tolerance_class(m, n, need);
                eprintln!(
                    "  edge {e} tolerance (geometry[{i}]): main {m:e}, now {n:e}, need at most \
                     {need:e}: {class:?}"
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
                    let need = vertex_need(b, &certs, vid);
                    assert!(
                        n >= need,
                        "{name}: vertex {v} tolerance {m:e} / {n:e} under {need:e}"
                    );
                    let class = tolerance_class(m, n, need);
                    eprintln!(
                        "  vertex {v} tolerance (geometry[{i}]): main {m:e}, now {n:e}, need at \
                         most {need:e}: {class:?}"
                    );
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
    // a moved domain bound moves the face's samples taken on it: it is classed as the most
    // distant of the face's moved samples. Where none moved (a bound moved by a few ulps that
    // the samples' evaluation does not see), every point the record takes on the face is
    // main's bit for bit (the digests), so their enclosed distances are main's: `equal`, once
    // every trim on the face's edges is certified on its edge
    domain_faces.sort();
    domain_faces.dedup();
    for f in domain_faces {
        let of_face: Vec<Class> = sample_class
            .iter()
            .filter(|(g, _)| *g == f)
            .map(|(_, c)| *c)
            .collect();
        if of_face.is_empty() {
            let on_face: Vec<_> = certs
                .iter()
                .filter(|c| edge_faces(b, c.edge).contains(&f))
                .collect();
            assert!(
                !on_face.is_empty(),
                "{name}: face {} has no trims",
                f.index()
            );
            for c in on_face {
                assert!(c.holds(), "{name}: face {} domain moved: {c:?}", f.index());
            }
            eprintln!(
                "  face {} domain: Equal (no sample moved, its trims certified)",
                f.index()
            );
            classes.push(Class::Equal);
            continue;
        }
        let class = if of_face.contains(&Class::Farther) {
            Class::Farther
        } else if of_face.contains(&Class::Equal) {
            Class::Equal
        } else {
            Class::Closer
        };
        eprintln!("  face {} domain: {class:?}", f.index());
        classes.push(class);
    }
    // trims on the touched edges: every other value is the main's (the digests), its trims
    // read the same in the record. A trim off its edge's tolerance on an untouched edge fails
    let mut held = 0;
    for c in &certs {
        if touched.contains(&c.edge) {
            assert!(c.holds(), "{name}: {c:?}");
            eprintln!(
                "  trim of {:?} on {:?}: bound {:e} (at midpoints {:e}) of {:e}, {} pieces, {}, \
                 {:e} past its surface's knots{}",
                c.edgeuse,
                c.edge,
                c.bound,
                c.sampled,
                c.tolerance,
                c.pieces,
                if c.reversed { "reversed" } else { "forward" },
                c.past,
                if c.at_vertex > 0.0 {
                    format!(
                        ", an end stretch in its vertex's ball ({:.3} of its tolerance)",
                        c.at_vertex
                    )
                } else {
                    String::new()
                }
            );
            held += 1;
        } else if !c.holds() {
            let edge = edges.iter().position(|&e| e == c.edge).unwrap();
            let edge_use = b
                .edge(c.edge)
                .unwrap()
                .edgeuses
                .iter()
                .position(|&u| u == c.edgeuse)
                .unwrap();
            panic!(
                "{name} {prefix}: edge {edge} use {edge_use}: a trim off its edge's tolerance on \
                 an untouched edge: {c:?}"
            );
        }
    }
    eprintln!(
        "{name}: {} trims certified, {held} on touched edges held",
        certs.len()
    );
    for c in &classes {
        assert_ne!(*c, Class::Farther, "{name}: a moved value is farther");
    }
    classes
}

mod local_operation {
    use solidcore::kernel::Kernel;

    use crate::units::topology_brep_local_operation::{input_brep, result_breps, Runner};
    use crate::UnitDiff;

    /// A trim off its edge's tolerance on an edge no moved value touches fails (#19318).
    /// `gen0002` moves no value; the first two trimmed uses of its first edge with two swap
    /// their trims (each now in the other face's parameters), so neither lies on its edge.
    /// (`box_face0_in` served until WR-495's exact mate left its result with no trimmed edge
    /// use, tangency-labs/solidcraft#21301.)
    #[test]
    #[should_panic(expected = "a trim off its edge's tolerance on an untouched edge")]
    fn an_off_trim_on_an_untouched_edge_fails() {
        let case = crate::case::regression_case(&Runner, "gen0002");
        let out = Runner.run_rust(&case).unwrap();
        let (_, mut bodies) = result_breps(&case).unwrap();
        let (_, b) = &mut bodies[0];
        // the check_brep level 2 runs on the body as built
        let mut raw = b.clone_standing_for_self().unwrap();
        let uses = solidcore::topology::brep::p1::edges(b, None)
            .unwrap()
            .into_iter()
            .map(|e| {
                let mut uses = b.edge(e).unwrap().edgeuses.clone();
                uses.retain(|&u| b.edgeuse(u).unwrap().uv_trim_curve.is_some());
                uses
            })
            .find(|u| u.len() >= 2)
            .expect("an edge with two trimmed uses");
        let (u0, u1) = (uses[0], uses[1]);
        let t0 = b.edgeuse(u0).unwrap().uv_trim_curve.clone();
        let t1 = b.edgeuse(u1).unwrap().uv_trim_curve.clone();
        b.edgeuse_mut(u0).unwrap().uv_trim_curve = t1;
        b.edgeuse_mut(u1).unwrap().uv_trim_curve = t0;
        super::certify_body("gen0002", &out, "", b, Some(&mut raw), &[], &[], true);
    }

    /// The `topology.brep_local_operation` cases whose bodies carry a whole-body trim
    /// certificate ([`local_operation_bodies_are_certified`]).
    const CASES: [&str; 64] = [
        "attributed_box_face0_in",
        "attributed_box_face0_out",
        "attributed_box_face1_out",
        "attributed_box_face2_out",
        "attributed_box_face3_in",
        "attributed_box_face4_out",
        "attributed_box_face5_tilt",
        "box_face0_in",
        "box_face0_out",
        "box_face1_out",
        "box_face2_out",
        "box_face3_in",
        "box_face4_out",
        "box_face5_tilt",
        "fc0000",
        "fc0006",
        "fc0008",
        "fc0025",
        "gen0000",
        "gen0002",
        "gen0003",
        "gen0004",
        "gen0006",
        "gen0009",
        "gen0011",
        "gen0016",
        "gen0021",
        "gen0025",
        "gen0026",
        "gen0031",
        "gen0033",
        "gen0039",
        "gen0040",
        "gen0041",
        "gen0042",
        "gen0043",
        "gen0047",
        "gen0048",
        "gen0050",
        "gen0051",
        "gen0052",
        "gen0054",
        "gen0055",
        "gen0057",
        "gen0058",
        "gen0059",
        "gen0061",
        "gen0065",
        "gen0067",
        "gen0068",
        "gen0069",
        "gen0070",
        "gen0071",
        "gen0072",
        "gen0074",
        "gen0076",
        "gen0077",
        "gen0079",
        "gen0080",
        "gen0081",
        "gen0082",
        "gen0087",
        "gen0092",
        "gen0095",
    ];

    /// Every body [`CASES`] write, on the current kernel: every UV trim is proved within its
    /// edge's current tolerance, by the kernel's bound
    /// ([`crate::certify::trim_tolerance::trim_bounds_any_surface`]) or, where that bound is
    /// over the tolerance, by the mean-value certifier
    /// ([`crate::trims_on_edges::certify_edgeuse`]), each such trim counted. A trim proved by
    /// neither is accepted only where its edge is a `not-enclosed` entry of
    /// `certify/trim_tolerance_open.tsv` (tangency-labs/solidcraft#21987), and every such
    /// entry of these cases must be reached. The body then passes [`super::certify_body`]
    /// with no moved value: every trim's certificate holds, and its `check_brep` level 2
    /// reports no line but the input model's own, figures and all, before the edit
    /// ([`INPUT_LINES`]).
    #[test]
    fn local_operation_bodies_are_certified() {
        let k = Kernel::default();
        let unit = Runner.id();
        let open: std::collections::BTreeSet<(String, String, usize, usize)> =
            crate::certify::trim_tolerance::not_enclosed_entries()
                .into_iter()
                .filter(|(u, c, ..)| u == unit && CASES.contains(&c.as_str()))
                .collect();
        let mut open_seen = std::collections::BTreeSet::new();
        let mut failures = Vec::new();
        let mut fallback = Vec::new();
        let mut inputs_with_lines = Vec::new();
        let (mut bodies, mut trims, mut by_kernel, mut open_trims) = (0, 0, 0, 0);
        for name in CASES {
            let case = crate::case::regression_case(&Runner, name);
            crate::certify::watch_written();
            let out = Runner.run_rust(&case);
            let written = crate::certify::take_written();
            let out = out.unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert!(!written.is_empty(), "{name}: no body written");
            // the input model's own level 2 lines, figures and all, before the edit
            let mut read = input_brep(&case).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            let input = super::validation::soundness(&mut Kernel::default(), &mut read)
                .unwrap()
                .check
                .lines();
            if !input.is_empty() {
                inputs_with_lines.push(format!("{name} ({})", input.len()));
            }
            for (bi, mut b) in written.into_iter().enumerate() {
                bodies += 1;
                let mut body_open = false;
                for e in solidcore::topology::brep::p1::edges(&b, None).expect("edges") {
                    let tol = b.edge(e).expect("edge").tolerance;
                    let key = (unit.to_string(), name.to_string(), bi, e.index() as usize);
                    for (eu, bound) in
                        crate::certify::trim_tolerance::trim_bounds_any_surface(&k, &b, e)
                    {
                        trims += 1;
                        let mean_value = match bound {
                            Some(x) if x > tol => {
                                crate::trims_on_edges::certify_edgeuse(&k, &b, eu, tol / 8.0)
                                    .ok()
                                    .filter(crate::trims_on_edges::TrimCertificate::holds)
                            }
                            _ => None,
                        };
                        match (bound, mean_value) {
                            (Some(x), None) if x <= tol => by_kernel += 1,
                            (Some(x), Some(c)) => fallback.push(format!(
                                "{name} body {bi} edge {} use {}: kernel B {x:e} over {tol:e}; \
                                 proved {:e} by trims_on_edges::certify_edgeuse",
                                e.index(),
                                eu.index(),
                                c.bound
                            )),
                            (None, _) if open.contains(&key) => {
                                open_trims += 1;
                                body_open = true;
                                open_seen.insert(key.clone());
                            }
                            (x, _) => failures.push(format!(
                                "{name} body {bi} edge {} use {}: B {x:?} over {tol:e}",
                                e.index(),
                                eu.index()
                            )),
                        }
                    }
                }
                if !body_open {
                    super::certify_body(name, &out, "", &mut b, None, &input, &[], false);
                }
            }
        }
        eprintln!(
            "local_operation_bodies_are_certified: {} cases, {bodies} bodies, {trims} trims: \
             {by_kernel} by the kernel's bound, {} by the mean-value certifier, {open_trims} \
             open; inputs with level 2 lines of their own: {inputs_with_lines:?}; by the \
             mean-value certifier:\n{}",
            CASES.len(),
            fallback.len(),
            fallback.join("\n")
        );
        assert!(
            failures.is_empty(),
            "{} failures:\n{}",
            failures.len(),
            failures.join("\n")
        );
        assert_eq!(open_seen, open, "not-enclosed entries not reached");
        assert_eq!(bodies, CASES.len(), "one body per case");
        assert_eq!(
            fallback.len(),
            MEAN_VALUE_CERTIFIED,
            "trims certified by the mean-value certifier, not the kernel's bound (#22037)"
        );
        assert_eq!(inputs_with_lines, INPUT_LINES, "inputs with level 2 lines");
    }

    /// The trims of [`CASES`]' bodies whose kernel bound is over their edge's tolerance and
    /// which the mean-value certifier proves instead (#22037): edge 89 of the four `fc`
    /// cases (bound 6.4e-3, proved 9.0e-7, tolerance 1e-5), and two each of `gen0000`,
    /// `gen0031`, `gen0041` and `gen0070` (bounds 8.5e-4 to 1.9e-2, proved at most 2.6e-6,
    /// tolerances 1e-5 and 2e-5). A change in the kernel's enclosure shows here.
    const MEAN_VALUE_CERTIFIED: usize = 12;

    /// The cases whose input model's `check_brep` level 2 reports lines of its own, with
    /// their count: `Solids/nurb_fillet.smb`'s B-spline surface discontinuity across a knot
    /// and three curve kinks at interior knots, which its four `fc` bodies keep.
    const INPUT_LINES: [&str; 4] = ["fc0000 (4)", "fc0006 (4)", "fc0008 (4)", "fc0025 (4)"];
}

/// The `blend.feature_executive` rows the copy moves (6: the four `extrusion.smb` rows move
/// because an intersection edge on a free-form face takes the intersector's trace). Four
/// more rows the copy moves are not here: on this kernel a value they pin moved off main's
/// ordering (`fillet_each_fillet1019`, `single_face_fillet1019_3`,
/// `single_face_fillet1019_6`) or their structure changed (`fillet_group_torii_fillet_b_rb23`),
/// so they could only be held to main's run; their kept trims stay certified by
/// [`crate::kept_trim_rows`]. The unit writes, for the base B-rep
/// and each feature B-rep, `topology.brep_mirror`'s geometry record without trims and
/// `graph`; each row is replayed with [`plain_digest`] (nothing blinded: every moved value is
/// listed), and each body's moved values are classed by [`certify_body`]: edge and vertex
/// tolerances against the trims' certified bounds, edge and surface samples by enclosed
/// distances, a moved surface domain as the samples it moves.
///
/// The two rows re-recorded on this kernel list every value that differs from main's row:
/// `extended_surface_7_LShape_I` 16 (values a few ulps from main's in both bodies) and
/// `fillet_group_fillet1019_rb23` 5: #17739 raises one vertex tolerance to cover twice its
/// vertex's gap, 4.7505e-4 in both bodies (`base_geometry[267]`, `feature0_geometry[171]`,
/// main 4.4536e-4), and three values of `feature0_geometry` move from main's 2.2107e-4 to
/// 4.2002e-4. All are classed against main's values like any moved value.
mod feature_executive {
    use super::{certify_body, plain_digest, Class, FALSE_POSITIVES};
    use crate::units::blend_feature_executive::{result_breps, Runner};
    use crate::UnitDiff;

    struct Row {
        case: &'static str,
        main: u64,
        new: u64,
        moved: &'static [super::Moved],
    }

    include!("operand_trim_rows/feature_executive.rs");

    #[test]
    fn feature_executive_rows_are_certified() {
        let (mut closer, mut equal) = (0, 0);
        for row in ROWS {
            let case = crate::case::regression_case(&Runner, row.case);
            let out = Runner.run_rust(&case).unwrap();
            assert_eq!(
                plain_digest(&out, row.moved, false),
                row.new,
                "{}: not the row",
                row.case
            );
            assert_eq!(
                plain_digest(&out, row.moved, true),
                row.main,
                "{}: more than the listed values moved",
                row.case
            );
            let (mut input, mut base, features) = result_breps(&case);
            // the input model's own level 2: a trim this branch copies carries its warnings
            let operand = solidcore::topology::brep::validation::soundness(
                &mut solidcore::kernel::Kernel::default(),
                &mut input,
            )
            .unwrap()
            .check
            .lines();
            let mut features: Vec<_> = features.into_iter().map(Option::unwrap).collect();
            let mut bodies = vec![("base_".to_string(), &mut base)];
            for (i, f) in features.iter_mut().enumerate() {
                bodies.push((format!("feature{i}_"), f));
            }
            for (prefix, (raw, b)) in bodies {
                for c in certify_body(
                    row.case,
                    &out,
                    &prefix,
                    b,
                    Some(raw),
                    &operand,
                    row.moved,
                    false,
                ) {
                    match c {
                        Class::Closer => closer += 1,
                        Class::Equal => equal += 1,
                        Class::Farther => panic!("{}: a moved value is {c:?}", row.case),
                    }
                }
            }
        }
        let false_positive: Vec<&str> = ROWS
            .iter()
            .map(|r| r.case)
            .filter(|c| FALSE_POSITIVES.iter().any(|b| b.0 == *c))
            .collect();
        eprintln!(
            "{} rows certified (level 2 false positives certified in {false_positive:?}): \
             moved values closer {closer}, equal {equal}",
            ROWS.len()
        );
        assert_eq!(ROWS.len(), 6);
    }
}

/// The rows of `topology.brep_p7` the copy moves (`boolean.merge_p4`'s are main's, bit for
/// bit, since a plane keeps `MakeFace`'s trim of a line edge): their outputs write whole
/// bodies (`topology.brep_p10`'s snapshot, `p7`'s destination graph), where the move is a UV
/// trim block that main left empty. A row is certified by
/// * **replay**: the case's Rust side, run again, has the output digest ([`digest`]) recorded
///   for this branch, so the run is the row;
/// * **only trims moved**: run again with every edge use's trim cleared on each body the
///   unit writes ([`crate::trim_hook`]) just before it is written, its output has the digest
///   main's (7d4445c0c) code has run alike, so every value but the trim blocks is main's,
///   bit for bit;
/// * **trims on edges**: every trim each written body holds lies on its edge within the
///   edge's tolerance ([`crate::trims_on_edges`], rounded interval arithmetic), so each
///   added trim is the edge's own.
pub(crate) mod trim_blocks {
    use super::digest;
    use crate::trim_hook::with_hook;
    use crate::trims_on_edges::{certify_brep, TrimCertificate};
    use crate::UnitDiff;
    use solidcore::kernel::Kernel;
    use solidcore::topology::Brep;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A row: its case, this branch's output digest and main's with trims cleared.
    pub(crate) struct Row {
        pub(crate) case: &'static str,
        pub(crate) new: u64,
        pub(crate) main: u64,
    }

    fn clear_trims(b: &mut Brep) {
        let ids: Vec<_> = b.edgeuses().ids().collect();
        for eu in ids {
            b.edgeuse_mut(eu).unwrap().uv_trim_curve = None;
        }
    }

    /// Certifies `rows` of `runner`'s unit (see the module docs): the trims certified.
    pub(crate) fn certify(runner: &dyn UnitDiff, rows: &[Row]) -> usize {
        let mut trims = 0;
        for row in rows {
            let case = crate::case::regression_case(runner, row.case);
            let certs: Rc<RefCell<Vec<TrimCertificate>>> = Rc::default();
            let sink = Rc::clone(&certs);
            let name = row.case;
            let out = with_hook(
                Box::new(move |b: &mut Brep| {
                    let k = Kernel::default();
                    let c = certify_brep(&k, b).unwrap_or_else(|e| panic!("{name}: {e}"));
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
        trims
    }

    /// The 6 `topology.brep_p7` rows.
    #[test]
    fn brep_p7_rows_are_certified() {
        let trims = certify(&crate::units::topology_brep_p7::Runner, BREP_P7);
        eprintln!("{} rows, {trims} trims", BREP_P7.len());
        assert_eq!(BREP_P7.len(), 6);
    }

    include!("operand_trim_rows/trim_blocks.rs");
}

/// The rows of `blend.fillet_executive_p3` the copy moves: a fillet then shell of a
/// Boolean's result, whose faces now keep their operands' UV trims (the box unions' fillet
/// rows are main's, bit for bit, since a plane keeps `MakeFace`'s trim of a line edge). A
/// row is certified as follows.
/// * **Replay.** The case's Rust side is run again; its output has the digest ([`digest`])
///   recorded for this branch, so the run is the row.
/// * **Only the listed values moved, up to a relabeling.** The output's vertices and edges
///   are renumbered by the row's committed permutations (`vertices`, `edges`: this branch's
///   index to main's; in `result_topology` and the blocks of `result_geometry`), each
///   `reversed` edge's use list read backwards (the same (face, side) uses, stored in the
///   other order), and each listed moved value put back to main's (7d4445c0c): the output
///   then has main's digest, bit for bit. So the body is main's, entity for entity, but for
///   the listed values.
/// * **Soundness.** The result body's `check_brep` level 2 reports nothing (as main's
///   bodies: both clean), it is one manifold solid of positive volume, and its volume is
///   enclosed ([`enclose_volume`], refined towards a relative width of 1e-9).
/// * **Geometry.** Each moved vertex point, edge sample or face sample is classed as in the
///   module docs (enclosed distances to the surfaces of the faces it lies on), each moved
///   vertex or edge tolerance against its need ([`vertex_need`], [`edge_need`], from the
///   body's trims certified on their edges); a moved edge interval or face domain bound is
///   classed as the most distant of its edge's or face's moved samples, or, where none
///   moved, by its exact distance to the bound's closed form (`DOMAIN_BOUNDS`). A vertex point
///   that is really farther with no certificate could only be MEASURED, listed in
///   `MEASURED_VERTICES` while it is farther and within its vertex need; none is listed.
/// * **Mass.** A moved volume with main's or this branch's value outside the enclosure is
///   classed by its distance to the true volume, enclosed by the enclosure ([`class_of`]);
///   with both inside it, neither is known nearer and the kernel's volume carries no
///   counted rounding, so the move is measured, not classed. The other mass properties (area,
///   centroid, inertia; at most 4.8e-7 relative) have no enclosure here: they are reported,
///   counted as measured, not classed.
///
/// [`enclose_volume`]: solidcore::mass::enclosure::enclose_volume
mod fillet_p3 {
    use super::{
        digest, edge_faces, edge_need, point_class, to_face, vertex_edges, vertex_need, Class,
        Moved,
    };
    use crate::trims_on_edges::certify_brep;
    use crate::units::blend_fillet_executive_p3::Runner;
    use crate::{Output, UnitDiff};
    use solidcore::kernel::Kernel;
    use solidcore::mass::enclosure::VolumeSign;
    use solidcore::math::Point3;
    use solidcore::topology::brep::p1;
    use solidcore::topology::Brep;
    use std::cell::RefCell;
    use std::ops::Range;
    use std::rc::Rc;

    struct Row {
        case: &'static str,
        main: u64,
        new: u64,
        /// this branch's vertex index -> main's (the others fixed)
        vertices: &'static [(usize, usize)],
        /// this branch's edge index -> main's (the others fixed)
        edges: &'static [(usize, usize)],
        /// this branch's edges whose use list is main's read backwards
        reversed: &'static [usize],
        moved: &'static [Moved],
    }

    include!("operand_trim_rows/fillet_p3.rs");

    type Uses = Vec<[i64; 2]>;
    /// An edge of `result_topology`: its type, vertices and uses.
    type TopoEdge = (i64, Vec<i64>, Uses);
    /// A face of `result_topology`: its type and loops.
    type TopoFace = (i64, Vec<Uses>);

    /// `result_topology`: per edge its type, vertices and (face, side) uses; per face its
    /// type and loops of (edge, side).
    fn parse(t: &[i64]) -> (Vec<TopoEdge>, Vec<TopoFace>) {
        let mut it = t.iter().copied();
        let mut next = || it.next().expect("result_topology ends early");
        let mut edges = Vec::new();
        for _ in 0..next() {
            let ty = next();
            let vs = (0..next()).map(|_| next()).collect();
            let us = (0..next()).map(|_| [next(), next()]).collect();
            edges.push((ty, vs, us));
        }
        let mut faces = Vec::new();
        for _ in 0..next() {
            let ty = next();
            let loops = (0..next())
                .map(|_| (0..next()).map(|_| [next(), next()]).collect())
                .collect();
            faces.push((ty, loops));
        }
        assert!(it.next().is_none(), "result_topology has a tail");
        (edges, faces)
    }

    /// The blocks of `result_geometry` ([`crate::util::primitive::append`]): per vertex,
    /// edge and face its range.
    fn blocks(g: &[f64], nv: usize, ne: usize, nf: usize) -> [Vec<Range<usize>>; 3] {
        let samples = |mut i: usize| {
            for _ in 0..3 {
                i += if g[i] == 0.0 { 4 } else { 1 };
            }
            i
        };
        let mut i = 0;
        let vs = (0..nv)
            .map(|_| {
                i += 4;
                i - 4..i
            })
            .collect();
        let es = (0..ne)
            .map(|_| {
                let j = i;
                i += 4;
                if g[j + 3] == 1.0 {
                    i = samples(i);
                }
                i += 2;
                j..i
            })
            .collect();
        let fs = (0..nf)
            .map(|_| {
                let j = i;
                i += 6;
                if g[j + 5] == 1.0 {
                    i = samples(i);
                }
                j..i
            })
            .collect();
        assert_eq!(i, g.len(), "result_geometry's blocks");
        [vs, es, fs]
    }

    fn map(perm: &[(usize, usize)], a: usize) -> usize {
        perm.iter().find(|p| p.0 == a).map_or(a, |p| p.1)
    }

    fn unmap(perm: &[(usize, usize)], b: usize) -> usize {
        perm.iter().find(|p| p.1 == b).map_or(b, |p| p.0)
    }

    /// `out` renumbered to main's labels (the row's permutations and reversed use lists).
    fn relabel(out: &Output, row: &Row) -> Output {
        let mut r = out.clone();
        let Some(t) = out.ints.get("result_topology") else {
            assert!(row.vertices.is_empty() && row.edges.is_empty() && row.reversed.is_empty());
            return r;
        };
        let (edges, faces) = parse(t);
        let nv = out.ints["api_status_vertex_edge_face_counts"][1] as usize;
        let g = &out.values["result_geometry"];
        let [vb, eb, fb] = blocks(g, nv, edges.len(), faces.len());
        let lab = |v: i64| map(row.vertices, v as usize) as i64;
        let mut ti = vec![edges.len() as i64];
        let mut go = Vec::new();
        for b in 0..nv {
            go.extend_from_slice(&g[vb[unmap(row.vertices, b)].clone()]);
        }
        for b in 0..edges.len() {
            let a = unmap(row.edges, b);
            let (ty, vs, us) = &edges[a];
            ti.extend([*ty, vs.len() as i64]);
            ti.extend(vs.iter().map(|&v| lab(v)));
            ti.push(us.len() as i64);
            let us: Vec<_> = if row.reversed.contains(&a) {
                us.iter().rev().collect()
            } else {
                us.iter().collect()
            };
            ti.extend(us.into_iter().flatten());
            go.extend_from_slice(&g[eb[a].clone()]);
        }
        ti.push(faces.len() as i64);
        for (ty, loops) in &faces {
            ti.extend([*ty, loops.len() as i64]);
            for l in loops {
                ti.push(l.len() as i64);
                for [e, o] in l {
                    ti.extend([map(row.edges, *e as usize) as i64, *o]);
                }
            }
        }
        for f in fb {
            go.extend_from_slice(&g[f]);
        }
        r.ints.insert("result_topology".into(), ti);
        r.values.insert("result_geometry".into(), go);
        r
    }

    fn worst(of: &[Class]) -> Class {
        assert!(!of.is_empty(), "a moved bound with no moved sample");
        if of.contains(&Class::Farther) {
            Class::Farther
        } else if of.contains(&Class::Equal) {
            Class::Equal
        } else {
            Class::Closer
        }
    }

    /// Face domain bounds known in closed form: (case, face, slot of its block, value). A
    /// moved bound with no moved sample of its face is classed by its exact distance to it.
    ///
    /// `real-box-fillet4v-r3-shell-closed`: the box `[0, 30] × [0, 20] × [0, 10]` with its
    /// four vertical edges rounded r 3 (axes at `x, y ∈ {3, 27} × {3, 17}`), shelled
    /// inwards by 2. Face 16 is the inner wall `x = 28` (a plane with `u = y`, `v = z`): the
    /// offset of the wall `x = 30` whose flat part runs `y ∈ [3, 17]` between the rounds'
    /// tangent lines (`y = 3` and `17`, where each axis lies at distance 3 from the wall), and
    /// whose offset rounds (radius 1, same axes) meet it on the same lines, between the floor
    /// and roof `z = 2` and `8`. Its natural domain's `u0` (slot 1) is therefore `3`.
    /// Face 11 is the cavity's floor `z = 2` (a plane with `u = y`, `v = x`): its boundary
    /// runs along the inner walls `x = 2`, `x = 28`, `y = 2` and `y = 18` (the rounds at the
    /// corners stay inside that rectangle), so its natural domain is `[2, 18] × [2, 28]` and
    /// its `v0` (slot 2) is `2`.
    const DOMAIN_BOUNDS: &[(&str, usize, usize, f64)] = &[
        ("real-box-fillet4v-r3-shell-closed", 16, 1, 3.0),
        ("real-box-fillet4v-r3-shell-closed", 11, 2, 2.0),
    ];

    /// Moved vertex points that are MEASURED, not certified: (case, vertex). Each is really
    /// farther from the faces it lies on than main's, and stays listed only while it is
    /// farther and its enclosed distance to each of those faces is within its
    /// [`vertex_need`]. None now: `real-box-fillet4v-r3-shell-closed` vertex 18 was listed
    /// while the table's main values came from before `CPoint::affine2`'s exact shared
    /// coordinates (#17896); against main ee673a19bf it moves by rounding only and is `Equal`.
    const MEASURED_VERTICES: &[(&str, usize)] = &[];

    /// Classes `row`'s moved geometry values (in the relabeled array `g`, key `key`) on this
    /// branch's body `b`.
    /// Also returns the moved vertices measured ([`MEASURED_VERTICES`]), not classed.
    fn certify_geometry(row: &Row, key: &str, g: &[f64], b: &Brep) -> (Vec<Class>, usize) {
        let k = Kernel::default();
        let name = row.case;
        let vertices = p1::vertices(b, None).unwrap();
        let edges = p1::edges(b, None).unwrap();
        let faces = p1::faces(b, None).unwrap();
        let [vb, eb, fb] = blocks(g, vertices.len(), edges.len(), faces.len());
        let certs = certify_brep(&k, b).unwrap_or_else(|e| panic!("{name}: {e}"));
        let moved: Vec<&Moved> = row.moved.iter().filter(|m| m.0 == key).collect();
        let main_of = |j: usize| moved.iter().find(|m| m.1 == j).map_or(g[j], |m| m.2);
        let point = |s: usize| {
            (
                Point3::new(main_of(s), main_of(s + 1), main_of(s + 2)),
                Point3::new(g[s], g[s + 1], g[s + 2]),
            )
        };
        let tolerance = |what: String, i: usize, need: f64| {
            let (m, n) = (main_of(i), g[i]);
            assert!(
                m >= need && n >= need,
                "{name}: {what} tolerance {m:e} / {n:e} under {need:e}"
            );
            let class = if n < m {
                Class::Closer
            } else if n - m <= need {
                Class::Equal
            } else {
                Class::Farther
            };
            eprintln!(
                "  {what} tolerance: main {m:e}, now {n:e}, need at most {need:e}: {class:?}"
            );
            class
        };
        // the sample (its first coordinate's index) at `i` of a block starting at `j`, its
        // samples from `j + head`
        let sample_at = |j: usize, head: usize, i: usize| {
            let mut s = j + head;
            for n in 0..3 {
                let len = if g[s] == 0.0 { 4 } else { 1 };
                if i < s + len {
                    assert!(len == 4 && i > s, "{name}: a moved sample status at {i}");
                    return Some((n, s + 1));
                }
                s += len;
            }
            None
        };
        let mut classes = Vec::new();
        let mut measured = 0;
        let mut done = Vec::new();
        // per edge or face: its moved samples' classes, and whether a bound moved
        let mut edge_samples: Vec<(usize, Class)> = Vec::new();
        let mut face_samples: Vec<(usize, Class)> = Vec::new();
        let (mut bound_edges, mut bound_faces) = (Vec::new(), Vec::new());
        // per moved face domain bound: (face, slot, main, now)
        let mut face_bounds: Vec<(usize, usize, f64, f64)> = Vec::new();
        for &&(_, i, _, _) in &moved {
            if let Some(bv) = vb.iter().position(|r| r.contains(&i)) {
                let vid = vertices[unmap(row.vertices, bv)];
                let j = vb[bv].start;
                if i - j == 3 {
                    classes.push(tolerance(
                        format!("vertex {bv}"),
                        i,
                        vertex_need(b, &certs, vid),
                    ));
                    continue;
                }
                if done.contains(&j) {
                    continue;
                }
                done.push(j);
                let (main, now) = point(j);
                assert_eq!(b.vertex(vid).unwrap().point, now, "{name}: vertex {bv}");
                let mut fs = Vec::new();
                for e in vertex_edges(b, vid) {
                    for f in edge_faces(b, e) {
                        if !fs.contains(&f) {
                            fs.push(f);
                        }
                    }
                }
                let class = point_class(&k, b, &fs, main, now);
                eprintln!("  vertex {bv}: main {main:?}, now {now:?}: {class:?}");
                if MEASURED_VERTICES.contains(&(name, bv)) {
                    assert_eq!(
                        class,
                        Class::Farther,
                        "{name}: vertex {bv} listed, not farther"
                    );
                    let need = vertex_need(b, &certs, vid);
                    for &f in &fs {
                        let (n, _) = to_face(&k, b, f, now);
                        assert!(
                            n.hi <= need,
                            "{name}: vertex {bv} {:e} off face {}, need {need:e}",
                            n.hi,
                            f.index()
                        );
                    }
                    eprintln!("  vertex {bv}: measured (within its need {need:e})");
                    measured += 1;
                    continue;
                }
                classes.push(class);
            } else if let Some(be) = eb.iter().position(|r| r.contains(&i)) {
                let eid = edges[unmap(row.edges, be)];
                let j = eb[be].start;
                match i - j {
                    0 => {
                        let need = edge_need(b, &certs, eid);
                        assert_eq!(b.edge(eid).unwrap().tolerance, g[i], "{name}: edge {be}");
                        classes.push(tolerance(format!("edge {be}"), i, need));
                    }
                    1 | 2 => {
                        eprintln!(
                            "  edge {be} interval: main {:e}, now {:e}",
                            main_of(i),
                            g[i]
                        );
                        bound_edges.push(be);
                    }
                    _ => {
                        let (n, s) = sample_at(j, 4, i)
                            .unwrap_or_else(|| panic!("{name}: edge {be} slot {} moved", i - j));
                        if done.contains(&s) {
                            continue;
                        }
                        done.push(s);
                        let (main, now) = point(s);
                        let class = point_class(&k, b, &edge_faces(b, eid), main, now);
                        eprintln!("  edge {be} sample {n}: main {main:?}, now {now:?}: {class:?}");
                        edge_samples.push((be, class));
                        classes.push(class);
                    }
                }
            } else if let Some(bf) = fb.iter().position(|r| r.contains(&i)) {
                let j = fb[bf].start;
                if (1..=4).contains(&(i - j)) {
                    eprintln!("  face {bf} domain: main {:e}, now {:e}", main_of(i), g[i]);
                    bound_faces.push(bf);
                    face_bounds.push((bf, i - j, main_of(i), g[i]));
                } else {
                    let (n, s) = sample_at(j, 6, i)
                        .unwrap_or_else(|| panic!("{name}: face {bf} slot {} moved", i - j));
                    if done.contains(&s) {
                        continue;
                    }
                    done.push(s);
                    let (main, now) = point(s);
                    let class = point_class(&k, b, &faces[bf..=bf], main, now);
                    eprintln!("  face {bf} sample {n}: main {main:?}, now {now:?}: {class:?}");
                    face_samples.push((bf, class));
                    classes.push(class);
                }
            } else {
                panic!("{name}: {key}[{i}] past the blocks");
            }
        }
        for (bounds, samples, what) in [
            (bound_edges, &edge_samples, "edge"),
            (bound_faces, &face_samples, "face"),
        ] {
            let mut bounds = bounds;
            bounds.sort_unstable();
            bounds.dedup();
            for x in bounds {
                let of: Vec<Class> = samples.iter().filter(|s| s.0 == x).map(|s| s.1).collect();
                if of.is_empty() {
                    // a face bound with a closed form (DOMAIN_BOUNDS) is classed against it
                    let closed: Option<Vec<Class>> = (what == "face")
                        .then(|| {
                            face_bounds
                                .iter()
                                .filter(|m| m.0 == x)
                                .map(|&(_, slot, main, now)| {
                                    let r = DOMAIN_BOUNDS
                                        .iter()
                                        .find(|d| d.0 == name && d.1 == x && d.2 == slot)?
                                        .3;
                                    // exact: each difference is within a factor 2 of `r`
                                    // (Sterbenz), so it rounds to itself
                                    let (dm, dn) = ((main - r).abs(), (now - r).abs());
                                    let class = match dn.partial_cmp(&dm)? {
                                        std::cmp::Ordering::Less => Class::Closer,
                                        std::cmp::Ordering::Equal => Class::Equal,
                                        std::cmp::Ordering::Greater => Class::Farther,
                                    };
                                    eprintln!(
                                        "  face {x} domain slot {slot}: main {main:e}, now \
                                         {now:e}, closed form {r:e}: {class:?}"
                                    );
                                    Some(class)
                                })
                                .collect()
                        })
                        .flatten();
                    match closed {
                        Some(of) if !of.is_empty() => {
                            let class = worst(&of);
                            eprintln!("  {what} {x} bound: {class:?} (closed form)");
                            classes.push(class);
                        }
                        _ => {
                            // OPEN: a bound moved, none of its samples did: not classed
                            eprintln!("  {what} {x} bound: NO MOVED SAMPLE (unclassed)");
                            classes.push(Class::Farther);
                        }
                    }
                    continue;
                }
                let class = worst(&of);
                eprintln!("  {what} {x} bound: {class:?}");
                classes.push(class);
            }
        }
        (classes, measured)
    }

    #[test]
    fn fillet_p3_rows_are_certified() {
        let (mut closer, mut equal, mut measured) = (0, 0, 0);
        let mut measured_vertices = 0;
        let mut farther = Vec::new();
        for row in ROWS {
            let name = row.case;
            let case = crate::case::regression_case(&Runner, name);
            let bodies: Rc<RefCell<Vec<Brep>>> = Rc::default();
            let into = Rc::clone(&bodies);
            crate::soundness::start_enclosing(1e-9);
            let out = crate::trim_hook::with_hook(
                Box::new(move |b: &mut Brep| {
                    into.borrow_mut()
                        .push(b.clone_standing_for_self().expect("a copy"));
                }),
                || Runner.run_rust(&case),
            );
            let found = crate::soundness::take();
            let out = out.unwrap_or_else(|e| panic!("{name}: {e:?}"));
            // replay
            assert_eq!(
                digest(&out.values, &out.ints),
                row.new,
                "{name}: not the row"
            );
            let mut r = relabel(&out, row);
            for &(key, i, main, now) in row.moved {
                let a = r.values.get_mut(key).unwrap_or_else(|| panic!("no {key}"));
                assert_eq!(a[i], now, "{name}: {key}[{i}]");
                a[i] = main;
            }
            assert_eq!(
                digest(&r.values, &r.ints),
                row.main,
                "{name}: more than the relabeling and the listed values moved"
            );
            let r = relabel(&out, row);
            // soundness
            assert_eq!(found.len(), 1, "{name}: one result body");
            let s = &found[0];
            eprintln!("{name}: {}", s.line());
            assert!(s.sound(), "{name}: {}", s.line());
            assert_eq!(s.manifold, Some(true), "{name}: {}", s.line());
            assert!(
                matches!(s.volume, Some(VolumeSign::Positive(_))),
                "{name}: {}",
                s.line()
            );
            let e = match &s.enclosure {
                Some(Ok(e)) => *e,
                other => panic!("{name}: no enclosure: {other:?}"),
            };
            let bodies = bodies.borrow();
            let b = bodies.last().expect("the result body");
            let (gk, mk) = if r.values.contains_key("result_geometry") {
                ("result_geometry", "mass_properties")
            } else {
                ("shell_geometry", "shell_mass_properties")
            };
            let volume = r.values[mk][1];
            let main_volume = row
                .moved
                .iter()
                .find(|m| m.0 == mk && m.1 == 1)
                .map_or(volume, |m| m.2);
            eprintln!(
                "  volume enclosure [{}, {}] (relative width {:e}): main {main_volume}, now {volume}",
                e.lo,
                e.hi,
                (e.hi - e.lo) / e.hi
            );
            // each value's distance to the true volume, which the enclosure holds
            let off = |v: f64| crate::certify::Enclosure {
                lo: (e.lo - v).max(v - e.hi).max(0.0).next_down().max(0.0),
                hi: (e.hi - v).max(v - e.lo).next_up(),
            };
            // each upper end is one rounded difference (half an ulp) stepped up one ulp: at
            // most two ulps above the exact difference
            let ulps = |x: f64| 2.0 * (x.next_up() - x);
            let (m, n) = (off(main_volume), off(volume));
            // two values inside the enclosure: each is at most its width off the true volume,
            // and neither is known nearer (both lower ends 0). The kernel's volume is not a
            // certified quantity, so no counted rounding covers its last bits: the move is
            // measured, not classed (#19318)
            let inside = |v: f64| e.lo <= v && v <= e.hi;
            let volume_class = if inside(main_volume) && inside(volume) {
                None
            } else {
                Some(super::class_of(m, n, ulps(m.hi) + ulps(n.hi)))
            };
            eprintln!(
                "  volume off the true by main {:?}, now {:?}: {volume_class:?}",
                off(main_volume),
                off(volume)
            );
            for &(key, i, main, now) in row.moved.iter().filter(|m| m.0 == mk) {
                if i == 1 {
                    match volume_class {
                        None => {
                            eprintln!(
                                "  volume: main {main}, now {now} (relative {:e}), both inside \
                                 the enclosure: measured",
                                (now - main).abs() / main.abs()
                            );
                            measured += 1;
                        }
                        Some(Class::Closer) => closer += 1,
                        Some(Class::Equal) => equal += 1,
                        Some(Class::Farther) => farther.push(name),
                    }
                } else {
                    eprintln!(
                        "  {key}[{i}]: main {main}, now {now} (relative {:e}): measured",
                        (now - main).abs() / main.abs()
                    );
                    measured += 1;
                }
            }
            let (classes, vertices_measured) = certify_geometry(row, gk, &r.values[gk], b);
            measured_vertices += vertices_measured;
            for c in classes {
                match c {
                    Class::Closer => closer += 1,
                    Class::Equal => equal += 1,
                    Class::Farther => farther.push(name),
                }
            }
        }
        eprintln!(
            "{} rows: moved values closer {closer}, equal {equal}, farther {} {farther:?}; mass \
             properties measured {measured}; vertices measured {measured_vertices}",
            ROWS.len(),
            farther.len()
        );
        assert!(farther.is_empty(), "moved values farther in {farther:?}");
        assert_eq!(
            measured_vertices,
            MEASURED_VERTICES.len(),
            "a listed measured vertex did not move"
        );
        assert_eq!(ROWS.len(), 2);
    }
}
