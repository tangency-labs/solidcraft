//! Five p8 boundary/discontinuity calls on concrete lines, circles, stationary
//! B-splines, planes, C0/warped grids, identity and C0 B-spline volumes.
//! Native output includes complete interval partition and start/mid/end labels,
//! object identities, curve parameters and gaps. No absolute tolerance floors.

use crate::case::CaseContext;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::transform::Transform;
use solidcore::intersect::curve_class::CurveClassification;
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType,
};
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec2, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::spiral::allocate_nurb_volume;
use solidcore::surface::b_spline_volume::BSplineVolume;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::{Volume, VolumeKind};
use solidcore::SolidError;
use solidcore_ffi::units::intersect_curve_class_p8 as ffi;

/// Differential runner for curve classification part 8.
pub struct Runner;
#[derive(Clone, Serialize, Deserialize)]
struct Input {
    record: Vec<f64>,
}
#[derive(Clone, Copy, PartialEq)]
enum H {
    Curve,
    Surface,
    Volume,
    /// A temporary boundary curve (`false`) or plane (`true`) built by the call.
    Temporary(usize, bool),
}
impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Curve | Self::Temporary(_, false) => ClassObjectKind::Curve,
            Self::Surface | Self::Temporary(_, true) => ClassObjectKind::Surface,
            Self::Volume => ClassObjectKind::Volume,
        }
    }
    fn temporary(index: usize, kind: ClassObjectKind) -> Option<Self> {
        Some(Self::Temporary(index, kind == ClassObjectKind::Surface))
    }
}
struct Geometry {
    curve: Curve,
    surface: Surface,
    volume: Volume,
}
impl ClassGeometry<H> for Geometry {
    fn curve(&self, o: H) -> Option<&Curve> {
        (o == H::Curve).then_some(&self.curve)
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        (o == H::Surface).then_some(&self.surface)
    }
    fn volume(&self, o: H) -> Option<&Volume> {
        (o == H::Volume).then_some(&self.volume)
    }
    fn point(&self, _o: H) -> Option<Point3> {
        None
    }
}
fn geometry(k: &Kernel, r: &[f64; 23]) -> solidcore::Result<Geometry> {
    let a = Vec3::new(r[6], r[7], r[8]);
    let b = Vec3::new(r[9], r[16], r[17]);
    let curve = match r[2] as u8 {
        0 => Line::create_line_segment(r[1] as u64, a, b, None)?.into(),
        1 => Ellipse::new_circle(
            k,
            a,
            Vec3::X,
            Vec3::Y,
            Extent1d::from_min_max(0., 360.),
            r[9],
            &EllipseOptions {
                dim: r[1] as u64,
                ..Default::default()
            },
        )?
        .into(),
        _ => BSplineCurve::from_data(
            r[1] as u64,
            2,
            5,
            2,
            &[CPoint::new(a.x, a.y, a.z, 1.); 3],
            &[0., 0., 0., 1., 1., 1.],
        )?
        .into(),
    };
    let domain = Extent2d::from_bounds(r[10], r[11], r[12], r[13]);
    let surface = if r[3] == 0. {
        let mut p = Plane::new(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Vec2::new(r[14], r[15]),
            &domain,
        );
        p.make_nurb()?;
        p.into()
    } else {
        let nu = r[4] as usize;
        let nv = r[5] as usize;
        let knots = |n: usize, min: f64, max: f64| {
            (0..n + 2)
                .map(|i| {
                    if i == 0 {
                        min
                    } else if i > n - 1 {
                        max
                    } else {
                        min + (max - min) * (i - 1) as f64 / (n - 1) as f64
                    }
                })
                .collect::<Vec<_>>()
        };
        let u = knots(nu, r[10], r[12]);
        let v = knots(nv, r[11], r[13]);
        let points = (0..nu)
            .map(|i| {
                (0..nv)
                    .map(|j| {
                        CPoint::new(
                            r[14] * (r[10] + (r[12] - r[10]) * i as f64 / (nu - 1) as f64),
                            r[15] * (r[11] + (r[13] - r[11]) * j as f64 / (nv - 1) as f64),
                            if r[3] == 2. {
                                r[21] * i as f64 * j as f64
                            } else {
                                0.
                            },
                            1.,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        // Import the record directly: the upstream convenience constructor has an
        // unrelated asymmetric knot-copy defect; this oracle uses the raw record too.
        let mut s = solidcore::nurbs::nurbs_srf::allocate_nurb_surface(
            nu - 1,
            nv - 1,
            1,
            1,
            nu + 1,
            nv + 1,
        )?;
        s.u_knots_mut().copy_from_slice(&u);
        s.v_knots_mut().copy_from_slice(&v);
        for (i, row) in points.iter().enumerate() {
            for (j, p) in row.iter().enumerate() {
                *s.cpt_mut(i, j)? = *p;
            }
        }
        BSplineSurface::from_nurb(s, false).into()
    };
    let volume = if r[3] < 2. {
        Volume::with_maps(VolumeKind::Transform(Transform::default()), None, None)
    } else {
        let mut s = allocate_nurb_volume([2; 3], [1; 3], [4; 3])?;
        for axis in 0..3 {
            s.knots_mut(axis).copy_from_slice(&[0., 0., 0.5, 1., 1.]);
        }
        for (i, p) in s.cpts_mut().iter_mut().enumerate() {
            *p = CPoint::new(
                (i / 9) as f64 * 0.5,
                ((i / 3) % 3) as f64 * 0.5,
                (i % 3) as f64 * 0.5,
                1.,
            );
        }
        Volume::with_maps(VolumeKind::BSpline(BSplineVolume::new(s)), None, None)
    };
    Ok(Geometry {
        curve,
        surface,
        volume,
    })
}
fn rust(input: Input) -> Result<Output, HarnessError> {
    let record = input.record;
    ffi::validate(&record).map_err(|e| HarnessError::new(e.to_string()))?;
    let (header, ivls) = record
        .split_at_checked(23)
        .ok_or_else(|| HarnessError::new("missing p8 header"))?;
    let r: &[f64; 23] = header
        .try_into()
        .map_err(|_| HarnessError::new("bad p8 header"))?;
    let k = Kernel::default();
    let g = geometry(&k, r).map_err(|e| HarnessError::new(e.to_string()))?;
    let mut cc = CurveClassification::new(&k);
    cc.set_curve(Some(H::Curve), false);
    cc.set_interval(&Extent1d::from_min_max(r[18], r[19]))
        .map_err(|e| HarnessError::new(e.to_string()))?;
    cc.set_src_zone_tol_3d(solidcore::geom::tol::zone_tol3d(&k, r[20]));
    cc.set_size(&k, r[22] as usize)
        .map_err(|e| HarnessError::new(e.to_string()))?;
    for (i, v) in ivls.as_chunks::<3>().0.iter().enumerate() {
        let &[a, b, class] = v;
        let iv = cc.at_mut(i).map_err(|e| HarnessError::new(e.to_string()))?;
        iv.set_interval(&Extent1d::from_min_max(a, b));
        let c = PointClassType::from_code(class as i32)
            .ok_or_else(|| HarnessError::new("bad p8 class"))?;
        iv.mid.set_class_object(
            c,
            match c {
                PointClassType::Surface => Some(H::Surface),
                PointClassType::Volume => Some(H::Volume),
                _ => None,
            },
        );
    }
    let result = match r[0] as u8 {
        0 => cc.intersect_natural_bounds(&k, &g, &g.surface, H::Surface),
        1 => cc.intersect_natural_uv_bounds(&k, &g, &g.surface, H::Surface),
        2 => cc.intersect_uv_discontinuities(&k, &g, &g.surface, H::Surface),
        3 => cc.intersect_3d_discontinuities(&k, &g, &g.surface, H::Surface),
        _ => cc.intersect_volume_discontinuities(&k, &g, &g.volume, H::Volume, r[21] != 0.),
    };
    if let Err(ref e) = result {
        if let Some(o) = Output::unported(e) {
            return Ok(o);
        }
    }
    let status = match result {
        Ok(()) => 0,
        Err(SolidError::Upstream { code, .. }) => code,
        _ => 1001,
    };
    let mut o = vec![f64::from(status), cc.size() as f64];
    // The shim numbers objects other than the curve/surface/volume from 3 in order of
    // first appearance; temporaries (dangling after the native call) get the same ids.
    let mut temporaries: Vec<H> = Vec::new();
    for iv in cc.intervals() {
        o.extend([iv.min(), iv.max()]);
        for p in [&iv.start, &iv.mid, &iv.end] {
            o.extend([
                f64::from(p.point_class().code()),
                match p.object() {
                    None => -1.,
                    Some(H::Surface) => 1.,
                    Some(H::Volume) => 2.,
                    Some(H::Curve) => 0.,
                    Some(t @ H::Temporary(..)) => {
                        if !temporaries.contains(&t) {
                            temporaries.push(t);
                        }
                        let at = temporaries.iter().position(|x| *x == t).unwrap_or(0);
                        (3 + at) as f64
                    }
                },
                p.t_param(),
                p.deviation(),
            ]);
        }
    }
    output(&o)
}
fn output(state: &[f64]) -> Result<Output, HarnessError> {
    let bad = || HarnessError::new("malformed p8 interval-state bundle");
    let [status, count, payload @ ..] = state else {
        return Err(bad());
    };
    if !count.is_finite() || *count < 0.0 || count.fract() != 0.0 || *count > 1_000_000.0 {
        return Err(bad());
    }
    let expected = (*count as usize)
        .checked_mul(14)
        .and_then(|n| n.checked_add(2))
        .ok_or_else(bad)?;
    if state.len() != expected {
        return Err(bad());
    }
    let mut labels = vec![*status as i64, *count as i64];
    let mut values = Vec::new();
    for iv in payload.as_chunks::<14>().0 {
        let [lo, hi, points @ ..] = iv;
        values.extend([*lo, *hi]);
        for p in points.as_chunks::<4>().0 {
            let [class, object, param, gap] = p;
            labels.extend([*class as i64, *object as i64]);
            values.extend([*param, *gap]);
        }
    }
    Ok(Output::new()
        .ints("labels", labels)
        .values("values", values))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.curve_class_p8"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Input = case.parse()?;
        let state = ffi::run(&i.record).map_err(|e| HarnessError::new(e.to_string()))?;
        output(&state)
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(case.parse()?)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let op = rng.below(5);
        let dim = if rng.below(2) == 0 { 2. } else { 3. };
        let kind = rng.below(3);
        let sk = rng.below(4);
        let nu = 2 + rng.below(3);
        let nv = 2 + rng.below(3);
        let a = rng.point3(-2., 2.);
        let b = rng.point3(-2., 2.);
        let scale = 10f64.powi(rng.below(9) as i32 - 4);
        let max = if kind == 1 { 360. } else { 1. };
        let t0 = if rng.below(3) == 0 { max * 0.2 } else { 0. };
        let t1 = if rng.below(3) == 0 { max * 0.8 } else { max };
        let n = 1 + rng.below(4) as usize;
        let mut r = vec![
            op as f64,
            dim,
            kind as f64,
            sk as f64,
            nu as f64,
            nv as f64,
            a[0] * scale,
            a[1] * scale,
            if dim == 2. { 0. } else { a[2] * scale },
            if kind == 1 { scale } else { b[0] * scale },
            -0.5,
            -0.25,
            1.5,
            1.25,
            scale,
            scale,
            b[1] * scale,
            if dim == 2. { 0. } else { b[2] * scale },
            t0,
            t1,
            1e-7,
            if op == 4 {
                (rng.below(2)) as f64
            } else {
                0.2 * scale
            },
            n as f64,
        ];
        let mids = [100., 107., 110.];
        let mut prev = t0;
        for i in 0..n {
            let end = if i + 1 == n {
                t1
            } else {
                t0 + (t1 - t0) * (i + 1) as f64 / n as f64
            };
            r.extend([prev, end, *rng.choose(&mids)?]);
            prev = end;
        }
        serde_json::to_value(Input { record: r }).ok()
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut c = ctx.clone();
        c.seeds = c.seeds.max(800);
        crate::runner::default_cases(self, &c)
    }
}

#[cfg(test)]
mod tests {
    use super::output;
    #[test]
    fn rejects_short_and_long_interval_bundles() {
        let mut state = vec![0.0, 1.0];
        state.extend([0.0; 14]);
        assert!(output(&state).is_ok());
        for n in 0..state.len() {
            assert!(output(&state[..n]).is_err());
        }
        state.push(0.0);
        assert!(output(&state).is_err());
        state[1] = 2.0;
        assert!(output(&state).is_err());
    }
}
