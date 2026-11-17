//! Spot welds: the display bodies of one resistance spot weld between two lapped sheets.
//!
//! Not upstream: SolidCraft's own (#15132). The bodies are built at the spot from exact
//! primitives (cylinders, spheres) and booleans between them only. They take no part as
//! input, so building them can never change a part.
//!
//! | Rust                          | What it is                                              |
//! |-------------------------------|---------------------------------------------------------|
//! | [`SpotWeld`]                  | One spot: where, which way, the two sheet thicknesses   |
//! | [`SpotLook`]                  | How the spot is drawn (six looks)                       |
//! | [`SpotZone`]                  | Which part of the look a body is (for the app's colours)|
//! | [`spot_weld`]                 | The spot's bodies                                       |
//! | [`proposed_spot_diameter`]    | The diameter the weld tool proposes, `5 √t`             |
//! | [`pin_dish_depth`]            | How deep the concave pin ends are dished                |
//!
//! # The geometry
//!
//! A spot sits on the *faying face*, the face where the two sheets touch. `normal` points
//! from the lower sheet into the upper one; the upper sheet's outer face lies
//! `upper_thickness` along it, the lower sheet's outer face `lower_thickness` against it.
//! Every size of a look is a fixed fraction of the spot diameter `d` (marks, balls, dent),
//! or follows the sheets (pins), so a look keeps its proportions at any size the user sets.
//!
//! The *marks* show the spot on both outer faces, as a real spot's electrode imprint does:
//! a light core of diameter `0.55 d`, a soft band out to `d` and a thin dark edge out to
//! `1.05 d`, each a flat body `0.006 d` thick lying on the face. Resistance spot welding
//! adds no filler, so a body inside the sheets ([`SpotZone::Inside`]) overlaps metal the
//! parts already have: it is drawn, never counted as volume or mass.
//!
//! # Not covered
//!
//! Flat sheets only: the marks are flat and the pins are straight, so the outer faces must
//! be planes square to `normal` at the spot. The caller (the weld tool) measures the
//! thicknesses and checks the faces; this module does not see the parts.

use crate::boolean::api_brep::boolean_difference;
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::primitive::api_primitives::{create_cylinder, create_sphere};
use crate::topology::brep::p5::transform;
use crate::topology::Brep;

const OP_SPOT: &str = "weld::spot";

/// Core of the marks: diameter as a fraction of the spot diameter.
pub const CORE: f64 = 0.55;
/// Outer edge of the marks: diameter as a fraction of the spot diameter.
pub const EDGE: f64 = 1.05;
/// Thickness of the flat marks as a fraction of the spot diameter (0.03 mm at 5 mm).
pub const MARK_THICKNESS: f64 = 0.006;
/// Small ball ([`SpotLook::MarksSmallBall`]): diameter as a fraction of the spot diameter.
pub const SMALL_BALL: f64 = 0.6;
/// Concave pin ends ([`SpotLook::MarksPinConcaveEnds`]): dish depth as a fraction of the
/// thinner sheet, held to the pin's radius ([`pin_dish_depth`]).
pub const PIN_DISH: f64 = 0.3;
/// Dent ([`SpotLook::Dent`]): height at the rim as a fraction of the spot diameter
/// (0.20 mm at 5 mm).
pub const DENT_RIM: f64 = 0.04;
/// Dent: height in the middle as a fraction of the spot diameter (0.02 mm at 5 mm).
pub const DENT_MIDDLE: f64 = 0.004;

/// How a spot weld is drawn. Alexander's shortlist of 17 Nov 2026; the weld tool may
/// offer them as a look option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpotLook {
    /// The marks on both outer faces, nothing else.
    Marks,
    /// A ball of the spot diameter centred on the faying face, standing proud of both outer
    /// faces; no marks.
    Ball,
    /// The marks and a ball of `0.6 d` centred on the faying face.
    MarksSmallBall,
    /// The marks and a pin of the core diameter through both sheets, flush with the
    /// outer faces.
    MarksThinPin,
    /// The marks and a pin of the spot diameter through both sheets, both ends dished in by
    /// `0.3` of the thinner sheet, at most a half sphere ([`pin_dish_depth`]).
    MarksPinConcaveEnds,
    /// The marks as a shallow inverted dome on each outer face: `0.04 d` high at the rim,
    /// `0.004 d` in the middle, read as the electrode's dent. Lies on the face; the face
    /// itself is not dented.
    Dent,
}

