//! #16735: every mass value the closure of tolerant edges moves in the goldens of
//! `blend.fillet_executive_p3`, `boolean.api_brep` and `offset.offset_executive_p2` is the old
//! integrator's leak through the edges' gaps, not a change of shape.
//!
//! Run (about the time of the three units' goldens, plus the measuring):
//! `cargo test --profile goldens -p goldens --test issue_16735_golden_mass_moves -- --ignored --nocapture`
//! It is `#[ignore]`d because it re-runs three whole golden units; CI's goldens job already
//! checks the values themselves. `ISSUE_16735_INVENTORY=<json>` (rows `[unit, case, field, old,
//! new]`, the PR's diff of the goldens) also gates every row of the diff (each moved value
//! within its component's old leak bound) and checks the recomputed old and new values
//! against the rows. `ISSUE_16735_FILTER=<substr>` runs only the cases whose name holds it.
//!
//! # What runs
//!
//! Each unit's cases run in this process with `goldens::dump::capture` on, so every body a
//! case measures is kept, just before the measure, as an exact copy (the `--dump-smb` hook;
//! whether the `.smb` file `--dump-smb` writes of it measures the same is reported). For each
//! body the test computes, on fresh copies:
//! * **new**: the runner's own call (`brep_mass_properties(1e-4, density 1, origin 0)`, or
//!   `brep_volume(1e-6)`); it must reproduce the case's output to 1e-12 of the component's
//!   scale (below), so the copy is the measured body;
//! * **old**: the old `brep_properties`, the faces over their own trims alone (the same
//!   `face_properties` calls in the same order, without `gap_closure`), through the same
//!   post-processing as `estimate`. With the case's output, its moved entries set to old, or
//!   as it is (new), the whole golden row (topology, statuses, every other value) must agree
//!   with one of the two: before the expectations commit the goldens hold old, after it new.
//!
//! # The leak, per component (the bound checked; the gaps sampled: MEASURED)
//!
//! The measure integrates about its box's middle `m` (the exact box for `brep_mass_properties`
//! since #17362, the loose box for `brep_volume`; the old path takes the same box). Every
//! volume integral
//! `∫ f dV` of the record is the flux of the field `F_f(x) = e_z ∫_{m_z}^{z} f(x, y, ζ) dζ`
//! (`closure_integrands`: volume `z`, first moments `x z`, `y z`, `z²/2`, second moments
//! `y² z + z³/3` and so on, all relative to `m`). The reported components are linear in these
//! integrals, with constants fixed by `m` and the old centroid, so each is the flux of one
//! field `F_c = e_z ∫_{m_z}^{z} f_c dζ` with, in world coordinates (the records' origin is 0):
//!
//! | component | `f_c` |
//! |---|---|
//! | area `[0]` | (not a volume integral: moves only by the open gaps' shadows, below) |
//! | volume `[1]`, mass `[2]` (density 1) | `1` |
//! | centroid `[3..6]` | `(x_a − c_a) / V'` (`c` the old centroid, `V'` the new volume: `m'c' = m c + ∫_D x` gives `c' − c = ∫_D (x − c) / m'`) |
//! | inertia `[6..9]` about 0 | `y² + z²`, `z² + x²`, `x² + y²` (the parallel-axis shift from `m` to 0 is linear in `m`, `S` and `M7`, and adds up to the world integrand) |
//! | products `[9..12]` about 0 | `y z`, `z x`, `x y` |
//!
//! The new value is the old one plus the flux of `F_c` through the closure, the ruled strips
//! between each trim's image `P` and its edge curve `E`, and the corner fans at the vertices.
//! A strip of width `g` (the gap `|P − E|`) and length `L` has area at most about `g L`, so
//! `|new − old| <= Σ_pieces area · sup|F_c|`, with `sup|F_c| <= sup|z − m_z| · sup|f_c|` over the
//! piece's box (its `z` range widened to `m_z`, the `ζ` of the integral). The pieces are the
//! strips between consecutive samples of each trim (width the larger of the two sampled gaps,
//! length the longer of the two sides, box the four points widened by both) and the corner
//! polygons `[trim end, edge end, vertex, edge start, trim start]` (fan area from their centre),
//! and the shadows of the uv gaps the trim boundary leaves open, which the faces are now
//! integrated across (`close_loops`; the old integrator lost each gap's region down to the
//! integration base, a second leak; `shadow_pieces`). A shadow also moves the face's own
//! area, by at most the shadow's area: that is the area's bound (the strips and fans leave
//! it unmoved).
//! For the volume this is `g L |z − m_z|`, the `tol L |x − o|` leak of the issue. A noise floor
//! of `1e-12` of the component's scale covers the quadrature (`V`; `|c|∞ + diag` for the
//! centroid; `V (|m|∞ + diag)²` for the moments).
//!
//! # The other checks
//!
//! * **Origin independence**: the new record integrated about `m`, the world origin and
//!   `m + diag·(0.37, −0.21, 0.55)`, at accuracy `1e-10` (the method's property, not the
//!   goldens' quadrature; `1e-9` or `1e-8` where the step control fails at `1e-10`, counted
//!   in the summary), agrees per component within `1e-9` of its scale times
//!   `1 + A |o − m| / V` (the faces' quadrature grows with the origin's distance; the
//!   per-component form of `volume_leak`'s `origin_allowance`).
//! * **Reported only, not the gate**: the tube bound's premise, every sampled gap within its
//!   edge's stored tolerance and every corner point within its vertex's (then the closed
//!   region differs from the solid only inside the tubes and balls, `Σ π tol_e² L_e +
//!   Σ 4/3 π tol_v³`, ARGUED in the PR and diagnostic only: the gaps are sampled, not
//!   certified); the bodies beyond it are counted and listed.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout,
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::many_single_char_names
)] // test

#[path = "../../solidcore/tests/support/volume_leak.rs"]
mod volume_leak;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use goldens::case::repo_root;
use goldens::{golden, registry, CaseContext, Output};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent3d::Extent3d;
use solidcore::mass::integrator::properties::{
    brep_mass_properties, brep_properties_in_box, brep_volume, edge_length, face_properties,
    MassPropertyBehavior, PreciseProperties, PropertyAttributes,
};
use solidcore::math::{Point2, Point3, Vec3, PI};
use solidcore::nurbs::b_spline_surface_n_lib::p2::{EvaluateFlags, OrientType};
use solidcore::query::brep::{
    calculate_bounding_box, calculate_tight_bounding_box, BoundingBoxOptions,
};
use solidcore::topology::brep::p1;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2::{upward_faceuse, PPF_ALL};
use solidcore::topology::{edge, loopuse, vertex};
use solidcore::{Brep, Kernel};

