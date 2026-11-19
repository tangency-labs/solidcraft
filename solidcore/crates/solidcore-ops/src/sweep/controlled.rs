//! The controlled sweep: a profile swept along a path of lines, arcs and helices by laws
//! (orientation, twist, end scale, up to two guide rails), skinned through the kernel's
//! stations ([`solidcore::sweep::stations`]), and which faces it made from which profile curve
//! and which are its caps, from the kernel's skin rather than any geometry.
//!
//! Every number is the kernel's: where the stations go ([`stations::fractions`], SolidCraft's
//! fit convention), each station's frame and the skin. The skin is a cubic interpolation through
//! the stations; its distance from the law surface between them is not certified (exact where
//! the sections vary linearly along a straight path, as an end scale along a line does;
//! #13637).
//!
//! Refused, by name: a path with corners (#13639) and a closed path
//! (#13638), each its own construction.

use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::sweep::api_surfaces::{create_skin_primitive_with_sources, NotTraced, SkinSources};
use solidcore::sweep::stations::{
    self, Laws, Orientation as KOrientation, Path, Piece as KPiece, RailFit as KRailFit, Stop,
};
use solidcore::topology::{Brep, FaceId};

use super::{bspline, parse, Affine, Curve, KCurve, Made};
use crate::body::{finish, Body, Expect};
use crate::error::status;
use crate::session::run;
use crate::{Error, Result};

/// SolidCraft's limits on a controlled sweep's laws: conventions of the app, not geometry
/// (the kernel refuses only what has no sweep; `solidcore::sweep::stations` module docs).
/// The most twist, four turns (radians, `8 pi`).
pub const MAX_TWIST: f64 = 25.132_741_228_718_345;
/// The smallest end scale (SolidCraft's).
pub const MIN_SCALE: f64 = 0.01;
/// The largest end scale (SolidCraft's).
pub const MAX_SCALE: f64 = 100.0;
/// The least angle a path meets a parallel profile's plane at (radians, 5 degrees;
/// SolidCraft's).
pub const MIN_PARALLEL_CROSSING: f64 = 0.087_266_462_599_716_47;
/// The least sine of the angle between the path and a direction orientation (SolidCraft's).
pub const MIN_DIRECTION_ACROSS: f64 = 1e-3;

/// One piece of a controlled sweep's path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece {
    /// From `a` to `b`.
    Line {
        /// Its start.
        a: [f64; 3],
        /// Its end.
        b: [f64; 3],
    },
    /// From `start`, `angle` radians right-handed about the unit `axis` through `centre`.
    Arc {
        /// A point of the axis.
        centre: [f64; 3],
        /// The axis (unit).
        axis: [f64; 3],
        /// Its start.
        start: [f64; 3],
        /// The angle run (radians, positive).
        angle: f64,
    },
    /// As [`Piece::Arc`], rising `rise` along the axis per radian.
    Helix {
        /// A point of the axis.
        centre: [f64; 3],
        /// The axis (unit).
        axis: [f64; 3],
        /// Its start.
        start: [f64; 3],
        /// The angle run (radians, positive).
        angle: f64,
        /// The rise per radian.
        rise: f64,
    },
}

/// How the profile turns as it follows the path ([`solidcore::sweep::stations::Orientation`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Orientation {
    /// Square to the path, carried by its rotation-minimizing frame.
    #[default]
    Perpendicular,
    /// As drawn.
    Parallel,
    /// Square to the path, its turn fixed by a direction.
    Direction([f64; 3]),
}

/// How guide rails size the profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RailFit {
    /// Evenly.
    #[default]
    Scale,
    /// Only across to the rail.
    Stretch,
}

/// A controlled sweep's laws. The default is a plain sweep.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlledSweep {
    /// How the profile turns.
    pub orientation: Orientation,
    /// The twist over the path (radians).
    pub twist: f64,
    /// The profile's scale at the end.
    pub end_scale: f64,
    /// Up to two guide rails, each one curve.
    pub rails: Vec<Curve>,
    /// How the rails size the profile.
    pub rail_fit: RailFit,
}

impl Default for ControlledSweep {
    fn default() -> Self {
        ControlledSweep {
            orientation: Orientation::Perpendicular,
            twist: 0.0,
            end_scale: 1.0,
            rails: Vec::new(),
            rail_fit: RailFit::Scale,
        }
    }
}

