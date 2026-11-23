//! Pcurves of a record's face ([`solidcore::eval::pcurve`]): each edge use's curve in its face's
//! surface parameters, from the record's 3D edges and surface. This checks the records' structure
//! as the record's validation does (counts against a [`Budget`], a use's interval in its curve's
//! natural range, a given pcurve as a valid 2D B-spline), converts them through
//! `solidcore-convert`, calls the kernel, and converts its pcurves back. The pcurve of a
//! B-spline edge folded across a cylinder or cone ([`folded_pcurve`]) is converted the same way.
//!
//! The request types a kernel that makes the remaining pcurves takes ([`TrimFace`], [`Trims`])
//! are here too: they hold records.

use solidcore::eval::pcurve as k;
use solidcore_record::brep::{
    check_curve, check_curve2, check_interval, check_surface, Budget, Curve, Curve2, Surface,
};

pub use solidcore::eval::pcurve::{Error, SURFACE_GAP_MARGIN};

type Result<T> = std::result::Result<T, Error>;

/// Why completion of an imported record's periodic seams failed.
#[derive(Debug)]
pub enum PeriodicSeamError {
    /// A surface or pcurve cannot be evaluated.
    Geometry(Error),
    /// Conversion of the record or the completed body failed.
    Conversion(solidcore_convert::brep::Unsupported),
    /// The kernel's topology edit failed, with its typed cause intact.
    Topology(solidcore::SolidError),
    /// Conversion did not keep an input face's index.
    InputFaceLost(usize),
    /// A seam edit made a face without reporting which input face it came from.
    OutputFaceLost(solidcore::FaceId),
}

impl std::fmt::Display for PeriodicSeamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Geometry(e) => write!(f, "{e}"),
            Self::Conversion(e) => write!(f, "{e}"),
            Self::Topology(e) => write!(f, "{e}"),
            Self::InputFaceLost(i) => write!(f, "seam completion lost input face {i}"),
            Self::OutputFaceLost(face) => write!(f, "seam completion lost the origin of {face:?}"),
        }
    }
}

impl std::error::Error for PeriodicSeamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Geometry(e) => Some(e),
            Self::Conversion(e) => Some(e),
            Self::Topology(e) => Some(e),
            Self::InputFaceLost(_) | Self::OutputFaceLost(_) => None,
        }
    }
}

