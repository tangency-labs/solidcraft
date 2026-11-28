//! The mitre of two rounds of one radius rolling on one plane, built section by section
//! (#23151).
//!
//! Upstream: none (solidcore-native; the fork's mitre is `ExactMitreCurve` for cylinders,
//! [`super::exact_mitre_curve`], and the rounds' traced intersection otherwise).
//!
//! | Rust                      | Upstream |
//! |---------------------------|----------|
//! | [`section_pair`]          | none     |
//! | [`section_mitre_curve`]   | none     |
//! | [`SectionPair`]           | none     |
//! | `Station` (private)       | none     |
//! | `meet` (private)          | none     |
//!
//! A Mitre turn's edge runs from X, where the two rounds' rails cross on the face they
//! share, to S on the side edge between their other faces. Both rounds are tangent to the
//! shared face along their rails, so at X they are tangent to each other, and beside X they
//! stand `a x d / r` apart: `a` the angle the rails cross at, `x` the distance along the
//! rails and `d` the height over the face. X is a tangential intersection point, where the
//! two normals do not give a marching trace its step (`docs/generic-blend.md` [2], [34]):
//! at a kink of 0.115 degrees (`a = 2e-3`) the trace ended 6.9e-4 from X, the face of one
//! round did not close and the result was open.
//!
//! **The construction.** A round of radius `r` on a plane is the envelope of the balls of
//! radius `r` centred on its spine, a curve in the plane `r` over the shared one
//! (`docs/generic-blend.md` [37], [6]). Its points at the height `h` over the shared plane
//! are the spine's offset by `rho(h) = sqrt(r^2 - (r - h)^2)` in that plane, so two such
//! rounds meet, at each height, where the two spines' offsets by one distance cross: over
//! the bisector of the two spines, the points equally far from both, from X (`rho = 0`,
//! under the spines' crossing) to S. Where both walls stand upright on the plane, both
//! sections are quarter circles made the same way and give one height to one value of the
//! parameter across, `v`, so the mitre's point at `v` is where `S1(u1, v) = S2(u2, v)`: two
//! unknowns, whose directions `dS1/du` and `dS2/du` make the rails' angle `a` at every `v`,
//! X included. The solve is conditioned by `1 / a` along the whole mitre, where the
//! intersection's is by `1 / (a d)`.
//!
//! **Which mitres take it** is read off the two rounds' definitions before anything is
//! solved ([`section_pair`]): circular sections of one constant radius, one plane among
//! their faces, and each one's other face a plane or an extrusion square to it. Cylinders
//! keep their plane conic ([`super::exact_mitre_curve`]), every other pair its trace.
//!
//! **What is proved.** At every station the two rounds' points agree within [`AGREE`] of
//! the part's size, and the end stations are the vertices X and S within the corner's
//! tolerance. The edge's curve and its two trims are fitted through the stations, each trim
//! held in its round's domain, and the distance between each round at its trim and the
//! edge is enclosed over the whole edge ([`proved_trim_gap`]); the stations are doubled
//! until both are proved within the corner's tolerance. A mitre of this kind that is not
//! proved is refused, naming what failed; it is not handed to the trace.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::foot_on_surface;
use super::proved_trim::{held_in_domain, proved_trim_gap};
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_geom::p1::{FilletBrep, FilletEdgeType, FilletGeomId, FilletVertexType};
use crate::blend::fillet_geom::p4::set_edge_curve;
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::global_solver::SolverOperation;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::crv_fit::p1::ParamMethod;
use crate::nurbs::knots::Side;
use crate::surface::procedural_blend::{BlendDefinition, NetScope};
use crate::surface::surf_of_extrusion::SurfOfExtrusion;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{edge, Brep, EdgeId, EdgeuseId, VertexId};

/// How closely the two rounds' points at one station must agree, as a fraction of the
/// part's size (1 + the largest coordinate at X + the mitre's length): the rounding of the
/// two evaluations and of the solve, as [`super::exact_mitre_curve`] holds its curves.
const AGREE: f64 = 1e-12;

/// The Newton steps a station's solve may take.
const STEPS: usize = 40;

/// The most intervals the mitre is cut into.
const MOST: usize = 256;

