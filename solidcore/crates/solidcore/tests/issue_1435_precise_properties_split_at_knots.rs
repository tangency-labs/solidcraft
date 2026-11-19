//! #1435 (WR-494): `SmFace::ComputePreciseProperties` splits both integration levels at the
//! surface's knots (the SolidCraft fork's tangency-labs/smlib@08d9a14; the only path, no
//! flag), so curved primitives' areas and inertia hold 1e-9 of their closed forms at a
//! requested accuracy of 1e-8.
//!
//! `box_w_slot_corner_moments_end_their_trim_pieces_on_knot_lines`: a trim piece also ends
//! where its trim crosses one of its surface's interior v knot lines on an unbanded face,
//! where the outer integrand has a kink.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{
    brep_area, brep_mass_properties, face_area, PropertyAttributes,
};
use solidcore::math::Vec3;
use solidcore::primitive::api_primitives::{
    create_cone, create_cylinder, create_sphere, create_torus,
};
use solidcore::topology::brep::p1::faces;
use solidcore::Kernel;
use std::f64::consts::PI;

const ACCURACY: f64 = 1e-8;
const REL: f64 = 1e-9;

fn assert_rel(name: &str, got: f64, exact: f64) {
    let err = (got - exact).abs() / exact.abs();
    assert!(err <= REL, "{name}: {got} vs {exact} ({err:e} relative)");
}

/// The issue's repro: the cylindrical face of `create_cylinder`.
#[test]
fn cylinder_side_face_area_holds_its_accuracy() {
    let mut k = Kernel::default();
    let mut b = create_cylinder(&mut k, Vec3::new(1.0, 2.0, -3.0), 5.0, 12.0).unwrap();
    let side = faces(&b, None).unwrap()[0];
    let a = face_area(&k, &mut b, side, ACCURACY).unwrap();
    assert_rel("cylinder side", a, 2.0 * PI * 5.0 * 12.0);
}

/// The issue's table: the whole brep's area of each curved primitive.
#[test]
fn curved_primitive_areas_hold_their_accuracy() {
    let mut k = Kernel::default();
    let origin = Vec3::new(1.0, 2.0, -3.0);
    let s_cone = (36.0_f64 + 81.0).sqrt();
    let s_frustum = (16.0_f64 + 81.0).sqrt();
    let cases: [(&str, solidcore::topology::Brep, f64); 5] = [
        (
            "cylinder r5 h12",
            create_cylinder(&mut k, origin, 5.0, 12.0).unwrap(),
            2.0 * PI * 25.0 + 2.0 * PI * 5.0 * 12.0,
        ),
        (
            "sphere r7",
            create_sphere(&mut k, origin, 7.0).unwrap(),
            4.0 * PI * 49.0,
        ),
        (
            "cone r6 h9",
            create_cone(&mut k, origin, 6.0, 0.0, 9.0).unwrap(),
            PI * (36.0 + 6.0 * s_cone),
        ),
        (
            "frustum r6..2 h9",
            create_cone(&mut k, origin, 6.0, 2.0, 9.0).unwrap(),
            PI * (36.0 + 4.0 + 8.0 * s_frustum),
        ),
        (
            "torus R10 r3",
            create_torus(&mut k, origin, 10.0, 3.0).unwrap(),
            4.0 * PI * PI * 30.0,
        ),
    ];
    for (name, mut b, exact) in cases {
        let a = brep_area(&k, &mut b, ACCURACY).unwrap();
        assert_rel(name, a, exact);
    }
}

/// The issue's `brep_properties` case: a torus R10 r3 at (1, -1, 0.5); its axial
/// inertia about the centre against V (R^2/2 + 5 r^2/8).
#[test]
fn torus_mass_properties_hold_their_accuracy() {
    let mut k = Kernel::default();
    let centre = Vec3::new(1.0, -1.0, 0.5);
    let (rr, r) = (10.0, 3.0);
    let mut b = create_torus(&mut k, centre, rr, r).unwrap();
    let store = AttributeStore::default();
    let key_of = |_| ObjectKey(0);
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    let m = brep_mass_properties(&k, &mut b, ACCURACY, 1.0, centre, &attributes).unwrap();
    let volume = 2.0 * PI * PI * rr * r * r;
    assert_rel("torus area", m.properties.area, 4.0 * PI * PI * rr * r);
    assert_rel("torus volume", m.properties.volume, volume);
    let ixx = volume * (rr * rr / 2.0 + 5.0 * r * r / 8.0);
    let izz = volume * (rr * rr + 3.0 * r * r / 4.0);
    assert_rel("torus Ixx", m.inertia.x, ixx);
    assert_rel("torus Iyy", m.inertia.y, ixx);
    assert_rel("torus Izz", m.inertia.z, izz);
}

