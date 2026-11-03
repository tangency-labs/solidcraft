//! The least scale of a corner's setbacks at which its G1 fill is certified convex (the
//! fork's `SmBlendSetback::CertifiedScale`, `sm_SbModelRounds`; tangency-labs/smlib
//! 250cb3f, f1d81f4; smlib#104; 20b1c01, smlib#106(a)).
//!
//! Upstream: `source/SMLib/src/SmBlendSetback.cpp`, `source/SMLib/inc/SmBlendSetback.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`certified_scale`] | `SmBlendSetback::CertifiedScale` |
//! | `model_rounds` (private) | `sm_SbModelRounds` |
//!
//! The corner's rounds are made from its geometry: edge i's round, of radius r between its
//! faces f and g (normals n out of the material), has its ball's centre C at
//! `V + t a + s r (n_f + n_g) / (1 + n_f . n_g)` at t along the edge from the vertex V
//! (`s = -1` at a convex edge, the ball in the material; `+1` at a concave one), its section
//! the arc from `C - s r n_f` to `C - s r n_g` about C, the rational quadratic through the
//! edge's point with the weight cos of half the arc, `(1 + n_f . n_g) / 2` under the root. A
//! round runs from its end section away from the vertex, linear along.
//!
//! Deviations: the fork's status, `rdScale`, `*plSolves` and `*ppReason` are one return
//! value ([`CertifiedScale`]) or a [`Refusal`]; the solves of a refused search are not
//! returned.

use super::graph::faces_common_point;
use super::{el, fill_hole_probe, smax, smin, Edge, Face, FillStats, Probe, Refusal, Round};
use crate::error::Result;
use crate::geom::vector3d::sqrt;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::nurbs::b_spline_curve::KnotType;
use crate::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType,
};
use crate::surface::surface::Surface;

/// What [`certified_scale`] finds.
#[derive(Debug, Clone, PartialEq)]
pub struct CertifiedScale {
    /// `rdScale`: the locally least certifying scale on a grid of `1e-3`, 1 where
    /// the setbacks themselves do; 0 where none up to 2 does.
    pub scale: f64,
    /// `*plSolves`: the quadratic programmes solved.
    pub solves: usize,
    /// `*plAttempts`: fills built by the search, at most 12.
    pub attempts: usize,
    /// `*ppReason`: why none certifies, with a scale of 0.
    pub reason: Option<&'static str>,
}

/// `sm_SbModelRounds`: the corner's constant circular rounds, one per edge, each ending at
/// its setback (module docs).
fn model_rounds(
    faces: &[Face],
    edges: &[Edge],
    setbacks: &[f64],
) -> core::result::Result<Vec<BSplineSurface>, Refusal> {
    let n = edges.len();
    if n < 3 || faces.len() != n || setbacks.len() != n {
        return Err(Refusal::invalid(
            "SmBlendSetback: a corner needs three or more edges, as many faces and setbacks",
        ));
    }
    // the vertex: the faces' planes' common point (least squares)
    let v = faces_common_point(faces).ok_or_else(|| {
        Refusal::invalid("SmBlendSetback: the corner's faces meet in no one point")
    })?;
    let mut surfaces = Vec::with_capacity(n);
    for (i, edge) in edges.iter().enumerate() {
        let [f, g] = edge.faces;
        let setback = el(setbacks, i)?;
        if f >= n || g >= n || !(edge.radius > 0.0) || !(setback > 0.0) {
            return Err(Refusal::invalid(
                "SmBlendSetback: an edge needs a positive radius, a positive setback and two of the faces given",
            ));
        }
        let nf = el(faces, f)?.normal;
        let ng = el(faces, g)?.normal;
        // convex where the other edge on face g runs into face f's material
        let mut side = 0.0;
        for (j, other) in edges.iter().enumerate() {
            if j != i && (other.faces[0] == g || other.faces[1] == g) {
                side = other.away.dot(nf);
            }
        }
        if side == 0.0 {
            return Err(Refusal::invalid(
                "SmBlendSetback: an edge's face holds no other edge of the corner",
            ));
        }
        let s = if side < 0.0 { -1.0 } else { 1.0 };
        let r = edge.radius;
        let c = nf.dot(ng);
        if !(1.0 + c > 1.0e-9) {
            return Err(Refusal::invalid(
                "SmBlendSetback: an edge's faces fold onto each other",
            ));
        }
        let sw = (nf + ng) * (s * r / (1.0 + c));
        let w = sqrt(0.5 * (1.0 + c));
        let len = 2.0 * r;
        let mut pts: Vec<Point3> = Vec::with_capacity(6);
        let mut wts: Vec<f64> = Vec::with_capacity(6);
        for k in 0..2 {
            let se = v + edge.away * (setback + f64::from(k) * len);
            let sc = se + sw;
            pts.push(sc - nf * (s * r));
            pts.push(se);
            pts.push(sc - ng * (s * r));
            wts.push(1.0);
            wts.push(w);
            wts.push(1.0);
        }
        let um = [2, 2];
        let vm = [3, 3];
        let uk = [setback, setback + len];
        let vk = [0.0, 1.0];
        let surface = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 1,
            v_degree: 2,
            control_points: &pts,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &um,
            v_multiplicities: &vm,
            u_knots: &uk,
            v_knots: &vk,
            knot_type: KnotType::Unspecified,
            weights: Some(&wts),
            uv_domain: None,
        })?;
        surfaces.push(surface);
    }
    Ok(surfaces)
}

