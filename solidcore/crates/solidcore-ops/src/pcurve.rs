//! Pcurves of a record's face ([`solidcore::eval::pcurve`]): each edge use's curve in its face's
//! surface parameters, from the record's 3D edges and surface. This checks the records' structure
//! as the record's validation does (counts against a [`Budget`], a use's interval in its curve's
//! natural range, a given pcurve as a valid 2D B-spline), converts them through
//! `solidcore-convert`, calls the kernel, and converts its pcurves back.
//!
//! The request types a kernel that makes the remaining pcurves takes ([`TrimFace`], [`Trims`])
//! are here too: they hold records.

use solidcore::eval::pcurve as k;
use solidcore_record::brep::{
    check_curve, check_curve2, check_interval, check_surface, Budget, Curve, Curve2, Surface,
};

pub use solidcore::eval::pcurve::{Error, SURFACE_GAP_MARGIN};

type Result<T> = std::result::Result<T, Error>;

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

/// The point of pcurve `c` at `t` (`None` outside its interval or for a malformed record)
/// ([`solidcore::eval::pcurve::eval`]).
#[must_use]
pub fn eval(c: &Curve2, t: f64) -> Option<[f64; 2]> {
    k::eval(&solidcore_convert::curve2(c), t)
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
