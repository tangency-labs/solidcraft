//! A surface extended past several sides of its natural box in turn, each side exactly
//! ([`Surface::extended_by_distance`]): [`Surface::extended_sides`], the one kernel entry the
//! callers that extend a surface "on every side" take.
//!
//! Not upstream: upstream's `CreateExtendedSurface` extends every side by an estimated
//! distance, fits its extension by least squares and removes knots within a tolerance; it is
//! not ported, this exact path replaces it.
//!
//! Each asked side, in [`BoxSide::ALL`]'s order, is extended on the box the sides before it
//! left (the result's natural box), so a later side runs along the earlier extensions. A side
//! is left as it is, and said so, where the surface closes in its direction (a full turn: an
//! extension would lap the surface) or where the side is not proved longer than a point (a
//! pole or an apex: its certified diameter's lower bound is 0), as upstream leaves a closed
//! direction and a singular side. Every other side is extended or refused with the side's
//! own typed error.
//!
//! Two more rules (owner, 2026-11-04):
//! - **Unbounded.** A plane's side at `+-INFINITE_PARAMETER` (the kernel's infinite plane) is
//!   left: the plane has no side there to extend past.
//! - **Analytic kinds as themselves.** A cone, sphere or torus (or an offset surface of one;
//!   a plane too for a caller keeping its trims, [`Surface::extended_sides_keeping_parameters`];
//!   otherwise a plane is widened as a plane) is extended by its own NURBS net continued Natural whatever the continuity asked, the
//!   source's net and parameters bit for bit, still on its analytic surface (certified):
//!   round its axis up to a quarter of the turn the source leaves open, along its generator
//!   never past its apex, pole or full turn, and said so where that caps the distance
//!   (`analytic`). A surface of revolution is extended along its generator as asked, on its
//!   B-spline form row by row: its rows are one generator turned and weighted, every
//!   construction here commutes with both, so the result is the revolution of the generator
//!   extended; its side round its axis is left.
//!
//! - **Ball patches as themselves.** A ball patch (the sphere's triangle with a pole) is
//!   extended by its own net continued Natural (the source bit for bit, every point still on
//!   the sphere, certified), round the pole up to a quarter of the turn it leaves open and
//!   away from it up to half the arc left to the antipode, its pole's side collapsed
//!   (`ball`).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use super::{Continuity, ExtendRefusal, Extended};
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::INFINITE_PARAMETER;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::surface::side::{Across, BoxSide};
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "surface::extend::Surface::extended_sides";

/// Halvings of an ask whose side was refused naming nothing, to name a distance that works.
const NAMING_HALVINGS: usize = 60;

/// What [`Surface::extended_sides`] did at one side of the source's box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SideExtension {
    /// Not asked.
    NotAsked,
    /// Extended: the length across it ([`Extended::across`]).
    Extended(Across),
    /// Left: the surface closes in the side's direction.
    Closed,
    /// Left: the side is not proved longer than a point.
    Collapsed,
    /// Left: a plane's side at `+-INFINITE_PARAMETER`, unbounded.
    Unbounded,
    /// Left: a revolution's side round its axis (a closed direction).
    AroundAxis,
    /// Extended less than asked, up to an apex, pole or full turn: the length across it.
    UpToLimit(Across),
}

/// For a cone, sphere, torus or surface of revolution, whether side `side` runs round its
/// axis (its NURBS u is the sweep unless swapped); `None` for any other kind.
pub(super) fn round_the_axis(s: &Surface, side: BoxSide) -> Option<bool> {
    let swap = match s.kind() {
        SurfaceKind::Cone(c) => c.rev().swap_uv(),
        SurfaceKind::Sphere(sp) => sp.base.swap_uv,
        SurfaceKind::Torus(t) => t.base.swap_uv,
        SurfaceKind::SurfOfRevolution(r) => r.swap_uv(),
        _ => return None,
    };
    Some(side.is_u() != swap)
}

/// A surface extended past several sides ([`Surface::extended_sides`]).
#[derive(Debug, Clone)]
pub struct ExtendedSides {
    /// The result.
    pub surface: Surface,
    /// What was done at each side of the source's box, in [`BoxSide::ALL`]'s order.
    pub sides: [SideExtension; 4],
    /// How far the result over the source's box is from the source: the sum of the sides'
    /// [`Extended::source`], each on the one before.
    pub source: f64,
    /// The largest construction bound of the sides' extensions ([`Extended::extension`]):
    /// each extension is within this of its construction.
    pub extension: f64,
}

impl Surface {
    /// The surface extended `distance` past each side of its natural box in `asked`, as
    /// `how` says, exactly (module docs), a side whose direction closes or that is not proved
    /// longer than a point left as it is.
    ///
    /// # Errors
    ///
    /// Those of [`Surface::extended_by_distance`] at the first side that is refused, and of
    /// the closure and diameter queries. A side refused as not exact with no distance named
    /// (it stands on the sides extended before it, so far out that nothing there is measured
    /// to the stop) is refused naming the longest of `distance / 2^i` at which every side
    /// extends, if any.
    pub fn extended_sides(
        &self,
        k: &Kernel,
        asked: &[BoxSide],
        how: Continuity,
        distance: f64,
    ) -> Result<ExtendedSides> {
        self.extended_sides_with(k, asked, how, distance, false)
    }

