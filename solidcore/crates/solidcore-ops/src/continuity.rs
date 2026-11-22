//! The edge-continuity check and curvature combs: which stations along which edges, the
//! tolerances the kernel's measures are compared with, and the verdict per seam.
//!
//! The recipe only: every number is the kernel's ([`solidcore::query::continuity`]), on record
//! order, compared with the caller's tolerances; ops chooses none. A seam's
//! [`SeamReport::order_at_stations`] holds at the stations measured, not between them: it is the
//! highest order whose test, and every lower one's, its worst station passes. The one verdict
//! that holds along the whole edge is [`SeamReport::g1_all_along`], the kernel's proved
//! tangency test (`solidcore::query::tangent_edge::tangent_all_along`) at the caller's angle.
//!
//! Separate sheets ([`sheet_seams`]): where two sheets' boundary edges run together within the
//! caller's zone is the kernel's ([`solidcore::curve::proximity::Overlap`], the run proved
//! both ways), and each seam is measured by the same kernel measure over that run
//! ([`solidcore::query::continuity::continuity_over`]), each face in its own body. Ops only
//! picks the pairs of boundary edges, and the side of the second sheet that agrees with the
//! first across the seam (the side sewing would orient it to: the two edge uses running
//! opposite ways along the run, in the sense the kernel's run reports).

use solidcore::curve::proximity::Overlap;
use solidcore::geom::object::Shape;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::api_queries::face_closest_point_in_shape;
use solidcore::query::brep::{BrepQuery, LiveBrepObjects};
use solidcore::query::continuity::{
    continuity_along, continuity_over, edge_comb, iso_comb, ContinuityStation, CurvatureAt,
};
use solidcore::query::tangent_edge::{tangent_all_along, Tangency};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2 as face_p2;
use solidcore::topology::{edge, Brep, BrepId, EdgeId, FaceId, FaceuseId};
use solidcore::Vec3;

use crate::body::Body;
use crate::measure::outer_faceuse;
use crate::order::{self, Order};
use crate::session::{answer, run_on, with_body};
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
    let measured = continuity_along(k, b, e, [a, c], stations)?;
    Ok(Ok(report(record_edge, measured, tolerances, g1_all_along)))
}

/// The report of a seam from the kernel's stations along it, judged against `tolerances`;
/// `None` where no station measured.
fn report(
    record_edge: usize,
    measured: Vec<ContinuityStation>,
    tolerances: [f64; 5],
    g1_all_along: G1AllAlong,
) -> Option<SeamReport> {
    let out: Vec<SeamStation> = measured
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
        return None;
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
    Some(SeamReport {
        edge: record_edge,
        order_at_stations: highest_met(worst_values, tolerances),
        g1_all_along,
        worst,
        stations: out,
    })
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

/// Two sheets' boundary edges that meet ([`SheetSeams`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetMeet {
    /// The two sheets, by their index in the list given, the first the lower.
    pub sheets: [usize; 2],
    /// Each sheet's edge, by its index in that sheet's record.
    pub edges: [usize; 2],
}

/// The seam where two separate sheets meet ([`sheet_seams`]).
#[derive(Clone, Debug, PartialEq)]
pub struct SheetSeam {
    /// The two sheets, by their index in the list given, the first the lower.
    pub sheets: [usize; 2],
    /// Each sheet's edge along the seam, by its index in that sheet's record.
    pub edges: [usize; 2],
    /// The part of each edge the seam runs over: `[low, high]` fractions of its curve's
    /// parameter over the edge (0 at its start, 1 at its end), the first sheet's, then the
    /// second's.
    pub along: [[f64; 2]; 2],
    /// The seam measured along the first sheet's edge over its part (its
    /// [`SeamReport::edge`] is `edges[0]`, its stations' fractions of that edge), between the
    /// first sheet's upward face use and the second sheet's use that agrees with it across
    /// the seam. [`SeamReport::g1_all_along`] is [`G1AllAlong::NotDecided`]: the kernel's
    /// proof runs on one edge between two face uses.
    pub report: SeamReport,
}

/// Where separate sheets meet, and each seam's continuity ([`sheet_seams`]).
#[derive(Clone, Debug, PartialEq)]
pub struct SheetSeams {
    /// The seams, in order of the sheets, then of the first sheet's edges, then the second's.
    pub seams: Vec<SheetSeam>,
    /// Pairs of edges whose ends meet within the zone but whose run the kernel does not prove
    /// within it: they may part between those ends.
    pub unproved: Vec<SheetMeet>,
    /// Seams of which no station measured.
    pub unmeasured: Vec<SheetMeet>,
}

/// Why a seam between separate sheets has no proved verdict on G1 all along it.
const G1_ONE_EDGE: &str =
    "the proof of G1 all along runs on one edge between two face uses; separate sheets meet along two edges";