/// Which faces of a controlled sweep came from where, in the body's record indices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SweptFaces {
    /// The start cap's faces.
    pub start: Vec<usize>,
    /// The end cap's faces.
    pub end: Vec<usize>,
    /// Each wall face, in the skin's order, with the index (into the profile) of the curve it
    /// was swept from, as the kernel's skin traced it (`create_skin_primitive_with_sources`).
    /// A curve with a kink of its own sweeps a wall on each side of it.
    pub walls: Vec<(usize, usize)>,
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn p3(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn kernel_piece(p: &Piece) -> KPiece {
    match *p {
        Piece::Line { a, b } => KPiece::Line { a: v3(a), b: v3(b) },
        Piece::Arc {
            centre,
            axis,
            start,
            angle,
        } => KPiece::Arc {
            centre: v3(centre),
            axis: v3(axis),
            start: v3(start),
            angle,
        },
        Piece::Helix {
            centre,
            axis,
            start,
            angle,
            rise,
        } => KPiece::Helix {
            centre: v3(centre),
            axis: v3(axis),
            start: v3(start),
            angle,
            rise,
        },
    }
}

/// The body's record index of each of `ids`.
fn indices(order: &[FaceId], ids: &[FaceId]) -> Option<Vec<usize>> {
    ids.iter()
        .map(|f| order.iter().position(|g| g == f))
        .collect()
}

/// The profile `profile` (closed loops in the plane whose normal is `profile_normal`) swept
/// along `path` by `laws`, capped at both ends, and which faces came from where.
///
/// # Errors
/// [`Error::Argument`] for no path pieces, a profile or rail the curves refuse, or more than
/// two rails; the kernel's refusal ([`Error::Refused`]) of a twist or end scale past
/// SolidCraft's limits ([`MAX_TWIST`], [`MIN_SCALE`], [`MAX_SCALE`], passed to it);
/// [`Error::Unsupported`] for a path with corners or a closed path, and for a skin the kernel
/// does not trace to its profile curves (a controlled sweep's sections are affine images of
/// one profile, so it always is: `create_skin_primitive_with_sources`) or a wall not swept
/// from one curve of each section; the kernel's refusals of the path, the laws and
/// the rails ([`Error::Refused`] for input it refuses, else [`Error::Status`]), with
/// SolidCraft's least crossing angle and sine
/// ([`MIN_PARALLEL_CROSSING`], [`MIN_DIRECTION_ACROSS`]); those of `finish`.
pub fn sweep_controlled(
    profile: &[Curve],
    profile_normal: [f64; 3],
    path: &[Piece],
    laws: &ControlledSweep,
) -> Result<(Body, SweptFaces)> {
    if path.is_empty() || laws.rails.len() > 2 {
        return Err(Error::Argument);
    }
    let made = run(
        "sweep_controlled",
        |k| -> solidcore::Result<Result<(Brep, Faces)>> {
            build(k, profile, profile_normal, path, laws)
        },
    )?;
    let (brep, faces) = made?;
    let body = Body::new(brep);
    let (_, order) = body.to_record()?;
    let start = indices(&order.faces, &faces.start);
    let end = indices(&order.faces, &faces.end);
    let (Some(start), Some(end)) = (start, end) else {
        return Err(Error::Exception);
    };
    let mut walls = Vec::new();
    for (f, curve) in faces.walls {
        let Some(i) = order.faces.iter().position(|g| *g == f) else {
            return Err(Error::Exception);
        };
        walls.push((i, curve));
    }
    Ok((body, SweptFaces { start, end, walls }))
}

/// The kernel's face ids of a controlled sweep's caps and walls.
struct Faces {
    start: Vec<FaceId>,
    end: Vec<FaceId>,
    walls: Vec<(FaceId, usize)>,
}

fn build(
    k: &mut Kernel,
    profile: &[Curve],
    profile_normal: [f64; 3],
    path: &[Piece],
    laws: &ControlledSweep,
) -> solidcore::Result<Result<(Brep, Faces)>> {
    let made: Made = match parse(k, profile) {
        Ok(m) => m,
        Err(e) => return Ok(Err(e)),
    };
    let mut rails: Vec<KCurve> = Vec::new();
    for rail in &laws.rails {
        let rail_made = match parse(k, std::slice::from_ref(rail)) {
            Ok(m) => m,
            Err(e) => return Ok(Err(e)),
        };
        let [c] = rail_made.curves.as_slice() else {
            return Ok(Err(Error::Argument));
        };
        rails.push(c.clone());
    }
    let kpath = Path {
        pieces: path.iter().map(kernel_piece).collect(),
    };
    if !kpath.corner_fractions()?.is_empty() {
        return Ok(Err(Error::Unsupported(
            "a controlled sweep along a path with corners (its runs meet at mitres: #13639)",
        )));
    }
    if kpath.is_closed()? {
        return Ok(Err(Error::Unsupported(
            "a controlled sweep round a closed path (its last section on its first: #13638)",
        )));
    }
    let klaws = Laws {
        orientation: match laws.orientation {
            Orientation::Perpendicular => KOrientation::Perpendicular,
            Orientation::Parallel => KOrientation::Parallel,
            Orientation::Direction(d) => KOrientation::Direction(v3(d)),
        },
        twist: laws.twist,
        end_scale: laws.end_scale,
        rails: rails.iter().collect(),
        rail_fit: match laws.rail_fit {
            RailFit::Scale => KRailFit::Scale,
            RailFit::Stretch => KRailFit::Stretch,
        },
        min_crossing: MIN_PARALLEL_CROSSING,
        min_across: MIN_DIRECTION_ACROSS,
        max_twist: MAX_TWIST,
        end_scale_within: [MIN_SCALE, MAX_SCALE],
    };
    let fractions = stations::fractions(&kpath, &klaws)?;
    let at: Vec<Stop> = fractions.into_iter().map(Stop::At).collect();
    let st = stations::stations(k, &kpath, v3(profile_normal), &klaws, &at)?;
    let Some(first) = st.first() else {
        return Ok(Err(Error::Argument));
    };
    let mut sections = Vec::new();
    let mut counts = Vec::new();
    for s in &st {
        let map = Affine {
            m: s.m.map(p3),
            t: p3(s.translation(first)),
        };
        let t = map.transform();
        for c in &made.curves {
            let Some(b) = bspline(c)? else {
                return Ok(Err(Error::Argument));
            };
            match b.affine_image(&t) {
                Ok((image, _)) => sections.push(KCurve::from(image)),
                Err(e) => return Ok(Err(status(e))),
            }
        }
        counts.push(made.curves.len());
    }
    let degree = st.len().saturating_sub(1).min(3);
    let refs: Vec<Option<&KCurve>> = sections.iter().map(Some).collect();
    let (skin, sources) = match create_skin_primitive_with_sources(k, &counts, &refs, degree, true)
    {
        Ok(r) => r,
        Err(e) => return Ok(Err(status(e))),
    };
    let traced = match sources {
        SkinSources::Traced(walls) => walls,
        SkinSources::NotTraced { why } => return Ok(Err(Error::Unsupported(not_traced(why)))),
    };
    // each wall holds one curve of each section, the same profile curve throughout: the
    // indices c, c + n, c + 2 n, ... of the sections' curves (n the profile's)
    let n = made.curves.len();
    let mut walls = Vec::new();
    for (f, curves) in traced {
        let Some(&c) = curves.first() else {
            return Ok(Err(Error::Unsupported(
                "a wall the skin traced to no profile curve",
            )));
        };
        let one_curve = c < n
            && curves.len() == st.len()
            && curves.iter().enumerate().all(|(s, &ci)| ci == c + s * n);
        if !one_curve {
            return Ok(Err(Error::Unsupported(
                "a wall not swept from one curve of each section",
            )));
        }
        walls.push((f, c));
    }
    let faces = Faces {
        start: skin.start_faces,
        end: skin.end_faces,
        walls,
    };
    Ok(match finish(k, skin.brep, Expect::SOLID)? {
        Ok(b) => Ok((b, faces)),
        Err(e) => Err(e),
    })
}

/// The refusal for a skin the kernel did not trace, by its reason.
fn not_traced(why: NotTraced) -> &'static str {
    match why {
        NotTraced::OpenProfiles => "a skin not traced to its profile curves: open profiles",
        NotTraced::SkinMovedKnots => {
            "a skin not traced to its profile curves: its knots are not the sections'"
        }
        NotTraced::FaceOffKnots => {
            "a skin not traced to its profile curves: a wall's bounds are not its knots"
        }
        NotTraced::EdgeUntraced => {
            "a skin not traced to its profile curves: a profile edge with no recorded curve"
        }
        NotTraced::SegmentMerged => {
            "a skin not traced to its profile curves: a curve merged into one knot"
        }
        NotTraced::KnotMapBroken => {
            "a skin not traced to its profile curves: where a section's knots went is not known"
        }
        NotTraced::SurfaceKnotsUnreadable => {
            "a skin not traced to its profile curves: its knots could not be read"
        }
        NotTraced::FaceDomainUnreadable => {
            "a skin not traced to its profile curves: a wall's domain could not be read"
        }
        NotTraced::SideFacesNotReached => {
            "a skin not traced to its profile curves: its walls were not made"
        }
    }
}