/// One station of the mitre: its point and its parameters on the two rounds.
#[derive(Clone, Copy)]
struct Station {
    point: Point3,
    uv: [Point2; 2],
}

/// The station where the two rounds meet at their parameters across `v`, from the guesses
/// `u` of their parameters along: Gauss-Newton on `S1(u1, v1) - S2(u2, v2)` in `u1, u2`.
/// `None` where the directions along are parallel, the solve does not settle or the two
/// points stand more than `agree` apart.
fn meet(
    k: &Kernel,
    surfaces: [&Surface; 2],
    v: [f64; 2],
    mut u: [f64; 2],
    agree: f64,
) -> Result<Option<Station>> {
    let flags = EvaluateFlags {
        u_side: Side::Left,
        v_side: Side::Left,
        only_upper_half: true,
        non_zero_tangents: false,
    };
    let [s1, s2] = surfaces;
    for _ in 0..STEPS {
        k.check_cancelled(OP)?;
        let d1 = s1.evaluate(k, Point2::new(u[0], v[0]), 1, 1, flags)?;
        let d2 = s2.evaluate(k, Point2::new(u[1], v[1]), 1, 1, flags)?;
        let at = |d: &[Vec<Point3>], i: usize| d.get(i).and_then(|row| row.first()).copied();
        let (Some(p1), Some(a), Some(p2), Some(b)) =
            (at(&d1, 0), at(&d1, 1), at(&d2, 0), at(&d2, 1))
        else {
            return Ok(None);
        };
        let r = p1 - p2;
        let (aa, ab, bb) = (a.dot(a), a.dot(b), b.dot(b));
        let det = aa * bb - ab * ab;
        if det <= EFF_ZERO * aa * bb {
            return Ok(None);
        }
        // the normal equations of r + a du1 - b du2
        let (ra, rb) = (r.dot(a), r.dot(b));
        let du1 = (ab * rb - bb * ra) / det;
        let du2 = (aa * rb - ab * ra) / det;
        u = [u[0] + du1, u[1] + du2];
        if du1.abs() + du2.abs() <= 4.0 * EFF_ZERO * (1.0 + u[0].abs() + u[1].abs()) {
            let uv = [Point2::new(u[0], v[0]), Point2::new(u[1], v[1])];
            let q1 = s1.evaluate_point(k, uv[0])?;
            let q2 = s2.evaluate_point(k, uv[1])?;
            if q1.distance(q2) > agree {
                return Ok(None);
            }
            return Ok(Some(Station {
                point: (q1 + q2) * 0.5,
                uv,
            }));
        }
    }
    Ok(None)
}

const OP: &str = "blend::blend_corner::section_mitre::section_mitre_curve";

/// A refusal of a mitre that is of this kind and is not proved.
const fn refused(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

const NO_ENDS: &str = "two rounds meeting at an angle on one flat face: the ends of their \
                       mitre are not on the sides of both rounds";
const NO_STATION: &str = "two rounds meeting at an angle on one flat face: they do not meet \
                          at one of the mitre's sections";
const NOT_PROVED: &str = "two rounds meeting at an angle on one flat face: their mitre is \
                          not proved within the tolerance of both rounds";

/// Two rounds whose mitre is built section by section, and the mitre's ends
/// ([`section_pair`]).
pub struct SectionPair {
    /// S, on the side edge between the rounds' other faces.
    s: VertexId,
    /// X, the rails' crossing.
    x: VertexId,
    /// Whether the edge starts at S.
    from_s: bool,
    /// The edge's two uses, on the first round and on the second.
    eus: [EdgeuseId; 2],
    /// The two rounds.
    geoms: [FilletGeomId; 2],
    /// Their surfaces.
    surfaces: [Surface; 2],
}

/// The plane a round of constant radius rolls on with a circular section, where its
/// other face stands upright on that plane: the plane's point and unit normal, and the
/// radius, for the plane of support `flat`. `None` for any other round.
fn upright_round(
    k: &Kernel,
    def: &BlendDefinition,
    flat: usize,
) -> Result<Option<(Point3, Vec3, f64)>> {
    let [r0, r1] = def.offsets;
    if def.law.is_some()
        || def.chamfer.is_some()
        || def.order != 1
        || def.swapped
        || r0.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater)
        || (r0 - r1).abs() > EFF_ZERO * r0
    {
        return Ok(None);
    }
    let (Some(floor), Some(wall)) = (def.supports.get(flat), def.supports.get(1 - flat)) else {
        return Ok(None);
    };
    let SurfaceKind::Plane(plane) = floor.kind() else {
        return Ok(None);
    };
    let (origin, normal, _) = plane.is_planar();
    let length = normal.length();
    if length.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        return Ok(None);
    }
    let normal = normal * (1.0 / length);
    // square to the plane: a plane whose normal lies in it, or an extrusion along its normal
    let upright = match wall.kind() {
        SurfaceKind::Plane(other) => {
            let (_, n, _) = other.is_planar();
            n.dot(normal).abs() <= AGREE * n.length()
        }
        SurfaceKind::SurfOfExtrusion(extrusion) => {
            let along = extrusion.extrusion_vector();
            along.cross(normal).length() <= AGREE * along.length()
        }
        SurfaceKind::BSpline(net) => {
            match SurfOfExtrusion::is_nurb_surface_surf_of_extrusion(k, net, 1.0)? {
                Some(extrusion) => {
                    let along = extrusion.extrusion_vector();
                    along.cross(normal).length() <= AGREE * along.length()
                }
                None => false,
            }
        }
        _ => false,
    };
    Ok(upright.then_some((origin, normal, r0)))
}