/// The corner whose model is filled (`CertifiedScale`'s arguments, for its `at` and
/// `model`).
struct Model<'a> {
    kernel: &'a Kernel,
    faces: &'a [Face],
    edges: &'a [Edge],
    setbacks: &'a [f64],
    most: Option<&'a [f64]>,
    size: f64,
}

impl Model<'_> {
    /// `at`: the setbacks at scale k, each k times its own, held at its most.
    fn at(&self, k: f64) -> Result<Vec<f64>> {
        let mut out = self.setbacks.to_vec();
        for (i, x) in out.iter_mut().enumerate() {
            *x *= k;
            if let Some(m) = self.most {
                *x = smin(*x, el(m, i)?);
            }
        }
        Ok(out)
    }

    /// `model`: the model's fill at scale k, its probe at degree 9 (`probe`), or the fill
    /// itself (taken certified: `true`).
    fn run(
        &self,
        k: f64,
        probe: Option<&mut Probe>,
        solves: &mut usize,
    ) -> core::result::Result<bool, Refusal> {
        let ks = self.at(k)?;
        let surfaces: Vec<Surface> = model_rounds(self.faces, self.edges, &ks)?
            .into_iter()
            .map(Surface::from)
            .collect();
        let mut rounds = Vec::with_capacity(surfaces.len());
        for (surface, &end) in surfaces.iter().zip(&ks) {
            rounds.push(Round {
                surface,
                along: SurfParamType::U,
                end,
                corner_above: false,
            });
        }
        let tol = 1.0e-9 * (1.0 + k * self.size);
        let mut as_fill = Probe {
            fill: true,
            ..Probe::default()
        };
        let is_probe = probe.is_some();
        let mut stats = FillStats::default();
        let result = fill_hole_probe(
            self.kernel,
            self.faces,
            &rounds,
            tol,
            probe.unwrap_or(&mut as_fill),
            &mut stats,
        );
        *solves += stats.solves;
        match result {
            Err(_) if !is_probe && stats.uncertified => Ok(false),
            Err(refusal) => Err(refusal),
            Ok(_) => Ok(!is_probe),
        }
    }
}

