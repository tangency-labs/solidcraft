//! The edge-continuity check and curvature combs: which stations along which edges, the
//! tolerances the kernel's measures are compared with, and the verdict per seam.
//!
//! The recipe only: every number is the kernel's ([`solidcore::query::continuity`]), on record
//! order, compared with the caller's tolerances; ops chooses none. A seam's
//! [`SeamReport::order_at_stations`] holds at the stations measured, not between them: it is the
//! highest order whose test, and every lower one's, its worst station passes. The one verdict
//! that holds along the whole edge is [`SeamReport::g1_all_along`], the kernel's proved
//! tangency test (`solidcore::query::tangent_edge::tangent_all_along`) at the caller's angle.

use solidcore::query::continuity::{continuity_along, edge_comb, iso_comb, CurvatureAt};
use solidcore::query::tangent_edge::{tangent_all_along, Tangency};
use solidcore::topology::{edge, Brep, EdgeId, FaceId};
use solidcore::Vec3;

use crate::body::Body;
use crate::measure::outer_faceuse;
use crate::order::{self, Order};
use crate::session::with_body;
use crate::{Error, Result};

/// Fewest and most stations along one edge: three are its two ends and its middle.
pub const MIN_STATIONS: usize = 3;
/// The most stations along one edge.
pub const MAX_STATIONS: usize = 257;
/// Fewest teeth of a comb.
pub const MIN_COMB_TEETH: usize = 2;
/// Most teeth of a comb.
pub const MAX_COMB_TEETH: usize = 2000;

/// What counts as continuous across a seam: the caller's (ops has no defaults of its own).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContinuityTolerances {
    /// Largest gap between the faces that still counts as coincident (G0), mm.
    pub gap_mm: f64,
    /// Largest angle between the faces' normals that still counts as tangent (G1), radians.
    pub angle_rad: f64,
    /// Largest curvature jump that still counts as curvature continuous (G2), 1/mm.
    pub curvature_per_mm: f64,
    /// Largest jump of the curvature's change across the seam that still counts as G3, 1/mm².
    pub curvature_change_per_mm2: f64,
    /// Largest jump of that change's rate that still counts as G4, 1/mm³.
    pub curvature_change_rate_per_mm3: f64,
}

impl ContinuityTolerances {
    /// The tolerances in order G0 to G4.
    fn by_order(&self) -> [f64; 5] {
        [
            self.gap_mm,
            self.angle_rad,
            self.curvature_per_mm,
            self.curvature_change_per_mm2,
            self.curvature_change_rate_per_mm3,
        ]
    }
}

/// One station of a seam: how far along the edge, where, and the five measures there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeamStation {
    /// 0 at the edge's start, 1 at its end (of its curve's parameter).
    pub edge_fraction: f64,
    /// The edge curve's point.
    pub point: [f64; 3],
    /// G0: the gap between the faces, mm.
    pub gap_mm: f64,
    /// G1: the angle between their outward normals, radians.
    pub normal_angle_rad: f64,
    /// The same angle in degrees, for display.
    pub normal_angle_deg: f64,
    /// G2: the curvature jump across the edge, 1/mm.
    pub curvature_jump_per_mm: f64,
    /// G3: the jump of the curvature's arc-length derivative, 1/mm².
    pub curvature_change_jump_per_mm2: f64,
    /// G4: the jump of its second derivative, 1/mm³.
    pub curvature_change_rate_jump_per_mm3: f64,
}

impl SeamStation {
    /// The five measures in order G0 to G4.
    fn by_order(&self) -> [f64; 5] {
        [
            self.gap_mm,
            self.normal_angle_rad,
            self.curvature_jump_per_mm,
            self.curvature_change_jump_per_mm2,
            self.curvature_change_rate_jump_per_mm3,
        ]
    }
}

/// One edge's report.
#[derive(Clone, Debug, PartialEq)]
pub struct SeamReport {
    /// The edge, by its index in the record.
    pub edge: usize,
    /// The stations that measured, in order along the edge.
    pub stations: Vec<SeamStation>,
    /// For each order G0 to G4, the index in `stations` of the station with the largest value.
    pub worst: [usize; 5],
    /// The highest order (0 to 4) whose measure, and every lower one's, is within its
    /// tolerance at every station measured; `None`: not even G0 there. It says nothing of the
    /// edge between the stations.
    pub order_at_stations: Option<usize>,
    /// Whether the faces meet within the caller's angle all along the edge, proved by the
    /// kernel ([`G1AllAlong`]).
    pub g1_all_along: G1AllAlong,
}

