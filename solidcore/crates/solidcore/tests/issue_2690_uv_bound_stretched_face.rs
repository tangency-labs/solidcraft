//! #2690: the G^n certificate's uv-curve term on a face whose parameters are scaled very
//! differently. The ball of radius 2 at the root of a cylinder of radius 8 standing on a
//! plane (as in `wr698_circle_round_definition`), with the plane's NURBS stretched in `u`:
//! `u` runs across 4000 along x and `v` across 40 along y, so `|S_u| = 4000` and
//! `|S_v| = 40`, and the foot's `u` moves a hundred times less than its `v` along the
//! round. A face point on the foot's uv curve is off the true foot by at most
//! `|S_u| |du| + |S_v| |dv|`; the fork's `(|S_u| + |S_v|) max(|du|, |dv|)` pairs the large
//! `|S_u|` with `v`'s error and refined such a round into far more spans than its
//! tolerance needs (a user design's fillet: 1,234 span certifications, 29 s).
//!
//! Pinned: the round is certified within its tolerance in at most a ceiling of spans (28
//! measured; the fork's form takes 32, the unstretched plate 16).
#![allow(clippy::unwrap_used, clippy::panic)]

use solidcore::blend::fillet_section::section::{ArcSection, GnSectionFsg};
use solidcore::geom::vector3d::Point3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use solidcore::surface::surface::Surface;

/// The quarter circle of radius `r` about the z axis at height `z`, from +x to +y, with x
/// mapped to `ax x + bx` and y to `ay y + by` (an affine image, for the uv curve).
fn quarter(dim: u64, r: f64, z: f64, [ax, bx]: [f64; 2], [ay, by]: [f64; 2]) -> BSplineCurve {
    let p = |x: f64, y: f64| Point3::new(ax * x + bx, ay * y + by, z);
    let cps = [p(r, 0.0), p(r, r), p(0.0, r)];
    let w = 0.5_f64.sqrt();
    BSplineCurve::create_canonical(&CanonicalData {
        dim,
        degree: 2,
        control_points: &cps,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[3, 3],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: Some(&[1.0, w, 1.0]),
        trim: None,
    })
    .unwrap()
}

fn surface(du: usize, dv: usize, cps: &[Point3], weights: Option<&[f64]>) -> Surface {
    Surface::from(
        BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: du,
            v_degree: dv,
            control_points: cps,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &[du + 1, du + 1],
            v_multiplicities: &[dv + 1, dv + 1],
            u_knots: &[0.0, 1.0],
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights,
            uv_domain: None,
        })
        .unwrap(),
    )
}

/// The plane z = 0 over `[-wx, wx] x [-wy, wy]`, u along x and v along y.
fn plate(wx: f64, wy: f64) -> Surface {
    surface(
        1,
        1,
        &[
            Point3::new(-wx, -wy, 0.0),
            Point3::new(-wx, wy, 0.0),
            Point3::new(wx, -wy, 0.0),
            Point3::new(wx, wy, 0.0),
        ],
        None,
    )
}

/// The cylinder of radius 8 about the z axis, a quarter turn (u) by 10 high (v).
fn cylinder() -> Surface {
    let w = 0.5_f64.sqrt();
    let mut cps = Vec::new();
    let mut ws = Vec::new();
    for (p, wi) in [(8.0, 0.0), (8.0, 8.0), (0.0, 8.0)]
        .into_iter()
        .zip([1.0, w, 1.0])
    {
        for z in [0.0, 10.0] {
            cps.push(Point3::new(p.0, p.1, z));
            ws.push(wi);
        }
    }
    surface(2, 1, &cps, Some(&ws))
}

/// The kernel of these rounds, with the result-invariant shortcuts `on` or off.
fn kernel(on: bool) -> Kernel {
    Kernel {
        memoize_curve_caches: on,
        ..Kernel::default()
    }
}

/// The root round on the plate `[-wx, wx] x [-wy, wy]`: (round, bound, spans).
fn root_round(k: &Kernel, wx: f64, wy: f64) -> (Surface, f64, usize) {
    let (f1, f2) = (plate(wx, wy), cylinder());
    let id = [1.0, 0.0];
    let center = quarter(3, 10.0, 2.0, id, id);
    let mut rail1 = quarter(3, 10.0, 0.0, id, id);
    let mut rail2 = quarter(3, 8.0, 2.0, id, id);
    // on the plate (u, v) = ((x + wx) / 2wx, (y + wy) / 2wy); on the cylinder (t, 0.2)
    let mut uv1 = quarter(2, 10.0, 0.0, [0.5 / wx, 0.5], [0.5 / wy, 0.5]);
    let mut uv2 = BSplineCurve::create_line_segment(
        2,
        Point3::new(0.0, 0.2, 0.0),
        Point3::new(1.0, 0.2, 0.0),
    )
    .unwrap();
    let mut g = GnSectionFsg::circle(ArcSection {
        approximate_arcs: false,
        ..ArcSection::default()
    });
    let round = g
        .build_with_radius(
            k,
            &center,
            [&mut rail1, &mut rail2],
            1e-3,
            [&f1, &f2],
            [&mut uv1, &mut uv2],
            false,
            2.0,
        )
        .unwrap();
    (round, g.certified_bound, g.certified_spans)
}

#[test]
fn a_stretched_plate_is_certified_in_few_spans() {
    let k = kernel(true);
    let (_, even_bound, even_spans) = root_round(&k, 20.0, 20.0);
    let (_, bound, spans) = root_round(&k, 2000.0, 20.0);
    println!(
        "#2690 plate 40 x 40: spans {even_spans} bound {even_bound:e}; \
         4000 x 40: spans {spans} bound {bound:e}"
    );
    assert!(bound > 0.0 && bound <= 1e-4, "{bound:e}");
    // the stretched plate's spans: a ceiling (#2690)
    assert!(spans <= SPAN_CEILING, "{spans} spans");
}

/// The stretched plate's spans: 28 measured, where the fork's form
/// (`(|S_u| + |S_v|) max(|du|, |dv|)`, which the certificate still steers its levels by)
/// takes 32 and the unstretched plate 16 (#2690).
const SPAN_CEILING: usize = 28;
