//! WR-580 (tangency-labs/solidcore#1519; the fork's tangency-labs/smlib@81cd61a): the seat
//! and top tubes of a bicycle frame (r 16; the top tube from (-150, 0, 820) to
//! (600, 0, 860), the seat tube from (0, 0, 300) to (-150, 0, 820)) as SolidCraft's bench
//! builds them: each axis rounded to 1e-6, its base and height to 1e-3, the circle started
//! 0.6131 rad round from its frame's x (Z x axis, or X for an axis within 0.9 of Z). The
//! top tube's wall cut by the seat's end plane runs within tolerance of the seat's end
//! circle between two of the joint's vertices: upstream's `SmEdgeDefinition::Imprint`
//! makes the intersection edge on the seat's end cap beside that circle, and the merge
//! copies the circle into the top tube's wall beside the curve's own edge (coincident
//! edges, `SmFace` tests 33 and 34).
//!
//! The fork builds the tubes with its C API's extrude; this builds each as
//! `SmApiCreateCylinder` (seam at its frame's +X) turned about Z to put the seam where the
//! bench's circle starts, about Y onto the axis, and moved to the base, as the
//! `boolean.api_brep` fixture `issue_1519_bench_tubes.json` does on both sides. The union
//! is run both with `proto_edge_splits_at_captured_vertices` (WR-579, #1517) set, as the
//! fork had it, and without it.
//!
//! Expected: `Imprint` takes the seat's end circle as the curve's edge on the cap (the
//! two whole curves proved within tolerance on their control points), and the union is
//! one manifold solid whose level-2 check reports nothing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::topology::brep::p1::{edges, faces, vertices};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::proto_topology::p1::api_transform::{
    api_rotate, api_translate, TransformObject,
};
use solidcore::topology::vertex;
use solidcore::{Brep, Kernel};

/// The tubes' radius.
const R: f64 = 16.0;

/// One tube as the test builds it: `SmApiCreateCylinder` at the origin, `length` along Z,
/// turned `z_deg` about Z and `y_deg` about Y, moved to `origin`.
#[derive(Clone, Copy)]
struct Tube {
    origin: [f64; 3],
    length: f64,
    z_deg: f64,
    y_deg: f64,
}

/// The top tube, from (-150, 0, 820) towards (600, 0, 860).
const TOP: Tube = Tube {
    origin: [-150.0, 0.0, 820.0],
    length: 751.066,
    z_deg: 125.12804241947079,
    y_deg: 86.94709765086374,
};

/// The seat tube, from (0, 0, 300) towards (-150, 0, 820).
const SEAT: Tube = Tube {
    origin: [0.0, 0.0, 300.0],
    length: 541.202,
    z_deg: 35.12804241947077,
    y_deg: -16.090829052954405,
};

impl Tube {
    /// The base centre.
    fn base(self) -> Vec3 {
        Vec3::from(self.origin)
    }

    /// The unit axis: Z turned `y_deg` about Y (the turn about Z keeps Z).
    fn axis(self) -> Vec3 {
        let y = self.y_deg.to_radians();
        Vec3::new(y.sin(), 0.0, y.cos())
    }

    /// The tube's volume, πr²L.
    fn volume(self) -> f64 {
        std::f64::consts::PI * R * R * self.length
    }

    /// The tube as the fixture's `moved` cylinder: `SmApiCreateCylinder` at the origin,
    /// turned `z_deg` about Z (its seam off +X) and `y_deg` about Y (its axis onto the
    /// tube's), then moved to `origin`.
    fn build(self, k: &mut Kernel) -> Brep {
        let mut b = create_cylinder(k, Point3::ZERO, R, self.length).unwrap();
        let o = Point3::ZERO;
        api_rotate(
            k,
            Some(TransformObject::Brep(&mut b)),
            o,
            Vec3::Z,
            self.z_deg,
        )
        .unwrap();
        api_rotate(
            k,
            Some(TransformObject::Brep(&mut b)),
            o,
            Vec3::Y,
            self.y_deg,
        )
        .unwrap();
        api_translate(k, Some(TransformObject::Brep(&mut b)), self.base()).unwrap();
        b
    }

    /// The interval of ρ in `[0, R]` over which `p + ρ u` is inside this tube, if any: one
    /// quadratic in ρ across the axis, a linear one along it (the tube is convex).
    fn ray_inside(self, p: Vec3, u: Vec3) -> Option<(f64, f64)> {
        let a = self.axis();
        let d = p - self.base();
        // along the axis: 0 <= (d + ρ u)·a <= L
        let (h0, h1) = (d.dot(a), u.dot(a));
        let (mut lo, mut hi) = (0.0_f64, R);
        if h1.abs() < 1e-15 {
            if h0 < 0.0 || h0 > self.length {
                return None;
            }
        } else {
            let (t0, t1) = (-h0 / h1, (self.length - h0) / h1);
            lo = lo.max(t0.min(t1));
            hi = hi.min(t0.max(t1));
        }
        // across it: |w0 + ρ w1|² <= R², w the part off the axis
        let w0 = d - a * h0;
        let w1 = u - a * h1;
        let (qa, qb, qc) = (w1.dot(w1), 2.0 * w0.dot(w1), w0.dot(w0) - R * R);
        if qa < 1e-15 {
            if qc > 0.0 {
                return None;
            }
        } else {
            let disc = qb * qb - 4.0 * qa * qc;
            if disc < 0.0 {
                return None;
            }
            let s = disc.sqrt();
            lo = lo.max((-qb - s) / (2.0 * qa));
            hi = hi.min((-qb + s) / (2.0 * qa));
        }
        (hi > lo).then_some((lo, hi))
    }
}