/// The kernel's proved answer to whether a seam is G1 all along it, at the caller's angle.
#[derive(Clone, Debug, PartialEq)]
pub enum G1AllAlong {
    /// The faces' normals are within the angle everywhere along the edge.
    Tangent,
    /// They part by more than the angle somewhere along it.
    NotTangent,
    /// A piece of the edge the kernel's bound cannot place on either side of the angle.
    Undecided,
    /// The kernel does not decide this edge: why.
    NotDecided(String),
}

/// The continuity report of a body.
#[derive(Clone, Debug, PartialEq)]
pub struct ContinuityReport {
    /// The seams measured, in record order.
    pub seams: Vec<SeamReport>,
    /// Edges asked for (or, for all, two-sided edges) of which no station measured.
    pub unmeasured: Vec<usize>,
}

/// A tooth of a curvature comb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CombTooth {
    /// Where the tooth stands.
    pub at: [f64; 3],
    /// The curvature vector there, 1/mm: towards the centre of curvature, its length the
    /// curvature (along an iso line: the normal curvature, along the surface normal).
    pub curvature: [f64; 3],
}

fn array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// The kernel's tooth as ops gives it.
pub(crate) fn tooth(c: CurvatureAt) -> CombTooth {
    CombTooth {
        at: array(c.point),
        curvature: array(c.curvature),
    }
}

/// The two faces edge `e` bounds, each once; `None` for an edge that is not a seam between
/// two faces (an open boundary, or a periodic face's own seam).
fn two_faces(b: &Brep, e: EdgeId) -> solidcore::Result<Option<[FaceId; 2]>> {
    let mut fs = edge::faces(b, e, None)?;
    fs.sort();
    fs.dedup();
    Ok(match fs.as_slice() {
        &[x, y] => Some([x, y]),
        _ => None,
    })
}

/// The highest order whose value, and every lower one's, is within its tolerance.
fn highest_met(worst: [f64; 5], tolerances: [f64; 5]) -> Option<usize> {
    let met = worst
        .iter()
        .zip(tolerances)
        .take_while(|(w, t)| **w <= *t)
        .count();
    met.checked_sub(1)
}

/// One seam: `stations` measures along edge `e` between the outward uses of its faces.
fn seam(
    k: &solidcore::Kernel,
    b: &Brep,
    (record_edge, e, faces): (usize, EdgeId, [FaceId; 2]),
    stations: usize,
    tolerances: [f64; 5],
    g1_all_along: G1AllAlong,
) -> solidcore::Result<Result<Option<SeamReport>>> {
    let mut uses = Vec::with_capacity(2);
    for f in faces {
        match outer_faceuse(b, f)? {
            Ok(fu) => uses.push(fu),
            Err(e) => return Ok(Err(e)),
        }
    }
    let (Some(&a), Some(&c)) = (uses.first(), uses.get(1)) else {
        return Ok(Err(Error::Exception));
    };
    let out: Vec<SeamStation> = continuity_along(k, b, e, [a, c], stations)?
        .into_iter()
        .map(|st| {
            let [jump, change, rate] = st.measure.jumps;
            SeamStation {
                edge_fraction: st.edge_fraction,
                point: array(st.measure.point),
                gap_mm: st.measure.gap,
                normal_angle_rad: st.measure.normal_angle,
                normal_angle_deg: st.normal_angle_deg,
                curvature_jump_per_mm: jump,
                curvature_change_jump_per_mm2: change,
                curvature_change_rate_jump_per_mm3: rate,
            }
        })
        .collect();
    if out.is_empty() {
        return Ok(Ok(None));
    }
    let mut worst = [0_usize; 5];
    let mut worst_values = [f64::INFINITY; 5];
    for (o, (w, v)) in worst.iter_mut().zip(worst_values.iter_mut()).enumerate() {
        let mut best: Option<(usize, f64)> = None;
        for (i, s) in out.iter().enumerate() {
            let x = s.by_order().get(o).copied().unwrap_or(f64::NAN);
            if best.is_none_or(|(_, b)| x > b) {
                best = Some((i, x));
            }
        }
        if let Some((i, x)) = best {
            *w = i;
            *v = x;
        }
    }
    Ok(Ok(Some(SeamReport {
        edge: record_edge,
        order_at_stations: highest_met(worst_values, tolerances),
        g1_all_along,
        worst,
        stations: out,
    })))
}