/// The units whose goldens the closure moves (corpus.parity is checked elsewhere).
const UNITS: [&str; 3] = [
    "blend.fillet_executive_p3",
    "boolean.api_brep",
    "offset.offset_executive_p2",
];
/// The goldens' default `--seeds`.
const SEEDS: usize = 16;
/// The mass record's accuracy (`brep_mass_properties(k, b, 1e-4, ..)` in the runners).
const MASS_ACCURACY: f64 = 1e-4;
/// Both units' `VOLUME_ACCURACY`.
const VOLUME_ACCURACY: f64 = 1e-6;
/// Samples along each trim.
const SAMPLES: u32 = 128;
/// The quadrature's share of a component's scale.
const NOISE: f64 = 1e-12;
/// Origin independence, per component scale.
const ORIGIN_REL: f64 = 1e-9;
/// The accuracies the origin independence is checked at, the first all three origins
/// converge at: the method's property, not the goldens' requested quadrature (at 1e-6 the
/// quadrature alone moves a volume by ~2e-7 of itself between origins, MEASURED; at 1e-10
/// some bodies' step control fails).
const ORIGIN_ACCURACY: [f64; 3] = [1e-10, 1e-9, 1e-8];

/// What a dumped body's measure is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    /// `mass_properties`: area, volume, mass, centroid, inertia and products about 0.
    Mass,
    /// `shell_mass_properties`: area, volume, mass.
    ShellMass,
    /// `brep_volume`.
    Volume,
}

fn kind_of(label: &str) -> Kind {
    match label {
        "mass" => Kind::Mass,
        "shell_mass" => Kind::ShellMass,
        "volume" => Kind::Volume,
        _ => panic!("unknown dump label {label}"),
    }
}

fn attributes(store: &AttributeStore) -> PropertyAttributes<'_> {
    PropertyAttributes {
        store,
        key_of: &|_| ObjectKey(0),
    }
}