/// The volume the two tubes share, computed without the kernel: in the top tube's frame
/// (station t along its axis, polar ρ and φ across it) each ray from the axis meets the
/// seat in one ρ interval (`Tube::ray_inside`), whose ∫ρ dρ is exact; the midpoint rule
/// over `n` stations and `n` angles does the rest. The top tube's axis runs at
/// cos⁻¹(-0.2256) to the seat's from the seat's end, so no seat point lies past station
/// 0.2256 · 541.2 + 16 < 140: the stations run over [0, 140].
fn overlap_volume(n: usize) -> f64 {
    let a = TOP.axis();
    let e1 = a.cross(Vec3::Y);
    let e1 = e1 * (1.0 / e1.length());
    let e2 = a.cross(e1);
    let (dt, dphi) = (140.0 / n as f64, 2.0 * std::f64::consts::PI / n as f64);
    let mut sum = 0.0;
    for i in 0..n {
        let p = TOP.base() + a * ((i as f64 + 0.5) * dt);
        for j in 0..n {
            let phi = (j as f64 + 0.5) * dphi;
            let u = e1 * phi.cos() + e2 * phi.sin();
            if let Some((lo, hi)) = SEAT.ray_inside(p, u) {
                sum += 0.5 * (hi * hi - lo * lo);
            }
        }
    }
    sum * dt * dphi
}

/// The distances from `p` to the six surfaces the tubes are bounded by: each tube's side
/// (its infinite cylinder) and its two end planes.
fn surface_distances(p: Point3) -> [f64; 6] {
    let mut out = [0.0; 6];
    for (i, t) in [TOP, SEAT].into_iter().enumerate() {
        let a = t.axis();
        let d = p - t.base();
        let h = d.dot(a);
        out[3 * i] = ((d - a * h).length() - R).abs();
        out[3 * i + 1] = h.abs();
        out[3 * i + 2] = (h - t.length).abs();
    }
    out
}

/// The union with WR-579 set to `split`.
struct Joint {
    /// Its level-2 check's failure lines.
    lines: Vec<String>,
    /// Its volume (`brep_volume` to 1e-9).
    volume: f64,
    /// Its faces, edges and vertices.
    counts: [usize; 3],
    /// Over its vertices, the distance to the second nearest of the six surfaces less the
    /// vertex's tolerance: at most 0 when every vertex lies on two of them.
    worst_vertex: f64,
}

/// The union with WR-579 set to `split`, or what went wrong.
fn join(split: bool) -> Result<Joint, String> {
    let mut k = Kernel::default();
    k.fixes.proto_edge_splits_at_captured_vertices = split;
    let top = TOP.build(&mut k);
    let seat = SEAT.build(&mut k);
    let mut r = boolean_union(&mut k, top, seat).map_err(|e| format!("union: {e:?}"))?;
    if !is_manifold_solid(&r).unwrap() {
        return Err("not a manifold solid".into());
    }
    let lines = check_brep(&mut k, &mut r, CheckLevel::Level2)
        .unwrap()
        .lines();
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let volume = brep_volume(&k, &mut r, 1e-9, &attrs).unwrap();
    let counts = [
        faces(&r, None).unwrap().len(),
        edges(&r, None).unwrap().len(),
        vertices(&r, None).unwrap().len(),
    ];
    let mut worst_vertex = f64::NEG_INFINITY;
    for v in vertices(&r, None).unwrap() {
        let mut d = surface_distances(vertex::point(&r, v).unwrap());
        d.sort_by(f64::total_cmp);
        worst_vertex = worst_vertex.max(d[1] - vertex::tolerance(&r, v).unwrap());
    }
    Ok(Joint {
        lines,
        volume,
        counts,
        worst_vertex,
    })
}

#[test]
fn wr580_tube_joint_no_coincident_edges() {
    // The overlap by the midpoint rule; halving the step moves it by under 0.01 mm³.
    let overlap = overlap_volume(2000);
    assert!((overlap - overlap_volume(1000)).abs() < 0.01);
    let want = TOP.volume() + SEAT.volume() - overlap;
    for split in [true, false] {
        let j = join(split).unwrap();
        // exactly nothing: no coincident edges (`SmFace` tests 33 and 34) and no other report
        assert!(
            j.lines.is_empty(),
            "WR-579 {split}: the check fails: {:?}",
            j.lines
        );
        // The two tubes less their overlap (about 6870 mm³ of 1.03e6). The joint's
        // intersection curves lie within the zone tolerance 1e-5 of their surfaces and the
        // faces they bound span under 1e4 mm², which moves the volume by under 0.1 mm³: the
        // slack. Either tube alone, or both without their overlap, is thousands of mm³ off.
        assert!(
            (j.volume - want).abs() < 0.1,
            "WR-579 {split}: volume {} vs {want}",
            j.volume
        );
        // One face per bounding surface (each tube's side and two end caps; each tube's cap
        // at the joint keeps a part outside the other tube), and V - E + F = 2 for the one
        // closed solid of genus 0 (8 - 12 + 6); a tube alone has 3 faces, 3 edges, 2 vertices.
        assert_eq!(
            j.counts,
            [6, 12, 8],
            "WR-579 {split}: faces, edges, vertices"
        );
        // every vertex lies within its tolerance of two of the six surfaces
        assert!(
            j.worst_vertex <= 0.0,
            "WR-579 {split}: a vertex lies {} past its tolerance off its second surface",
            j.worst_vertex
        );
    }
}