impl Body {
    /// The edge-continuity check: `stations` stations at even steps of each edge curve's
    /// parameter, the ends included, on the record edges `edges` (empty: every edge that is a
    /// seam between two faces), each measured by the kernel (gap, normal angle, and the jumps
    /// of curvature, its change and that change's rate) between the faces' outward uses, and
    /// judged against the caller's `tolerances` at those stations only; with each seam, the
    /// kernel's proved verdict on G1 all along it at `tolerances.angle_rad`.
    ///
    /// # Errors
    /// [`Error::Argument`] for a station count outside [`MIN_STATIONS`]..=[`MAX_STATIONS`], a
    /// tolerance that is negative or not finite, an index past the edges, or an edge asked for
    /// that is not a seam between two faces; [`Error::Unsupported`] for a face with material on
    /// both sides; the kernel's error.
    pub fn continuity_report(
        &self,
        order: &Order,
        edges: &[usize],
        stations: usize,
        tolerances: ContinuityTolerances,
    ) -> Result<ContinuityReport> {
        let tol = tolerances.by_order();
        if !(MIN_STATIONS..=MAX_STATIONS).contains(&stations)
            || !tol.iter().all(|t| t.is_finite() && *t >= 0.0)
        {
            return Err(Error::Argument);
        }
        let all = edges.is_empty();
        let mut picked: Vec<(usize, EdgeId)> = Vec::new();
        if all {
            for (i, &e) in order.edges.iter().enumerate() {
                picked.push((i, e));
            }
        } else {
            let mut sorted = edges.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            for i in sorted {
                picked.push((i, order::edge(order, i)?));
            }
        }
        with_body("continuity_report", self, |k, b| {
            let mut seams = Vec::new();
            let mut unmeasured = Vec::new();
            for &(i, e) in &picked {
                let Some(faces) = two_faces(b, e)? else {
                    if all {
                        continue;
                    }
                    return Ok(Err(Error::Argument));
                };
                let g1 = match tangent_all_along(k, b, e, tolerances.angle_rad) {
                    Ok(Some(Tangency::Tangent)) => G1AllAlong::Tangent,
                    Ok(Some(Tangency::NotTangent)) => G1AllAlong::NotTangent,
                    Ok(Some(Tangency::Undecided { .. })) => G1AllAlong::Undecided,
                    Ok(None) => G1AllAlong::NotDecided("not an edge between two face uses".into()),
                    Err(
                        err @ (solidcore::SolidError::Unsupported { .. }
                        | solidcore::SolidError::InvalidInput { .. }),
                    ) => G1AllAlong::NotDecided(err.to_string()),
                    Err(err) => return Err(err),
                };
                match seam(k, b, (i, e, faces), stations, tol, g1)? {
                    Ok(Some(s)) => seams.push(s),
                    Ok(None) => unmeasured.push(i),
                    Err(err) => return Ok(Err(err)),
                }
            }
            Ok(Ok(ContinuityReport { seams, unmeasured }))
        })?
    }

    /// The curvature comb of record edge `edge`: `teeth` teeth at even steps of its curve's
    /// parameter over the edge, the ends included (a closed edge's end is its start, once),
    /// each the kernel's curvature vector there. Stations where the curve has no tangent are
    /// left out.
    ///
    /// # Errors
    /// [`Error::Argument`] for a tooth count outside [`MIN_COMB_TEETH`]..=[`MAX_COMB_TEETH`] or
    /// an index past the edges; the kernel's error.
    pub fn edge_comb(&self, order: &Order, edge: usize, teeth: usize) -> Result<Vec<CombTooth>> {
        if !(MIN_COMB_TEETH..=MAX_COMB_TEETH).contains(&teeth) {
            return Err(Error::Argument);
        }
        let e = order::edge(order, edge)?;
        with_body("edge_comb", self, |k, b| {
            Ok(Ok(edge_comb(k, b, e, teeth)?
                .into_iter()
                .map(tooth)
                .collect()))
        })?
    }

    /// The curvature comb along an iso line of record face `face`: the line of its surface's
    /// `u` parameter (`along_u`) at `v = fixed`, or of `v` at `u = fixed`, over `span` of the
    /// running parameter; `teeth` teeth at even steps of it, the ends included, each the
    /// kernel's normal curvature along the line times the surface normal. Stations at a pole
    /// are left out.
    ///
    /// # Errors
    /// [`Error::Argument`] for a tooth count outside [`MIN_COMB_TEETH`]..=[`MAX_COMB_TEETH`], an
    /// index past the faces, or a parameter that is not finite; the kernel's error.
    pub fn iso_comb(
        &self,
        order: &Order,
        face: usize,
        along_u: bool,
        fixed: f64,
        span: [f64; 2],
        teeth: usize,
    ) -> Result<Vec<CombTooth>> {
        if !(MIN_COMB_TEETH..=MAX_COMB_TEETH).contains(&teeth) {
            return Err(Error::Argument);
        }
        let f = order::face(order, face)?;
        with_body("iso_comb", self, |k, b| {
            match iso_comb(k, b, f, along_u, fixed, span, teeth) {
                Ok(teeth) => Ok(Ok(teeth.into_iter().map(tooth).collect())),
                Err(solidcore::SolidError::InvalidInput { .. }) => Ok(Err(Error::Argument)),
                Err(e) => Err(e),
            }
        })?
    }
}