mod box_w_slot {
    use solidcore::curve::curve::Curve;
    use solidcore::geom::attribute::AttributeStore;
    use solidcore::geom::extent1d::Extent1d;
    use solidcore::geom::extent3d::Extent3d;
    use solidcore::mass::integrator::properties::{brep_mass_properties_default, face_properties};
    use solidcore::math::{Point2, Vec3};
    use solidcore::query::brep::calculate_tight_bounding_box;
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::brep::p1 as bl;
    use solidcore::topology::brep::p10::TopologyEnv;
    use solidcore::topology::brep::p7::read_from_file;
    use solidcore::topology::database_io::FileType;
    use solidcore::topology::face::p2::PPF_ALL;
    use solidcore::topology::topology::NotifyRecords;
    use solidcore::topology::FaceId;
    use solidcore::{Brep, Kernel};

    const NAME: &str = "Fillet/box_w_slot.smb";

    /// `Fillet/box_w_slot.smb` read with healing on, as `SmBrep::ReadFromFile`, its eight
    /// sphere corners (radius 0.06) keeping the file's NURB and UV trims (the sphere test
    /// keeps the NURB it tested): their trims cross the NURB's interior v knot lines.
    fn read(k: &mut Kernel) -> Brep {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../upstream/TestFiles/pt_TestFiles")
            .join(NAME);
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
        let data = std::fs::read(path).unwrap();
        read_from_file(k, &mut b, data, FileType::Ascii, false, &mut env).unwrap();
        b
    }

