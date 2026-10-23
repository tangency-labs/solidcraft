//! Exact patches of the analytic surfaces as the ssi-corpus describes them
//! (github.com/jyviko/ssi-corpus): a plane is bilinear, a cylinder or cone is a
//! rational quadratic arc swept along a line, a sphere or torus is a rational
//! quadratic arc revolved (The NURBS Book, A7.1 and A8.1). Angles are in
//! degrees. Each patch also keeps its analytic surface (`Patch::analytic`), so
//! its implicit equation can be read (STEP's forms, as `surface3` holds them).

use crate::geom3::{Frame, V3 as Exact3};
use crate::intersect::Options;
use crate::piece::{add, cross, norm, scale, sub, unit, V3};
use crate::surface3::Surf;
use crate::Patch;

/// A rational quadratic arc of the unit circle from a0 to a1 degrees, in
/// segments of at most 90 degrees (A7.1): Cartesian control points [cos, sin],
/// weights, and knots over the domain [a0, a1].
struct Arc {
    points: Vec<[f64; 2]>,
    weights: Vec<f64>,
    knots: Vec<f64>,
}

fn arc(a0: f64, a1: f64) -> Arc {
    let sweep = a1 - a0;
    let n = ((sweep.abs() / 90.0 - 1e-9).ceil() as usize).max(1);
    let step = sweep / n as f64;
    let corner = (step / 2.0).to_radians().cos();
    let at = |deg: f64| [deg.to_radians().cos(), deg.to_radians().sin()];
    let mut points = vec![at(a0)];
    let mut weights = vec![1.0];
    let mut knots = vec![a0, a0, a0];
    for k in 0..n {
        let start = a0 + k as f64 * step;
        let middle = at(start + step / 2.0);
        points.push([middle[0] / corner, middle[1] / corner]);
        points.push(at(start + step));
        weights.extend([corner, 1.0]);
        if k > 0 {
            knots.extend([start, start]);
        }
    }
    knots.extend([a1, a1, a1]);
    Arc {
        points,
        weights,
        knots,
    }
}

/// A patch from a grid of Cartesian poles `[i][j]` (i along u) and their weights.
fn patch_of(
    u_degree: usize,
    v_degree: usize,
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    grid: Vec<Vec<V3>>,
    weights: Vec<Vec<f64>>,
) -> Patch {
    Patch {
        u_degree,
        v_degree,
        u_count: grid.len(),
        v_count: grid.first().map_or(0, Vec::len),
        poles: grid.into_iter().flatten().collect(),
        weights: Some(weights.into_iter().flatten().collect()),
        u_knots,
        v_knots,
        analytic: None,
    }
}

/// `ImplicitCAM`'s frame through `o` with x axis `x` and z axis `z`.
fn frame(o: V3, x: V3, z: V3) -> Frame {
    let v = |p: V3| Exact3::new(p[0], p[1], p[2]);
    Frame {
        o: v(o),
        x: v(x),
        z: v(z),
    }
}

/// The unit axes of a corpus frame: x, y and z = x × y.
fn axes(xaxis: V3, yaxis: V3) -> (V3, V3, V3) {
    let (x, y) = (unit(xaxis), unit(yaxis));
    (x, y, unit(cross(x, y)))
}

/// A circle arc in the frame's (x, y) plane swept along a line in v: pole(C(i), L(j)).
fn swept(circle: &Arc, line: [f64; 2], pole: impl Fn([f64; 2], f64) -> V3) -> Patch {
    let grid = circle
        .points
        .iter()
        .map(|&c| line.iter().map(|&h| pole(c, h)).collect())
        .collect();
    let weights = circle.weights.iter().map(|&w| vec![w, w]).collect();
    patch_of(
        2,
        1,
        circle.knots.clone(),
        vec![line[0], line[0], line[1], line[1]],
        grid,
        weights,
    )
}

/// A meridian arc (rho, z) revolved by a circle arc about the frame's z axis (A8.1).
fn revolved(o: V3, (x, y, z): (V3, V3, V3), circle: &Arc, meridian: &Arc) -> Patch {
    let grid = circle
        .points
        .iter()
        .map(|c| {
            meridian
                .points
                .iter()
                .map(|&[rho, h]| {
                    add(
                        add(o, scale(x, rho * c[0])),
                        add(scale(y, rho * c[1]), scale(z, h)),
                    )
                })
                .collect()
        })
        .collect();
    let weights = circle
        .weights
        .iter()
        .map(|&wu| meridian.weights.iter().map(|&wv| wu * wv).collect())
        .collect();
    patch_of(
        2,
        2,
        circle.knots.clone(),
        meridian.knots.clone(),
        grid,
        weights,
    )
}