fn comp(v: Vec3, i: usize) -> f64 {
    match i {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// `estimate`'s API post-processing of `raw` integrated about `o`: the record with the
/// moments about the world origin.
fn record(kind: Kind, raw: &PreciseProperties, o: Point3) -> Vec<f64> {
    let s = raw.moments[4];
    let m = raw.mass;
    let d = o;
    let shift = |a: usize, b: usize| {
        comp(d, a) * comp(s, b) + comp(d, b) * comp(s, a) + comp(d, a) * comp(d, b) * m
    };
    let inertia = raw.moments[7]
        + Vec3::new(
            shift(1, 1) + shift(2, 2),
            shift(0, 0) + shift(2, 2),
            shift(0, 0) + shift(1, 1),
        );
    let products = raw.moments[6] + Vec3::new(shift(1, 2), shift(2, 0), shift(0, 1));
    let c = o + s / m;
    match kind {
        Kind::Mass => vec![
            raw.area, raw.volume, m, c.x, c.y, c.z, inertia.x, inertia.y, inertia.z, products.x,
            products.y, products.z,
        ],
        Kind::ShellMass => vec![raw.area, raw.volume, m],
        Kind::Volume => vec![raw.volume],
    }
}

/// The new code's record of `b` (the runner's call).
fn new_record(k: &Kernel, b: &mut Brep, kind: Kind) -> Option<Vec<f64>> {
    let store = AttributeStore::default();
    match kind {
        Kind::Mass | Kind::ShellMass => {
            let r = brep_mass_properties(k, b, MASS_ACCURACY, 1.0, Vec3::ZERO, &attributes(&store))
                .ok()?;
            let p = &r.properties;
            let mut v = vec![p.area, p.volume, p.mass];
            if kind == Kind::Mass {
                for x in [r.centroid, r.inertia, r.products] {
                    v.extend([x.x, x.y, x.z]);
                }
            }
            Some(v)
        }
        Kind::Volume => Some(vec![brep_volume(
            k,
            b,
            VOLUME_ACCURACY,
            &attributes(&store),
        )
        .ok()?]),
    }
}

fn accuracy(kind: Kind) -> f64 {
    if kind == Kind::Volume {
        VOLUME_ACCURACY
    } else {
        MASS_ACCURACY
    }
}

/// The box the measure of `kind` integrates in: `brep_mass_properties` (`estimate`) takes
/// the body's exact box (#17362), `brep_volume` the loose one. Its middle, the faces' error
/// scale (`brep_properties_in_box`'s `estimated`), its diagonal and the box.
fn mid_and_estimated(k: &Kernel, b: &mut Brep, kind: Kind) -> (Point3, f64, f64, Extent3d) {
    let bbox = if kind == Kind::Volume {
        calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default()).unwrap()
    } else {
        calculate_tight_bounding_box(k, &mut *b).unwrap()
    };
    let size = bbox.size();
    // fail closed: a NaN middle or size would make every `d > floor` test false
    assert!(
        [
            bbox.mid().x,
            bbox.mid().y,
            bbox.mid().z,
            size.x,
            size.y,
            size.z
        ]
        .iter()
        .all(|v| v.is_finite()),
        "the measure's box is not finite: {bbox:?}"
    );
    let e = size.z.max(size.x.max(size.y));
    (bbox.mid(), e * e / 100.0, size.length(), bbox)
}

/// The region on `fu`'s side is material (upstream's reading: `IsVoid` FALSE).
fn material(b: &Brep, fu: solidcore::topology::FaceuseId) -> bool {
    let r = eu1::faceuse_region(b, fu).unwrap().unwrap();
    b.region(r).unwrap().is_void() == 0
}

/// The old `brep_properties` (before #16735) of a body with default behaviour: each face over
/// its own trims, the same `face_properties` calls in the same order. A body with wire edges
/// or shell vertices is not covered (none here: the test fails on one).
fn old_raw(
    k: &Kernel,
    b: &mut Brep,
    acc: f64,
    origin: Point3,
    estimated: f64,
) -> PreciseProperties {
    let mut total = PreciseProperties::default();
    for f in p1::faces(b, None).unwrap() {
        let up = b.face(f).unwrap().faceuse.unwrap();
        let mate = b.faceuse(up).unwrap().mate.unwrap();
        let separating =
            eu1::faceuse_region(b, up).unwrap() != eu1::faceuse_region(b, mate).unwrap();
        if !separating {
            continue;
        }
        for fu in [up, mate] {
            let other = b.faceuse(fu).unwrap().mate.unwrap();
            let do_area = material(b, fu) && !material(b, other);
            let do_volume = material(b, fu);
            if !(do_area || do_volume) {
                continue;
            }
            let orientation = b.faceuse(fu).unwrap().orientation;
            let d = face_properties(k, b, f, orientation, acc, origin, estimated, 0.0, PPF_ALL)
                .unwrap();
            if do_volume {
                total.volume += d.volume;
                total.mass += d.volume;
                for (t, x) in total.moments.iter_mut().zip(d.moments.iter()).skip(4) {
                    *t += *x;
                }
            }
            if do_area {
                total.area += d.area;
                for (t, x) in total.moments.iter_mut().zip(d.moments.iter()).take(4) {
                    *t += *x;
                }
            }
        }
    }
    for e in p1::edges(b, None).unwrap() {
        assert!(!edge::is_wire(b, e).unwrap(), "a wire edge: not covered");
    }
    for v in p1::vertices(b, None).unwrap() {
        assert!(
            !vertex::is_shell_vertex(b, v).unwrap(),
            "a shell vertex: not covered"
        );
    }
    total
}

/// The new `brep_properties_in_box` about `origin`, in the measure's own box `bbox`
/// ([`mid_and_estimated`]).
fn new_raw(
    k: &Kernel,
    b: &mut Brep,
    acc: f64,
    origin: Point3,
    bbox: &Extent3d,
) -> solidcore::Result<PreciseProperties> {
    let store = AttributeStore::default();
    brep_properties_in_box(
        k,
        b,
        acc,
        origin,
        MassPropertyBehavior::default(),
        &attributes(&store),
        bbox,
    )
}

/// The new record about `m`, the world origin and `o3`, at the first of
/// [`ORIGIN_ACCURACY`]'s ladder all three converge at, and that accuracy.
fn about_origins(
    k: &Kernel,
    b: &mut Brep,
    kind: Kind,
    m: Point3,
    o3: Point3,
    bbox: &Extent3d,
) -> Option<(f64, f64, [Vec<f64>; 3])> {
    ORIGIN_ACCURACY.iter().find_map(|&acc| {
        let area = new_raw(k, b, acc, m, bbox).ok()?.area;
        let mut at = |o: Point3| {
            new_raw(k, b, acc, o, bbox)
                .ok()
                .map(|r| record(kind, &r, o))
        };
        Some((acc, area, [at(m)?, at(Point3::ZERO)?, at(o3)?]))
    })
}

/// A piece of the closure: its area and a box holding it; `shadow` for an open gap's
/// shadow ([`shadow_pieces`]), whose area the face integral itself moves.
#[derive(Debug, Clone, Copy)]
struct Piece {
    area: f64,
    lo: Vec3,
    hi: Vec3,
    shadow: bool,
}

fn bounds(points: &[Vec3], pad: f64) -> (Vec3, Vec3) {
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points {
        lo = Vec3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = Vec3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    let pad = Vec3::new(pad, pad, pad);
    (lo - pad, hi + pad)
}

/// The nearest point of `c` on `[t0, t1]` to `p`: Gauss-Newton from `guess`, then, when that
/// lands beyond `tol`, the better of it and Gauss-Newton from the best of a 256-point scan.
fn nearest(
    k: &Kernel,
    c: &solidcore::curve::curve::Curve,
    (t0, t1): (f64, f64),
    p: Point3,
    guess: f64,
    tol: f64,
) -> (f64, Point3) {
    let newton = |guess: f64| {
        let mut t = guess.max(t0).min(t1);
        for _ in 0..64 {
            let e = c.evaluate(k, t, 1, true, false).unwrap();
            let df = e[1].dot(e[1]);
            if df.is_nan() || df <= 0.0 {
                break;
            }
            let next = (t - (e[0] - p).dot(e[1]) / df).max(t0).min(t1);
            let step = (next - t).abs();
            t = next;
            if step <= 1e-15 * (1.0 + t.abs()) {
                break;
            }
        }
        (t, c.evaluate_point(k, t).unwrap())
    };
    let first = newton(guess);
    if (first.1 - p).length() <= tol {
        return first;
    }
    let mut best = (f64::INFINITY, t0);
    for j in 0..=256 {
        let t = t0 + (t1 - t0) * f64::from(j) / 256.0;
        let d = (c.evaluate_point(k, t).unwrap() - p).length();
        if d < best.0 {
            best = (d, t);
        }
    }
    let second = newton(best.1);
    if (second.1 - p).length() < (first.1 - p).length() {
        second
    } else {
        first
    }
}

/// One edge use of a loop, as `gap_closure` sees it.
struct Side {
    trim_ends: [Vec3; 2],
    edge_ends: [Vec3; 2],
    vertex: Vec3,
    vertex_tol: f64,
}

/// The closure's pieces of `b` (every face with a material side), the worst sampled gap over
/// its edge's tolerance and the worst corner point's distance over its vertex's.
fn closure_pieces(k: &Kernel, b: &mut Brep) -> (Vec<Piece>, f64, f64, String) {
    let mut pieces = Vec::new();
    let (mut worst_gap, mut worst_corner) = (0.0_f64, 0.0_f64);
    let mut worst_at = String::new();
    for f in p1::faces(b, None).unwrap() {
        let up = b.face(f).unwrap().faceuse.unwrap();
        let mate = b.faceuse(up).unwrap().mate.unwrap();
        if eu1::faceuse_region(b, up).unwrap() == eu1::faceuse_region(b, mate).unwrap()
            || !(material(b, up) || material(b, mate))
        {
            continue;
        }
        let s = b.face(f).unwrap().surface.clone().unwrap();
        let fu = upward_faceuse(b, f).unwrap().unwrap();
        for lu in eu1::faceuse_loopuses(b, fu, false, None).unwrap() {
            if loopuse::is_vertex_loopuse(b, lu).unwrap() {
                continue;
            }
            let mut sides = Vec::new();
            for eu in eu1::loopuse_edgeuses(b, lu).unwrap() {
                let same = eu1::orientation(b, eu).unwrap() == OrientType::Same;
                let trim = eu1::uv_trim_curve(b, eu).unwrap().0.cloned().unwrap();
                let vu = b.edgeuse(eu).unwrap().vertexuse.unwrap();
                let v = b.vertexuse(vu).unwrap().vertex.unwrap();
                let vp = b.vertex(v).unwrap().point;
                let vertex_tol = vertex::tolerance(b, v).unwrap();
                let e = eu1::edge(b, eu).unwrap().unwrap();
                let ivl = b.edge(e).unwrap().interval;
                let (e0, e1) = (ivl.min(), ivl.max());
                let curve = edge::curve(b, e).unwrap().cloned();
                let tol = if curve.is_some() {
                    edge::tolerance(b, e).unwrap()
                } else {
                    vertex_tol
                };
                let ti = trim.natural_interval();
                let (c0, c1) = (ti.min(), ti.max());
                let at = |tau: f64| {
                    let uv = trim.evaluate_point(k, tau).unwrap();
                    s.evaluate(k, Point2::new(uv.x, uv.y), 0, 0, EvaluateFlags::default())
                        .unwrap()[0][0]
                };
                let mut ps = Vec::new();
                let mut es = Vec::new();
                let mut last = e0;
                for i in 0..=SAMPLES {
                    let l = f64::from(i) / f64::from(SAMPLES);
                    let p = at(c0 + (c1 - c0) * l);
                    let q = match &curve {
                        Some(c) => {
                            let guess = if i == 0 { e0 + (e1 - e0) * l } else { last };
                            let (t, q) = nearest(k, c, (e0, e1), p, guess, tol);
                            last = t;
                            q
                        }
                        None => vp,
                    };
                    let ratio = (p - q).length() / tol;
                    if ratio > worst_gap {
                        worst_gap = ratio;
                        worst_at = format!(
                            "{f:?} {e:?} at {l} of the trim: gap {:e}, tol {tol:e}, same {same}, p {p:?}, q {q:?}",
                            (p - q).length(),
                        );
                    }
                    ps.push(p);
                    es.push(q);
                }
                for i in 0..SAMPLES as usize {
                    let g = (ps[i] - es[i])
                        .length()
                        .max((ps[i + 1] - es[i + 1]).length());
                    let len = (ps[i + 1] - ps[i])
                        .length()
                        .max((es[i + 1] - es[i]).length());
                    let (lo, hi) = bounds(&[ps[i], ps[i + 1], es[i], es[i + 1]], g + len);
                    pieces.push(Piece {
                        area: g * len,
                        lo,
                        hi,
                        shadow: false,
                    });
                }
                let (pa, pb) = (at(c0), at(c1));
                let (ea, eb) = match &curve {
                    Some(c) => (
                        c.evaluate_point(k, e0).unwrap(),
                        c.evaluate_point(k, e1).unwrap(),
                    ),
                    None => (vp, vp),
                };
                sides.push(Side {
                    trim_ends: if same { [pa, pb] } else { [pb, pa] },
                    edge_ends: if same { [ea, eb] } else { [eb, ea] },
                    vertex: vp,
                    vertex_tol,
                });
            }
            for (i, side) in sides.iter().enumerate() {
                let next = &sides[(i + 1) % sides.len()];
                let poly = [
                    side.trim_ends[1],
                    side.edge_ends[1],
                    next.vertex,
                    next.edge_ends[0],
                    next.trim_ends[0],
                ];
                for p in poly {
                    worst_corner = worst_corner.max((p - next.vertex).length() / next.vertex_tol);
                }
                let centre = poly.iter().fold(Vec3::ZERO, |a, p| a + *p) * 0.2;
                let mut area = 0.0;
                for j in 0..5 {
                    area += 0.5
                        * (poly[j] - centre)
                            .cross(poly[(j + 1) % 5] - centre)
                            .length();
                }
                let (lo, hi) = bounds(&poly, 0.0);
                pieces.push(Piece {
                    area,
                    lo,
                    hi,
                    shadow: false,
                });
            }
        }
        pieces.extend(shadow_pieces(k, b, f, &s));
    }
    (pieces, worst_gap, worst_corner, worst_at)
}

/// 8-point Gauss-Legendre nodes and weights on [-1, 1].
const GL8: [(f64, f64); 8] = [
    (-0.960_289_856_497_536_2, 0.101_228_536_290_376_26),
    (-0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (-0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (-0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (0.960_289_856_497_536_2, 0.101_228_536_290_376_26),
];

/// The shadows of the uv gaps face `f`'s oriented trim boundary leaves open, which
/// `brep_properties` now closes with uv lines (`close_loops`, fadb9fa47): consecutive
/// curves whose end and start differ by `0 < |d|^2 <= EFF_ZERO (1 + |p|)`. The face
/// integral runs iso-u lines from the integration base `v0` (the trims' lowest v, no lower
/// than the face's domain), so an open gap from `a` to `z` loses the region over the line,
/// `u` between `a.u` and `z.u`, `v` from `v0` to the line: the old integrator's second
/// leak. Each is one piece: its area `sum |F(u, v)| |du|` along the line (8-point
/// Gauss-Legendre), `F(u, v) = int_{v0}^{v} |S_u x S_v| dv` (64 panels of 8 points; the
/// line is at most about 1e-6 long, so its points' boxes hold the region), and the box of
/// the surface over the iso-u lines from `v0` at the line's two ends (65 samples each,
/// padded by the largest step between samples and by `|du| sup |S_u|`). The area is
/// MEASURED (quadrature), as the strips' widths are.
fn shadow_pieces(
    k: &Kernel,
    b: &mut Brep,
    f: solidcore::topology::FaceId,
    s: &solidcore::surface::surface::Surface,
) -> Vec<Piece> {
    use solidcore::math::EFF_ZERO;
    use solidcore::nurbs::b_spline_curve::p2::BoundingBoxRequest;
    let key = |_: solidcore::EntityRef| solidcore::geom::object::ObjectRef {
        type_code: 0,
        key: ObjectKey(0),
    };
    let t = solidcore::topology::face::p1::create_oriented_trim_boundary(
        k, b, f, true, true, None, &key,
    )
    .unwrap();
    let end = |c: &solidcore::curve::curve::Curve, last: bool| {
        let iv = c.natural_interval();
        c.evaluate_point(k, if last { iv.max() } else { iv.min() })
            .unwrap()
    };
    let mut gaps = Vec::new();
    let mut start = 0;
    for &n in &t.loops {
        let lp = &t.curves[start..start + n];
        for (i, c) in lp.iter().enumerate() {
            let next = &lp[(i + 1) % n];
            let (a, z) = (end(c, true), end(next, false));
            if a != z && (z - a).length_squared() <= EFF_ZERO * (1.0 + z.max_dimension()) {
                gaps.push((a, z));
            }
        }
        start += n;
    }
    if gaps.is_empty() {
        return Vec::new();
    }
    let req = BoundingBoxRequest {
        normal: true,
        pseudo: false,
        polar: false,
    };
    let mut base = f64::INFINITY;
    for c in &t.curves {
        let bb = c
            .calculate_bounding_box(k, &c.natural_interval(), req, true)
            .unwrap()
            .normal
            .unwrap();
        base = base.min(bb.min().y);
    }
    for (a, z) in &gaps {
        base = base.min(a.y).min(z.y);
    }
    base = base.max(b.face(f).unwrap().uv_domain.min().y);
    let flags = EvaluateFlags::default();
    let eval = |u: f64, v: f64| {
        let d = s.evaluate(k, Point2::new(u, v), 1, 1, flags).unwrap();
        (d[0][0], d[1][0], d[0][1])
    };
    let mut out = Vec::new();
    for (a, z) in gaps {
        let du = z.x - a.x;
        let mut area = 0.0;
        for (x, w) in GL8 {
            let sp = 0.5 + 0.5 * x;
            let (u, v) = (a.x + du * sp, a.y + (z.y - a.y) * sp);
            let mut big_f = 0.0;
            for i in 0..64 {
                let lo = base + (v - base) * f64::from(i) / 64.0;
                let hi = base + (v - base) * f64::from(i + 1) / 64.0;
                for (y, wy) in GL8 {
                    let t = 0.5 * (lo + hi) + 0.5 * (hi - lo) * y;
                    let (_, su, sv) = eval(u, t);
                    big_f += 0.5 * (hi - lo) * wy * su.cross(sv).length();
                }
            }
            area += 0.5 * w * big_f.abs() * du.abs();
        }
        let mut pts = Vec::new();
        let (mut step, mut su_max) = (0.0_f64, 0.0_f64);
        for end in [a, z] {
            let mut last: Option<Vec3> = None;
            for i in 0..=64 {
                let v = base + (end.y - base) * f64::from(i) / 64.0;
                let (p, su, _) = eval(end.x, v);
                su_max = su_max.max(su.length());
                if let Some(q) = last {
                    step = step.max((p - q).length());
                }
                last = Some(p);
                pts.push(p);
            }
        }
        let (lo, hi) = bounds(&pts, step + du.abs() * su_max);
        out.push(Piece {
            area,
            lo,
            hi,
            shadow: true,
        });
    }
    out
}

fn sup_abs(lo: f64, hi: f64) -> f64 {
    lo.abs().max(hi.abs())
}

/// `sup |F_c|` over a piece, `F_c = e_z ∫_{m_z}^{z} f_c dζ` (see the module's table).
fn field_sup(kind: Kind, c: usize, p: &Piece, m: Point3, centroid: Point3, volume: f64) -> f64 {
    let zf = sup_abs(p.lo.z - m.z, p.hi.z - m.z);
    // f_c's box: the piece's, its z range widened to m_z
    let lo = Vec3::new(p.lo.x, p.lo.y, p.lo.z.min(m.z));
    let hi = Vec3::new(p.hi.x, p.hi.y, p.hi.z.max(m.z));
    let ax = |a: usize| sup_abs(comp(lo, a), comp(hi, a));
    let sq = |a: usize| ax(a) * ax(a);
    let f = match (kind, c) {
        (Kind::Volume, 0) | (Kind::Mass | Kind::ShellMass, 1 | 2) => 1.0,
        (Kind::Mass, 3..=5) => {
            let a = c - 3;
            sup_abs(
                comp(lo, a) - comp(centroid, a),
                comp(hi, a) - comp(centroid, a),
            ) / volume.abs()
        }
        (Kind::Mass, 6) => sq(1) + sq(2),
        (Kind::Mass, 7) => sq(2) + sq(0),
        (Kind::Mass, 8) => sq(0) + sq(1),
        (Kind::Mass, 9) => ax(1) * ax(2),
        (Kind::Mass, 10) => ax(2) * ax(0),
        (Kind::Mass, 11) => ax(0) * ax(1),
        // the area: no volume integral, no leak
        _ => 0.0,
    };
    zf * f
}

/// A component's natural scale (for the noise floor and origin independence).
fn scale(kind: Kind, c: usize, rec: &[f64], m: Point3, diag: f64) -> f64 {
    let mm = m.x.abs().max(m.y.abs()).max(m.z.abs()) + diag;
    match (kind, c) {
        (Kind::Mass, 3..=5) => rec[3].abs().max(rec[4].abs()).max(rec[5].abs()) + diag,
        (Kind::Mass, 6..=11) => rec[1].abs() * mm * mm,
        (_, 0) if kind != Kind::Volume => rec[0].abs(),
        _ => rec[usize::from(kind != Kind::Volume)].abs(),
    }
}

/// The entries of the case's output a dumped body's record fills: `(field, index)` per
/// record component.
fn targets(unit: &str, kind: Kind, n: usize, len: usize) -> Vec<(&'static str, usize)> {
    match (unit, kind) {
        (_, Kind::Mass) => (0..len).map(|i| ("mass_properties", i)).collect(),
        (_, Kind::ShellMass) => (0..len).map(|i| ("shell_mass_properties", i)).collect(),
        ("boolean.api_brep", Kind::Volume) => {
            assert_eq!(n, 0, "one measured volume per boolean case");
            vec![("measures", 1)]
        }
        (_, Kind::Volume) => vec![("volumes", 2 * n + 1)],
    }
}

/// The goldens' own agreement window for two values (`compare::values_agree`, no floor).
fn golden_tol(a: f64, b: f64) -> f64 {
    goldens::REL_TOL * a.abs().max(b.abs())
}

/// An inventory row's key: `(unit, case, field)`.
type RowKey = (String, String, String);

/// The inventory's rows by `(unit, case, field)`: `(old, new)`.
fn inventory() -> Option<BTreeMap<RowKey, (f64, f64)>> {
    let path = std::env::var_os("ISSUE_16735_INVENTORY")?;
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    Some(
        rows.into_iter()
            .map(|r| {
                (
                    (
                        r[0].as_str().unwrap().to_string(),
                        r[1].as_str().unwrap().to_string(),
                        r[2].as_str().unwrap().to_string(),
                    ),
                    (r[3].as_f64().unwrap(), r[4].as_f64().unwrap()),
                )
            })
            .collect(),
    )
}

/// Per unit, what was measured.
#[derive(Debug, Default)]
struct Summary {
    cases: usize,
    bodies: usize,
    moved: usize,
    worst_leak_ratio: f64,
    worst_origin: f64,
    worst_gap: f64,
    worst_corner: f64,
    worst_tube: f64,
    worst_reload: f64,
    worst_inventory: f64,
    inventory_rows: usize,
    golden_old: usize,
    golden_new: usize,
    origin_accuracy: BTreeMap<String, usize>,
    /// Bodies whose `--dump-smb` file measures the same, differently, or does not read back.
    smb_same: usize,
    smb_differs: usize,
    smb_unreadable: usize,
    /// Inventory rows whose move was checked against the leak bound.
    inventory_gated: usize,
    /// Inventory rows whose old golden is stale (within the goldens' 1e-9 of today's old).
    stale_golden: usize,
    /// Bodies with a sampled gap or corner point beyond its stored tolerance (informational).
    over_tolerance: usize,
    /// Bodies whose run measure failed (status) but whose reload measures: not compared.
    run_failed: usize,
    /// Bodies dumped with an edge use lacking its UV trim (the measure makes them).
    missing_trims: usize,
}

/// Whether the run's measure of the body succeeded (its status entry is 0).
fn run_measured(out: &Output, kind: Kind, tg: &[(&str, usize)]) -> bool {
    match kind {
        Kind::Mass => out.ints.get("mass_status").and_then(|s| s.first()) == Some(&0),
        Kind::ShellMass => {
            out.ints
                .get("shell_counts_manifold_mass_status")
                .and_then(|s| s.get(5))
                == Some(&0)
        }
        // the status before the volume: boolean's `0`, offset's native `SM_OK` (1000)
        Kind::Volume => tg.first().is_some_and(|&(field, i)| {
            out.values
                .get(field)
                .and_then(|v| v.get(i.wrapping_sub(1)))
                .is_some_and(|&s| s == 0.0 || s == 1000.0)
        }),
    }
}

/// The edge uses of `b`'s faces lacking a UV trim.
fn missing_trims(b: &Brep) -> usize {
    let mut n = 0;
    for f in p1::faces(b, None).unwrap() {
        let Some(fu) = upward_faceuse(b, f).unwrap() else {
            continue;
        };
        for lu in eu1::faceuse_loopuses(b, fu, false, None).unwrap() {
            for eu in eu1::loopuse_edgeuses(b, lu).unwrap() {
                n += usize::from(eu1::uv_trim_curve(b, eu).unwrap().0.is_none());
            }
        }
    }
    n
}

#[test]
#[ignore = "re-runs three golden units (minutes); see the module docs"]
fn moved_golden_mass_rows_are_the_old_leak() {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let filter = std::env::var("ISSUE_16735_FILTER").ok();
    let inventory = inventory();
    let mut gated: BTreeSet<RowKey> = BTreeSet::new();
    let reg = registry();
    let mut failures = Vec::new();
    let mut notes = Vec::new();
    let mut report = String::new();
    for unit_id in UNITS {
        let unit = reg.iter().find(|u| u.id() == unit_id).unwrap();
        let gold = golden::read(&golden::default_dir(), unit_id).unwrap();
        let mut sum = Summary::default();
        let mut cases = unit.cases(&ctx).unwrap();
        if let Some(f) = &filter {
            cases.retain(|c| c.name.contains(f.as_str()));
        }
        for case_in in &cases {
            let case = case_in.name.clone();
            sum.cases += 1;
            // the case's Rust side, keeping exact copies of the bodies it measures
            goldens::dump::capture();
            let out =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unit.run_rust(case_in)));
            let captured = goldens::dump::take();
            let Ok(Ok(out)) = out else {
                failures.push(format!("{unit_id} {case}: harness error or panic"));
                continue;
            };
            let mut old_out: Output = out.clone();
            let measured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for cap in &captured {
                    let (label, n) = (cap.label.as_str(), cap.n);
                    let kind = kind_of(label);
                    {
                        sum.bodies += 1;
                        let k = Kernel::default();
                        let b0 = cap.brep.as_ref().unwrap();
                        let copy = || b0.clone_standing_for_self().unwrap();
                        let mut b = copy();
                        let Some(new) = new_record(&k, &mut b, kind) else {
                            // the copy's measure failed: if the run's failed too, the golden
                            // row checks its status; if the run's succeeded, this body's
                            // checks cannot run, which is a failure (the status entry alone
                            // decides, so one component's target is enough)
                            if run_measured(&out, kind, &targets(unit_id, kind, n, 1)) {
                                failures.push(format!(
                                    "{unit_id} {case} {label}.{n}: the run measured this body \
                                     but its copy does not"
                                ));
                            }
                            continue;
                        };
                        let tg = targets(unit_id, kind, n, new.len());
                        let tag = format!("{unit_id} {case} {label}.{n}");
                        if !run_measured(&out, kind, &tg) {
                            // the run's measure failed where the copy's succeeds: nothing of
                            // it is in the golden
                            sum.run_failed += 1;
                            continue;
                        }
                        // informational: whether `--dump-smb`'s file measures the same
                        match goldens::dump::smb_bytes(&k, b0).and_then(|bytes| {
                            let mut kr = Kernel::default();
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                volume_leak::read_smb(&mut kr, &bytes, true)
                            }))
                            .map(|mut r| new_record(&kr, &mut r, kind))
                            .map_err(|_| "unreadable".to_string())
                        }) {
                            Ok(Some(r)) if r == new => sum.smb_same += 1,
                            Ok(r) => {
                                sum.smb_differs += 1;
                                notes.push(format!("{tag}: its .smb measures {r:?}, not {new:?}"));
                            }
                            Err(e) => {
                                sum.smb_unreadable += 1;
                                notes.push(format!("{tag}: its .smb does not read back: {e}"));
                            }
                        }
                        // the old integrator on a fresh copy
                        let mut b_old = copy();
                        sum.missing_trims += usize::from(missing_trims(&b_old) > 0);
                        let (m, estimated, diag, bbox) = mid_and_estimated(&k, &mut b_old, kind);
                        let old = record(
                            kind,
                            &old_raw(&k, &mut b_old, accuracy(kind), m, estimated),
                            m,
                        );
                        let floor = |c: usize| NOISE * scale(kind, c, &new, m, diag);
                        // the copy is the measured body: it reproduces the run (to the noise
                        // floor)
                        for (c, (&(field, i), &v)) in tg.iter().zip(&new).enumerate() {
                            let Some(&o) = out.values.get(field).and_then(|x| x.get(i)) else {
                                failures.push(format!("{tag}: output lacks {field}[{i}]"));
                                continue;
                            };
                            let d = (o - v).abs();
                            sum.worst_reload = sum.worst_reload.max(d / floor(c) * NOISE);
                            if d > floor(c) {
                                failures
                                    .push(format!("{tag}: reload {field}[{i}] {v:e} vs run {o:e}"));
                            }
                        }
                        // the output as the old integrator gives it: the golden's own value
                        // where the recomputed one agrees with it to the noise floor
                        for (c, (&(field, i), &v)) in tg.iter().zip(&old).enumerate() {
                            let stored = gold
                                .get(&case)
                                .and_then(|g| g["values"][field].as_array())
                                .and_then(|a| a.get(i))
                                .and_then(serde_json::Value::as_f64)
                                .filter(|&g| (g - v).abs() <= floor(c));
                            if let Some(x) =
                                old_out.values.get_mut(field).and_then(|x| x.get_mut(i))
                            {
                                *x = stored.unwrap_or(v);
                            }
                        }
                        if let Some(inv) = &inventory {
                            for (j, &(field, i)) in tg.iter().enumerate() {
                                let key = (
                                    unit_id.to_string(),
                                    case.clone(),
                                    format!("values.{field}[{i}]"),
                                );
                                if let Some(&(io, inew)) = inv.get(&key) {
                                    sum.inventory_rows += 1;
                                    let d = (io - old[j]).abs().max((inew - new[j]).abs());
                                    sum.worst_inventory =
                                        sum.worst_inventory.max(d / floor(j) * NOISE);
                                    // a golden blessed before a change that moved it within
                                    // the goldens' own 1e-9 is stale, not the old integrator:
                                    // reported; the gate then measures the move from today's
                                    // old integrator
                                    let stale = (io - old[j]).abs() > floor(j)
                                        && (io - old[j]).abs() <= golden_tol(io, old[j]);
                                    if stale {
                                        sum.stale_golden += 1;
                                        notes.push(format!(
                                            "{tag}: {field}[{i}]'s golden {io:e} is stale: the old integrator gives {:e} today (within the goldens' 1e-9)",
                                            old[j]
                                        ));
                                    }
                                    if (inew - new[j]).abs() > floor(j)
                                        || ((io - old[j]).abs() > floor(j) && !stale)
                                    {
                                        failures.push(format!(
                                        "{tag}: inventory {field}[{i}] old {io:e} new {inew:e}, here old {:e} new {:e}",
                                        old[j], new[j]
                                    ));
                                    }
                                }
                            }
                        }
                        // the leak bound, per component
                        let mut b_s = copy();
                        new_record(&k, &mut b_s, kind); // the trims as the measure leaves them
                        let (pieces, gap, corner, gap_at) = closure_pieces(&k, &mut b_s);
                        sum.worst_gap = sum.worst_gap.max(gap);
                        sum.worst_corner = sum.worst_corner.max(corner);
                        // informational: the tube bound's premise (not the gate)
                        if gap > 1.0 {
                            sum.over_tolerance += 1;
                            notes.push(format!(
                                "{tag}: a sampled gap is {gap:.3} x its edge tolerance ({gap_at})"
                            ));
                        }
                        if corner > 1.0 {
                            sum.over_tolerance += 1;
                            notes.push(format!(
                                "{tag}: a corner point is {corner:.3} x its vertex tolerance"
                            ));
                        }
                        let vi = usize::from(kind != Kind::Volume);
                        let centroid = if kind == Kind::Mass {
                            Point3::new(old[3], old[4], old[5])
                        } else {
                            Point3::ZERO
                        };
                        let mut moved = false;
                        let mut allowance = Vec::new();
                        for c in 0..new.len() {
                            let bound: f64 = pieces
                                .iter()
                                .map(|p| {
                                    // a shadow moves the face's own area by at most its area
                                    if p.shadow && kind != Kind::Volume && c == 0 {
                                        p.area
                                    } else {
                                        p.area * field_sup(kind, c, p, m, centroid, new[vi])
                                    }
                                })
                                .sum();
                            let allowed = bound + NOISE * scale(kind, c, &new, m, diag);
                            allowance.push(allowed);
                            let d = (new[c] - old[c]).abs();
                            moved |= d != 0.0;
                            if bound > 0.0 {
                                sum.worst_leak_ratio = sum.worst_leak_ratio.max(d / bound);
                            }
                            if d.is_nan() || d > allowed {
                                failures.push(format!(
                                "{tag}: [{c}] |new - old| = {d:e} (old {:e}, new {:e}) > leak bound {bound:e} + floor",
                                old[c], new[c]
                            ));
                            }
                        }
                        sum.moved += usize::from(moved);
                        // the gate on the diff's own rows: each moved golden value within
                        // its component's old leak bound
                        if let Some(inv) = &inventory {
                            for (j, &(field, i)) in tg.iter().enumerate() {
                                let key = (
                                    unit_id.to_string(),
                                    case.clone(),
                                    format!("values.{field}[{i}]"),
                                );
                                if let Some(&(io, inew)) = inv.get(&key) {
                                    sum.inventory_gated += 1;
                                    gated.insert(key.clone());
                                    // from a stale golden (above): from today's old value
                                    let io = if (io - old[j]).abs() <= floor(j) {
                                        io
                                    } else {
                                        old[j]
                                    };
                                    if (inew - io).abs().is_nan()
                                        || (inew - io).abs() > allowance[j]
                                    {
                                        failures.push(format!(
                                            "{tag}: row {field}[{i}] moved {io:e} -> {inew:e} ({:e}), beyond its old leak bound {:e}",
                                            (inew - io).abs(),
                                            allowance[j]
                                        ));
                                    }
                                }
                            }
                        }
                        // origin independence
                        let o3 = m + Vec3::new(0.37, -0.21, 0.55) * diag;
                        match about_origins(&k, &mut b_s, kind, m, o3, &bbox) {
                        None => failures.push(format!(
                            "{tag}: no accuracy of {ORIGIN_ACCURACY:?} converges about all three origins"
                        )),
                        Some((acc, area, [at_m, rest @ ..])) => {
                            *sum.origin_accuracy.entry(format!("{acc:e}")).or_insert(0) += 1;
                            let volume = at_m[vi].abs();
                            for (o, r) in [Point3::ZERO, o3].into_iter().zip(rest) {
                                // the faces' quadrature grows with the integrand, so with the
                                // origin's distance: 1e-9 of the scale times (1 + A |o - m| / V)
                                // (`volume_leak`'s `origin_allowance`, per component)
                                let grow = 1.0 + area * (o - m).length() / volume;
                                for c in 0..r.len() {
                                    let s = scale(kind, c, &at_m, m, diag) * grow;
                                    // (a body of no volume: both 0)
                                    let d = if r[c] == at_m[c] {
                                        0.0
                                    } else {
                                        (r[c] - at_m[c]).abs() / s
                                    };
                                    sum.worst_origin = sum.worst_origin.max(d);
                                    if d.is_nan() || d > ORIGIN_REL {
                                        // the old integrator's spread there, for scale
                                        let old_at = |o: Point3| {
                                            let mut b = copy();
                                            record(kind, &old_raw(&k, &mut b, acc, o, estimated), o)
                                        };
                                        let old_spread = (old_at(o)[c] - old_at(m)[c]).abs();
                                        failures.push(format!(
                                            "{tag}: [{c}] about {o:?} at {acc:e}: {:e} vs {:e} about the middle ({d:e} of scale; the old integrator's spread {old_spread:e})",
                                            r[c], at_m[c]
                                        ));
                                    }
                                }
                            }
                        }
                    }
                        // the tube bound, relative to the volume
                        let mut tube = 0.0;
                        for e in p1::edges(&b_s, None).unwrap() {
                            let t = edge::tolerance(&b_s, e).unwrap();
                            tube += PI * t * t * edge_length(&k, &b_s, e, 1e-9).unwrap_or(0.0);
                        }
                        for v in p1::vertices(&b_s, None).unwrap() {
                            let t = vertex::tolerance(&b_s, v).unwrap();
                            tube += 4.0 / 3.0 * PI * t * t * t;
                        }
                        sum.worst_tube = sum.worst_tube.max(tube / new[vi].abs());
                    }
                }
            }));
            if let Err(p) = measured {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_default();
                failures.push(format!("{unit_id} {case}: panic while measuring: {msg}"));
                continue;
            }
            // the golden row: the output with old values, or as it is (new)
            let Some(g) = gold.get(&case) else {
                continue; // known failing: no golden line
            };
            let floors = unit.abs_floors();
            let as_new = golden::diff(g, &golden::encode(&case, &out), floors);
            let as_old = golden::diff(g, &golden::encode(&case, &old_out), floors);
            match (as_old, as_new) {
                (None, _) => sum.golden_old += 1,
                (_, None) => sum.golden_new += 1,
                (Some(a), Some(_)) => {
                    failures.push(format!(
                        "{unit_id} {case}: golden row differs: {:?}",
                        a.details
                    ));
                }
            }
        }
        let _ = writeln!(report, "{unit_id}: {sum:?}");
    }
    // every inventory row of these units (and the filter) must have been gated: a row of no
    // measured body, field or component would otherwise pass unchecked
    if let Some(inv) = &inventory {
        for key in inv.keys() {
            let (unit_id, case, field) = key;
            let applies = UNITS.contains(&unit_id.as_str())
                && filter.as_ref().is_none_or(|f| case.contains(f.as_str()));
            if applies && !gated.contains(key) {
                failures.push(format!(
                    "{unit_id} {case}: inventory row {field} was never checked"
                ));
            }
        }
    }
    println!("reported only:\n{}", notes.join("\n"));
    println!("{report}");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}\n{report}",
        failures.len(),
        failures.join("\n")
    );
}