/// Complete an import's periodic topology after its lifted pcurves are known, before the
/// record is published. Returns the completed record and each output face's input index;
/// `None` leaves a record with in-domain trims and complete sectors unchanged. The kernel
/// removes crossed internal cuts, splits genuine boundaries at the base cut, inserts seams
/// and translates the pieces.
///
/// # Errors
/// Typed evaluation, conversion and topology failures, including a lost face origin.
pub fn complete_periodic_seams(
    record: &solidcore_record::brep::Brep,
) -> std::result::Result<Option<(solidcore_record::brep::Brep, Vec<usize>)>, PeriodicSeamError> {
    use solidcore_convert::brep::{to_kernel, to_record};
    use solidcore_record::brep::Loop;
    use std::collections::{BTreeMap, BTreeSet};

    let mut targets = Vec::new();
    for (i, face) in record.faces.iter().enumerate() {
        let surface = solidcore_convert::surface(face.parameters())
            .map_err(|e| PeriodicSeamError::Conversion(e.into()))?;
        let domain = solidcore::eval::SurfaceEval::new(&surface)
            .map_err(|e| PeriodicSeamError::Geometry(e.into()))?
            .domain();
        if domain.period.iter().all(Option::is_none) {
            continue;
        }
        let mut outside = false;
        let mut seen = BTreeSet::new();
        let mut internal_seam = false;
        for l in &face.loops {
            if let Loop::Edges { edges } = l {
                for u in edges {
                    outside |= k::leaves_base_period(domain, &solidcore_convert::curve2(&u.pcurve));
                    internal_seam |= !seen.insert(u.edge);
                }
            }
        }
        // An internal seam may have in-domain trims yet leave a 3D vertex sector
        // crossing the cut. Let the kernel inspect those sectors before taking the
        // no-edit path; bounds alone do not establish seam validity.
        if outside || internal_seam {
            targets.push(i);
        }
    }
    if targets.is_empty() {
        return Ok(None);
    }
    let mut kernel = solidcore::Kernel::default();
    let (mut body, order) =
        to_kernel(&mut kernel, record, true).map_err(PeriodicSeamError::Conversion)?;
    let mut origins: BTreeMap<_, _> = order
        .faces
        .iter()
        .enumerate()
        .map(|(i, &f)| (f, i))
        .collect();
    let mut changed = false;
    for i in targets {
        let f = *order
            .faces
            .get(i)
            .ok_or(PeriodicSeamError::InputFaceLost(i))?;
        let Some(pieces) =
            solidcore::topology::face::p1::complete_imported_face_seams(&mut kernel, &mut body, f)
                .map_err(PeriodicSeamError::Topology)?
        else {
            continue;
        };
        changed = true;
        for piece in pieces {
            origins.insert(piece, i);
        }
    }
    if !changed {
        return Ok(None);
    }
    let (completed, order) =
        to_record(&mut kernel, &mut body).map_err(PeriodicSeamError::Conversion)?;
    let faces = order
        .faces
        .iter()
        .map(|&f| {
            origins
                .get(&f)
                .copied()
                .ok_or(PeriodicSeamError::OutputFaceLost(f))
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(Some((completed, faces)))
}

/// One oriented edge of a loop, as [`pcurves`] reads it: the edge's index (a seam edge, used
/// twice, is fitted once), its curve record and interval, whether the loop runs along it, the
/// edge record's own tolerance (its pcurve is fitted to that, never closer than the face's), and
/// the use's exact pcurve where the producer holds one (`given`, on the use's interval in its
/// curve's direction). A given pcurve is taken as it is, only moved by whole periods with its
/// loop.
#[derive(Clone, Copy, Debug)]
pub struct EdgeUse<'a> {
    /// The edge's index.
    pub edge: u32,
    /// The edge's curve.
    pub curve: &'a Curve,
    /// The edge's interval on its curve.
    pub interval: [f64; 2],
    /// Whether the loop runs along the edge.
    pub forward: bool,
    /// The edge's own tolerance.
    pub tolerance: f64,
    /// The use's exact pcurve where the producer holds one.
    pub given: Option<&'a Curve2>,
}

/// One edge of a face handed to a kernel for its pcurves ([`TrimFace`]).
#[derive(Clone, Debug)]
pub struct TrimEdge<'a> {
    /// What the edge is.
    pub curve: &'a Curve,
    /// The NURBS form the edge is parameterised on, when it has one.
    pub nurbs: Option<&'a Curve>,
    /// The edge's interval.
    pub interval: [f64; 2],
    /// Into [`TrimFace::vertices`].
    pub vertices: [u32; 2],
    /// The edge's tolerance.
    pub tolerance: f64,
}

/// One edge use of a [`TrimFace`]'s loop: its edge (into [`TrimFace::edges`]), whether the loop
/// runs along it, and its exact pcurve where it has one.
#[derive(Clone, Debug)]
pub struct TrimUse {
    /// Into [`TrimFace::edges`].
    pub edge: u32,
    /// Whether the loop runs along the edge.
    pub forward: bool,
    /// The use's exact pcurve, where it has one.
    pub pcurve: Option<Curve2>,
}

/// A face whose pcurves are partly unknown, as a kernel takes it to make them: its surface (what
/// it is, the NURBS form it is parameterised on, the parameter box), its edges and their
/// vertices, and its loops (relative to the surface's natural normal).
#[derive(Clone, Debug)]
pub struct TrimFace<'a> {
    /// What the surface is.
    pub surface: &'a Surface,
    /// The NURBS form the face is parameterised on, when it has one.
    pub nurbs: Option<&'a Surface>,
    /// The face's parameter box, u then v.
    pub domain: [[f64; 2]; 2],
    /// The face's edges.
    pub edges: Vec<TrimEdge<'a>>,
    /// The edges' vertices.
    pub vertices: Vec<[f64; 3]>,
    /// Each vertex's tolerance, as `vertices`: how far along its curve a kernel may move an
    /// edge's end at that vertex (its edges end within it there) when the caller measures the
    /// gap that makes and covers it with the edge's tolerance. Empty where the caller covers
    /// none: an end then moves within the edge's own tolerance only.
    pub vertex_tolerances: Vec<f64>,
    /// The face's loops.
    pub loops: Vec<Vec<TrimUse>>,
    /// The face's tolerance.
    pub tolerance: f64,
}