/// `SmBlendSetback::CertifiedScale`: the smallest certifying scale on the grid whose neighbour below fails; monotonicity checked, not proved.
///
/// The grid is thousandths in `[1, 2]`. Each setback is multiplied by the scale before
/// being held at its `most`. The search returns 1 if the original fill certifies, 0 with
/// its reason if the fill at 2 does not, otherwise bisects integer grid indices. At most
/// 12 fills are built. Certification is of the QP's least-energy solution at fixed
/// subdivision depth; neither that verdict nor the room-limited feasible sets are proved
/// monotone. The result is certified at its own scale and its immediate predecessor fails.
///
/// # Errors
/// [`Refusal`] with `SM_ERR_INVALID_INPUT` and its reason where the corner is not one of
/// planes and straight edges, or `most` is not one per edge; a failing fill's refusal.
pub fn certified_scale(
    kernel: &Kernel,
    faces: &[Face],
    edges: &[Edge],
    setbacks: &[f64],
    most: Option<&[f64]>,
) -> core::result::Result<CertifiedScale, Refusal> {
    let mut solves = 0;
    let mut size = 0.0;
    for &x in setbacks {
        size = smax(size, x.abs());
    }
    if most.is_some_and(|m| m.len() != setbacks.len()) {
        return Err(Refusal::invalid(
            "SmBlendSetback: one most setback per edge",
        ));
    }
    let ctx = Model {
        kernel,
        faces,
        edges,
        setbacks,
        most,
        size,
    };
    let mut attempts = 0;
    let mut certified = |i: i32| {
        attempts += 1;
        ctx.run(f64::from(i) / 1000.0, None, &mut solves)
    };
    if certified(1000)? {
        return Ok(CertifiedScale {
            scale: 1.0,
            solves,
            attempts,
            reason: None,
        });
    }
    if !certified(2000)? {
        return Ok(CertifiedScale {
            scale: 0.0,
            solves,
            attempts,
            reason: Some(
                "SmBlendSetback: no G1 fill is certified convex at the corner's setbacks up to twice them",
            ),
        });
    }
    // The smallest certifying scale on the grid whose neighbour below fails;
    // monotonicity checked, not proved. Both ends of the final bracket were tested.
    let (mut lo, mut hi) = (1000, 2000);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if certified(mid)? {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Ok(CertifiedScale {
        scale: f64::from(hi) / 1000.0,
        solves,
        attempts,
        reason: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;

    fn corner(radii: [f64; 3], frame: usize, concave: bool) -> (Vec<Face>, Vec<Edge>) {
        let unit = |i: usize| {
            let mut xyz = [0.0; 3];
            xyz[(i + frame) % 3] = 1.0;
            Vec3::new(xyz[0], xyz[1], xyz[2])
        };
        let faces = (0..3)
            .map(|i| Face {
                point: Point3::default(),
                normal: unit(i) * if concave { 1.0 } else { -1.0 },
            })
            .collect();
        let edges = (0..3)
            .map(|i| Edge {
                away: unit(i),
                radius: radii[i],
                faces: [(i + 1) % 3, (i + 2) % 3],
            })
            .collect();
        (faces, edges)
    }

    #[test]
    fn thousandth_scales_and_attempt_budget() {
        let kernel = Kernel::default();
        for (radii, setbacks, most, want, want_solves) in [
            ([2.0, 3.0, 4.0], [4.5, 4.75, 6.0], None, 1.056, 35),
            ([1.0, 1.5, 2.0], [2.25, 2.375, 3.0], None, 1.056, 35),
            (
                [2.0, 3.0, 4.0],
                [4.5, 4.75, 4.5],
                Some([20.0, 20.0, 4.5]),
                1.046,
                39,
            ),
            ([2.0, 3.0, 4.0], [4.25; 3], None, 1.110, 34),
            ([5.0; 3], [8.0; 3], None, 1.0, 2),
            // Every edge held at its original setback: no scale can certify.
            (
                [2.0, 3.0, 4.0],
                [4.5, 4.75, 6.0],
                Some([4.5, 4.75, 6.0]),
                0.0,
                10,
            ),
        ] {
            let (faces, edges) = corner(radii, 0, false);
            let result = certified_scale(
                &kernel,
                &faces,
                &edges,
                &setbacks,
                most.as_ref().map(<[_; 3]>::as_slice),
            )
            .unwrap();
            println!(
                "scale {} attempts {} solves {}",
                result.scale, result.attempts, result.solves
            );
            assert_eq!(result.scale, want);
            assert!(result.attempts <= 12);
            assert_eq!(result.solves, want_solves, "SMLib 20b1c01 QP count");
            assert_eq!(
                result.attempts,
                if want == 1.0 {
                    1
                } else if want == 0.0 {
                    2
                } else {
                    12
                }
            );
            assert_eq!(result.reason.is_some(), want == 0.0);
        }
    }

    /// Separate expensive run mode, as requested by the handoff. Seventeen descriptions
    /// cover axis permutations, concavity, scale changes and room-limited growth.
    /// The smallest certifying scale on the grid whose neighbour below fails;
    /// monotonicity checked, not proved.
    #[test]
    #[ignore = "explicit setback monotonicity gate: many full QP fills"]
    fn setback_monotone_grid() {
        let kernel = Kernel::default();
        let mut corners = 0;
        for frame in 0..3 {
            for (radii, setbacks, most, want, concave) in [
                ([2.0, 3.0, 4.0], [4.5, 4.75, 6.0], None, 1056, false),
                ([1.0, 1.5, 2.0], [2.25, 2.375, 3.0], None, 1056, true),
                (
                    [2.0, 3.0, 4.0],
                    [4.5, 4.75, 4.5],
                    Some([20.0, 20.0, 4.5]),
                    1046,
                    false,
                ),
                ([2.0, 3.0, 4.0], [4.25; 3], None, 1110, false),
                ([5.0; 3], [8.0; 3], None, 1000, false),
                ([3.0; 3], [4.5; 3], None, 1000, false),
            ] {
                // Two equal-radius permutations suffice, keeping seventeen corners.
                if frame == 2 && radii == [3.0; 3] {
                    continue;
                }
                let (faces, edges) = corner(radii, frame, concave);
                let most = most.as_ref().map(<[_; 3]>::as_slice);
                let result = certified_scale(&kernel, &faces, &edges, &setbacks, most).unwrap();
                assert_eq!(result.scale, f64::from(want) / 1000.0);
                let model = Model {
                    kernel: &kernel,
                    faces: &faces,
                    edges: &edges,
                    setbacks: &setbacks,
                    most,
                    size: setbacks.iter().copied().fold(0.0, f64::max),
                };
                let mut grid = vec![1000, want + 20, want + 100, 1500, 2000];
                grid.extend((want - 3..=want + 3).filter(|i| *i >= 1000));
                grid.sort_unstable();
                grid.dedup();
                for i in grid {
                    let mut solves = 0;
                    assert_eq!(
                        model.run(f64::from(i) / 1000.0, None, &mut solves).unwrap(),
                        i >= want,
                        "corner {corners} grid {i}"
                    );
                }
                corners += 1;
            }
        }
        assert_eq!(corners, 17);
    }
}