/// One sheet's boundary edges: the record index, the kernel edge and the upward face use of
/// its one face.
fn boundary(b: &Brep, order: &Order) -> solidcore::Result<Result<Vec<(usize, EdgeId, FaceuseId)>>> {
    let mut out = Vec::new();
    for (i, &e) in order.edges.iter().enumerate() {
        if !eu1::edge_is_lamina(b, e)? {
            continue;
        }
        let [f] = edge::faces(b, e, None)?[..] else {
            continue;
        };
        match outer_faceuse(b, f)? {
            Ok(fu) => out.push((i, e, fu)),
            Err(err) => return Ok(Err(err)),
        }
    }
    Ok(Ok(out))
}

/// Whether edge `e`'s use in face use `fu` runs along its curve's parameter; `None` for a use
/// of unknown orientation or no use of `e` in `fu`.
fn runs_forward(b: &Brep, e: EdgeId, fu: FaceuseId) -> solidcore::Result<Option<bool>> {
    for u in edge::edgeuses(b, e, None, None)? {
        if eu1::faceuse(b, u)? == Some(fu) {
            return Ok(match eu1::orientation(b, u)? {
                OrientType::Same => Some(true),
                OrientType::Opposite => Some(false),
                OrientType::Unknown => None,
            });
        }
    }
    Ok(None)
}

/// The continuity of the seams where separate `sheets` meet (each with its [`Order`]): every
/// pair of boundary edges (edges one face uses once) of two different sheets whose curves the
/// kernel proves run together within `zone_mm` over a run of positive length
/// ([`solidcore::curve::proximity::Overlap`]) is a seam over that run, measured as
/// [`Body::continuity_report`] measures a seam inside one body: `stations` stations at even
/// steps of the first sheet's edge over the run, the ends included, by the kernel
/// ([`solidcore::query::continuity::continuity_over`]), judged against `tolerances`. The
/// second sheet's face is measured on the side that agrees with the first's upward side
/// across the seam, as sewing the two would orient it. The sheets are unchanged.
///
/// # Errors
/// [`Error::Argument`] for fewer than two sheets, a sheet given twice, a zone not finite and
/// positive, a station count outside [`MIN_STATIONS`]..=[`MAX_STATIONS`] or a tolerance that is
/// negative or not finite; [`Error::Unsupported`] for a face with material on both sides;
/// [`Error::Internal`] for a sheet an earlier panic left unfinished; the kernel's error.
pub fn sheet_seams(
    sheets: &[(&Body, &Order)],
    zone_mm: f64,
    stations: usize,
    tolerances: ContinuityTolerances,
) -> Result<SheetSeams> {
    let tol = tolerances.by_order();
    let twice = sheets
        .iter()
        .enumerate()
        .any(|(i, (a, _))| sheets.iter().skip(i + 1).any(|(b, _)| std::ptr::eq(*a, *b)));
    if sheets.len() < 2
        || twice
        || !(zone_mm.is_finite() && zone_mm > 0.0)
        || !(MIN_STATIONS..=MAX_STATIONS).contains(&stations)
        || !tol.iter().all(|t| t.is_finite() && *t >= 0.0)
    {
        return Err(Error::Argument);
    }
    if sheets.iter().any(|(b, _)| b.is_poisoned()) {
        return Err(Error::Internal(
            "continuity::sheet_seams: a sheet was left unfinished by an earlier panic".into(),
        ));
    }
    let inputs: Vec<(&'static str, &Body)> = sheets.iter().map(|(b, _)| ("sheet", *b)).collect();
    run_on("continuity::sheet_seams", &inputs, |k| {
        // each sheet once (refused above when given twice), read only, under the kernel's lock
        let guards: Vec<_> = sheets.iter().map(|(b, _)| b.lock()).collect();
        let mut edges = Vec::with_capacity(sheets.len());
        for (g, (_, order)) in guards.iter().zip(sheets) {
            match boundary(g, order)? {
                Ok(e) => edges.push(e),
                Err(err) => return Ok(Err(err)),
            }
        }
        let mut out = SheetSeams {
            seams: Vec::new(),
            unproved: Vec::new(),
            unmeasured: Vec::new(),
        };
        for (i, (ba, ea)) in guards.iter().zip(&edges).enumerate() {
            for (j, (bb, eb)) in guards.iter().zip(&edges).enumerate().skip(i + 1) {
                for &(ia, a, fa) in ea {
                    for &(ib, b, fb) in eb {
                        k.check_cancelled("continuity::sheet_seams")?;
                        let meet = SheetMeet {
                            sheets: [i, j],
                            edges: [ia, ib],
                        };
                        let (Some(ca), Some(cb)) = (edge::curve(ba, a)?, edge::curve(bb, b)?)
                        else {
                            continue;
                        };
                        let (va, vb) = (edge::interval(ba, a)?, edge::interval(bb, b)?);
                        let run = match ca.proved_overlap(k, &va, cb, &vb, zone_mm)? {
                            Overlap::Apart => continue,
                            Overlap::Unproved(_) => {
                                out.unproved.push(meet);
                                continue;
                            }
                            Overlap::Proved(run) => run,
                        };
                        let (Some(da), Some(db)) =
                            (runs_forward(ba, a, fa)?, runs_forward(bb, b, fb)?)
                        else {
                            out.unmeasured.push(meet);
                            continue;
                        };
                        // agreeing sides run the seam opposite ways; else the second sheet's
                        // other side agrees
                        let agree = (da == db) != run.same_sense;
                        let fb = if agree {
                            fb
                        } else {
                            let (Some(x), Some(y)) = face_p2::faceuses(bb, b_face(bb, fb)?)? else {
                                out.unmeasured.push(meet);
                                continue;
                            };
                            if x == fb {
                                y
                            } else {
                                x
                            }
                        };
                        let measured = continuity_over(
                            k,
                            ca,
                            va,
                            run.fractions[0],
                            [(&**ba, fa), (&**bb, fb)],
                            stations,
                        )?;
                        let g1 = G1AllAlong::NotDecided(G1_ONE_EDGE.into());
                        match report(ia, measured, tol, g1) {
                            Some(report) => out.seams.push(SheetSeam {
                                sheets: [i, j],
                                edges: [ia, ib],
                                along: run.fractions,
                                report,
                            }),
                            None => out.unmeasured.push(meet),
                        }
                    }
                }
            }
        }
        Ok(Ok(out))
    })?
}

/// The face of face use `fu`.
fn b_face(b: &Brep, fu: FaceuseId) -> solidcore::Result<FaceId> {
    b.faceuse(fu)?.face.ok_or_else(|| {
        solidcore::SolidError::invalid_input("continuity::sheet_seams", "a face use of no face")
    })
}

/// The most points [`principal_curvatures`] takes.
pub const MAX_CURVATURE_POINTS: usize = 1 << 20;

/// A sheet's principal curvatures at a point ([`principal_curvatures`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Principal {
    /// The larger principal curvature, 1/mm, convex positive against the face's normal (a
    /// sphere's outside reads `1 / r`).
    pub k1: f64,
    /// The smaller.
    pub k2: f64,
    /// Gaussian curvature, 1/mm²: `k1 * k2`, independent of the face's orientation.
    pub gaussian: f64,
    /// Mean curvature, 1/mm: `(k1 + k2) / 2`, convex positive against the face's normal.
    pub mean: f64,
    /// The unit direction of `k1`.
    pub d1: [f64; 3],
    /// The unit direction of `k2`.
    pub d2: [f64; 3],
}