    /// [`Surface::extended_sides`] for a caller that keeps the face's UV trims on the result
    /// (the shell's extended offset replaces a face's surface with UV trim creation off): a
    /// plane too is extended by its own net, a B-spline on the plane's own parameters (owner,
    /// 2026-11-04: the cone, sphere and torus rule), where [`Surface::extended_sides`] widens
    /// it as a plane (its kind kept, which a caller intersecting with it reads).
    ///
    /// # Errors
    ///
    /// As [`Surface::extended_sides`].
    pub fn extended_sides_keeping_parameters(
        &self,
        k: &Kernel,
        asked: &[BoxSide],
        how: Continuity,
        distance: f64,
    ) -> Result<ExtendedSides> {
        self.extended_sides_with(k, asked, how, distance, true)
    }

    fn extended_sides_with(
        &self,
        k: &Kernel,
        asked: &[BoxSide],
        how: Continuity,
        distance: f64,
        keeping: bool,
    ) -> Result<ExtendedSides> {
        // a ball patch as its sphere (`ball`)
        if let SurfaceKind::BallPatch(patch) = self.kind() {
            if let Some(e) = super::ball::ball_extended_sides(k, self, patch, asked, distance)? {
                return Ok(e);
            }
        }
        // a cone, sphere or torus (a plane too when keeping) by its own net (`analytic`)
        if let Some(e) =
            super::analytic::analytic_extended_sides(k, self, asked, distance, keeping)?
        {
            return Ok(e);
        }
        match self.extended_in_turn(k, asked, how, distance) {
            Err(SolidError::ExtendRefused {
                operation,
                refusal: ExtendRefusal::NotExact,
                distance: None,
            }) => {
                let mut tried = distance;
                let mut named = None;
                for _ in 0..NAMING_HALVINGS {
                    tried *= 0.5;
                    if self.extended_in_turn(k, asked, how, tried).is_ok() {
                        named = Some(tried);
                        break;
                    }
                }
                Err(SolidError::ExtendRefused {
                    operation,
                    refusal: ExtendRefusal::NotExact,
                    distance: named,
                })
            }
            other => other,
        }
    }

    /// The sides of `asked` extended `distance` in turn, each on the box the sides before it
    /// left.
    fn extended_in_turn(
        &self,
        k: &Kernel,
        asked: &[BoxSide],
        how: Continuity,
        distance: f64,
    ) -> Result<ExtendedSides> {
        let mut current: Option<Extended> = None;
        // the side of the current result each side of the source's box is on
        let mut on = BoxSide::ALL;
        let mut sides = [SideExtension::NotAsked; 4];
        let (mut source, mut extension) = (0.0_f64, 0.0_f64);
        for ((&source_side, done), at) in BoxSide::ALL.iter().zip(sides.iter_mut()).zip(0..) {
            if !asked.contains(&source_side) {
                continue;
            }
            let side = *on.get(at).ok_or_else(|| SolidError::sm_err(OP))?;
            let surface = current.as_ref().map_or(self, |e| &e.surface);
            let dom = surface.natural_uv_domain_with_nurb(k)?;
            let param = if side.is_u() {
                SurfParamType::U
            } else {
                SurfParamType::V
            };
            // an offset surface closes and collapses where its base does
            let mut carrier = surface;
            while let SurfaceKind::OffsetSurface(o) = carrier.kind() {
                carrier = o.base_surface();
            }
            if carrier.is_closed(k, &dom, param, None, false)?.0 {
                *done = SideExtension::Closed;
                continue;
            }
            if carrier.side_diameter(k, &dom, side)?.lower <= 0.0 {
                *done = SideExtension::Collapsed;
                continue;
            }
            if matches!(carrier.kind(), SurfaceKind::Plane(_))
                && side.at(&dom).abs() >= INFINITE_PARAMETER
            {
                *done = SideExtension::Unbounded;
                continue;
            }
            // a revolution's side round its axis is left; along it, its B-spline form row by
            // row as asked, which is again a revolution (its rows are one generator turned
            // and weighted, and every construction commutes with both)
            if round_the_axis(carrier, side) == Some(true) {
                *done = SideExtension::AroundAxis;
                continue;
            }
            let grown = surface.extended_by_distance(k, &dom, side, how, distance)?;
            for s in &mut on {
                *s = grown.side_of(*s);
            }
            source += grown.source;
            extension = extension.max(grown.extension);
            *done = SideExtension::Extended(grown.across);
            current = Some(grown);
        }
        let surface = match current {
            // a Natural continuation of the net is still on an exact round's S (#2487)
            Some(e) if how == Continuity::Natural => self.carry_round_on_continuation(e.surface),
            Some(e) => e.surface,
            None => self.copy(k)?,
        };
        Ok(ExtendedSides {
            surface,
            sides,
            source,
            extension,
        })
    }
}