/// Which part of a look a body is, so the app can colour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpotZone {
    /// A mark's light core.
    Core,
    /// A mark's soft band.
    Band,
    /// A mark's thin dark edge.
    Edge,
    /// A body through or between the sheets (ball, pin), or the ball of [`SpotLook::Ball`].
    Inside,
}

/// Which outer face a mark lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpotSide {
    /// The upper sheet's outer face (along `normal`).
    Upper,
    /// The lower sheet's outer face (against `normal`).
    Lower,
    /// Neither: a body through or between the sheets.
    Through,
}

/// One body of a spot weld.
#[derive(Debug)]
pub struct SpotPiece {
    /// The body.
    pub body: Brep,
    /// Which part of the look it is.
    pub zone: SpotZone,
    /// Which face it lies on.
    pub side: SpotSide,
}

/// One spot weld.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotWeld {
    /// The spot's centre on the faying face.
    pub at: Point3,
    /// The faying face's normal, from the lower sheet into the upper one (need not be unit).
    pub normal: Vec3,
    /// Distance from the faying face to the upper sheet's outer face, along `normal`.
    pub upper_thickness: f64,
    /// Distance from the faying face to the lower sheet's outer face, against `normal`.
    pub lower_thickness: f64,
    /// The spot diameter: a user setting; [`proposed_spot_diameter`] is the tool's proposal.
    pub diameter: f64,
    /// How the spot is drawn.
    pub look: SpotLook,
}

/// The spot diameter the weld tool proposes: `5 √t`, `t` the thinner sheet in mm.
///
/// Sources: TWI's FAQ on interface failures in resistance spot welds gives `5 √t` as the
/// typical target diameter; the AWS C1.1 guidelines take the thinner sheet's parameters
/// for sheets of different thickness. The user can always set another diameter.
#[must_use]
pub fn proposed_spot_diameter(upper_thickness: f64, lower_thickness: f64) -> f64 {
    5.0 * upper_thickness.min(lower_thickness).sqrt()
}

/// The depth of each dished end of [`SpotLook::MarksPinConcaveEnds`]' pin: `0.3` of the
/// thinner sheet, held to the pin's radius `d / 2`. A deeper dish would need a sphere wider
/// than the pin, which cuts the pin short below the outer faces instead of dishing its end
/// (a spot diameter under `0.6` of the thinner sheet); held there, the dish is a half
/// sphere and the pin's rim stays on the faces.
#[must_use]
pub fn pin_dish_depth(diameter: f64, upper_thickness: f64, lower_thickness: f64) -> f64 {
    (PIN_DISH * upper_thickness.min(lower_thickness)).min(diameter / 2.0)
}

/// The bodies of `spot`, placed at the spot.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite point, a zero or non-finite normal, or a
/// thickness or diameter that is not finite and positive; the errors of the primitives,
/// booleans and the placement they come from.
pub fn spot_weld(k: &mut Kernel, spot: &SpotWeld) -> Result<Vec<SpotPiece>> {
    let positive = |v: f64| v.is_finite() && v > 0.0;
    if !positive(spot.upper_thickness) || !positive(spot.lower_thickness) {
        return Err(SolidError::invalid_input(
            OP_SPOT,
            "a sheet thickness that is not positive",
        ));
    }
    if !positive(spot.diameter) {
        return Err(SolidError::invalid_input(
            OP_SPOT,
            "a spot diameter that is not positive",
        ));
    }
    let finite = |p: Vec3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    if !finite(spot.at) || !finite(spot.normal) {
        return Err(SolidError::invalid_input(
            OP_SPOT,
            "a spot point or normal that is not finite",
        ));
    }
    let Some((_, z)) = spot.normal.length_and_unit() else {
        return Err(SolidError::invalid_input(OP_SPOT, "a zero normal"));
    };
    let mut pieces = local_pieces(k, spot)?;
    let placement = placement(spot.at, z);
    for piece in &mut pieces {
        transform(k, &mut piece.body, &placement, None, false)?;
    }
    Ok(pieces)
}

/// The placement taking the local frame (faying face at the origin, normal along +Z) to
/// the spot.
fn placement(at: Point3, z: Vec3) -> Axis2Placement {
    let axis = if z.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let x = axis - z * axis.dot(z);
    let x = x * (1.0 / x.length());
    let y = z.cross(x);
    Axis2Placement::new(at, x, y)
}