    /// The face of a loop edge use.
    fn face_of(b: &Brep, eu: &solidcore::topology::Edgeuse) -> Option<FaceId> {
        let solidcore::topology::EdgeuseOwner::Loopuse(Some(lu)) = eu.owner else {
            return None;
        };
        let l = b.loopuse(lu).ok()?.loop_?;
        Some(b.loop_(l).ok()?.face)
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

    /// The cuts of `[a, b]` where the UV curve `t` crosses one of the knot lines `uk` (in
    /// u) or `vk` (in v): the surface is a different polynomial patch on each side, so a
    /// Gauss rule converges only on the pieces between them. Sign changes are found on 512
    /// steps and bisected to the last bit; with `vk` alone, the v crossings only.
    fn knot_cuts(k: &Kernel, t: &Curve, a: f64, b: f64, uk: &[f64], vk: &[f64]) -> Vec<f64> {
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

    /// The integrator's volume group integrands at `x` (relative to the origin) for the
    /// area vector `n`: the volume `z n_z`, then the static moments, the second moments,
    /// the products (yz, zx, xy) and the axial moments, each times `z n_z`.
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

    /// The trims of the sphere face `f` (each over its edge's interval) chained into one
    /// closed loop, each with its sense along it.
    fn trim_chain<'a>(k: &Kernel, b: &'a Brep, f: FaceId) -> Vec<((&'a Curve, Extent1d), f64)> {
        let face = b.face(f).unwrap();
        let s = face.surface.as_ref().unwrap();
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
        let img = |t: &Curve, x: f64| {
            let uv = t.evaluate_point(k, x).unwrap();
            s.evaluate_point(k, Point2::new(uv.x, uv.y)).unwrap()
        };
        let mut chain = vec![(pieces.remove(0), 1.0)];
        while !pieces.is_empty() {
            let ((lt, liv), sense) = *chain.last().unwrap();
            let end = img(lt, if sense > 0.0 { liv.max() } else { liv.min() });
            let gap = |(t, iv): &(&Curve, Extent1d), at_min: bool| {
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
        chain
    }

    /// The integrals of the sphere face `f` over its own UV trims, with `n` Gauss points on
    /// every piece between trim knots and surface knot crossings, about `origin`: the area
    /// by Gauss-Bonnet (`r^2 * |sum of (1 - cos theta) d phi|` along the trims' images,
    /// about the direction of the face's middle), then the volume group of
    /// [`volume_integrands`] by Green in UV (`sum of G(u, v) dv`, `G(u, v)` the integral of
    /// the integrands with `S_u x S_v` from the domain's `u0` to `u`, itself split at the u
    /// knots), out of the sphere. Also the volume by Stokes, line integrals alone: on the
    /// sphere `x . n = r + (c - origin) . n`, so with `a = (-y z, x z, 0) / 3`, whose curl
    /// is `z e_z - x / 3`, it is `(r A + (c - origin) . S) / 3 + sum of a . dx`, `A` the
    /// Gauss-Bonnet area and `S` the vector area `1/2 * sum of (x - c) x dx`; it fixes
    /// which way Green's sum is out.
    fn sphere_face_trims(
        k: &Kernel,
        b: &Brep,
        f: FaceId,
        n: usize,
        origin: Vec3,
    ) -> ([f64; 14], f64) {
        let face = b.face(f).unwrap();
        let s = face.surface.as_ref().unwrap();
        let SurfaceKind::Sphere(sp) = s.kind() else {
            panic!("{f:?} is not a sphere")
        };
        let (centre, radius) = (sp.base().position().origin(), sp.radius());
        let nurb = sp.base().base().gw_nurb().unwrap();
        let (uk, vk) = (nurb.u_knots().to_vec(), nurb.v_knots().to_vec());
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
        let g = gauss(n);
        let u0 = face.uv_domain.min().x;
        let mut gb = 0.0;
        let mut va = Vec3::new(0.0, 0.0, 0.0);
        let mut al = 0.0;
        let mut volume = [0.0; 13];
        for ((t, iv), sense) in trim_chain(k, b, f) {
            let (knots, _) = t.knots(k, Some(&iv)).unwrap();
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
                                for (o, val) in gv
                                    .iter_mut()
                                    .zip(volume_integrands(p2 - origin, a1.cross(a2)))
                                {
                                    *o += wt2 * h2 * val;
                                }
                            }
                        }
                        if u < u0 {
                            for o in &mut gv {
                                *o = -*o;
                            }
                        }
                        for (o, val) in volume.iter_mut().zip(gv) {
                            *o += sense * wt * h * val * d[1].y;
                        }
                    }
                }
            }
        }
        let way = if gb < 0.0 { -1.0 } else { 1.0 };
        let area = radius * radius * gb.abs();
        let stokes = (radius * area + (centre - origin).dot(va * way)) / 3.0 + al * way;
        let out = if (volume[0] < 0.0) == (stokes < 0.0) {
            1.0
        } else {
            -1.0
        };
        let mut r = [0.0; 14];
        r[0] = area;
        for (o, v) in r[1..].iter_mut().zip(volume) {
            *o = v * out;
        }
        (r, stokes)
    }

    /// The integrator's share of the sphere face `f` (the faceuse bounding the solid) in
    /// the area and the volume group about `origin`, and the same over the face's own trims
    /// (`n = 32`, checked against `n = 24`, and its volume by Green against Stokes'): each
    /// `[area, volume group (13)]`.
    fn shares(
        k: &Kernel,
        b: &Brep,
        m: &mut Brep,
        f: FaceId,
        accuracy: f64,
        origin: Vec3,
        estimated: f64,
    ) -> ([f64; 14], [f64; 14]) {
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
        let ((t24, _), (t32, stokes)) = (
            sphere_face_trims(k, b, f, 24, origin),
            sphere_face_trims(k, b, f, 32, origin),
        );
        let scale = t32[0].max(1.0);
        for (i, (a, c)) in t32.iter().zip(t24).enumerate() {
            assert!(
                (a - c).abs() <= 1e-14 * scale,
                "{f:?} [{i}]: n=32 {a} n=24 {c}"
            );
        }
        assert!(
            (stokes - t32[1]).abs() <= 2e-13 * scale,
            "{f:?}: Stokes {stokes} Green {}",
            t32[1]
        );
        (integ, t32)
    }

    /// `Fillet/box_w_slot.smb` with its sphere corners' file NURBs and trims, whose trims
    /// cross the NURBs' interior v knot lines on faces too few lines apart to be banded.
    /// `brep_mass_properties` at 1e-4 about the origin (the `blend.fillet_executive_p3`
    /// `source-file-box_w_slot-all-edges` row's call) is held to the reference made from
    /// that row with the corners' integrated shares replaced by their shares over their
    /// own trims (line integrals on the same NURB and trims, split at every knot crossing
    /// and converged). Before the integrator ended its trim pieces on those lines, the
    /// corners' kinks left `cy`, `Pyz` and `Pxy` the farthest from it.
    #[test]
    fn box_w_slot_corner_moments_end_their_trim_pieces_on_knot_lines() {
        let mut k = Kernel::default();
        let b = read(&mut k);
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
        // the case reaches the change: unbanded corners whose trims cross interior v knot
        // lines
        let mut crossings = 0;
        for &f in &spheres {
            let s = b.face(f).unwrap().surface.as_ref().unwrap();
            let SurfaceKind::Sphere(sp) = s.kind() else {
                unreachable!()
            };
            let vk = sp.base().base().gw_nurb().unwrap().v_knots().to_vec();
            let (lo, hi) = (vk[0], vk[vk.len() - 1]);
            let mut inner: Vec<f64> = vk.into_iter().filter(|&v| v > lo && v < hi).collect();
            inner.dedup();
            assert!(inner.len() < 12, "{f:?}: {} v knot lines", inner.len());
            for ((t, iv), _) in trim_chain(&k, &b, f) {
                let (knots, _) = t.knots(&k, Some(&iv)).unwrap();
                for w in knots.windows(2) {
                    crossings += knot_cuts(&k, t, w[0], w[1], &[], &inner).len() - 2;
                }
            }
        }
        assert!(crossings > 0);

        let size = |e: Extent3d| {
            let s = e.size();
            let d = s.x.max(s.y).max(s.z);
            d * d / 100.0
        };
        let mut w = read(&mut k);
        let tight = calculate_tight_bounding_box(&k, &mut w).unwrap();
        let mid = tight.mid();

        // The geometry is the same: integrated tightly, each corner's shares are those its
        // trims bound, so the reference differs from a row only by the row's quadrature.
        let mut m = read(&mut k);
        for &f in &spheres {
            let (integ, trims) = shares(&k, &b, &mut m, f, 1e-12, Vec3::ZERO, size(tight));
            for (i, (a, r)) in integ.iter().zip(trims).enumerate() {
                assert!(
                    (a - r).abs() <= 1e-14,
                    "{f:?} share {i}: integrated {a} trims {r}"
                );
            }
        }

        let mp = brep_mass_properties_default(&k, &mut w, 1e-4, 1.0, Vec3::ZERO).unwrap();
        let flat = |area: f64, volume: f64, c: Vec3, i: Vec3, p: Vec3| {
            [
                area, volume, volume, c.x, c.y, c.z, i.x, i.y, i.z, p.x, p.y, p.z,
            ]
        };
        let row = flat(
            mp.properties.area,
            mp.properties.volume,
            mp.centroid,
            mp.inertia,
            mp.products,
        );
        // the row's precise properties about `mid`, the corners swapped for their trims'
        let mut total = [0.0; 14];
        total[0] = mp.properties.area;
        total[1] = mp.properties.volume;
        for (i, v) in mp.properties.moments[4..].iter().enumerate() {
            total[2 + 3 * i] = v.x;
            total[3 + 3 * i] = v.y;
            total[4 + 3 * i] = v.z;
        }
        let mut m = read(&mut k);
        for &f in &spheres {
            let (integ, trims) = shares(&k, &b, &mut m, f, 1e-4, mid, size(tight));
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
        // Every component within 1e-11 of the reference, which the trims' shares hold to
        // 1e-14 each (n = 24 against n = 32; eight corners) on components of magnitude
        // 0.03 or more. With the kinks inside pieces, cy, Pyz and Pxy were 7.3e-10,
        // 7.1e-10 and 7.1e-10 relative off it, and the volume 2.7e-11.
        for (i, (r, e)) in row.iter().zip(reference).enumerate() {
            assert!(
                (r - e).abs() <= 1e-11 * e.abs(),
                "mass[{i}]: row {r}, reference {e}"
            );
        }
        // The row, pinned. With the gap-closure strips also split at the knot crossings,
        // the volume and second moments are within 1.5e-14 relative of the same call at
        // 1e-12 (MEASURED); with the strips across the crossings they were up to 3.8e-12
        // relative off it.
        assert_eq!(
            row,
            [
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
            ]
        );
    }
}