/// A kernel that makes the pcurves of a face's uses that have no closed form, keeping the given
/// ones: for each loop, every use's pcurve on its edge's interval in its curve's direction.
pub trait Trims {
    /// The pcurves of every loop's uses.
    ///
    /// # Errors
    /// Why the face's pcurves could not be made.
    fn trims(&self, face: &TrimFace<'_>) -> std::result::Result<Vec<Vec<Curve2>>, String>;
}

/// For each loop of a face on `surface`, the pcurve of each of its edge uses, in order, within
/// `tolerance` mm (or the use's own edge tolerance where larger) of the edge when lifted onto
/// the surface ([`solidcore::eval::pcurve::pcurves`]).
///
/// # Errors
/// [`Error::Invalid`] for malformed records, [`Error::Failed`] where a use has no pcurve within
/// the tolerance, [`Error::TooLarge`] past the caps.
pub fn pcurves(
    surface: &Surface,
    loops: &[Vec<EdgeUse<'_>>],
    tolerance: f64,
) -> Result<Vec<Vec<Curve2>>> {
    pcurves_within(surface, loops, tolerance, None)
}

/// [`pcurves`] for a face whose parameter box is known (`domain`, u then v)
/// ([`solidcore::eval::pcurve::pcurves_within`]).
///
/// # Errors
/// As [`pcurves`].
pub fn pcurves_within(
    surface: &Surface,
    loops: &[Vec<EdgeUse<'_>>],
    tolerance: f64,
    domain: Option<[[f64; 2]; 2]>,
) -> Result<Vec<Vec<Curve2>>> {
    let face = Converted::new(surface, loops)?;
    let out = k::pcurves_within(&face.surface, &face.loops(), tolerance, domain)?;
    Ok(out
        .into_iter()
        .map(|l| {
            l.into_iter()
                .map(solidcore_convert::record_curve2)
                .collect()
        })
        .collect())
}

/// For each loop of a face on `surface`, each use's pcurve where it has a closed form (or is
/// given), else `None` ([`solidcore::eval::pcurve::exact_pcurves`]).
///
/// # Errors
/// As [`pcurves`], but a use with no closed form is `None`, not an error.
pub fn exact_pcurves(
    surface: &Surface,
    loops: &[Vec<EdgeUse<'_>>],
    tolerance: f64,
    domain: Option<[[f64; 2]; 2]>,
) -> Result<Vec<Vec<Option<Curve2>>>> {
    let face = Converted::new(surface, loops)?;
    let out = k::exact_pcurves(&face.surface, &face.loops(), tolerance, domain)?;
    Ok(out
        .into_iter()
        .map(|l| {
            l.into_iter()
                .map(|c| c.map(solidcore_convert::record_curve2))
                .collect()
        })
        .collect())
}

/// The pcurve on a cylinder or cone face `label` (on its NURBS form `form` when the face has
/// one) of a B-spline edge `curve` folded across it, on the edge's `interval`
/// ([`solidcore::eval::folded::folded_pcurve`]). `None` for another face or curve, a record
/// that does not convert, or past the kernel's piece budget.
#[must_use]
pub fn folded_pcurve(
    label: &Surface,
    form: Option<&Surface>,
    curve: &Curve,
    interval: [f64; 2],
) -> Option<Curve2> {
    // Only a spline on a cylinder or cone has one: nothing else is converted.
    if !matches!(label, Surface::Cylinder { .. } | Surface::Cone { .. })
        || !matches!(curve, Curve::Spline { .. } | Curve::RationalSpline { .. })
    {
        return None;
    }
    let label = solidcore_convert::surface(label).ok()?;
    let form = form.map(solidcore_convert::surface).transpose().ok()?;
    let curve = solidcore_convert::curve(curve).ok()?;
    solidcore::eval::folded::folded_pcurve(&label, form.as_ref(), &curve, interval)
        .map(solidcore_convert::record_curve2)
}

/// The parameters that bracket monotone pieces in both coordinates of a trim curve.
pub fn monotone_breaks(c: &Curve2) -> crate::Result<Vec<f64>> {
    check_curve2(c, &mut Budget::input()).map_err(|_| crate::Error::Argument)?;
    let curve = solidcore_convert::curve2(c);
    crate::session::run("pcurve::monotone_breaks", |k| {
        Ok(Ok(curve.monotone_breaks(k)?))
    })?
}

/// The point of pcurve `c` at `t` (`None` outside its interval or for a malformed record)
/// ([`solidcore::eval::pcurve::eval`]).
#[must_use]
pub fn eval(c: &Curve2, t: f64) -> Option<[f64; 2]> {
    k::eval(&solidcore_convert::curve2(c), t)
}

/// The point of pcurve `c` at `t` and its first derivative there (`None` outside its interval or
/// for a malformed record) ([`solidcore::eval::pcurve::eval_d1`]).
#[must_use]
pub fn eval_d1(c: &Curve2, t: f64) -> Option<([f64; 2], [f64; 2])> {
    k::eval_d1(&solidcore_convert::curve2(c), t)
}

/// The pieces of pcurve `c`'s interval that the flux of a field through its face is integrated
/// over, a Gauss rule on each: one or two to each knot span, at most 64 (`None` for a malformed
/// record) ([`solidcore::eval::pcurve::flux_pieces`]).
#[must_use]
pub fn flux_pieces(c: &Curve2) -> Option<Vec<[f64; 2]>> {
    k::flux_pieces(&solidcore_convert::curve2(c))
}

/// One use's records as the kernel's values.
struct Use {
    edge: u32,
    curve: solidcore::eval::Curve,
    interval: [f64; 2],
    forward: bool,
    tolerance: f64,
    given: Option<solidcore::eval::Curve2>,
}

/// A face's records checked and converted.
struct Converted {
    surface: solidcore::eval::Surface,
    loops: Vec<Vec<Use>>,
}

impl Converted {
    /// The records' structure, checked as the record's validation does, then converted: the
    /// surface, and each use's curve, interval and given pcurve, against one budget.
    fn new(surface: &Surface, loops: &[Vec<EdgeUse<'_>>]) -> Result<Self> {
        let invalid = |e: solidcore_record::brep::Invalid| Error::Invalid(e.to_string());
        let mut budget = Budget::input();
        check_surface(surface, &mut budget).map_err(invalid)?;
        let mut out = Vec::new();
        // A seam edge used twice is checked once, as the kernel fits it once.
        let mut checked: Vec<u32> = Vec::new();
        for (w, l) in loops.iter().enumerate() {
            let mut uses = Vec::new();
            for (i, u) in l.iter().enumerate() {
                let at = |e: solidcore_record::brep::Invalid| {
                    Error::Invalid(format!("loop {w} use {i}: {e}"))
                };
                if let Some(g) = u.given {
                    check_curve2(g, &mut budget).map_err(|e| {
                        Error::Invalid(format!("loop {w} use {i}: given pcurve: {e}"))
                    })?;
                } else if !checked.contains(&u.edge) {
                    check_curve(u.curve, &mut budget).map_err(at)?;
                    check_interval(&u.interval, u.curve.natural()).map_err(at)?;
                    checked.push(u.edge);
                }
                uses.push(Use {
                    edge: u.edge,
                    curve: solidcore_convert::curve(u.curve).map_err(at)?,
                    interval: u.interval,
                    forward: u.forward,
                    tolerance: u.tolerance,
                    given: u.given.map(solidcore_convert::curve2),
                });
            }
            out.push(uses);
        }
        Ok(Converted {
            surface: solidcore_convert::surface(surface).map_err(invalid)?,
            loops: out,
        })
    }

    /// The kernel's edge uses, borrowing the converted values.
    fn loops(&self) -> Vec<Vec<k::EdgeUse<'_>>> {
        self.loops
            .iter()
            .map(|l| {
                l.iter()
                    .map(|u| k::EdgeUse {
                        edge: u.edge,
                        curve: &u.curve,
                        interval: u.interval,
                        forward: u.forward,
                        tolerance: u.tolerance,
                        given: u.given.as_ref(),
                    })
                    .collect()
            })
            .collect()
    }
}