/// The pieces in the local frame: faying face `z = 0`, upper outer face `z = t_u`,
/// lower outer face `z = -t_l`.
fn local_pieces(k: &mut Kernel, spot: &SpotWeld) -> Result<Vec<SpotPiece>> {
    let d = spot.diameter;
    let (t_u, t_l) = (spot.upper_thickness, spot.lower_thickness);
    let at = |z: f64| Point3::new(0.0, 0.0, z);
    let mut pieces = Vec::new();
    let inside = |body| SpotPiece {
        body,
        zone: SpotZone::Inside,
        side: SpotSide::Through,
    };
    match spot.look {
        SpotLook::Marks | SpotLook::Ball | SpotLook::Dent => {}
        SpotLook::MarksSmallBall => {
            pieces.push(inside(create_sphere(k, at(0.0), SMALL_BALL * d / 2.0)?));
        }
        SpotLook::MarksThinPin => {
            pieces.push(inside(create_cylinder(
                k,
                at(-t_l),
                CORE * d / 2.0,
                t_u + t_l,
            )?));
        }
        SpotLook::MarksPinConcaveEnds => {
            let r = d / 2.0;
            let c = pin_dish_depth(d, t_u, t_l);
            let rs = (r * r + c * c) / (2.0 * c);
            let mut pin = create_cylinder(k, at(-t_l), r, t_u + t_l)?;
            let upper = create_sphere(k, at(t_u - c + rs), rs)?;
            pin = boolean_difference(k, pin, upper)?;
            let lower = create_sphere(k, at(-t_l + c - rs), rs)?;
            pin = boolean_difference(k, pin, lower)?;
            pieces.push(inside(pin));
        }
    }
    if spot.look == SpotLook::Ball {
        pieces.push(inside(create_sphere(k, at(0.0), d / 2.0)?));
        return Ok(pieces);
    }
    for (side, face_z, sense) in [(SpotSide::Upper, t_u, 1.0), (SpotSide::Lower, -t_l, -1.0)] {
        if spot.look == SpotLook::Dent {
            pieces.extend(dent(k, d, face_z, sense, side)?);
        } else {
            pieces.extend(marks(k, d, face_z, sense, side)?);
        }
    }
    Ok(pieces)
}

/// The three zones as (inner diameter, outer diameter) fractions of the spot diameter.
const ZONES: [(f64, f64, SpotZone); 3] = [
    (0.0, CORE, SpotZone::Core),
    (CORE, 1.0, SpotZone::Band),
    (1.0, EDGE, SpotZone::Edge),
];

/// A zone's flat body of `height`, its base on `base_z`: a disc for the core, a ring
/// otherwise.
fn zone_body(
    k: &mut Kernel,
    d: f64,
    inner: f64,
    outer: f64,
    base_z: f64,
    height: f64,
) -> Result<Brep> {
    let base = Point3::new(0.0, 0.0, base_z);
    let disc = create_cylinder(k, base, outer * d / 2.0, height)?;
    if inner <= 0.0 {
        return Ok(disc);
    }
    let hole = create_cylinder(
        k,
        Point3::new(0.0, 0.0, base_z - height),
        inner * d / 2.0,
        3.0 * height,
    )?;
    boolean_difference(k, disc, hole)
}

/// The flat marks on the outer face at `face_z`, standing out along `sense`.
fn marks(
    k: &mut Kernel,
    d: f64,
    face_z: f64,
    sense: f64,
    side: SpotSide,
) -> Result<Vec<SpotPiece>> {
    let h = MARK_THICKNESS * d;
    let base_z = if sense > 0.0 { face_z } else { face_z - h };
    ZONES
        .iter()
        .map(|&(inner, outer, zone)| {
            Ok(SpotPiece {
                body: zone_body(k, d, inner, outer, base_z, h)?,
                zone,
                side,
            })
        })
        .collect()
}

/// The dent on the outer face at `face_z`: each zone cut down by one sphere so the top is
/// dished, `DENT_RIM d` high at the edge's outer rim and `DENT_MIDDLE d` in the middle.
fn dent(k: &mut Kernel, d: f64, face_z: f64, sense: f64, side: SpotSide) -> Result<Vec<SpotPiece>> {
    let (rim, middle) = (DENT_RIM * d, DENT_MIDDLE * d);
    let ro = EDGE * d / 2.0;
    let c = rim - middle;
    let rs = (ro * ro + c * c) / (2.0 * c);
    let base_z = if sense > 0.0 { face_z } else { face_z - rim };
    let centre = Point3::new(0.0, 0.0, face_z + sense * (middle + rs));
    ZONES
        .iter()
        .map(|&(inner, outer, zone)| {
            let block = zone_body(k, d, inner, outer, base_z, rim)?;
            let sphere = create_sphere(k, centre, rs)?;
            Ok(SpotPiece {
                body: boolean_difference(k, block, sphere)?,
                zone,
                side,
            })
        })
        .collect()
}