/// The plane `origin + a·xaxis + b·yaxis` over `a` in `u`, `b` in `v`.
pub fn plane(origin: V3, xaxis: V3, yaxis: V3, u: [f64; 2], v: [f64; 2]) -> Patch {
    let (x, _, z) = axes(xaxis, yaxis);
    let grid = u
        .iter()
        .map(|&a| {
            v.iter()
                .map(|&b| add(origin, add(scale(xaxis, a), scale(yaxis, b))))
                .collect()
        })
        .collect();
    let mut patch = patch_of(
        1,
        1,
        vec![u[0], u[0], u[1], u[1]],
        vec![v[0], v[0], v[1], v[1]],
        grid,
        vec![vec![1.0; 2]; 2],
    );
    patch.analytic = Some(Surf::Plane {
        f: frame(origin, x, z),
    });
    patch
}

/// The cylinder of `radius` about the axis through `origin` along xaxis × yaxis,
/// over `angle` from xaxis toward yaxis and `height` along the axis.
pub fn cylinder(
    origin: V3,
    xaxis: V3,
    yaxis: V3,
    radius: f64,
    angle: [f64; 2],
    height: [f64; 2],
) -> Patch {
    let (x, y, z) = axes(xaxis, yaxis);
    let r = radius;
    let mut patch = swept(&arc(angle[0], angle[1]), height, |c, h| {
        add(
            origin,
            add(scale(x, r * c[0]), add(scale(y, r * c[1]), scale(z, h))),
        )
    });
    patch.analytic = Some(Surf::Cylinder {
        f: frame(origin, x, z),
        r,
    });
    patch
}

/// The full cone with its apex at `apex`, axis xaxis × yaxis and the given half
/// angle, over `height` along the axis from the apex.
pub fn cone(apex: V3, xaxis: V3, yaxis: V3, half_angle_deg: f64, height: [f64; 2]) -> Patch {
    let (x, y, z) = axes(xaxis, yaxis);
    let half = half_angle_deg.to_radians();
    let t = half.tan();
    let mut patch = swept(&arc(0.0, 360.0), height, |c, h| {
        add(
            apex,
            add(
                scale(x, h * t * c[0]),
                add(scale(y, h * t * c[1]), scale(z, h)),
            ),
        )
    });
    // STEP's cone with radius 0 at its origin: the apex.
    patch.analytic = Some(Surf::Cone {
        f: frame(apex, x, z),
        r: 0.0,
        a: half,
    });
    patch
}

/// The sphere of `radius` about `center` over longitudes `lon` (from xaxis
/// toward yaxis) and latitudes `lat` (toward xaxis × yaxis).
pub fn sphere(
    center: V3,
    xaxis: V3,
    yaxis: V3,
    radius: f64,
    lon: [f64; 2],
    lat: [f64; 2],
) -> Patch {
    let frame_axes = axes(xaxis, yaxis);
    let mut meridian = arc(lat[0], lat[1]);
    meridian
        .points
        .iter_mut()
        .for_each(|p| *p = [radius * p[0], radius * p[1]]);
    let mut patch = revolved(center, frame_axes, &arc(lon[0], lon[1]), &meridian);
    patch.analytic = Some(Surf::Sphere {
        f: frame(center, frame_axes.0, frame_axes.2),
        r: radius,
    });
    patch
}

/// The torus about `center` with axis xaxis × yaxis, over `major` angles about
/// the axis and `minor` angles about the tube. Minor angle 0 is the outer
/// equator, rising toward the axis direction (the corpus's SMLib convention).
pub fn torus(
    center: V3,
    xaxis: V3,
    yaxis: V3,
    major_radius: f64,
    minor_radius: f64,
    major: [f64; 2],
    minor: [f64; 2],
) -> Patch {
    let frame_axes = axes(xaxis, yaxis);
    let (big, small) = (major_radius, minor_radius);
    let mut meridian = arc(minor[0], minor[1]);
    meridian
        .points
        .iter_mut()
        .for_each(|p| *p = [big + small * p[0], small * p[1]]);
    let mut patch = revolved(center, frame_axes, &arc(major[0], major[1]), &meridian);
    patch.analytic = Some(Surf::Torus {
        f: frame(center, frame_axes.0, frame_axes.2),
        major: big,
        minor: small,
    });
    patch
}

/// Options that resolve features down to the length `feature` between these
/// patches: `resolution` becomes `feature` over the diagonal of the patches'
/// poles (never coarser than the default), and `link` keeps the defaults'
/// ratio to it (1 to 100). Left at its default beside a finer resolution, `link`
/// was a third of a 1e-6 loop's radius (ssi-corpus c7_tiny_loop_parab_1e-6,
/// four open arcs).
pub fn resolving(feature: f64, patches: &[&Patch]) -> Options {
    let defaults = Options::default();
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for pole in patches.iter().flat_map(|patch| &patch.poles) {
        for ((l, h), &x) in lo.iter_mut().zip(&mut hi).zip(pole) {
            *l = l.min(x);
            *h = h.max(x);
        }
    }
    let size = match norm(sub(hi, lo)) {
        s if s > 0.0 => s,
        _ => 1.0,
    };
    let resolution = defaults.resolution.min(feature / size);
    Options {
        resolution,
        link: resolution * (defaults.link / defaults.resolution),
        ..defaults
    }
}