/// Whether the mitre `mitre` is one this module builds, read off the two rounds before
/// anything is solved (module docs): a Mitre turn's edge between the rails' crossing X and
/// the side vertex S, on two rounds kept as their definitions, of one constant radius with
/// circular sections, rolling on one plane, each one's other face square to that plane.
/// `None` for every other edge, which keeps its own construction.
///
/// # Errors
///
/// [`crate::error::SolidError::NotFound`] for a dangling id; those of reading the faces.
pub fn section_pair(k: &Kernel, pseudo: &FilletBrep, mitre: EdgeId) -> Result<Option<SectionPair>> {
    let Some(start) = edge::vertex(pseudo.brep(), mitre)? else {
        return Ok(None);
    };
    let Some(end) = edge::other_vertex(pseudo.brep(), mitre, Some(start))? else {
        return Ok(None);
    };
    let from_s = pseudo.fillet_vertex(start)?.vertex_type == FilletVertexType::RailXEdgeuse;
    let (s, x) = if from_s { (start, end) } else { (end, start) };
    {
        let sv = pseudo.fillet_vertex(s)?;
        let xv = pseudo.fillet_vertex(x)?;
        if sv.vertex_type != FilletVertexType::RailXEdgeuse
            || xv.vertex_type != FilletVertexType::RailXRail
            || !sv.is_processed()
            || !xv.is_processed()
        {
            return Ok(None);
        }
    }
    let Some(prim) = eu1::primary_edgeuse(pseudo.brep(), mitre)? else {
        return Ok(None);
    };
    let Some(mate) = eu1::mate(pseudo.brep(), prim)? else {
        return Ok(None);
    };
    let (Some(g0), Some(g1)) = (
        pseudo.fillet_edgeuse(prim)?.fillet_geom(),
        pseudo.fillet_edgeuse(mate)?.fillet_geom(),
    ) else {
        return Ok(None);
    };
    if g0 == g1 {
        return Ok(None);
    }
    let (Some(surface0), Some(surface1)) = (
        pseudo.geom(g0)?.fillet_surface(None)?,
        pseudo.geom(g1)?.fillet_surface(None)?,
    ) else {
        return Ok(None);
    };
    let (SurfaceKind::ProceduralBlend(blend0), SurfaceKind::ProceduralBlend(blend1)) =
        (surface0.kind(), surface1.kind())
    else {
        return Ok(None);
    };
    // one plane among the faces of both, each one's other face square to it
    let mut shared = false;
    for (flat0, flat1) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        let (Some((o0, n0, r0)), Some((o1, n1, r1))) = (
            upright_round(k, blend0.definition(), flat0)?,
            upright_round(k, blend1.definition(), flat1)?,
        ) else {
            continue;
        };
        let size = 1.0 + o0.max_dimension() + o1.max_dimension();
        shared = (r0 - r1).abs() <= EFF_ZERO * r0
            && n0.cross(n1).length() <= AGREE
            && (o1 - o0).dot(n0).abs() <= AGREE * size;
        if shared {
            break;
        }
    }
    if !shared {
        return Ok(None);
    }
    Ok(Some(SectionPair {
        s,
        x,
        from_s,
        eus: [prim, mate],
        geoms: [g0, g1],
        surfaces: [surface0.clone(), surface1.clone()],
    }))
}