/// `--dump-smb`'s writer is off by default and writes nothing.
#[test]
fn dumping_is_off_by_default() {
    let mut k = Kernel::default();
    let b = solidcore::primitive::api_primitives::create_box(
        &mut k,
        Point3::new(0.0, 0.0, 0.0),
        1.0,
        1.0,
        1.0,
    )
    .unwrap();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("issue_16735_dump_off");
    goldens::dump::body(&k, "mass", &b);
    assert!(!goldens::dump::path(&dir, "u", "c", "mass", 0).exists());
    // on: the body round-trips with its volume
    goldens::dump::start(&dir, "u", "c/1");
    goldens::dump::body(&k, "mass", &b);
    goldens::dump::body(&k, "mass", &b);
    goldens::dump::stop();
    goldens::dump::body(&k, "mass", &b);
    assert!(!goldens::dump::path(&dir, "u", "c/1", "mass", 2).exists());
    let file = goldens::dump::path(&dir, "u", "c/1", "mass", 1);
    let mut r = volume_leak::read_smb(&mut k, &std::fs::read(file).unwrap(), true);
    assert_eq!(volume_leak::counts(&r), [6, 12, 8]);
    let v = new_record(&k, &mut r, Kind::Volume).unwrap()[0];
    assert!((v - 1.0).abs() <= 1e-12, "{v}");
}