/// The principal curvatures of `sheet` at each of `points`: the nearest point within each
/// face's boundaries is found by the kernel, and the nearest face answers with the kernel's
/// [`solidcore::query::continuity::principal_curvatures`] at that point (`k1 >= k2`, convex positive
/// against the face's normal in its shell, with their directions, Gaussian and mean curvature).
/// `None` if no face has a nearest point, or that point is singular, such as a pole.
///
/// # Errors
/// [`Error::Argument`] for more than [`MAX_CURVATURE_POINTS`] points or a coordinate not finite
/// or past the bound; those of [`Body::from_record`]; the kernel's status.
pub fn principal_curvatures(
    sheet: &solidcore_record::brep::Brep,
    points: &[[f64; 3]],
) -> Result<Vec<Option<Principal>>> {
    if points.len() > MAX_CURVATURE_POINTS || !points.iter().all(|p| crate::sweep::coord_ok(p)) {
        return Err(Error::Argument);
    }
    let (body, _) = Body::from_record(sheet, true)?;
    if points.is_empty() {
        return Ok(Vec::new());
    }
    with_body("continuity::principal_curvatures", &body, |k, b| {
        let faces = b.face_list().to_vec();
        let id = BrepId::from_index(0);
        let mut nearest: Vec<Option<(f64, FaceId, Vec3)>> = vec![None; points.len()];
        for f in faces {
            let Some(shape) = answer(Shape::new(k, &(id, &*b), &[], &[(id, f)], &[], &[]))? else {
                continue;
            };
            let query = BrepQuery::of_brep(b);
            let identity = query.effective_identity();
            let objects = LiveBrepObjects::new(b, &identity, None)?;
            for (p, best) in points.iter().zip(&mut nearest) {
                let p = Vec3::new(p[0], p[1], p[2]);
                let Some(closest) =
                    answer(face_closest_point_in_shape(k, &shape, &objects, (id, f), p))?
                else {
                    continue;
                };
                if best.is_none_or(|(distance, _, _)| closest.distance < distance) {
                    *best = Some((closest.distance, f, closest.point));
                }
            }
        }
        let mut out = Vec::with_capacity(points.len());
        for closest in nearest {
            let Some((_, f, p)) = closest else {
                out.push(None);
                continue;
            };
            let principal = solidcore::query::continuity::principal_curvatures(k, b, f, p)?;
            out.push(principal.map(|pr| Principal {
                k1: pr.k1,
                k2: pr.k2,
                gaussian: pr.gaussian,
                mean: pr.mean,
                d1: array(pr.d1),
                d2: array(pr.d2),
            }));
        }
        Ok(Ok(out))
    })?
}