/// The mitre `mitre` of `corner` between the two rounds of `pair` ([`section_pair`]), built
/// section by section and proved (module docs): the edge's curve through the stations, its
/// trims on both rounds, and X and S at the end stations.
///
/// # Errors
///
/// `Unsupported`, naming what failed, where the mitre's ends are not on the rounds' sides,
/// the rounds do not meet at a section or the curve is not proved within the corner's
/// tolerance of both rounds; [`crate::error::SolidError::NotFound`] for a dangling id;
/// those of the evaluations, the drops, the curve fits and the enclosure; cancellation.
#[allow(clippy::too_many_lines)] // one construction
pub fn section_mitre_curve(
    k: &Kernel,
    target: &Brep,
    pseudo: &mut FilletBrep,
    corner: &FilletCorner,
    mitre: EdgeId,
    pair: SectionPair,
) -> Result<()> {
    let SectionPair {
        s,
        x,
        from_s,
        eus,
        geoms,
        surfaces: [surface0, surface1],
    } = pair;
    // rounds kept as their definitions: each evaluates as itself here, not as its net
    let _scope = NetScope::exact(k);
    let surfaces = [&surface0, &surface1];
    let tol = corner.approx_tol_3d;
    let x_pnt = pseudo.vertex_point(x)?;
    let s_pnt = pseudo.vertex_point(s)?;
    let agree = AGREE * (1.0 + x_pnt.max_dimension() + x_pnt.distance(s_pnt));

    // X and S on each round: on its sides across, one each
    let mut at_x = [Point2::default(); 2];
    let mut at_s = [Point2::default(); 2];
    for (ii, surface) in surfaces.into_iter().enumerate() {
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        let (lo, hi) = (domain.min().y, domain.max().y);
        let side = |v: f64| {
            let reach = 1e-9 * (hi - lo);
            if (v - lo).abs() <= reach {
                Some(lo)
            } else if (v - hi).abs() <= reach {
                Some(hi)
            } else {
                None
            }
        };
        let mut found = [Point2::default(); 2];
        for (p, slot) in [x_pnt, s_pnt].into_iter().zip(&mut found) {
            let dropped =
                surface.drop_point::<NoObject>(k, p, &domain, &DropPointOptions::default())?;
            if !dropped.success || dropped.gap > tol {
                return Err(refused(NO_ENDS));
            }
            let Some(uv) = foot_on_surface(k, surface, p, dropped.result_uv)? else {
                return Err(refused(NO_ENDS));
            };
            let Some(v) = side(uv.y) else {
                return Err(refused(NO_ENDS));
            };
            *slot = Point2::new(uv.x, v);
        }
        let [x_uv, s_uv] = found;
        if x_uv.y == s_uv.y {
            return Err(refused(NO_ENDS));
        }
        if let (Some(x_slot), Some(s_slot)) = (at_x.get_mut(ii), at_s.get_mut(ii)) {
            *x_slot = x_uv;
            *s_slot = s_uv;
        }
    }
    let [x0, x1] = at_x;
    let [s0, s1] = at_s;
    // the station at t: 0 at X, 1 at S, the rounds' parameters across running together
    let station = |t: f64, guess: [f64; 2]| {
        let v = [x0.y + t * (s0.y - x0.y), x1.y + t * (s1.y - x1.y)];
        meet(k, surfaces, v, guess, agree)
    };
    let between = |t: f64, a: &Station, b: &Station, ta: f64, tb: f64| {
        let w = (t - ta) / (tb - ta);
        [
            a.uv[0].x + w * (b.uv[0].x - a.uv[0].x),
            a.uv[1].x + w * (b.uv[1].x - a.uv[1].x),
        ]
    };
    let (Some(first), Some(last)) = (station(0.0, [x0.x, x1.x])?, station(1.0, [s0.x, s1.x])?)
    else {
        return Err(refused(NO_STATION));
    };
    if first.point.distance(x_pnt) > tol || last.point.distance(s_pnt) > tol {
        return Err(refused(NO_ENDS));
    }

    // the stations, doubled until the curves through them are proved on both rounds
    let mut count = 8;
    let (curve, uvs) = loop {
        let mut stations = vec![first];
        for ii in 1..count {
            let t = ii as f64 / count as f64;
            let prev = stations.last().copied().unwrap_or(first);
            let guess = between(t, &prev, &last, (ii - 1) as f64 / count as f64, 1.0);
            let Some(found) = station(t, guess)? else {
                return Err(refused(NO_STATION));
            };
            stations.push(found);
        }
        stations.push(last);
        // along the edge: from its start
        let ordered: Vec<(f64, Station)> = stations
            .iter()
            .enumerate()
            .map(|(ii, st)| (ii as f64 / count as f64, *st))
            .collect();
        let ordered: Vec<(f64, Station)> = if from_s {
            ordered
                .into_iter()
                .rev()
                .map(|(t, st)| (1.0 - t, st))
                .collect()
        } else {
            ordered
        };
        let through = |pick: &dyn Fn(&Station) -> Point3, dim: u64| -> Result<BSplineCurve> {
            let (params, points): (Vec<f64>, Vec<Point3>) =
                ordered.iter().map(|(t, st)| (*t, pick(st))).unzip();
            BSplineCurve::create_interpolating_curve_with_params(
                ParamMethod::ChordLength,
                dim,
                3,
                &points,
                &[],
                None,
                false,
                Some(&params),
            )
        };
        let curve = Curve::from(through(&|st| st.point, 3)?);
        let mut trims = Vec::with_capacity(2);
        for (ii, surface) in surfaces.into_iter().enumerate() {
            let mut trim = through(
                &|st| {
                    let uv = st.uv.get(ii).copied().unwrap_or_default();
                    Point3::new(uv.x, uv.y, 0.0)
                },
                2,
            )?;
            held_in_domain(&mut trim, &surface.natural_uv_domain_with_nurb(k)?)?;
            let trim = Curve::from(trim);
            let ivl = curve.natural_interval();
            if proved_trim_gap(k, surface, &trim, &curve, ivl, tol)?.is_none() {
                break;
            }
            trims.push(trim);
        }
        if let Ok(both) = <[Curve; 2]>::try_from(trims) {
            break (curve, both);
        }
        if count >= MOST {
            return Err(refused(NOT_PROVED));
        }
        count *= 2;
    };

    // S's place on the side edge, as the exact mitre notes it
    if let Some(EntityRef::Edgeuse(side_eu)) =
        pseudo.fillet_vertex(s)?.point_classification().object()
    {
        if let Some(side_edge) = eu1::edge(target, side_eu)? {
            if let Some(side_curve) = edge::curve(target, side_edge)? {
                let ivl = edge::interval(target, side_edge)?;
                let dropped = side_curve.drop_point(
                    k,
                    &ivl,
                    last.point,
                    None,
                    tol,
                    None,
                    SolverOperation::Minimize,
                )?;
                if dropped.success {
                    pseudo
                        .fillet_vertex_mut(s)?
                        .set_original_t_param(dropped.parameter);
                }
            }
        }
    }
    set_edge_curve(pseudo.brep_mut(), mitre, Some(curve))?;
    pseudo.set_vertex_point(x, first.point)?;
    pseudo.set_vertex_point(s, last.point)?;
    let [uv0, uv1] = uvs;
    for ((eu, g), uv) in eus.into_iter().zip(geoms).zip([uv0, uv1]) {
        pseudo.set_edgeuse_uv_curve(eu, Some(uv), true, true)?;
        pseudo.geom_mut(g)?.add_side_fillet_edgeuse(eu);
    }
    pseudo
        .fillet_edge_mut(mitre)?
        .set_fillet_edge_type(FilletEdgeType::Precomputed);
    Ok(())
}
